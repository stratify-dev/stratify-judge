use crate::model::Span;
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::HashMap;
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

/// Everything a judge needs about the repo under analysis: the file
/// inventory and on-demand source reads, both scoped by the same
/// `stratify.toml` `[ignore] paths` globs the engine honors.
pub struct RepoContext {
    root: PathBuf,
    files: Vec<FileEntry>,
    cache: RefCell<HashMap<String, Option<String>>>,
}

impl RepoContext {
    pub fn new(root: PathBuf) -> io::Result<Self> {
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
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
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
        self.cache.borrow_mut().insert(rel.to_string(), read.clone());
        read
    }

    /// The span's byte range widened to whole lines, which is what a model
    /// needs to read a function. Byte offsets that do not land on a char
    /// boundary are clamped outward to the nearest one.
    pub fn function_source(&self, span: &Span) -> Option<String> {
        let text = self.file_text(&span.file)?;
        if text.is_empty() {
            return None;
        }
        let start = span.start_byte.min(text.len());
        let end = span.end_byte.clamp(start, text.len());
        let line_start = text[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line_end = text[end..]
            .find('\n')
            .map(|i| end + i + 1)
            .unwrap_or_else(|| text.len());
        let mut lo = line_start;
        while lo < text.len() && !text.is_char_boundary(lo) {
            lo += 1;
        }
        let mut hi = line_end;
        while hi < text.len() && !text.is_char_boundary(hi) {
            hi += 1;
        }
        text.get(lo..hi).map(|s| s.to_string())
    }
}

/// Read `[ignore] paths` from `stratify.toml` at the root. Absent or
/// unparseable means no globs, matching the engine's own fallback.
fn load_ignore_globs(root: &Path) -> GlobSet {
    let Ok(text) = std::fs::read_to_string(root.join("stratify.toml")) else {
        return GlobSet::empty();
    };
    let cfg: IgnoreToml = toml::from_str(&text).unwrap_or_default();
    let mut b = GlobSetBuilder::new();
    for p in &cfg.ignore.paths {
        if let Ok(g) = Glob::new(p) {
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
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/sample-repo");
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
    fn language_is_derived_from_the_extension() {
        assert_eq!(language_of("a/b.rs"), Some("rust"));
        assert_eq!(language_of("a/b.tsx"), Some("typescript"));
        assert_eq!(language_of("a/b.txt"), None);
    }
}
