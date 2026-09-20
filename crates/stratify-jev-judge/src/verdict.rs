use crate::model::{Confidence, Finding, Severity};
use serde::{Deserialize, Serialize};

/// What the model concluded about one finding. Deliberately small: four
/// states are auditable by a reviewer and scoreable against a label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Dismiss,
    Weaken,
    Keep,
    Strengthen,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Judgment {
    pub judge: String,
    pub verdict: Verdict,
    pub reason: String,
    /// Raw probabilities, keyed by canonical question name.
    pub answers: serde_json::Map<String, serde_json::Value>,
    pub model: String,
}

/// Move a finding along the confidence ladder and record why. Nothing is
/// ever removed: a dismissed finding stays in the report and falls below
/// the display threshold instead.
pub fn apply(finding: &mut Finding, judgment: &Judgment, full_strength: Severity) {
    let original = serde_json::json!({
        "severity": finding.severity,
        "confidence": finding.confidence,
    });

    match judgment.verdict {
        Verdict::Dismiss => {
            finding.confidence = Confidence::Unknown;
            finding.severity = Severity::Info;
        }
        Verdict::Weaken => {
            finding.confidence = Confidence::Likely;
            finding.severity = finding.severity.step_down();
        }
        Verdict::Keep => {}
        Verdict::Strengthen => {
            finding.confidence = Confidence::Certain;
            finding.severity = full_strength;
        }
    }

    finding.extra.insert(
        "judgment".to_string(),
        serde_json::json!({
            "judge": judgment.judge,
            "verdict": judgment.verdict,
            "original": original,
            "answers": judgment.answers,
            "reason": judgment.reason,
            "model": judgment.model,
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Confidence, Finding, Severity, Span};

    fn finding(sev: Severity, conf: Confidence) -> Finding {
        Finding {
            rule: "dead_code".into(),
            severity: sev,
            message: "possibly unused function `helper`".into(),
            span: Span {
                file: "src/a.rs".into(),
                start_byte: 0,
                end_byte: 1,
                start_line: 1,
            },
            confidence: conf,
            extra: serde_json::Map::new(),
        }
    }

    fn judgment(v: Verdict) -> Judgment {
        Judgment {
            judge: "dead_code".into(),
            verdict: v,
            reason: "reached by a framework".into(),
            answers: serde_json::Map::new(),
            model: "jev-1.13.0".into(),
        }
    }

    #[test]
    fn dismiss_drops_to_unknown_and_info() {
        let mut f = finding(Severity::Warning, Confidence::Certain);
        apply(&mut f, &judgment(Verdict::Dismiss), Severity::Warning);
        assert_eq!(f.confidence, Confidence::Unknown);
        assert_eq!(f.severity, Severity::Info);
    }

    #[test]
    fn weaken_steps_severity_down_once() {
        let mut f = finding(Severity::Error, Confidence::Certain);
        apply(&mut f, &judgment(Verdict::Weaken), Severity::Warning);
        assert_eq!(f.confidence, Confidence::Likely);
        assert_eq!(f.severity, Severity::Warning);
    }

    #[test]
    fn weaken_leaves_info_at_info_because_info_is_the_floor() {
        let mut f = finding(Severity::Info, Confidence::Certain);
        apply(&mut f, &judgment(Verdict::Weaken), Severity::Warning);
        assert_eq!(f.severity, Severity::Info);
        assert_eq!(f.confidence, Confidence::Likely);
    }

    #[test]
    fn keep_changes_nothing_but_still_records_the_judgment() {
        let mut f = finding(Severity::Info, Confidence::Likely);
        apply(&mut f, &judgment(Verdict::Keep), Severity::Warning);
        assert_eq!(f.severity, Severity::Info);
        assert_eq!(f.confidence, Confidence::Likely);
        assert_eq!(f.extra["judgment"]["verdict"], "keep");
    }

    #[test]
    fn strengthen_restores_the_rules_full_strength() {
        let mut f = finding(Severity::Info, Confidence::Likely);
        apply(&mut f, &judgment(Verdict::Strengthen), Severity::Warning);
        assert_eq!(f.confidence, Confidence::Certain);
        assert_eq!(f.severity, Severity::Warning);
    }

    #[test]
    fn the_original_severity_and_confidence_are_always_recorded() {
        let mut f = finding(Severity::Warning, Confidence::Certain);
        apply(&mut f, &judgment(Verdict::Dismiss), Severity::Warning);
        assert_eq!(f.extra["judgment"]["original"]["severity"], "warning");
        assert_eq!(f.extra["judgment"]["original"]["confidence"], "certain");
        assert_eq!(f.extra["judgment"]["reason"], "reached by a framework");
        assert_eq!(f.extra["judgment"]["model"], "jev-1.13.0");
    }
}
