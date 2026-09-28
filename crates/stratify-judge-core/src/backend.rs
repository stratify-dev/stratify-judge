use crate::config::JudgeConfig;
use serde::Deserialize;

/// Which model endpoint to ask, and what it can hold.
///
/// The tool talks a protocol, `POST /v1/systemone`, not a vendor. Two
/// models speak it: TypeSafe's hosted Jev and Convai's Laya, which runs
/// locally under Apache 2.0. Everything that differs between them lives
/// here, so nothing else in the codebase names a vendor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backend {
    /// Preset or config name. Part of the cache key, because an answer
    /// from one model is not interchangeable with another's.
    pub name: String,
    pub url: String,
    /// Jev requires a model. Laya's documented example omits it, so this
    /// is skipped from the request rather than sent as null.
    pub model: Option<String>,
    pub api_key_env: String,
    /// False means "send the key if one is set", not "never send one".
    /// Laya binds without auth unless LAYA_API_KEY is set, in which case
    /// it requires the bearer header.
    pub api_key_required: bool,
    /// Budget for state plus the longest single question, in tokens.
    pub state_tokens: usize,
}

impl Backend {
    pub fn jev() -> Backend {
        Backend {
            name: "jev".into(),
            url: "https://api.typesafe.ai".into(),
            model: Some("jev-latest".into()),
            api_key_env: "TYPESAFE_API_KEY".into(),
            api_key_required: true,
            state_tokens: 32_768,
        }
    }

    /// The 8,192 assumes the multilingual checkpoint served with
    /// `max_len=8192`, the only Laya configuration that can hold a
    /// dead_code request. Serving the 512-token English checkpoint means
    /// setting `state_tokens` in config, which then trips the context
    /// floor. That is the intended outcome, not a workaround to find.
    pub fn laya() -> Backend {
        Backend {
            name: "laya".into(),
            url: "http://127.0.0.1:8000".into(),
            model: None,
            api_key_env: "LAYA_API_KEY".into(),
            api_key_required: false,
            state_tokens: 8_192,
        }
    }

    pub fn preset(name: &str) -> Option<Backend> {
        match name {
            "jev" => Some(Backend::jev()),
            "laya" => Some(Backend::laya()),
            _ => None,
        }
    }

    /// Budget for one request's state, keeping the 75% headroom the
    /// reviewed batching used (24k of 32k). Saturating, so a degenerate
    /// configured budget yields 0 and trips the context floor rather than
    /// overflowing.
    pub fn token_ceiling(&self) -> usize {
        self.state_tokens.saturating_mul(3) / 4
    }
}

/// A `[judge.backends.<name>]` table. Every field is optional so a table
/// naming one key overrides only that key and keeps the preset's rest.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BackendOverride {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub api_key_required: Option<bool>,
    #[serde(default)]
    pub state_tokens: Option<usize>,
}

impl BackendOverride {
    pub fn apply(self, mut base: Backend) -> Backend {
        if let Some(v) = self.url {
            base.url = v;
        }
        if let Some(v) = self.model {
            base.model = Some(v);
        }
        if let Some(v) = self.api_key_env {
            base.api_key_env = v;
        }
        if let Some(v) = self.api_key_required {
            base.api_key_required = v;
        }
        if let Some(v) = self.state_tokens {
            base.state_tokens = v;
        }
        base
    }
}

/// Resolve the backend to use: the preset named by `name`, overlaid with
/// its config table, overlaid with the flags. A name with neither a preset
/// nor a config entry is an error naming both places searched.
///
/// The flags deliberately do not change the backend's `name`, auth rules
/// or budget. Pointing `--base-url` at a different model without
/// `--backend` therefore keeps the original backend's rules, which is why
/// the CLI reports the resolved backend before spending anything.
pub fn resolve_backend(
    name: &str,
    cfg: &JudgeConfig,
    url_flag: Option<&str>,
    model_flag: Option<&str>,
) -> Result<Backend, String> {
    let over = cfg.backends.get(name).cloned();
    let base = match (Backend::preset(name), over) {
        (Some(preset), Some(over)) => over.apply(preset),
        (Some(preset), None) => preset,
        (None, Some(over)) => {
            // Config-only backend. Start from a neutral shape so an
            // incomplete table cannot inherit a vendor's url or auth.
            let neutral = Backend {
                name: name.to_string(),
                url: String::new(),
                model: None,
                api_key_env: format!("{}_API_KEY", name.to_uppercase()),
                api_key_required: false,
                state_tokens: 0,
            };
            over.apply(neutral)
        }
        (None, None) => {
            return Err(format!(
                "unknown backend `{name}`: it is not a built-in preset \
                 (jev, laya) and there is no [judge.backends.{name}] table \
                 in stratify.toml or stratify-judge.toml"
            ))
        }
    };

    let mut out = base;
    if let Some(u) = url_flag {
        out.url = u.to_string();
    }
    if let Some(m) = model_flag {
        out.model = Some(m.to_string());
    }
    if out.url.is_empty() {
        return Err(format!(
            "backend `{name}` has no url: set `url` under \
             [judge.backends.{name}], or pass --base-url"
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::JudgeConfig;

    #[test]
    fn the_two_presets_match_the_spec() {
        let j = Backend::jev();
        assert_eq!(j.name, "jev");
        assert_eq!(j.url, "https://api.typesafe.ai");
        assert_eq!(j.model.as_deref(), Some("jev-latest"));
        assert_eq!(j.api_key_env, "TYPESAFE_API_KEY");
        assert!(j.api_key_required);
        assert_eq!(j.state_tokens, 32_768);

        let l = Backend::laya();
        assert_eq!(l.name, "laya");
        assert_eq!(l.url, "http://127.0.0.1:8000");
        assert_eq!(l.model, None, "Laya's documented example omits the model");
        assert_eq!(l.api_key_env, "LAYA_API_KEY");
        assert!(!l.api_key_required);
        assert_eq!(l.state_tokens, 8_192);
    }

    #[test]
    fn the_ceiling_keeps_the_reviewed_headroom() {
        // 24k of 32k is what the reviewed batching used.
        assert_eq!(Backend::jev().token_ceiling(), 24_576);
        assert_eq!(Backend::laya().token_ceiling(), 6_144);
        // No panic on a degenerate budget; the context floor catches it later.
        let zero = Backend {
            state_tokens: 0,
            ..Backend::laya()
        };
        assert_eq!(zero.token_ceiling(), 0);
    }

    #[test]
    fn an_unknown_preset_is_not_a_preset() {
        assert!(Backend::preset("jev").is_some());
        assert!(Backend::preset("laya").is_some());
        assert!(Backend::preset("gpt").is_none());
    }

    /// Review Focus 2: a table naming one key must not zero the rest.
    #[test]
    fn a_partial_override_keeps_the_presets_other_fields() {
        let over = BackendOverride {
            url: Some("http://localhost:9999".into()),
            ..BackendOverride::default()
        };
        let b = over.apply(Backend::laya());
        assert_eq!(b.url, "http://localhost:9999");
        assert_eq!(b.api_key_env, "LAYA_API_KEY", "kept from the preset");
        assert!(!b.api_key_required, "kept from the preset");
        assert_eq!(b.state_tokens, 8_192, "kept from the preset");
        assert_eq!(b.name, "laya", "the name identifies the model, not the url");
    }

    #[test]
    fn config_overlays_the_preset_and_flags_overlay_config() {
        // JudgeConfig is the inner type, so its own TOML carries no
        // [judge] header. Production reads that header through the wrapper
        // in JudgeConfig::load; these tests exercise the inner shape.
        let cfg: JudgeConfig = toml::from_str(
            r#"
[backends.laya]
state_tokens = 1024
"#,
        )
        .unwrap();
        let b = resolve_backend("laya", &cfg, None, None).unwrap();
        assert_eq!(b.state_tokens, 1024, "config beats the preset");

        let b = resolve_backend("laya", &cfg, Some("http://elsewhere:1"), Some("m")).unwrap();
        assert_eq!(b.url, "http://elsewhere:1", "the flag beats both");
        assert_eq!(b.model.as_deref(), Some("m"));
        assert_eq!(b.state_tokens, 1024, "untouched by the flags");
    }

    /// Review Focus 1: the first thing someone tries. A url flag alone must
    /// not silently adopt the target's auth rules or token budget.
    #[test]
    fn a_url_flag_alone_keeps_the_default_backends_rules() {
        let cfg = JudgeConfig::default();
        let b = resolve_backend("jev", &cfg, Some("http://127.0.0.1:8000"), None).unwrap();
        assert_eq!(b.url, "http://127.0.0.1:8000");
        assert_eq!(
            b.name, "jev",
            "still jev, so the cache key does not collide"
        );
        assert!(b.api_key_required, "still demands TYPESAFE_API_KEY");
        assert_eq!(
            b.state_tokens, 32_768,
            "still jev's budget, which is why --backend exists"
        );
    }

    #[test]
    fn an_unknown_backend_names_both_places_it_looked() {
        let cfg = JudgeConfig::default();
        let err = resolve_backend("mistral", &cfg, None, None).unwrap_err();
        assert!(err.contains("mistral"), "got {err}");
        assert!(err.contains("preset"), "got {err}");
        assert!(err.contains("judge.backends.mistral"), "got {err}");
    }

    #[test]
    fn a_config_only_backend_needs_no_preset() {
        let cfg: JudgeConfig = toml::from_str(
            r#"
[backends.mine]
url = "http://10.0.0.5:8000"
state_tokens = 4096
api_key_env = "MINE_KEY"
api_key_required = false
"#,
        )
        .unwrap();
        let b = resolve_backend("mine", &cfg, None, None).unwrap();
        assert_eq!(b.name, "mine");
        assert_eq!(b.url, "http://10.0.0.5:8000");
        assert_eq!(b.model, None);
        assert_eq!(b.state_tokens, 4096);
    }

    #[test]
    fn a_config_only_backend_with_no_url_anywhere_is_an_error() {
        let cfg: JudgeConfig = toml::from_str(
            r#"
[backends.mine]
state_tokens = 4096
"#,
        )
        .unwrap();
        let err = resolve_backend("mine", &cfg, None, None).unwrap_err();
        assert!(err.contains("mine"), "got {err}");
        assert!(err.contains("--base-url"), "got {err}");
        assert!(err.contains("[judge.backends.mine]"), "got {err}");
    }

    #[test]
    fn a_url_flag_can_supply_the_url_a_config_table_omits() {
        let cfg: JudgeConfig = toml::from_str(
            r#"
[backends.mine]
state_tokens = 4096
api_key_env = "MINE_KEY"
api_key_required = false
"#,
        )
        .unwrap();
        let b = resolve_backend("mine", &cfg, Some("http://10.0.0.5:8000"), None).unwrap();
        assert_eq!(b.name, "mine");
        assert_eq!(b.url, "http://10.0.0.5:8000");
        assert_eq!(b.state_tokens, 4096, "kept from config");
        assert_eq!(b.api_key_env, "MINE_KEY", "kept from config");
        assert!(!b.api_key_required, "kept from config");
    }
}
