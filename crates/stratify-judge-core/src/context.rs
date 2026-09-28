use crate::model::Span;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::{Path, PathBuf};

/// One file in scope, matching what the engine would have parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub rel: String,
    pub language: Option<&'static str>,
}

/// Map a path to the engine's language name, by extension.
pub fn language_of(path: &str) -> Option<&'static str> {
    match path.rsplit('.').next()? {
        "java" => Some("java"),
        "rb" => Some("ruby"),
        "ts" | "tsx" | "mts" | "cts" => Some("typescript"),
        "py" | "pyi" => Some("python"),
        "go" => Some("go"),
        "rs" => Some("rust"),
        _ => None,
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct IgnoreSection {
    #[serde(default)]
    paths: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct IgnoreToml {
    #[serde(default)]
    ignore: IgnoreSection,
}

/// Identifier -> every (file index, 1-based line) it occurs at.
type IdentifierIndex = HashMap<String, Vec<(usize, usize)>>;

/// Everything a judge needs about the repo under analysis: the file
/// inventory and on-demand source reads, both scoped by the same
/// `stratify.toml` `[ignore] paths` globs the engine honors.
#[derive(Debug)]
pub struct RepoContext {
    root: PathBuf,
    files: Vec<FileEntry>,
    cache: RefCell<HashMap<String, Option<String>>>,
    /// Built lazily on first `occurrences` call, then reused for the whole run.
    index: RefCell<Option<IdentifierIndex>>,
}

impl RepoContext {
    pub fn new(root: PathBuf) -> io::Result<Self> {
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("not a directory: {}", root.display()),
            ));
        }
        let globs = load_ignore_globs(&root);
        let mut files = Vec::new();
        for entry in WalkBuilder::new(&root).build() {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let rel = entry
                .path()
                .strip_prefix(&root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            if globs.is_match(&rel) {
                continue;
            }
            let language = language_of(&rel);
            if language.is_none() {
                continue;
            }
            files.push(FileEntry { rel, language });
        }
        files.sort_by(|a, b| a.rel.cmp(&b.rel));
        Ok(RepoContext {
            root,
            files,
            cache: RefCell::new(HashMap::new()),
            index: RefCell::new(None),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every whole-word occurrence of `name` across the file inventory.
    ///
    /// The index is built once on first call and reused for the rest of the
    /// run, so this stays one pass over the repo no matter how many findings
    /// ask. Without it, judging N findings against M files would be N times M
    /// scans.
    ///
    /// Name collisions make the result a hint, not a proof, which is the
    /// right strength for a model input. Present it as a count with sample
    /// lines, never as a claim that a call exists.
    pub fn occurrences(&self, name: &str) -> Vec<Occurrence> {
        if name.is_empty() {
            return Vec::new();
        }
        self.build_index();
        let index = self.index.borrow();
        let Some(hits) = index.as_ref().and_then(|m| m.get(name)) else {
            return Vec::new();
        };
        // Group by file so each file's text is fetched and split once, not
        // once per hit. A common identifier can appear hundreds of times in
        // one file, and resolving each hit separately would clone the whole
        // file every time, undoing what build_index exists to protect.
        let mut by_file: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (file_idx, line) in hits {
            by_file.entry(*file_idx).or_default().push(*line);
        }
        let mut out = Vec::new();
        for (file_idx, mut lines) in by_file {
            // The index holds one entry per token, so a line using the name
            // twice yields two identical sites. Counting tokens and calling
            // them sites would overstate the evidence.
            lines.sort_unstable();
            lines.dedup();
            let Some(entry) = self.files.get(file_idx) else {
                continue;
            };
            let Some(text) = self.file_text(&entry.rel) else {
                continue;
            };
            let split: Vec<&str> = text.lines().collect();
            for line in lines {
                if let Some(content) = split.get(line - 1) {
                    out.push(Occurrence {
                        file: entry.rel.clone(),
                        line,
                        text: content.trim().to_string(),
                    });
                }
            }
        }
        out
    }

    fn build_index(&self) {
        if self.index.borrow().is_some() {
            return;
        }
        let mut map: IdentifierIndex = HashMap::new();
        for (file_idx, entry) in self.files.iter().enumerate() {
            let Some(text) = self.file_text(&entry.rel) else {
                continue;
            };
            for (i, line) in text.lines().enumerate() {
                for_each_identifier(line, |ident| {
                    map.entry(ident.to_string())
                        .or_default()
                        .push((file_idx, i + 1));
                });
            }
        }
        *self.index.borrow_mut() = Some(map);
    }

    /// Whether `line` in `file` sits inside test code.
    ///
    /// A deliberate heuristic, and the single decisive fact for the
    /// `#[cfg(test)]` helper shape: that attribute sits on the enclosing
    /// module, not on the function, so nothing about the function itself
    /// reveals it. Brace counting ignores braces inside strings and
    /// comments, which is acceptable for a model hint and would not be for
    /// a parser.
    pub fn in_test_context(&self, file: &str, line: usize) -> bool {
        if path_is_tests(file) {
            return true;
        }
        let Some(text) = self.file_text(file) else {
            return false;
        };
        let lines: Vec<&str> = text.lines().collect();
        let mut depth: i32 = 0;
        let mut test_depth: Option<i32> = None;
        for (i, l) in lines.iter().enumerate() {
            if i + 1 > line {
                break;
            }
            if test_depth.is_none() {
                let following = lines[i + 1..]
                    .iter()
                    .find(|n| !n.trim().is_empty())
                    .copied();
                if opens_test_block(l, following) {
                    test_depth = Some(depth);
                }
            }
            for ch in l.chars() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if test_depth.is_some_and(|d| depth <= d) {
                            test_depth = None;
                        }
                    }
                    _ => {}
                }
            }
        }
        test_depth.is_some()
    }

    pub fn files(&self) -> &[FileEntry] {
        &self.files
    }

    /// Whole-file text, read once and memoized. None when unreadable.
    pub fn file_text(&self, rel: &str) -> Option<String> {
        if let Some(hit) = self.cache.borrow().get(rel) {
            return hit.clone();
        }
        let read = std::fs::read_to_string(self.root.join(rel)).ok();
        self.cache
            .borrow_mut()
            .insert(rel.to_string(), read.clone());
        read
    }

    /// The span's byte range widened to whole lines, which is what a model
    /// needs to read a function.
    ///
    /// The span's OWN offsets are snapped to char boundaries before any
    /// slicing. They arrive as raw byte offsets from an engine report, so a
    /// stale report, a newer engine, or a file edited since the scan can put
    /// one mid-character, and slicing there panics. Snapping the line
    /// bounds afterward would protect nothing: they come from searching for
    /// '\n', which is ASCII and therefore always already on a boundary.
    pub fn function_source(&self, span: &Span) -> Option<String> {
        let text = self.file_text(&span.file)?;
        if text.is_empty() {
            return None;
        }
        let start = floor_boundary(&text, span.start_byte.min(text.len()));
        let end = ceil_boundary(&text, span.end_byte.clamp(start, text.len()));
        let line_start = text[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line_end = text[end..]
            .find('\n')
            .map(|i| end + i + 1)
            .unwrap_or_else(|| text.len());
        Some(text[line_start..line_end].to_string())
    }
}

/// One place a name appears in the repository, outside its own declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub file: String,
    pub line: usize,
    pub text: String,
}

/// Call `f` with every identifier-like run in `line`.
///
/// Bytes at or above 0x80 count as identifier bytes, so `créerUtilisateur`
/// stays one identifier. Go, Python, Ruby, TypeScript and Java all permit
/// non-ASCII identifiers, and splitting them would make `occurrences`
/// return nothing for such a function, which the `no` criterion then reads
/// as proof that no caller exists.
///
/// This cannot panic. A run ends at the first non-identifier byte, which is
/// always ASCII and therefore a char boundary. A run starts at the first
/// identifier byte after a non-identifier one, and a UTF-8 continuation
/// byte never follows an ASCII byte, so a run never starts mid-character.
fn for_each_identifier(line: &str, mut f: impl FnMut(&str)) {
    let bytes = line.as_bytes();
    let mut start: Option<usize> = None;
    for (i, &b) in bytes.iter().enumerate() {
        let is_ident = b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80;
        match (is_ident, start) {
            (true, None) => start = Some(i),
            (false, Some(st)) => {
                f(&line[st..i]);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(st) = start {
        f(&line[st..]);
    }
}

/// True when the path alone says the file holds tests.
fn path_is_tests(rel: &str) -> bool {
    let stem = rel.rsplit('/').next().unwrap_or(rel);
    rel.starts_with("tests/")
        || rel.contains("/tests/")
        || rel.contains("/__tests__/")
        || stem.starts_with("test_")
        || stem.contains("_test.")
        || stem.contains(".test.")
        || stem.contains("_spec.")
        || stem.contains(".spec.")
}

/// True when `t` reaches an opening brace before any statement terminator.
fn opens_a_block(t: &str) -> bool {
    match (t.find('{'), t.find(';')) {
        (Some(b), Some(semi)) => b < semi,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

/// True when `t` names a test module, as a whole word. Plain `starts_with`
/// would also match `mod testsuite`, an ordinary production module.
fn names_test_module(t: &str) -> bool {
    ["mod tests", "pub mod tests"].iter().any(|p| {
        t.strip_prefix(p)
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
    })
}

/// True when this line opens a block whose contents are tests. `following`
/// is the next non-blank line, since `#[cfg(test)]` sits on its own line
/// above the `mod` it applies to.
///
/// The opener must actually open a block. `#[cfg(test)] use tempfile::…;`,
/// `mod tests;` and a Python class all name tests without opening a braced
/// block, and arming on them leaves the flag set over the next function,
/// which then reads as test code and gets dismissed. That is C1's failure
/// mode reversed: silently deleting a real finding instead of real code.
///
/// Python is deliberately absent. Its blocks are indentation-delimited, so
/// brace counting can never close them and the flag would stay armed for
/// the rest of the file. `path_is_tests` covers the usual Python layouts
/// (`test_*.py`, `*_test.py`, `tests/`) and is the safer instrument.
fn opens_test_block(line: &str, following: Option<&str>) -> bool {
    let t = line.trim();
    // A bare `#[cfg(test)]` attribute never opens a block itself: the `mod`
    // it applies to is the next line, so only this branch consults
    // `following`. `mod tests;` and `describe(...)` are complete statements
    // on their own line, and peeking past them would let an unrelated
    // brace on the next, unconnected line arm the flag: `mod tests;`
    // followed by a production function's own `{` measured exactly this.
    if t.starts_with("#[cfg(test)]") {
        return opens_a_block(t) || following.map(str::trim).is_some_and(opens_a_block);
    }
    (names_test_module(t) || t.starts_with("describe(")) && opens_a_block(t)
}

/// Largest char boundary at or below `i`. Index 0 is always a boundary, so
/// this terminates.
fn floor_boundary(text: &str, mut i: usize) -> usize {
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Smallest char boundary at or above `i`. `text.len()` is always a
/// boundary, so this terminates.
fn ceil_boundary(text: &str, mut i: usize) -> usize {
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Read `[ignore] paths` from `stratify.toml` at the root. Absent or
/// unparseable means no globs, matching the engine's own fallback.
fn load_ignore_globs(root: &Path) -> GlobSet {
    let Ok(text) = std::fs::read_to_string(root.join("stratify.toml")) else {
        return GlobSet::empty();
    };
    let cfg: IgnoreToml = toml::from_str(&text).unwrap_or_default();
    compile_ignore_globs(&cfg.ignore.paths)
}

/// Compile ignore globs exactly as the engine does, in
/// `stratify-analysis/src/ignore.rs`: `literal_separator(true)` so `*` does
/// not cross `/` while `**` does. Plain `Glob::new` leaves that flag false,
/// which would make `build/*.log` match `build/sub/a.log` here but not in
/// the engine, and the two tools would silently disagree about scope.
/// Bad patterns are skipped, matching the engine again.
pub fn compile_ignore_globs(paths: &[String]) -> GlobSet {
    let mut b = GlobSetBuilder::new();
    for p in paths {
        if let Ok(g) = GlobBuilder::new(p).literal_separator(true).build() {
            b.add(g);
        }
    }
    b.build().unwrap_or_else(|_| GlobSet::empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Span;
    use std::path::PathBuf;

    fn fixture() -> RepoContext {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/sample-repo");
        RepoContext::new(root).unwrap()
    }

    #[test]
    fn walk_honors_stratify_toml_ignore_paths() {
        let ctx = fixture();
        let rels: Vec<&str> = ctx.files().iter().map(|f| f.rel.as_str()).collect();
        assert!(rels.contains(&"src/lib.rs"));
        assert!(
            !rels.iter().any(|r| r.starts_with("vendor/")),
            "vendor/** is ignored in stratify.toml, got {rels:?}"
        );
    }

    #[test]
    fn function_source_expands_a_span_to_whole_lines() {
        let ctx = fixture();
        let text = ctx.file_text("src/lib.rs").unwrap();
        // Byte range covering only "fn helper" inside the helper declaration.
        let start = text.find("fn helper").unwrap();
        let span = Span {
            file: "src/lib.rs".into(),
            start_byte: start,
            end_byte: start + 9,
            start_line: 5,
        };
        let src = ctx.function_source(&span).unwrap();
        assert!(src.starts_with("fn helper() -> u32 {"), "got: {src:?}");
        assert!(src.ends_with('\n') || src.ends_with('}'), "got: {src:?}");
    }

    #[test]
    fn function_source_is_none_for_a_missing_file() {
        let ctx = fixture();
        let span = Span {
            file: "src/nope.rs".into(),
            start_byte: 0,
            end_byte: 1,
            start_line: 1,
        };
        assert!(ctx.function_source(&span).is_none());
    }

    #[test]
    fn a_span_landing_mid_character_does_not_panic() {
        let ctx = fixture();
        let text = ctx.file_text("src/unicode.rs").unwrap();
        // 'h' is one byte, then 'é' occupies the next two, so find + 2
        // lands strictly inside 'é' and is not a char boundary.
        let inside = text.find("héllo").unwrap() + 2;
        assert!(
            !text.is_char_boundary(inside),
            "fixture must be multi-byte here"
        );
        let span = Span {
            file: "src/unicode.rs".into(),
            start_byte: inside,
            end_byte: inside + 1,
            start_line: 1,
        };
        let src = ctx
            .function_source(&span)
            .expect("returns a line, never panics");
        assert!(src.contains("héllo"), "got {src:?}");
    }

    #[test]
    fn a_single_star_glob_does_not_cross_a_directory_separator() {
        // The engine compiles globs with literal_separator(true). Diverging
        // here means the two tools disagree about which files are in scope.
        let shallow = compile_ignore_globs(&["vendor/*.rs".to_string()]);
        assert!(shallow.is_match("vendor/skipme.rs"));
        assert!(!shallow.is_match("vendor/sub/skipme.rs"));

        let deep = compile_ignore_globs(&["vendor/**".to_string()]);
        assert!(deep.is_match("vendor/sub/skipme.rs"));
    }

    #[test]
    fn a_missing_root_is_an_error_not_an_empty_inventory() {
        let err = RepoContext::new(PathBuf::from("/definitely/not/a/real/path")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn occurrences_finds_whole_word_uses_and_skips_substrings() {
        let ctx = fixture();
        // `helper` is declared once in src/lib.rs and called once from `used`.
        let hits = ctx.occurrences("helper");
        assert!(hits.len() >= 2, "got {hits:?}");
        assert!(hits.iter().all(|o| o.file == "src/lib.rs"));
        assert!(hits.iter().any(|o| o.text.contains("helper() + 1")));
        // A substring of a longer identifier is not an occurrence.
        assert!(ctx.occurrences("help").is_empty());
        assert!(ctx.occurrences("").is_empty());
    }

    #[test]
    fn occurrences_are_empty_for_an_unknown_name() {
        assert!(fixture()
            .occurrences("no_such_identifier_anywhere")
            .is_empty());
    }

    #[test]
    fn in_test_context_is_false_for_ordinary_production_code() {
        let ctx = fixture();
        let line = ctx
            .file_text("src/lib.rs")
            .unwrap()
            .lines()
            .position(|l| l.contains("fn helper"))
            .unwrap()
            + 1;
        assert!(!ctx.in_test_context("src/lib.rs", line));
    }

    #[test]
    fn in_test_context_is_true_inside_a_cfg_test_module() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/a.rs"),
            "pub fn prod() -> u32 {\n    1\n}\n\n#[cfg(test)]\nmod tests {\n    fn helper() {}\n}\n\npub fn after() {}\n",
        )
        .unwrap();
        let ctx = RepoContext::new(dir.path().to_path_buf()).unwrap();
        // `fn helper` sits on line 7, inside the test module.
        assert!(ctx.in_test_context("src/a.rs", 7));
        // `fn prod` on line 1 and `fn after` on line 10 do not.
        assert!(!ctx.in_test_context("src/a.rs", 1));
        assert!(!ctx.in_test_context("src/a.rs", 10));
    }

    /// F1: an opener that names tests without opening a braced block left
    /// the flag armed over the next function, which then read as test code
    /// and got dismissed. That is the reverse of promoting live code: it
    /// silently deletes a real finding.
    #[test]
    fn an_opener_that_opens_no_block_does_not_arm_test_context() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        for (name, body, probe) in [
            (
                "a.rs",
                "#[cfg(test)]\nuse std::fmt::Debug;\n\npub fn production_thing() -> u32 {\n    1\n}\n",
                4,
            ),
            ("b.rs", "mod tests;\n\npub fn important() -> u32 {\n    1\n}\n", 3),
            ("c.rs", "mod testsuite;\n\npub fn thing() -> u32 {\n    1\n}\n", 3),
        ] {
            std::fs::write(dir.path().join("src").join(name), body).unwrap();
            let ctx = RepoContext::new(dir.path().to_path_buf()).unwrap();
            assert!(
                !ctx.in_test_context(&format!("src/{name}"), probe),
                "{name}: production code must not read as test context"
            );
        }
    }

    #[test]
    fn a_cfg_test_attribute_above_a_module_still_arms() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/a.rs"),
            "#[cfg(test)]\nmod tests {\n    fn helper() {}\n}\n\npub fn after() {}\n",
        )
        .unwrap();
        let ctx = RepoContext::new(dir.path().to_path_buf()).unwrap();
        assert!(
            ctx.in_test_context("src/a.rs", 3),
            "the helper is inside the module"
        );
        assert!(
            !ctx.in_test_context("src/a.rs", 6),
            "after the module closes it is not"
        );
    }

    #[test]
    fn a_line_using_the_name_twice_counts_as_one_site() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/a.rs"),
            "fn twice() -> u32 {\n    helper() + helper()\n}\nfn helper() -> u32 { 1 }\n",
        )
        .unwrap();
        let ctx = RepoContext::new(dir.path().to_path_buf()).unwrap();
        let hits = ctx.occurrences("helper");
        let lines: Vec<usize> = hits.iter().map(|o| o.line).collect();
        assert_eq!(lines, vec![2, 4], "one site per line, not one per token");
    }

    #[test]
    fn a_non_ascii_identifier_is_one_identifier() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/a.py"),
            "def cr\u{e9}erUtilisateur():\n    pass\n\ncr\u{e9}erUtilisateur()\n",
        )
        .unwrap();
        let ctx = RepoContext::new(dir.path().to_path_buf()).unwrap();
        // Splitting on the accent would return nothing, and the resolver
        // question's "no" criterion reads an empty list as proof of no caller.
        assert_eq!(ctx.occurrences("cr\u{e9}erUtilisateur").len(), 2);
        assert!(ctx.occurrences("erUtilisateur").is_empty());
    }

    #[test]
    fn a_path_under_tests_counts_as_test_context_on_its_own() {
        let ctx = fixture();
        assert!(ctx.in_test_context("tests/whatever.rs", 1));
        assert!(ctx.in_test_context("src/thing_test.rs", 1));
        assert!(!ctx.in_test_context("src/lib.rs", 1));
    }

    #[test]
    fn language_is_derived_from_the_extension() {
        assert_eq!(language_of("a/b.rs"), Some("rust"));
        assert_eq!(language_of("a/b.tsx"), Some("typescript"));
        assert_eq!(language_of("a/b.txt"), None);
    }
}
