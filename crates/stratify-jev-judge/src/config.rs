use serde::Deserialize;
use std::path::Path;

fn d_model() -> String { "jev-latest".to_string() }
fn d_concurrency() -> usize { 8 }
fn d_batch_findings() -> usize { 10 }
fn d_batch_files() -> usize { 25 }
fn d_dismiss_at() -> f64 { 0.75 }
fn d_api_at() -> f64 { 0.75 }
fn d_low_at() -> f64 { 0.25 }
fn d_explanation_at() -> f64 { 0.70 }

#[derive(Debug, Clone, Deserialize)]
pub struct DeadCodeThresholds {
    /// framework_invoked or test_only above this gives Dismiss.
    #[serde(default = "d_dismiss_at")]
    pub dismiss_at: f64,
    /// external_api above this gives Weaken.
    #[serde(default = "d_api_at")]
    pub api_at: f64,
    /// All three Nouls below this is a precondition for Strengthen.
    #[serde(default = "d_low_at")]
    pub low_at: f64,
    /// Choice confidence floor for genuinely_unused to give Strengthen.
    #[serde(default = "d_explanation_at")]
    pub explanation_at: f64,
}

impl Default for DeadCodeThresholds {
    fn default() -> Self {
        DeadCodeThresholds {
            dismiss_at: d_dismiss_at(),
            api_at: d_api_at(),
            low_at: d_low_at(),
            explanation_at: d_explanation_at(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Thresholds {
    #[serde(default)]
    pub dead_code: DeadCodeThresholds,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JevConfig {
    #[serde(default = "d_model")]
    pub model: String,
    #[serde(default = "d_concurrency")]
    pub concurrency: usize,
    #[serde(default = "d_batch_findings")]
    pub batch_findings: usize,
    #[serde(default = "d_batch_files")]
    pub batch_files: usize,
    #[serde(default)]
    pub thresholds: Thresholds,
}

impl Default for JevConfig {
    fn default() -> Self {
        JevConfig {
            model: d_model(),
            concurrency: d_concurrency(),
            batch_findings: d_batch_findings(),
            batch_files: d_batch_files(),
            thresholds: Thresholds::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Wrapper {
    #[serde(default)]
    jev: Option<JevConfig>,
}

impl JevConfig {
    /// `[jev]` from `stratify.toml`, overridden wholesale by
    /// `stratify-jev.toml` when that file exists. Unparseable or absent
    /// falls back to defaults, matching how the engine reads its own tables.
    pub fn load(root: &Path) -> JevConfig {
        for name in ["stratify-jev.toml", "stratify.toml"] {
            let Ok(text) = std::fs::read_to_string(root.join(name)) else {
                continue;
            };
            let w: Wrapper = match toml::from_str(&text) {
                Ok(w) => w,
                Err(_) => continue,
            };
            if let Some(cfg) = w.jev {
                return cfg;
            }
        }
        JevConfig::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_spec_when_no_config_exists() {
        let dir = tempfile::tempdir().unwrap();
        let c = JevConfig::load(dir.path());
        assert_eq!(c.model, "jev-latest");
        assert_eq!(c.concurrency, 8);
        assert_eq!(c.batch_findings, 10);
        assert_eq!(c.thresholds.dead_code.dismiss_at, 0.75);
        assert_eq!(c.thresholds.dead_code.explanation_at, 0.70);
    }

    #[test]
    fn reads_the_jev_table_out_of_stratify_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("stratify.toml"),
            r#"
[ignore]
paths = ["vendor/**"]

[jev]
concurrency = 3

[jev.thresholds.dead_code]
dismiss_at = 0.9
"#,
        )
        .unwrap();
        let c = JevConfig::load(dir.path());
        assert_eq!(c.concurrency, 3);
        assert_eq!(c.thresholds.dead_code.dismiss_at, 0.9);
        // Unset keys keep their defaults.
        assert_eq!(c.thresholds.dead_code.api_at, 0.75);
        assert_eq!(c.model, "jev-latest");
    }

    #[test]
    fn stratify_jev_toml_overrides_stratify_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("stratify.toml"), "[jev]\nconcurrency = 3\n").unwrap();
        std::fs::write(dir.path().join("stratify-jev.toml"), "[jev]\nconcurrency = 16\n").unwrap();
        assert_eq!(JevConfig::load(dir.path()).concurrency, 16);
    }

    #[test]
    fn an_unparseable_config_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("stratify.toml"), "[jev\nbroken").unwrap();
        assert_eq!(JevConfig::load(dir.path()).concurrency, 8);
    }
}
