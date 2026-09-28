use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One answer. Variants are tried in order, so the most constrained shape
/// (Noul, which has only `noul`) comes first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Answer {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Score {
        score: f64,
        legend: BTreeMap<String, String>,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
}

impl Answer {
    /// Probability of yes, 0 to 1. A Noul carries no separate confidence:
    /// 0.5 means yes and no are equally likely, not "medium intensity".
    pub fn as_noul(&self) -> Option<f64> {
        match self {
            Answer::Noul { noul } => Some(*noul),
            _ => None,
        }
    }

    /// The selected option and the distribution's confidence.
    pub fn as_choice(&self) -> Option<(&str, f64)> {
        match self {
            Answer::Choice {
                choice, confidence, ..
            } => Some((choice.as_str(), *confidence)),
            _ => None,
        }
    }

    /// The score (which may fall between levels) and its confidence.
    pub fn as_score(&self) -> Option<(f64, f64)> {
        match self {
            Answer::Score {
                score, confidence, ..
            } => Some((*score, *confidence)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemOneResponse {
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    pub usage: Usage,
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = r#"{
      "model": "jev-1.13.0",
      "answers": {
        "framework_invoked": { "noul": 0.91 },
        "explanation": {
          "choice": "framework_invoked",
          "probabilities": { "framework_invoked": 0.88, "cannot_tell": 0.12 },
          "confidence": 0.88
        },
        "cognitive_load": {
          "score": 2.4,
          "legend": { "0": "flat", "3": "tangled" },
          "probabilities": { "0": 0.1, "3": 0.5 },
          "confidence": 0.62
        }
      },
      "usage": { "input_tokens": 412, "output_tokens": 0 }
    }"#;

    #[test]
    fn parses_all_three_answer_shapes() {
        let r: SystemOneResponse = serde_json::from_str(BODY).unwrap();
        assert_eq!(r.model, "jev-1.13.0");
        assert_eq!(r.usage.input_tokens, 412);
        assert_eq!(r.answers["framework_invoked"].as_noul(), Some(0.91));
        assert_eq!(
            r.answers["explanation"].as_choice(),
            Some(("framework_invoked", 0.88))
        );
        assert_eq!(r.answers["cognitive_load"].as_score(), Some((2.4, 0.62)));
    }

    /// Task 6 writes answers to a disk cache as JSON and reads them back,
    /// so the round trip is a contract another task depends on. Without
    /// this test, a later change to the enum that breaks serialization
    /// passes every test in this crate and surfaces as a confusing cache
    /// failure instead of an answer-shape failure.
    #[test]
    fn every_answer_variant_survives_a_json_round_trip() {
        let cases = vec![
            Answer::Noul { noul: 0.91 },
            Answer::Choice {
                choice: "framework_invoked".into(),
                probabilities: [("framework_invoked".to_string(), 0.88)]
                    .into_iter()
                    .collect(),
                confidence: 0.88,
            },
            Answer::Score {
                score: 2.4,
                legend: [("0".to_string(), "flat".to_string())]
                    .into_iter()
                    .collect(),
                probabilities: [("0".to_string(), 0.1)].into_iter().collect(),
                confidence: 0.62,
            },
        ];
        for original in cases {
            let text = serde_json::to_string(&original).unwrap();
            let back: Answer = serde_json::from_str(&text).unwrap();
            assert_eq!(back, original, "round trip changed the answer: {text}");
        }
    }

    #[test]
    fn accessors_return_none_for_the_wrong_shape() {
        let r: SystemOneResponse = serde_json::from_str(BODY).unwrap();
        assert_eq!(r.answers["framework_invoked"].as_choice(), None);
        assert_eq!(r.answers["explanation"].as_noul(), None);
        assert_eq!(r.answers["cognitive_load"].as_noul(), None);
    }
}
