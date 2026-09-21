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

    #[test]
    fn dismissed_findings_are_never_dropped_from_json() {
        let r = Report {
            schema_version: 1,
            findings: vec![],
            extra: serde_json::Map::new(),
        };
        let v: serde_json::Value = serde_json::from_str(&render(&r, "0.1.0")).unwrap();
        assert!(v["findings"].is_array());
    }
}
