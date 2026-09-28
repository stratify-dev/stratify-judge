use stratify_judge_core::model::{Confidence, Finding, Report, Severity};

/// A finding is shown when its confidence reaches the threshold, or when
/// the caller asked to see everything.
pub fn visible(f: &Finding, min: Confidence, show_dismissed: bool) -> bool {
    show_dismissed || f.confidence >= min
}

fn label(s: Severity) -> &'static str {
    match s {
        Severity::Info => "info",
        Severity::Warning => "warn",
        Severity::Error => "error",
    }
}

fn reason_of(f: &Finding) -> Option<&str> {
    f.extra.get("judgment")?.get("reason")?.as_str()
}

/// Human output keeps the engine's line shape and adds a reason under
/// anything the model moved, then one summary line.
pub fn render(report: &Report, min: Confidence, show_dismissed: bool) -> String {
    let mut out = String::new();
    let mut shown = 0usize;

    for f in &report.findings {
        if !visible(f, min, show_dismissed) {
            continue;
        }
        shown += 1;
        out.push_str(&format!(
            "{:<5} {}:{}  {}\n",
            label(f.severity),
            f.span.file,
            f.span.start_line,
            f.message
        ));
        if let Some(r) = reason_of(f) {
            out.push_str(&format!("      jev: {r}\n"));
        }
    }

    let total = report.findings.len();
    // How many the display threshold hid, not how many the model judged
    // Dismiss: those are different counts, and "dismissed" claiming the
    // latter while computing the former read as 0 even when findings were
    // genuinely dismissed, under --show-dismissed.
    let hidden = total - shown;
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!("{total} findings, {shown} shown, {hidden} hidden"));
    if hidden > 0 && !show_dismissed {
        out.push_str(". Re-run with --show-dismissed to see them");
    }
    out.push_str(".\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use stratify_judge_core::model::{Confidence, Finding, Report, Severity, Span};

    fn finding(conf: Confidence, judged: bool) -> Finding {
        let mut extra = serde_json::Map::new();
        if judged {
            extra.insert(
                "judgment".into(),
                serde_json::json!({
                    "verdict": "dismiss",
                    "reason": "reached by a framework (0.91)"
                }),
            );
        }
        Finding {
            rule: "dead_code".into(),
            severity: Severity::Info,
            message: "possibly unused function `helper`".into(),
            span: Span {
                file: "src/a.rs".into(),
                start_byte: 0,
                end_byte: 1,
                start_line: 6,
            },
            confidence: conf,
            extra,
        }
    }

    fn report(findings: Vec<Finding>) -> Report {
        Report {
            schema_version: 1,
            findings,
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn dismissed_findings_are_hidden_at_the_default_threshold() {
        let r = report(vec![
            finding(Confidence::Unknown, true),
            finding(Confidence::Likely, false),
        ]);
        let out = render(&r, Confidence::Likely, false);
        assert_eq!(out.matches("possibly unused").count(), 1, "got:\n{out}");
    }

    #[test]
    fn show_dismissed_brings_them_back() {
        let r = report(vec![finding(Confidence::Unknown, true)]);
        let out = render(&r, Confidence::Likely, true);
        assert!(out.contains("possibly unused"), "got:\n{out}");
    }

    #[test]
    fn a_judged_finding_prints_its_reason_underneath() {
        let r = report(vec![finding(Confidence::Likely, true)]);
        let out = render(&r, Confidence::Unknown, false);
        assert!(
            out.contains("jev: reached by a framework (0.91)"),
            "got:\n{out}"
        );
    }

    /// The behavior the whole product exists for: a finding the model
    /// dismissed must not fail a build, even at a severity that otherwise
    /// would. Only confidence separates it from one that should.
    #[test]
    fn visibility_is_what_separates_a_dismissed_warning_from_a_live_one() {
        let dismissed = Finding {
            severity: Severity::Warning,
            confidence: Confidence::Unknown,
            ..finding(Confidence::Unknown, true)
        };
        let live = Finding {
            severity: Severity::Warning,
            confidence: Confidence::Certain,
            ..finding(Confidence::Certain, false)
        };
        // Same severity, opposite outcomes, decided by confidence alone.
        assert!(!visible(&dismissed, Confidence::Likely, false));
        assert!(visible(&live, Confidence::Likely, false));
        // And --show-dismissed brings the dismissed one back into view.
        assert!(visible(&dismissed, Confidence::Likely, true));
    }

    /// M3: the count is how many findings the display threshold hid, not
    /// how many the model judged Dismiss. "dismissed" claimed the latter
    /// while computing the former, and read as 0 dismissed under
    /// --show-dismissed even when findings were genuinely dismissed.
    #[test]
    fn the_summary_names_shown_and_hidden_counts() {
        let r = report(vec![
            finding(Confidence::Unknown, true),
            finding(Confidence::Unknown, true),
            finding(Confidence::Likely, false),
        ]);
        let out = render(&r, Confidence::Likely, false);
        assert!(out.contains("3 findings, 1 shown, 2 hidden"), "got:\n{out}");
    }

    #[test]
    fn an_empty_report_says_so_without_a_dismissed_hint() {
        let out = render(&report(vec![]), Confidence::Likely, false);
        assert!(out.contains("0 findings"), "got:\n{out}");
        assert!(!out.contains("--show-dismissed"), "got:\n{out}");
    }
}
