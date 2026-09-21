use stratify_jev_judge::model::Report;

/// Keeps the engine's findings shape so existing consumers still parse,
/// and never drops a finding: a dismissed one is present with its
/// confidence lowered and its judgment attached.
pub fn render(report: &Report, tool_version: &str) -> String {
    let mut v = serde_json::to_value(report).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(obj) = v.as_object_mut() {
        obj.insert(
            "judged_by".to_string(),
            serde_json::Value::String(format!("stratify-jev/{tool_version}")),
        );
    }
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use stratify_jev_judge::model::{Confidence, Finding, Report, Severity, Span};

    #[test]
    fn output_keeps_the_findings_shape_and_names_the_tool() {
        let r = Report {
            schema_version: 1,
            findings: vec![Finding {
                rule: "dead_code".into(),
                severity: Severity::Info,
                message: "possibly unused function `helper`".into(),
                span: Span {
                    file: "src/a.rs".into(),
                    start_byte: 0,
                    end_byte: 1,
                    start_line: 6,
                },
                confidence: Confidence::Unknown,
                extra: serde_json::Map::new(),
            }],
            extra: serde_json::Map::new(),
        };
        let v: serde_json::Value = serde_json::from_str(&render(&r, "0.1.0")).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["judged_by"], "stratify-jev/0.1.0");
        assert_eq!(v["findings"][0]["rule"], "dead_code");
        assert_eq!(v["findings"][0]["confidence"], "unknown");
    }

    /// The auditability guarantee. A dismissed finding leaves human output
    /// but must stay in the JSON with its judgment intact, or the model can
    /// hide something with no trace.
    #[test]
    fn a_dismissed_finding_survives_in_json_with_its_judgment() {
        let mut extra = serde_json::Map::new();
        extra.insert(
            "judgment".into(),
            serde_json::json!({
                "verdict": "dismiss",
                "reason": "reached by a framework (0.91)",
                "answers": { "framework_invoked": 0.91 },
                "original": { "severity": "warning", "confidence": "certain" }
            }),
        );
        let r = Report {
            schema_version: 1,
            findings: vec![Finding {
                rule: "dead_code".into(),
                severity: Severity::Info,
                message: "possibly unused function `helper`".into(),
                span: Span {
                    file: "src/a.rs".into(),
                    start_byte: 0,
                    end_byte: 1,
                    start_line: 6,
                },
                // Dismissed: below the default display threshold.
                confidence: Confidence::Unknown,
                extra,
            }],
            extra: serde_json::Map::new(),
        };
        let v: serde_json::Value = serde_json::from_str(&render(&r, "0.1.0")).unwrap();
        assert_eq!(v["findings"].as_array().unwrap().len(), 1, "never dropped");
        assert_eq!(v["findings"][0]["confidence"], "unknown");
        assert_eq!(v["findings"][0]["judgment"]["verdict"], "dismiss");
        assert_eq!(v["findings"][0]["judgment"]["answers"]["framework_invoked"], 0.91);
        assert_eq!(v["findings"][0]["judgment"]["original"]["severity"], "warning");
    }
}
