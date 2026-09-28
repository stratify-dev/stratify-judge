use serde::{Deserialize, Serialize};

/// Ordering matters: Info < Warning < Error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    /// One step down. Info is the floor and stays Info.
    pub fn step_down(self) -> Severity {
        match self {
            Severity::Error => Severity::Warning,
            Severity::Warning => Severity::Info,
            Severity::Info => Severity::Info,
        }
    }
}

/// Ordering matters: Unknown < Likely < Certain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Unknown,
    Likely,
    Certain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub file: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
}

/// One engine finding. `extra` round-trips any field a newer engine adds,
/// including the `judgment` object this tool writes back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    pub confidence: Confidence,
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub findings: Vec<Finding>,
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Report {
    /// Schema this tool understands. A higher value is passed through untouched.
    pub const KNOWN_SCHEMA_VERSION: u32 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "schema_version": 1,
      "findings": [{
        "rule": "dead_code",
        "severity": "info",
        "message": "possibly unused function `helper`",
        "span": {"file":"src/a.rs","start_byte":0,"end_byte":10,"start_line":3},
        "confidence": "likely",
        "future_field": 42
      }]
    }"#;

    #[test]
    fn parses_an_engine_report() {
        let r: Report = serde_json::from_str(SAMPLE).unwrap();
        assert_eq!(r.schema_version, 1);
        assert_eq!(r.findings[0].rule, "dead_code");
        assert_eq!(r.findings[0].severity, Severity::Info);
        assert_eq!(r.findings[0].confidence, Confidence::Likely);
        assert_eq!(r.findings[0].span.start_line, 3);
    }

    #[test]
    fn round_trip_preserves_unknown_fields() {
        let r: Report = serde_json::from_str(SAMPLE).unwrap();
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["findings"][0]["future_field"], 42);
    }

    #[test]
    fn severity_and_confidence_order_low_to_high() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
        assert!(Confidence::Unknown < Confidence::Likely);
        assert!(Confidence::Likely < Confidence::Certain);
    }
}
