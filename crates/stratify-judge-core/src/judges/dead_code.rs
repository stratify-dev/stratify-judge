use crate::config::{DeadCodeThresholds, Thresholds};
use crate::context::{language_of, RepoContext};
use crate::judges::Judge;
use crate::model::{Confidence, Finding, Severity};
use crate::verdict::{Judgment, Verdict};
use std::collections::BTreeMap;
use systemone_client::{Answer, NoulCriteria, Question};

pub struct DeadCodeJudge;

/// The engine writes "unused function `name`" or "possibly unused
/// function `name`". Pull the backticked name out of either.
pub fn function_name(message: &str) -> Option<&str> {
    let start = message.find('`')? + 1;
    let rest = &message[start..];
    let end = rest.find('`')?;
    // An empty pair of backticks is not a name. Returning Some("") would
    // put a nameless function in the state, indistinguishable from the
    // absent case, and the questions would then refer to nothing.
    if end == 0 {
        return None;
    }
    Some(&rest[..end])
}

/// Attributes, annotations, and decorators directly above the function.
/// These are the strongest framework signal in most languages, so only
/// real markers are collected: a comment or a blank line is walked past,
/// never captured. Collecting `#` comments would poison the signal in
/// Python and Ruby, where `#` starts a comment rather than an attribute.
fn attributes_above(ctx: &RepoContext, file: &str, start_line: usize) -> Vec<String> {
    let Some(text) = ctx.file_text(file) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = start_line.saturating_sub(1);
    while i > 0 {
        let line = lines.get(i - 1).map(|s| s.trim()).unwrap_or("");
        let is_marker = line.starts_with("#[") || line.starts_with('@');
        let is_skippable = line.is_empty() || line.starts_with("//") || line.starts_with('#');
        if is_marker {
            out.push(line.to_string());
        } else if !is_skippable {
            break;
        }
        i -= 1;
    }
    out.reverse();
    out
}

/// Where else this function's name appears, excluding its own declaration
/// line. Split in-file from repo-wide because they answer different
/// questions: in-file occurrences catch a private helper whose only caller
/// was itself unreachable, repo-wide ones catch a call the engine could not
/// resolve across a crate boundary.
///
/// Sample lines are capped so a common name cannot blow past the state
/// budget. Name collisions make this a hint, never a proof, and the
/// question wording says so.
fn occurrence_summary(
    ctx: &RepoContext,
    decl: &crate::model::Span,
    name: &str,
) -> serde_json::Value {
    const MAX_SAMPLES: usize = 8;
    let all = ctx.occurrences(name);
    let mut elsewhere: Vec<_> = all
        .into_iter()
        .filter(|o| !(o.file == decl.file && o.line == decl.start_line))
        .collect();
    let in_file = elsewhere.iter().filter(|o| o.file == decl.file).count();
    let repo_wide = elsewhere.len();

    // Rank before truncating. Path order is not relevance: measured on the
    // real repo, `span` has 183 occurrences whose first eight by path are
    // all struct-field lines and not one is a call. The question asks the
    // model whether the listed entries read like calls, so showing it eight
    // that do not, while real calls exist, steers it to the answer that
    // satisfies the Strengthen precondition.
    elsewhere.sort_by_key(|o| (!looks_like_call(&o.text, name), o.file != decl.file));
    let samples: Vec<String> = elsewhere
        .iter()
        .take(MAX_SAMPLES)
        .map(|o| format!("{}:{}: {}", o.file, o.line, o.text))
        .collect();
    serde_json::json!({
        "in_this_file": in_file,
        "repo_wide": repo_wide,
        "sample_sites": samples,
    })
}

/// Whether a source line uses `name` the way a call site would. A hint for
/// ranking evidence, never a parse: `name(`, `.name(` and `::name` cover
/// plain calls, method calls and qualified paths across the six supported
/// languages.
fn looks_like_call(text: &str, name: &str) -> bool {
    text.contains(&format!("{name}("))
        || text.contains(&format!(".{name}("))
        || text.contains(&format!("::{name}"))
}

/// Import-looking lines from the head of the file, capped so state stays small.
fn imports_of(ctx: &RepoContext, file: &str) -> Vec<String> {
    let Some(text) = ctx.file_text(file) else {
        return Vec::new();
    };
    text.lines()
        .take(200)
        .map(str::trim)
        .filter(|l| {
            l.starts_with("use ")
                || l.starts_with("import ")
                || l.starts_with("from ")
                || l.starts_with("require")
                || l.starts_with("package ")
        })
        .take(40)
        .map(str::to_string)
        .collect()
}

/// Repo-level signals that change what "unreachable" means.
fn project_markers(ctx: &RepoContext) -> Vec<String> {
    let mut out = Vec::new();
    let root = ctx.root();
    if let Ok(t) = std::fs::read_to_string(root.join("Cargo.toml")) {
        if t.contains("publish = false") {
            out.push("Cargo.toml declares publish = false".into());
        }
        // `publish = false` almost never appears in a workspace, because
        // unpublished members simply omit the key. Without this marker the
        // external_api question points at an empty array on exactly the
        // repos where it matters most, and a `pub` item crossing a crate
        // boundary inside the repo looks like a public API.
        if t.contains("[workspace]") {
            // No member count. A glob like members = ["crates/*"] makes any
            // substring tally wrong, and a wrong number in text sent to the
            // model is worse than no number.
            out.push(
                "Cargo workspace: `pub` items are called across member crates inside \
                 this repository, not only by outside consumers"
                    .into(),
            );
        }
    }
    if let Ok(t) = std::fs::read_to_string(root.join("package.json")) {
        // Match key and value together. `t.contains("\"private\"") &&
        // t.contains("true")` fires on `"private": false` beside any
        // unrelated `true`, which would wrongly steer external_api low on a
        // genuinely published package.
        if serde_json::from_str::<serde_json::Value>(&t)
            .ok()
            .and_then(|v| v.get("private").and_then(serde_json::Value::as_bool))
            == Some(true)
        {
            out.push("package.json declares private: true".into());
        }
    }
    if root.join("config/routes.rb").exists() || root.join("app/controllers").is_dir() {
        out.push("Rails application layout detected".into());
    }
    if root.join("pom.xml").exists() || root.join("build.gradle").exists() {
        out.push("Maven or Gradle project detected".into());
    }
    out
}

impl DeadCodeJudge {
    pub fn judge(
        &self,
        finding: &Finding,
        answers: &BTreeMap<String, Answer>,
        cfg: &DeadCodeThresholds,
    ) -> Judgment {
        let fw = answers.get("framework_invoked").and_then(Answer::as_noul);
        let test = answers.get("test_only").and_then(Answer::as_noul);
        let api = answers.get("external_api").and_then(Answer::as_noul);
        let resolver = answers
            .get("resolver_missed_a_call")
            .and_then(Answer::as_noul);
        let explanation = answers.get("explanation").and_then(Answer::as_choice);

        let mut raw = serde_json::Map::new();
        if let Some(v) = fw {
            raw.insert("framework_invoked".into(), serde_json::json!(v));
        }
        if let Some(v) = test {
            raw.insert("test_only".into(), serde_json::json!(v));
        }
        if let Some(v) = api {
            raw.insert("external_api".into(), serde_json::json!(v));
        }
        if let Some(v) = resolver {
            raw.insert("resolver_missed_a_call".into(), serde_json::json!(v));
        }
        if let Some((pick, conf)) = explanation {
            raw.insert(
                "explanation".into(),
                serde_json::json!({ "choice": pick, "confidence": conf }),
            );
        }

        let (verdict, reason) = decide(
            fw,
            test,
            api,
            resolver,
            explanation,
            (finding.severity, finding.confidence),
            cfg,
        );

        Judgment {
            judge: "dead_code".into(),
            verdict,
            reason,
            answers: raw,
            model: String::new(), // filled by the driver from the response
        }
    }
}

/// Policy lives here alone, separate from the answers, so retuning a
/// threshold never needs a new request.
///
/// `engine` is the severity and confidence the engine itself assigned. The
/// policy needs it for two reasons: to know whether a verdict would change
/// anything, and because the engine's own hedging is the single strongest
/// signal available about why a function looks unreachable.
fn decide(
    fw: Option<f64>,
    test: Option<f64>,
    api: Option<f64>,
    resolver: Option<f64>,
    explanation: Option<(&str, f64)>,
    engine: (Severity, Confidence),
    cfg: &DeadCodeThresholds,
) -> (Verdict, String) {
    // A missing answer never produces an action. Absence is not evidence.
    let (Some(fw), Some(test), Some(api)) = (fw, test, api) else {
        return (
            Verdict::Keep,
            "no usable answer, left as the engine found it".into(),
        );
    };
    // An absent resolver answer is unknown, not low and not high. It must
    // not trigger Dismiss, or a model that omits the answer would dismiss
    // every finding. It must not permit Strengthen either, since a cache
    // entry predating this question carries no resolver evidence at all.
    if let Some(r) = resolver {
        if r >= cfg.resolver_at {
            return (
                Verdict::Dismiss,
                format!("a call site exists that the engine could not resolve ({r:.2})"),
            );
        }
    }

    if fw >= cfg.dismiss_at {
        return (
            Verdict::Dismiss,
            format!("reached by a framework ({fw:.2})"),
        );
    }
    if test >= cfg.dismiss_at {
        return (Verdict::Dismiss, format!("test-only helper ({test:.2})"));
    }
    if let Some(("entrypoint", conf)) = explanation {
        if conf >= cfg.explanation_at {
            return (
                Verdict::Dismiss,
                format!("program entry point, not called from inside the repo ({conf:.2})"),
            );
        }
    }
    // Checked before the public-API branch: a claimed resolved call is
    // stronger false-positive evidence than a public-API guess, and a
    // finding scoring high on both would otherwise only Weaken.
    if let Some(("resolver_limitation", conf)) = explanation {
        if conf >= cfg.explanation_at {
            return (
                Verdict::Dismiss,
                format!(
                    "the analyzer missed a real call rather than finding dead code ({conf:.2})"
                ),
            );
        }
    }
    if api >= cfg.api_at {
        // Severity has no step below Info, and public symbols in library
        // mode are exactly the population that arrives there, so Weaken
        // would leave severity untouched. Dismiss drops confidence to
        // Unknown, which the display threshold reads. Above the floor the
        // step down is real and Weaken is the proportionate action.
        return if engine.0 <= Severity::Info {
            (
                Verdict::Dismiss,
                format!("public API surface for outside consumers ({api:.2})"),
            )
        } else {
            (
                Verdict::Weaken,
                format!("public API surface for outside consumers ({api:.2})"),
            )
        };
    }
    if fw < cfg.low_at
        && test < cfg.low_at
        && api < cfg.low_at
        && resolver.is_some_and(|r| r < cfg.low_at)
    {
        if let Some(("genuinely_unused", conf)) = explanation {
            if conf >= cfg.explanation_at {
                // The guard that holds even when every question fails.
                // Confidence::Likely on a dead_code finding means the engine
                // reached the symbol through an edge it could not confirm, or
                // the symbol is public in library mode. Both mean "a caller
                // may exist that I could not verify". Promoting exactly that
                // population to Certain inverts the engine's own epistemics,
                // and nothing in the state outranks it.
                if engine.1 == Confidence::Certain {
                    return (
                        Verdict::Strengthen,
                        format!("no caller and no hidden entry point ({conf:.2})"),
                    );
                }
                return (
                    Verdict::Keep,
                    format!(
                        "looks unused ({conf:.2}), but the engine could not confirm \
                         reachability either, so its hedge stands"
                    ),
                );
            }
        }
    }
    (Verdict::Keep, "no clear signal either way".into())
}

impl Judge for DeadCodeJudge {
    fn rule(&self) -> &'static str {
        "dead_code"
    }

    fn version(&self) -> u32 {
        // 3: question text now names its state root (`{root}.function`), so
        // every question in a batch binds to the finding it is about, and
        // test_only finally references in_test_context. A version-2 answer
        // was produced from text naming a bare key that is not present at
        // the top level of a batched request.
        3
    }

    fn full_strength(&self) -> Severity {
        Severity::Warning
    }

    fn state_for(&self, finding: &Finding, ctx: &RepoContext) -> serde_json::Value {
        let file = finding.span.file.as_str();
        let name = function_name(&finding.message).unwrap_or_default();
        serde_json::json!({
            "function": {
                "name": name,
                "file": file,
                "line": finding.span.start_line,
                "language": language_of(file),
                "source": ctx.function_source(&finding.span).unwrap_or_default(),
                "attributes": attributes_above(ctx, file, finding.span.start_line),
            },
            "file_imports": imports_of(ctx, file),
            "project_markers": project_markers(ctx),
            "engine_message": finding.message,
            // The engine's own hedge. "likely" means it reached the symbol
            // through an edge it could not confirm, or the symbol is public
            // in library mode: either way a caller may exist that it could
            // not verify. That is the strongest single clue about why a
            // function looks unreachable, and it costs nothing to include.
            "engine_confidence": finding.confidence,
            // The decisive fact for the #[cfg(test)] helper shape. That
            // attribute sits on the enclosing module, not on the function,
            // so nothing about the function itself reveals it.
            "in_test_context": ctx.in_test_context(file, finding.span.start_line),
            // Where else this name appears. Without it the state carries no
            // caller information of any kind, so a private helper called
            // four times inside its own file looks identical to dead code.
            "occurrences": occurrence_summary(ctx, &finding.span, name),
        })
    }

    fn questions(&self) -> BTreeMap<String, Question> {
        [
            (
                "framework_invoked".to_string(),
                Question::noul(
                    "Would a framework, dependency-injection container, reflection call, \
                     route table, serializer, or plugin registry invoke the function in \
                     `{root}.function` without any explicit call to it appearing in \
                     source?",
                    Some(NoulCriteria {
                        yes: "Something outside ordinary call syntax reaches this function: \
                              an annotation or attribute registers it, a container wires it, \
                              a route or event table names it, or a serializer calls it by \
                              convention.".into(),
                        no: "Only an ordinary call written in source would reach this \
                             function.".into(),
                    }),
                ),
            ),
            (
                "test_only".to_string(),
                Question::noul(
                    "Given `{root}.in_test_context`, does the function in `{root}.function` \
                     exist only to support tests, such as a fixture builder, a test \
                     helper, or an assertion utility?",
                    Some(NoulCriteria {
                        yes: "Its purpose is setting up or supporting tests, and production \
                              code has no reason to call it.".into(),
                        no: "It carries production behavior, whatever else also calls it.".into(),
                    }),
                ),
            ),
            (
                "external_api".to_string(),
                Question::noul(
                    "Is the function in `{root}.function` part of a public API surface \
                     intended for consumers outside this repository, given \
                     `{root}.project_markers`?",
                    Some(NoulCriteria {
                        yes: "It is exported for other codebases to call, and no caller inside \
                              this repository is expected.".into(),
                        no: "It is internal to this repository, whether or not the language \
                             marks it public.".into(),
                    }),
                ),
            ),
            (
                "resolver_missed_a_call".to_string(),
                Question::noul(
                    "The engine that produced this finding resolves calls statically and \
                     cannot follow every one, particularly across crate, package, or module \
                     boundaries. Given `{root}.occurrences`, which lists where this name \
                     appears \
                     elsewhere in the repository, does a real call site exist that the \
                     engine failed to connect to this function?",
                    Some(NoulCriteria {
                        yes: "The occurrence list shows the name used somewhere that reads \
                              like a call, so a caller exists and the engine simply could \
                              not resolve it."
                            .into(),
                        no: "The occurrence list shows no use that reads like a call. It is \
                             empty, or its entries are only the declaration, an import, a \
                             comment, or a different symbol that happens to share the name."
                            .into(),
                    }),
                ),
            ),
            (
                "explanation".to_string(),
                Question::choice(
                    "A static analyzer found no call to the function in `{root}.function` \
                     anywhere in this repository. `{root}.occurrences` lists where the \
                     name appears \
                     elsewhere, so a real call site there would mean the analyzer failed to \
                     resolve it rather than that the code is dead. Given \
                     `{root}.occurrences`, `{root}.in_test_context`, and \
                     `{root}.engine_confidence`, what best explains the \
                     absence of a detected call?",
                    [
                        ("framework_invoked", "A framework, container, or registry calls it without an explicit call site."),
                        ("test_support", "It exists to support tests."),
                        ("public_api", "It is exported for consumers outside this repository."),
                        ("entrypoint", "It is a program entry point, such as a main function or a CLI command handler."),
                        ("resolver_limitation", "A real call exists somewhere in this repository, but the analyzer could not resolve it, for example across a crate, package, or module boundary."),
                        ("genuinely_unused", "Nothing calls it and nothing is expected to. The code is dead."),
                        ("cannot_tell", "The available evidence does not settle the question."),
                    ],
                ),
            ),
        ]
        .into_iter()
        .collect()
    }

    fn judge(
        &self,
        finding: &Finding,
        answers: &BTreeMap<String, Answer>,
        cfg: &Thresholds,
    ) -> Judgment {
        DeadCodeJudge::judge(self, finding, answers, &cfg.dead_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DeadCodeThresholds;
    use crate::context::RepoContext;
    use crate::model::{Confidence, Finding, Severity, Span};
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use systemone_client::Answer;

    fn ctx() -> RepoContext {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/sample-repo");
        RepoContext::new(root).unwrap()
    }

    fn finding() -> Finding {
        let text = std::fs::read_to_string(ctx().root().join("src/lib.rs")).unwrap();
        let start = text.find("fn helper").unwrap();
        Finding {
            rule: "dead_code".into(),
            severity: Severity::Info,
            message: "possibly unused function `helper`".into(),
            span: Span {
                file: "src/lib.rs".into(),
                start_byte: start,
                end_byte: start + 9,
                start_line: 5,
            },
            confidence: Confidence::Likely,
            extra: serde_json::Map::new(),
        }
    }

    /// A finding the engine did NOT hedge on: private and unreached, so it
    /// arrives at full strength. Shapes 2 and 3 of the ground truth.
    fn certain_finding() -> Finding {
        Finding {
            severity: Severity::Warning,
            confidence: Confidence::Certain,
            message: "unused function `helper`".into(),
            ..finding()
        }
    }

    fn noul(v: f64) -> Answer {
        Answer::Noul { noul: v }
    }

    fn choice(pick: &str, conf: f64) -> Answer {
        Answer::Choice {
            choice: pick.into(),
            probabilities: BTreeMap::new(),
            confidence: conf,
        }
    }

    fn answers(
        fw: f64,
        test: f64,
        api: f64,
        resolver: f64,
        pick: &str,
        conf: f64,
    ) -> BTreeMap<String, Answer> {
        [
            ("framework_invoked".to_string(), noul(fw)),
            ("test_only".to_string(), noul(test)),
            ("external_api".to_string(), noul(api)),
            ("resolver_missed_a_call".to_string(), noul(resolver)),
            ("explanation".to_string(), choice(pick, conf)),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn extracts_the_function_name_from_either_message_form() {
        assert_eq!(
            function_name("unused function `neverCalled`"),
            Some("neverCalled")
        );
        assert_eq!(
            function_name("possibly unused function `helper`"),
            Some("helper")
        );
        assert_eq!(function_name("no backticks here"), None);
        assert_eq!(function_name("one backtick `here"), None);
        // An empty pair is not a name. Some("") would put a nameless
        // function in the state, indistinguishable from the absent case.
        assert_eq!(function_name("unused function ``"), None);
    }

    #[test]
    fn state_carries_the_occurrence_evidence() {
        let s = DeadCodeJudge.state_for(&finding(), &ctx());
        // `helper` is called once from `used`, so the state must show a
        // caller even though the call graph did not connect it.
        assert!(s["occurrences"]["in_this_file"].as_u64().unwrap() >= 1);
        assert!(s["occurrences"]["repo_wide"].as_u64().unwrap() >= 1);
        let samples = s["occurrences"]["sample_sites"].as_array().unwrap();
        assert!(
            samples
                .iter()
                .any(|v| v.as_str().unwrap().contains("helper() + 1")),
            "got {samples:?}"
        );
        assert_eq!(s["in_test_context"], false);
        assert_eq!(s["engine_confidence"], "likely");
    }

    /// F2: samples used to be the first eight by path order. On the real
    /// repo that meant a common name's eight samples contained no call at
    /// all, steering the resolver question to the answer that permits
    /// Strengthen.
    #[test]
    fn sample_sites_put_call_like_lines_first() {
        assert!(looks_like_call("    helper() + 1", "helper"));
        assert!(looks_like_call("    self.helper()", "helper"));
        assert!(looks_like_call("    crate::helper", "helper"));
        assert!(!looks_like_call("    span: helper,", "helper"));
        assert!(!looks_like_call("    // helper is gone", "helper"));

        let s = DeadCodeJudge.state_for(&finding(), &ctx());
        let samples = s["occurrences"]["sample_sites"].as_array().unwrap();
        assert!(
            samples[0].as_str().unwrap().contains("helper() + 1"),
            "the call site must rank first, got {samples:?}"
        );
    }

    #[test]
    fn state_carries_the_function_source_and_its_identity() {
        let s = DeadCodeJudge.state_for(&finding(), &ctx());
        assert_eq!(s["function"]["name"], "helper");
        assert_eq!(s["function"]["language"], "rust");
        assert_eq!(s["function"]["file"], "src/lib.rs");
        assert!(
            s["function"]["source"]
                .as_str()
                .unwrap()
                .contains("fn helper"),
            "got {}",
            s["function"]["source"]
        );
    }

    /// 1-based line number of the first line starting with `needle`.
    fn line_of(ctx: &RepoContext, file: &str, needle: &str) -> usize {
        ctx.file_text(file)
            .unwrap()
            .lines()
            .position(|l| l.trim_start().starts_with(needle))
            .expect("fixture line exists")
            + 1
    }

    #[test]
    fn attributes_capture_a_rust_attribute() {
        let ctx = ctx();
        let line = line_of(&ctx, "src/lib.rs", "fn orphan");
        assert_eq!(
            attributes_above(&ctx, "src/lib.rs", line),
            vec!["#[allow(dead_code)]".to_string()]
        );
    }

    /// The bug this guards: `#` starts a comment in Python and Ruby, so a
    /// prefix check that accepts a bare `#` feeds every comment above a
    /// function to the model as an "attribute", poisoning the strongest
    /// framework signal in the state.
    #[test]
    fn attributes_keep_a_decorator_and_drop_python_comments() {
        let ctx = ctx();
        let line = line_of(&ctx, "src/app.py", "def health");
        let attrs = attributes_above(&ctx, "src/app.py", line);
        assert_eq!(attrs, vec!["@app.route(\"/health\")".to_string()]);
    }

    #[test]
    fn attributes_are_empty_when_only_prose_sits_above() {
        let ctx = ctx();
        let line = line_of(&ctx, "src/app.py", "def untouched");
        assert!(attributes_above(&ctx, "src/app.py", line).is_empty());
    }

    /// Every question must name its state root. Without the placeholder, a
    /// batched request sends byte-identical text for every slot and nothing
    /// on the wire says which finding a question is about.
    #[test]
    fn every_question_references_its_state_root() {
        for (name, q) in DeadCodeJudge.questions() {
            let v = serde_json::to_value(&q).unwrap();
            let text = v["instructions"].as_str().unwrap();
            assert!(
                text.contains("{root}."),
                "question `{name}` names no state root: {text}"
            );
            assert!(
                !text.contains("`function`"),
                "question `{name}` still names a bare state key: {text}"
            );
        }
    }

    #[test]
    fn asks_five_questions_with_canonical_names() {
        let q = DeadCodeJudge.questions();
        let mut names: Vec<&str> = q.keys().map(|s| s.as_str()).collect();
        names.sort();
        assert_eq!(
            names,
            [
                "explanation",
                "external_api",
                "framework_invoked",
                "resolver_missed_a_call",
                "test_only",
            ]
        );
    }

    #[test]
    fn a_confident_framework_hit_dismisses() {
        let j = DeadCodeJudge.judge(
            &finding(),
            &answers(0.91, 0.02, 0.1, 0.05, "framework_invoked", 0.9),
            &DeadCodeThresholds::default(),
        );
        assert_eq!(j.verdict, Verdict::Dismiss);
        assert!(j.reason.contains("framework"), "got {}", j.reason);
    }

    #[test]
    fn a_confident_test_helper_dismisses() {
        let j = DeadCodeJudge.judge(
            &finding(),
            &answers(0.05, 0.88, 0.1, 0.05, "test_support", 0.9),
            &DeadCodeThresholds::default(),
        );
        assert_eq!(j.verdict, Verdict::Dismiss);
        assert!(j.reason.contains("test"), "got {}", j.reason);
    }

    #[test]
    fn all_signals_low_plus_a_confident_explanation_strengthens() {
        // Only where the engine itself was Certain. The hedged case is
        // covered by a_finding_the_engine_hedged_on_is_never_strengthened.
        let j = DeadCodeJudge.judge(
            &certain_finding(),
            &answers(0.04, 0.03, 0.05, 0.02, "genuinely_unused", 0.85),
            &DeadCodeThresholds::default(),
        );
        assert_eq!(j.verdict, Verdict::Strengthen);
    }

    #[test]
    fn low_signals_with_an_unsure_explanation_keeps() {
        let j = DeadCodeJudge.judge(
            &finding(),
            &answers(0.04, 0.03, 0.05, 0.02, "genuinely_unused", 0.4),
            &DeadCodeThresholds::default(),
        );
        assert_eq!(j.verdict, Verdict::Keep);
    }

    #[test]
    fn a_confident_non_unused_explanation_never_strengthens() {
        // Guards the genuinely_unused conjunct. Mutating the match to
        // Some((_, conf)) makes this the only failing test in the suite.
        let j = DeadCodeJudge.judge(
            &certain_finding(),
            &answers(0.04, 0.03, 0.05, 0.02, "cannot_tell", 0.90),
            &DeadCodeThresholds::default(),
        );
        assert_eq!(j.verdict, Verdict::Keep);
    }

    #[test]
    fn a_finding_the_engine_hedged_on_is_never_strengthened() {
        // The engine reports Likely when it could not confirm reachability.
        // Promoting that population to Certain inverts its own epistemics,
        // and this is the shape every cross-crate false positive takes.
        let j = DeadCodeJudge.judge(
            &finding(),
            &answers(0.04, 0.03, 0.05, 0.02, "genuinely_unused", 0.85),
            &DeadCodeThresholds::default(),
        );
        assert_eq!(j.verdict, Verdict::Keep);
        assert!(
            j.reason.contains("engine could not confirm"),
            "got {}",
            j.reason
        );
    }

    #[test]
    fn a_confident_entrypoint_is_dismissed() {
        let j = DeadCodeJudge.judge(
            &certain_finding(),
            &answers(0.04, 0.03, 0.05, 0.02, "entrypoint", 0.90),
            &DeadCodeThresholds::default(),
        );
        assert_eq!(j.verdict, Verdict::Dismiss);
    }

    #[test]
    fn public_api_dismisses_at_the_info_floor_and_weakens_above_it() {
        let cfg = DeadCodeThresholds::default();
        let a = answers(0.05, 0.05, 0.9, 0.05, "public_api", 0.9);
        // Already at Info: Weaken would write back identical values, so the
        // only lever with an observable effect is Dismiss.
        assert_eq!(
            DeadCodeJudge.judge(&finding(), &a, &cfg).verdict,
            Verdict::Dismiss
        );
        // At Warning/Certain the step down is real.
        assert_eq!(
            DeadCodeJudge.judge(&certain_finding(), &a, &cfg).verdict,
            Verdict::Weaken
        );
    }

    #[test]
    fn thresholds_are_inclusive_at_the_boundary() {
        // Pins >= against >. Nothing else in the suite distinguishes them.
        let cfg = DeadCodeThresholds::default();
        let at = answers(cfg.dismiss_at, 0.0, 0.0, 0.0, "cannot_tell", 0.0);
        assert_eq!(
            DeadCodeJudge.judge(&finding(), &at, &cfg).verdict,
            Verdict::Dismiss
        );
        let just_below = answers(cfg.dismiss_at - 0.001, 0.0, 0.0, 0.0, "cannot_tell", 0.0);
        assert_ne!(
            DeadCodeJudge.judge(&finding(), &just_below, &cfg).verdict,
            Verdict::Dismiss
        );
    }

    #[test]
    fn a_resolver_hit_dismisses_even_with_every_other_signal_low() {
        // The cross-crate shape: nothing special about the function, but the
        // name appears at a real call site the engine could not connect.
        let j = DeadCodeJudge.judge(
            &certain_finding(),
            &answers(0.04, 0.03, 0.05, 0.92, "resolver_limitation", 0.9),
            &DeadCodeThresholds::default(),
        );
        assert_eq!(j.verdict, Verdict::Dismiss);
        assert!(j.reason.contains("could not resolve"), "got {}", j.reason);
    }

    #[test]
    fn an_absent_resolver_answer_neither_dismisses_nor_strengthens() {
        // A cached answer predating this question carries no resolver
        // evidence. Treating absence as low would arm Strengthen on exactly
        // the population the question exists to protect; treating it as high
        // would dismiss everything.
        let mut a = answers(0.04, 0.03, 0.05, 0.02, "genuinely_unused", 0.85);
        a.remove("resolver_missed_a_call");
        let j = DeadCodeJudge.judge(&certain_finding(), &a, &DeadCodeThresholds::default());
        assert_eq!(j.verdict, Verdict::Keep);
    }

    #[test]
    fn a_middling_answer_keeps() {
        let j = DeadCodeJudge.judge(
            &finding(),
            &answers(0.5, 0.4, 0.3, 0.3, "cannot_tell", 0.4),
            &DeadCodeThresholds::default(),
        );
        assert_eq!(j.verdict, Verdict::Keep);
    }

    #[test]
    fn missing_answers_keep_rather_than_guessing() {
        let j = DeadCodeJudge.judge(&finding(), &BTreeMap::new(), &DeadCodeThresholds::default());
        assert_eq!(j.verdict, Verdict::Keep);
    }

    #[test]
    fn every_raw_probability_is_recorded_on_the_judgment() {
        let j = DeadCodeJudge.judge(
            &finding(),
            &answers(0.91, 0.02, 0.1, 0.05, "framework_invoked", 0.9),
            &DeadCodeThresholds::default(),
        );
        assert_eq!(j.answers["framework_invoked"], 0.91);
        assert_eq!(j.answers["explanation"]["choice"], "framework_invoked");
    }
}
