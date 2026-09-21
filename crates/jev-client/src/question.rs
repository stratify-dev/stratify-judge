use serde::Serialize;
use std::collections::BTreeMap;

/// Optional yes/no clarification on a Noul. The API names these keys
/// "true" and "false".
#[derive(Debug, Clone, Serialize)]
pub struct NoulCriteria {
    #[serde(rename = "true")]
    pub yes: String,
    #[serde(rename = "false")]
    pub no: String,
}

/// One typed question. Instructions are a JSON value so a caller can send
/// a plain string or a structured object, as the API allows.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    Noul {
        instructions: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    Choice {
        instructions: serde_json::Value,
        /// At most 255 options. Always include a no-match option.
        criteria: BTreeMap<String, String>,
    },
    Score {
        instructions: serde_json::Value,
        /// 2 to 10 levels, ordered low to high, each describing a situation.
        criteria: Vec<String>,
    },
}

impl Question {
    /// Replace every `{root}` placeholder in this question's instructions
    /// with `root`, returning the rendered question.
    ///
    /// A batched request puts each finding's state under its own key
    /// (`finding_0`, `finding_1`, ...) and namespaces question names by
    /// slot. Without this, every slot's questions would carry byte-identical
    /// text referring to a bare state key that does not exist at the top
    /// level, and nothing on the wire would bind a question to the finding
    /// it is about. The model would receive N indistinguishable copies of
    /// one question against N candidates.
    ///
    /// Criteria are deliberately not rendered: they describe what counts as
    /// an answer, never where the evidence lives.
    pub fn with_state_root(&self, root: &str) -> Question {
        fn render(v: &serde_json::Value, root: &str) -> serde_json::Value {
            match v {
                serde_json::Value::String(t) => {
                    serde_json::Value::String(t.replace("{root}", root))
                }
                serde_json::Value::Array(items) => {
                    serde_json::Value::Array(items.iter().map(|i| render(i, root)).collect())
                }
                serde_json::Value::Object(map) => serde_json::Value::Object(
                    map.iter()
                        .map(|(k, i)| (k.clone(), render(i, root)))
                        .collect(),
                ),
                other => other.clone(),
            }
        }
        match self {
            Question::Noul {
                instructions,
                criteria,
            } => Question::Noul {
                instructions: render(instructions, root),
                criteria: criteria.clone(),
            },
            Question::Choice {
                instructions,
                criteria,
            } => Question::Choice {
                instructions: render(instructions, root),
                criteria: criteria.clone(),
            },
            Question::Score {
                instructions,
                criteria,
            } => Question::Score {
                instructions: render(instructions, root),
                criteria: criteria.clone(),
            },
        }
    }

    pub fn noul(instructions: impl Into<String>, criteria: Option<NoulCriteria>) -> Question {
        Question::Noul {
            instructions: serde_json::Value::String(instructions.into()),
            criteria,
        }
    }

    pub fn choice<K, V>(
        instructions: impl Into<String>,
        options: impl IntoIterator<Item = (K, V)>,
    ) -> Question
    where
        K: Into<String>,
        V: Into<String>,
    {
        Question::Choice {
            instructions: serde_json::Value::String(instructions.into()),
            criteria: options
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    pub fn score<S: Into<String>>(
        instructions: impl Into<String>,
        levels: impl IntoIterator<Item = S>,
    ) -> Question {
        Question::Score {
            instructions: serde_json::Value::String(instructions.into()),
            criteria: levels.into_iter().map(Into::into).collect(),
        }
    }
}

/// One POST body for /v1/systemone.
#[derive(Debug, Clone, Serialize)]
pub struct SystemOneRequest {
    pub state: serde_json::Value,
    pub model: String,
    pub questions: BTreeMap<String, Question>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn noul_serializes_with_its_type_tag_and_criteria() {
        let q = Question::noul(
            "Would a framework invoke `finding_0.function`?",
            Some(NoulCriteria {
                yes: "A framework, container, or route table invokes it.".into(),
                no: "Only explicit calls in source would reach it.".into(),
            }),
        );
        let v = serde_json::to_value(&q).unwrap();
        assert_eq!(v["type"], "noul");
        assert_eq!(
            v["criteria"]["true"],
            "A framework, container, or route table invokes it."
        );
    }

    #[test]
    fn noul_without_criteria_omits_the_key() {
        let q = Question::noul("Is it test-only?", None);
        let v = serde_json::to_value(&q).unwrap();
        assert!(v.get("criteria").is_none(), "got {v}");
    }

    #[test]
    fn choice_keeps_its_option_keys() {
        let q = Question::choice(
            "Why does nothing call it?",
            [
                ("genuinely_unused", "No caller exists anywhere."),
                ("cannot_tell", "The evidence does not settle it."),
            ],
        );
        let v = serde_json::to_value(&q).unwrap();
        assert_eq!(v["type"], "choice");
        assert_eq!(
            v["criteria"]["cannot_tell"],
            "The evidence does not settle it."
        );
    }

    #[test]
    fn score_criteria_stay_ordered_low_to_high() {
        let q = Question::score(
            "How valuable is extraction?",
            ["Level 0", "Level 1", "Level 2"],
        );
        let v = serde_json::to_value(&q).unwrap();
        assert_eq!(v["criteria"][0], "Level 0");
        assert_eq!(v["criteria"][2], "Level 2");
    }

    #[test]
    fn with_state_root_renders_the_placeholder_in_instructions_only() {
        let q = Question::noul(
            "Would a framework invoke `{root}.function`, given `{root}.occurrences`?",
            Some(NoulCriteria {
                yes: "Something outside ordinary call syntax reaches it.".into(),
                no: "Only an ordinary call would reach it.".into(),
            }),
        );
        let v = serde_json::to_value(q.with_state_root("finding_3")).unwrap();
        let text = v["instructions"].as_str().unwrap();
        assert!(text.contains("finding_3.function"), "got {text}");
        assert!(text.contains("finding_3.occurrences"), "got {text}");
        assert!(!text.contains("{root}"), "placeholder must be gone: {text}");
        // Criteria say what counts as an answer, not where evidence lives.
        assert_eq!(
            v["criteria"]["true"],
            "Something outside ordinary call syntax reaches it."
        );
    }

    #[test]
    fn with_state_root_leaves_a_question_without_the_placeholder_alone() {
        let q = Question::choice("Why?", [("a", "A"), ("cannot_tell", "Unclear")]);
        let before = serde_json::to_value(&q).unwrap();
        let after = serde_json::to_value(q.with_state_root("finding_0")).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn request_carries_state_model_and_named_questions() {
        let req = SystemOneRequest {
            state: json!({ "finding_0": { "name": "helper" } }),
            model: "jev-latest".into(),
            questions: [("framework_invoked".to_string(), Question::noul("q", None))]
                .into_iter()
                .collect(),
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "jev-latest");
        assert_eq!(v["state"]["finding_0"]["name"], "helper");
        assert_eq!(v["questions"]["framework_invoked"]["type"], "noul");
    }
}
