use crate::config::{DeadCodeThresholds, Thresholds};
use crate::context::{language_of, RepoContext};
use crate::judges::Judge;
use crate::model::{Finding, Severity};
use crate::verdict::{Judgment, Verdict};
use jev_client::{Answer, NoulCriteria, Question};
use std::collections::BTreeMap;

pub struct DeadCodeJudge;

/// The engine writes "unused function `name`" or "possibly unused
/// function `name`". Pull the backticked name out of either.
pub fn function_name(message: &str) -> Option<&str> {
    let start = message.find('`')? + 1;
    let rest = &message[start..];
    let end = rest.find('`')?;
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
    }
    if let Ok(t) = std::fs::read_to_string(root.join("package.json")) {
        if t.contains("\"private\"") && t.contains("true") {
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
        answers: &BTreeMap<String, Answer>,
        cfg: &DeadCodeThresholds,
    ) -> Judgment {
        let fw = answers.get("framework_invoked").and_then(Answer::as_noul);
        let test = answers.get("test_only").and_then(Answer::as_noul);
        let api = answers.get("external_api").and_then(Answer::as_noul);
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
        if let Some((pick, conf)) = explanation {
            raw.insert(
                "explanation".into(),
                serde_json::json!({ "choice": pick, "confidence": conf }),
            );
        }

        let (verdict, reason) = decide(fw, test, api, explanation, cfg);

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
fn decide(
    fw: Option<f64>,
    test: Option<f64>,
    api: Option<f64>,
    explanation: Option<(&str, f64)>,
    cfg: &DeadCodeThresholds,
) -> (Verdict, String) {
    // A missing answer never produces an action. Absence is not evidence.
    let (Some(fw), Some(test), Some(api)) = (fw, test, api) else {
        return (Verdict::Keep, "no usable answer, left as the engine found it".into());
    };

    if fw >= cfg.dismiss_at {
        return (Verdict::Dismiss, format!("reached by a framework ({fw:.2})"));
    }
    if test >= cfg.dismiss_at {
        return (Verdict::Dismiss, format!("test-only helper ({test:.2})"));
    }
    if api >= cfg.api_at {
        return (
            Verdict::Weaken,
            format!("public API surface for outside consumers ({api:.2})"),
        );
    }
    if fw < cfg.low_at && test < cfg.low_at && api < cfg.low_at {
        if let Some(("genuinely_unused", conf)) = explanation {
            if conf >= cfg.explanation_at {
                return (
                    Verdict::Strengthen,
                    format!("no caller and no hidden entry point ({conf:.2})"),
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
        1
    }

    fn full_strength(&self) -> Severity {
        Severity::Warning
    }

    fn state_for(&self, finding: &Finding, ctx: &RepoContext) -> serde_json::Value {
        let file = finding.span.file.as_str();
        serde_json::json!({
            "function": {
                "name": function_name(&finding.message).unwrap_or_default(),
                "file": file,
                "line": finding.span.start_line,
                "language": language_of(file),
                "source": ctx.function_source(&finding.span).unwrap_or_default(),
                "attributes": attributes_above(ctx, file, finding.span.start_line),
            },
            "file_imports": imports_of(ctx, file),
            "project_markers": project_markers(ctx),
            "engine_message": finding.message,
        })
    }

    fn questions(&self) -> BTreeMap<String, Question> {
        [
            (
                "framework_invoked".to_string(),
                Question::noul(
                    "Would a framework, dependency-injection container, reflection call, \
                     route table, serializer, or plugin registry invoke the function in \
                     `function` without any explicit call to it appearing in source?",
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
                    "Does the function in `function` exist only to support tests, such as a \
                     fixture builder, a test helper, or a assertion utility?",
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
                    "Is the function in `function` part of a public API surface intended for \
                     consumers outside this repository, given `project_markers`?",
                    Some(NoulCriteria {
                        yes: "It is exported for other codebases to call, and no caller inside \
                              this repository is expected.".into(),
                        no: "It is internal to this repository, whether or not the language \
                             marks it public.".into(),
                    }),
                ),
            ),
            (
                "explanation".to_string(),
                Question::choice(
                    "Nothing in this repository calls the function in `function`. What best \
                     explains why?",
                    [
                        ("framework_invoked", "A framework, container, or registry calls it without an explicit call site."),
                        ("test_support", "It exists to support tests."),
                        ("public_api", "It is exported for consumers outside this repository."),
                        ("entrypoint", "It is a program entry point, such as a main function or a CLI command handler."),
                        ("genuinely_unused", "Nothing calls it and nothing is expected to. The code is dead."),
                        ("cannot_tell", "The available evidence does not settle the question."),
                    ],
                ),
            ),
        ]
        .into_iter()
        .collect()
    }

    fn judge(&self, answers: &BTreeMap<String, Answer>, cfg: &Thresholds) -> Judgment {
        DeadCodeJudge::judge(self, answers, &cfg.dead_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DeadCodeThresholds;
    use crate::context::RepoContext;
    use crate::model::{Confidence, Finding, Severity, Span};
    use jev_client::Answer;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn ctx() -> RepoContext {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/sample-repo");
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

    fn answers(fw: f64, test: f64, api: f64, pick: &str, conf: f64) -> BTreeMap<String, Answer> {
        [
            ("framework_invoked".to_string(), noul(fw)),
            ("test_only".to_string(), noul(test)),
            ("external_api".to_string(), noul(api)),
            ("explanation".to_string(), choice(pick, conf)),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn extracts_the_function_name_from_either_message_form() {
        assert_eq!(function_name("unused function `neverCalled`"), Some("neverCalled"));
        assert_eq!(function_name("possibly unused function `helper`"), Some("helper"));
        assert_eq!(function_name("no backticks here"), None);
    }

    #[test]
    fn state_carries_the_function_source_and_its_identity() {
        let s = DeadCodeJudge.state_for(&finding(), &ctx());
        assert_eq!(s["function"]["name"], "helper");
        assert_eq!(s["function"]["language"], "rust");
        assert_eq!(s["function"]["file"], "src/lib.rs");
        assert!(
            s["function"]["source"].as_str().unwrap().contains("fn helper"),
            "got {}", s["function"]["source"]
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

    #[test]
    fn asks_four_questions_with_canonical_names() {
        let q = DeadCodeJudge.questions();
        let mut names: Vec<&str> = q.keys().map(|s| s.as_str()).collect();
        names.sort();
        assert_eq!(names, ["explanation", "external_api", "framework_invoked", "test_only"]);
    }

    #[test]
    fn a_confident_framework_hit_dismisses() {
        let j = DeadCodeJudge.judge(&answers(0.91, 0.02, 0.1, "framework_invoked", 0.9), &DeadCodeThresholds::default());
        assert_eq!(j.verdict, Verdict::Dismiss);
        assert!(j.reason.contains("framework"), "got {}", j.reason);
    }

    #[test]
    fn a_confident_test_helper_dismisses() {
        let j = DeadCodeJudge.judge(&answers(0.05, 0.88, 0.1, "test_support", 0.9), &DeadCodeThresholds::default());
        assert_eq!(j.verdict, Verdict::Dismiss);
        assert!(j.reason.contains("test"), "got {}", j.reason);
    }

    #[test]
    fn a_public_api_symbol_weakens_rather_than_dismissing() {
        let j = DeadCodeJudge.judge(&answers(0.05, 0.05, 0.9, "public_api", 0.9), &DeadCodeThresholds::default());
        assert_eq!(j.verdict, Verdict::Weaken);
    }

    #[test]
    fn all_signals_low_plus_a_confident_explanation_strengthens() {
        let j = DeadCodeJudge.judge(&answers(0.04, 0.03, 0.05, "genuinely_unused", 0.85), &DeadCodeThresholds::default());
        assert_eq!(j.verdict, Verdict::Strengthen);
    }

    #[test]
    fn low_signals_with_an_unsure_explanation_keeps() {
        let j = DeadCodeJudge.judge(&answers(0.04, 0.03, 0.05, "genuinely_unused", 0.4), &DeadCodeThresholds::default());
        assert_eq!(j.verdict, Verdict::Keep);
    }

    #[test]
    fn a_middling_answer_keeps() {
        let j = DeadCodeJudge.judge(&answers(0.5, 0.4, 0.3, "cannot_tell", 0.4), &DeadCodeThresholds::default());
        assert_eq!(j.verdict, Verdict::Keep);
    }

    #[test]
    fn missing_answers_keep_rather_than_guessing() {
        let j = DeadCodeJudge.judge(&BTreeMap::new(), &DeadCodeThresholds::default());
        assert_eq!(j.verdict, Verdict::Keep);
    }

    #[test]
    fn every_raw_probability_is_recorded_on_the_judgment() {
        let j = DeadCodeJudge.judge(&answers(0.91, 0.02, 0.1, "framework_invoked", 0.9), &DeadCodeThresholds::default());
        assert_eq!(j.answers["framework_invoked"], 0.91);
        assert_eq!(j.answers["explanation"]["choice"], "framework_invoked");
    }
}
