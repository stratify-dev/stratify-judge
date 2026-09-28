# stratify-judge: Model-Agnostic Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename the project away from one vendor's model name, and make the model endpoint a configurable `Backend` so the tool works against TypeSafe's hosted Jev or a locally served Laya without code changes.

**Architecture:** A `Backend` struct carries everything that differs between models: url, optional model id, optional auth, and a token budget. The `systemone-client` crate becomes protocol-named rather than vendor-named, with optional auth and an optional `model` field. The driver's token ceiling comes from the backend instead of a constant, and a context floor refuses to send a request a backend cannot hold. The judgment design does not change at all.

**Tech Stack:** Rust 2021, tokio, reqwest, serde, clap 4, cargo-dist, wiremock (dev).

**Spec:** `docs/specs/2026-09-27-model-agnostic-judge-design.md`

## Global Constraints

- Repo is `~/dev/stratify-jev` until Task 1 renames it; every later path uses the new names.
- Crate names after Task 1: `systemone-client`, `stratify-judge-core`, `stratify-judge-cli`. Binary: `stratify-judge`.
- **The judgment design does not change.** The five dead-code questions, their criteria text, the confidence ladder, the structural guard against strengthening a finding the engine marked `Likely`, and the cache-completeness gate all stay exactly as reviewed. A task that changes question text is out of scope.
- **Every failure stays a pass-through.** A missing required key, an unreadable root, a failed request, and the new context-floor error all print the report and let `--fail-on` decide the exit code. Nothing swallows the report.
- `Backend::jev()`: url `https://api.typesafe.ai`, model `Some("jev-latest")`, env `TYPESAFE_API_KEY`, required `true`, `state_tokens` 32768.
- `Backend::laya()`: url `http://127.0.0.1:8000`, model `None`, env `LAYA_API_KEY`, required `false`, `state_tokens` 8192.
- Token ceiling is `state_tokens * 3 / 4`, preserving the 75% headroom the reviewed batching used.
- No em dashes anywhere, including help text and error messages.
- Every commit message ends with:
  `Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>`
- TDD: failing test first, watch it fail for the expected reason, then implement.
- `cargo test && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check` clean at every commit except Task 1's intermediate rename steps.

## Review Focus

Five things the spec implies that no task's own happy path exercises, most likely to bite first:

1. **`--base-url` pointing at Laya without `--backend laya`.** This is what a person tries first. Flags overlay the *resolved* backend, so they get Jev's rules at Laya's address: a demand for `TYPESAFE_API_KEY` and a 32k ceiling against an 8k server. Task 6 tests that the resolved backend is reported so the mismatch is visible, and Task 2 tests that `--base-url` alone keeps the default backend's name and auth rules.
2. **A partial `[judge.backends.<name>]` table.** Overriding only `url` must keep the preset's env var, auth requirement and token budget rather than zeroing them. Task 2 tests a one-key override.
3. **`state_tokens = 0` or a value below one finding's cost.** Must trip the context floor with a legible message, never divide by zero or send a request that will 422. Task 5 tests both zero and 512.
4. **A committed cache from before the backend entered the key.** Every entry must miss rather than serve a Jev answer to a Laya run. Task 4 tests that two backends produce different keys for identical state.
5. **A backend with `api_key_required = false` and the key actually set.** Laya requires a bearer header when `LAYA_API_KEY` is set, so "not required" must mean "send it if present", not "never send it". Task 3 tests both.

---

### Task 1: Rename everything, no behavior change

**Files:**
- Rename: `crates/jev-client/` → `crates/systemone-client/`
- Rename: `crates/stratify-jev-judge/` → `crates/stratify-judge-core/`
- Rename: `crates/stratify-jev-cli/` → `crates/stratify-judge-cli/`
- Modify: root `Cargo.toml`, all three crate `Cargo.toml`s, every `use` of a renamed crate, `README.md`, `docs/plan-1-decision-log.md` header note

**Interfaces:**
- Consumes: nothing.
- Produces: crates `systemone-client`, `stratify-judge-core`, `stratify-judge-cli`; binary `stratify-judge`. Every public item keeps its current name and signature.

This task is mechanical. Its whole value is that the suite proves nothing changed.

- [ ] **Step 1: Record the pre-rename baseline**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cd ~/dev/stratify-jev
cargo test 2>&1 | grep "^test result" | tee /tmp/baseline.txt
```

Expected: six `test result: ok` lines, four carrying tests, 118 passed in total. Keep this file; Step 6 diffs against it. The diff is the check, not the count in this sentence.

- [ ] **Step 2: Move the directories with git**

```bash
git mv crates/jev-client crates/systemone-client
git mv crates/stratify-jev-judge crates/stratify-judge-core
git mv crates/stratify-jev-cli crates/stratify-judge-cli
```

- [ ] **Step 3: Rewrite the manifests**

Root `Cargo.toml` members become:

```toml
members = ["crates/systemone-client", "crates/stratify-judge-core", "crates/stratify-judge-cli"]
```

In `crates/systemone-client/Cargo.toml` set `name = "systemone-client"`.
In `crates/stratify-judge-core/Cargo.toml` set `name = "stratify-judge-core"` and change the dependency to `systemone-client = { path = "../systemone-client" }`.
In `crates/stratify-judge-cli/Cargo.toml` set `name = "stratify-judge-cli"`, change both path dependencies, and set the binary:

```toml
[[bin]]
name = "stratify-judge"
path = "src/main.rs"
```

- [ ] **Step 4: Rewrite every crate reference in source**

Crate names reach source as underscored paths:

```bash
grep -rl 'jev_client\|stratify_jev_judge' crates/ | xargs sed -i '' \
  -e 's/jev_client/systemone_client/g' \
  -e 's/stratify_jev_judge/stratify_judge_core/g'
```

Then check nothing was missed and no stray `stratify-jev` remains in source or manifests:

```bash
grep -rn 'jev_client\|stratify_jev_judge\|stratify-jev-' crates/ Cargo.toml || echo "clean"
```

- [ ] **Step 5: Rewrite the user-facing names in docs**

In `README.md`, replace every `stratify-jev` with `stratify-judge`, including the invocation examples and the cache path prose. In `docs/plan-1-decision-log.md`, leave the historical ledger text alone (it records what was true then) but add one line under the existing header note:

```markdown
This log predates the rename to `stratify-judge`. Where it says `stratify-jev`,
`jev-client` or `stratify-jev-judge`, read `stratify-judge`, `systemone-client`
and `stratify-judge-core`. The rulings themselves are unaffected.
```

- [ ] **Step 6: Prove nothing changed**

```bash
cargo test 2>&1 | grep "^test result" > /tmp/after.txt
diff /tmp/baseline.txt /tmp/after.txt && echo "identical"
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
ls target/debug/stratify-judge 2>/dev/null || cargo build 2>&1 | tail -2
```

Expected: `identical`, clippy and fmt clean, and a binary named `stratify-judge`. If the test counts differ at all, stop: a mechanical rename cannot change test outcomes, so something else moved.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor: rename away from one vendor's model name

The tool talks a protocol, POST /v1/systemone, not a vendor, and two
models speak it. jev-client becomes systemone-client because it
implements the protocol rather than one endpoint; the judge crates and
the binary drop "jev".

Mechanical: every public item keeps its name and signature, and the
suite reports the same 118 passes before and after.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: The `Backend` type, presets, and resolution

**Files:**
- Create: `crates/stratify-judge-core/src/backend.rs`
- Modify: `crates/stratify-judge-core/src/lib.rs` (add `pub mod backend;`)
- Modify: `crates/stratify-judge-core/src/config.rs` (rename the config table, add `backends`)

**Interfaces:**
- Consumes: nothing from earlier tasks beyond the renamed crate.
- Produces: `Backend { name: String, url: String, model: Option<String>, api_key_env: String, api_key_required: bool, state_tokens: usize }`; `Backend::jev()`, `Backend::laya()`, `Backend::preset(&str) -> Option<Backend>`, `Backend::token_ceiling(&self) -> usize`; `BackendOverride` (all fields `Option`) with `apply(self, Backend) -> Backend`; `resolve_backend(name: &str, cfg: &JudgeConfig, url_flag: Option<&str>, model_flag: Option<&str>) -> Result<Backend, String>`. `JevConfig` is renamed to `JudgeConfig` and gains `pub backends: BTreeMap<String, BackendOverride>`; its TOML table is `[judge]` with `[judge.backends.<name>]` children.

- [ ] **Step 1: Write the failing tests**

Append to `crates/stratify-judge-core/src/backend.rs`:

```rust
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
        assert_eq!(b.name, "jev", "still jev, so the cache key does not collide");
        assert!(b.api_key_required, "still demands TYPESAFE_API_KEY");
        assert_eq!(b.state_tokens, 32_768, "still jev's budget, which is why --backend exists");
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
    fn a_config_only_backend_with_no_url_anywhere_is_an_error() {
        let cfg: JudgeConfig = toml::from_str(
            r#"
[backends.mine]
state_tokens = 4096
"#,
        )
        .unwrap();
        let err = resolve_backend("mine", &cfg, None, None).unwrap_err();
        assert!(err.contains("no url"), "got {err}");
        assert!(err.contains("--base-url"), "says how to supply one: {err}");
    }

    #[test]
    fn a_url_flag_can_supply_the_url_a_config_table_omits() {
        // A table that sets only a budget is a reasonable thing to write:
        // the endpoint's capacity is stable while its address moves between
        // environments. The url check therefore runs after the flags.
        let cfg: JudgeConfig = toml::from_str(
            r#"
[backends.mine]
state_tokens = 4096
"#,
        )
        .unwrap();
        let b = resolve_backend("mine", &cfg, Some("http://10.0.0.9:8000"), None).unwrap();
        assert_eq!(b.url, "http://10.0.0.9:8000");
        assert_eq!(b.state_tokens, 4096, "the table's budget survives");
        assert_eq!(b.name, "mine");
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
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p stratify-judge-core backend`
Expected: FAIL, `cannot find type Backend in this scope`.

- [ ] **Step 3: Write `backend.rs`**

Prepend to `crates/stratify-judge-core/src/backend.rs`:

```rust
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
    // Checked after the flags, so `--base-url` can supply the url for a
    // config table that sets only a budget. A url is the one field with no
    // sensible default: a preset carries one, and a config-only backend has
    // to get it from somewhere.
    if out.url.is_empty() {
        return Err(format!(
            "backend `{name}` has no url: set `url` under \
             [judge.backends.{name}], or pass --base-url"
        ));
    }
    Ok(out)
}
```

Add `pub mod backend;` to `crates/stratify-judge-core/src/lib.rs`.

- [ ] **Step 4: Rename the config type and add `backends`**

In `crates/stratify-judge-core/src/config.rs`, rename `JevConfig` to `JudgeConfig` throughout, rename the deserialized table from `jev` to `judge` in the wrapper struct, and add the map:

```rust
    /// `[judge.backends.<name>]` tables, overlaying the built-in presets.
    #[serde(default)]
    pub backends: std::collections::BTreeMap<String, crate::backend::BackendOverride>,
```

`JudgeConfig` must derive `Default` for the tests above; it already derives `Clone` and `Deserialize`. Add `backends: BTreeMap::new()` to its manual `Default` impl.

Update `JudgeConfig::load` to read `[judge]` from `stratify.toml`, overridden by `stratify-judge.toml`. The file name changes from `stratify-jev.toml`; update the doc comment and the existing test that writes it.

Then fix every `JevConfig` reference across the workspace:

```bash
grep -rl 'JevConfig' crates/ | xargs sed -i '' 's/JevConfig/JudgeConfig/g'
```

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p stratify-judge-core`
Expected: PASS, including all eight new `backend::tests` cases and the existing `config::tests` with `[judge]` in place of `[jev]`.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat: a Backend type carrying everything a model differs by

url, optional model id, which env var holds the key, whether a key is
required at all, and how many tokens of state the endpoint can hold.
Two presets: jev hosted, laya local.

Resolution is preset, then config table, then flags, and the flags
deliberately do not change the backend's name, auth rules or budget. A
--base-url pointed at a different model therefore keeps the original
backend's rules, which is why the CLI reports what it resolved.

api_key_required false means "send the key if one is set", not "never
send one": Laya binds without auth unless LAYA_API_KEY is set, and then
requires the bearer header.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Optional auth and optional model in `systemone-client`

**Files:**
- Modify: `crates/systemone-client/src/question.rs` (`SystemOneRequest.model`)
- Modify: `crates/systemone-client/src/client.rs` (`new`, `from_key`, `for_backend`)
- Modify: `crates/systemone-client/Cargo.toml` (depend on `stratify-judge-core`? No: see note)
- Test: inline in both modules

**Note on the dependency direction:** `Backend` lives in `stratify-judge-core`, which already depends on `systemone-client`. Adding the reverse would be a cycle. So `for_backend` does **not** live in the client. Instead the client keeps `Client::new(base, key: Option<String>)` and `stratify-judge-core` gains `backend::client_for(&Backend) -> Option<Client>`. Put it in `backend.rs`, next to the type whose rules it reads.

**Interfaces:**
- Consumes: `Backend` from Task 2.
- Produces: `SystemOneRequest { state, model: Option<String>, questions }` with `model` skipped when `None`; `Client::new(base: String, key: Option<String>) -> Client`; `Client::from_key(base, raw_key: Option<String>) -> Option<Client>` keeps its current meaning for a required key; and in `stratify-judge-core`, `backend::client_for(b: &Backend) -> Option<Client>`.

- [ ] **Step 1: Write the failing tests for the request shape**

Append to `crates/systemone-client/src/question.rs` tests:

```rust
    #[test]
    fn a_request_without_a_model_omits_the_key_entirely() {
        let req = SystemOneRequest {
            state: json!({ "finding_0": {} }),
            model: None,
            questions: BTreeMap::new(),
        };
        let v = serde_json::to_value(&req).unwrap();
        assert!(
            v.get("model").is_none(),
            "Laya's documented example omits model; null is not the same thing: {v}"
        );
    }

    #[test]
    fn a_request_with_a_model_still_sends_it() {
        let req = SystemOneRequest {
            state: json!({}),
            model: Some("jev-latest".into()),
            questions: BTreeMap::new(),
        };
        assert_eq!(serde_json::to_value(&req).unwrap()["model"], "jev-latest");
    }
```

- [ ] **Step 2: Write the failing tests for auth**

Append to `crates/systemone-client/src/client.rs` tests:

```rust
    #[tokio::test]
    async fn a_client_without_a_key_sends_no_authorization_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body()))
            .mount(&server)
            .await;
        let c = Client::new(server.uri(), None);
        assert!(c.ask(&req()).await.is_ok(), "a local server with no auth is normal");

        // wiremock 0.6 has no negation matcher, so assert on what was
        // actually sent. An absent header and an empty bearer are
        // different things on the wire, and only one of them is correct.
        let sent = server.received_requests().await.unwrap();
        assert_eq!(sent.len(), 1);
        assert!(
            sent[0].headers.get("authorization").is_none(),
            "no key means no header at all"
        );
    }

    #[tokio::test]
    async fn a_client_with_a_key_still_sends_the_bearer_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("authorization", "Bearer k"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body()))
            .mount(&server)
            .await;
        let c = Client::new(server.uri(), Some("k".into()));
        assert!(c.ask(&req()).await.is_ok());
    }
```

The existing `use wiremock::matchers::{header, method};` covers both tests. Do not reach for a negation matcher: wiremock 0.6.5 has `header_exists` but no `not()` and no `Negate` matcher, which is why the first test inspects `received_requests()` instead.

- [ ] **Step 3: Run to verify both fail**

Run: `cargo test -p systemone-client`
Expected: FAIL. The request tests fail to compile (`model` is `String`, not `Option<String>`); the auth tests fail to compile (`new` takes two `String`s).

- [ ] **Step 4: Implement**

In `question.rs`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct SystemOneRequest {
    pub state: serde_json::Value,
    /// Jev requires a model. Laya omits it, so an absent model must
    /// serialize as no key at all rather than as null.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub questions: BTreeMap<String, Question>,
}
```

In `client.rs`, change the field and constructor:

```rust
pub struct Client {
    http: reqwest::Client,
    base: String,
    /// None for an endpoint that needs no auth, such as a local
    /// laya-serve without LAYA_API_KEY set.
    key: Option<String>,
    retry: RetryPolicy,
}

impl Client {
    pub fn new(base: String, key: Option<String>) -> Client { /* as before, storing key */ }
```

and in `ask`, replace the unconditional `.bearer_auth(&self.key)` with:

```rust
            let mut request = self.http.post(&url).json(req);
            if let Some(k) = &self.key {
                request = request.bearer_auth(k);
            }
            let sent = request.send().await;
```

Update `from_key` to wrap the key in `Some`, and every existing call site (`from_env_at`, the tests) accordingly.

- [ ] **Step 5: Add `client_for` in `stratify-judge-core`**

Append to `crates/stratify-judge-core/src/backend.rs`, with `use systemone_client::Client;` at the top of the file:

```rust
/// A client for this backend, or None when it requires a key and none is
/// set, which stays a pass-through exactly as before.
///
/// Deliberately not a Result: a missing key is a supported mode, not an
/// error, and the pass-through behavior depends on it staying that way.
///
/// `api_key_required == false` means "send the key if one is set", not
/// "never send one". A local laya-serve binds without auth until
/// LAYA_API_KEY is set, and then requires the bearer header.
pub fn client_for(b: &Backend) -> Option<Client> {
    // One definition of "usable key", shared with the client crate rather
    // than reimplemented here. An inline `.filter(|k| !k.trim().is_empty())`
    // would agree today and drift tomorrow, and it could only be tested by
    // writing the process environment, which races reqwest's own
    // proxy-variable reads on a threaded test runner.
    let key = systemone_client::usable_key(std::env::var(&b.api_key_env).ok());
    match (b.api_key_required, key) {
        (true, None) => None,
        (_, key) => Some(Client::new(b.url.clone(), key)),
    }
}
```

And its tests, which set no environment variables:

```rust
    /// Review Focus 5: "not required" must mean "send it if present".
    #[test]
    fn a_backend_that_does_not_require_a_key_still_builds_a_client() {
        // LAYA_API_KEY is almost certainly unset here, which is the point.
        assert!(client_for(&Backend::laya()).is_some());
    }

    #[test]
    fn a_backend_that_requires_a_missing_key_builds_nothing() {
        let b = Backend {
            api_key_env: "STRATIFY_JUDGE_KEY_THAT_IS_NOT_SET".into(),
            api_key_required: true,
            ..Backend::jev()
        };
        assert!(client_for(&b).is_none(), "stays a pass-through");
    }
```

- [ ] **Step 6: Run to verify everything passes**

Run: `cargo test && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS. Existing driver and CLI tests that construct `Client::new(uri, "k".into())` need `Some("k".into())`; fix them as part of this task.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat: optional auth and optional model on the wire

A local laya-serve binds without authentication unless LAYA_API_KEY is
set, so a client with no key is normal operation rather than a degraded
mode. And Laya's documented request omits `model`, which is not the same
as sending null, so the field is skipped when absent.

client_for lives in stratify-judge-core rather than the client crate,
because Backend lives there and the reverse dependency would be a cycle.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: The cache key includes the backend

**Files:**
- Modify: `crates/stratify-judge-core/src/cache.rs`
- Modify: `crates/stratify-judge-core/src/driver.rs` (both `cache_key` call sites)

**Interfaces:**
- Consumes: `Backend` from Task 2.
- Produces: `cache_key(judge: &str, version: u32, backend: &Backend, state: &Value, questions: &BTreeMap<String, Question>) -> String`. The `model: &str` parameter is replaced by `backend: &Backend`.

- [ ] **Step 1: Write the failing test**

In `crates/stratify-judge-core/src/cache.rs` tests, replace `key_changes_when_any_input_changes`'s model case and add:

```rust
    /// Review Focus 4: the cache is designed to be committed, so a
    /// cross-backend hit would serve verdicts from a model the user did
    /// not run.
    #[test]
    fn two_backends_never_share_a_key() {
        let s = json!({ "name": "helper" });
        let q = questions();
        let jev = cache_key("dead_code", 1, &Backend::jev(), &s, &q);
        let laya = cache_key("dead_code", 1, &Backend::laya(), &s, &q);
        assert_ne!(jev, laya);

        // Same name, different model id: still distinct.
        let pinned = Backend {
            model: Some("jev-1.13.0".into()),
            ..Backend::jev()
        };
        assert_ne!(jev, cache_key("dead_code", 1, &pinned, &s, &q));

        // An absent model must not hash the same as an empty one.
        let empty = Backend {
            model: Some(String::new()),
            ..Backend::laya()
        };
        assert_ne!(laya, cache_key("dead_code", 1, &empty, &s, &q));
    }
```

Add `use crate::backend::Backend;` to the test module.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p stratify-judge-core cache`
Expected: FAIL to compile, `expected &str, found &Backend`.

- [ ] **Step 3: Implement**

In `cache.rs`, change the signature and feed the backend before the model:

```rust
pub fn cache_key(
    judge: &str,
    version: u32,
    backend: &Backend,
    state: &serde_json::Value,
    questions: &BTreeMap<String, Question>,
) -> String {
    let mut h = Sha256::new();
    feed(&mut h, judge.as_bytes());
    h.update(version.to_le_bytes());
    // The backend identifies the model that answered. Without it a
    // committed cache would serve one model's verdicts to another's run.
    feed(&mut h, backend.name.as_bytes());
    // Tagged, so an absent model and an empty one are different inputs.
    match &backend.model {
        Some(m) => {
            h.update([1u8]);
            feed(&mut h, m.as_bytes());
        }
        None => h.update([0u8]),
    }
    feed(&mut h, &serde_json::to_vec(state).unwrap_or_default());
    feed(&mut h, &serde_json::to_vec(questions).unwrap_or_default());
    format!("{:x}", h.finalize())
}
```

In `driver.rs`, both call sites pass `&self.backend` instead of `&self.cfg.model`. `Driver` gains a `backend: Backend` field, set in `Driver::new`, which now takes it: `Driver::new(client: Option<Client>, cache: Cache, cfg: JudgeConfig, backend: Backend)`. Update every construction in tests to pass `Backend::jev()`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p stratify-judge-core`
Expected: PASS.

- [ ] **Step 5: Make the backend the only source of "which model"**

The cache key now reads `self.backend`, but two places still read
`self.cfg.model`, so `Driver` has two independent sources for one fact. Before
this task they were the same field and could not disagree. Now they can, and
the split is invisible to the suite because every test pairs
`JudgeConfig::default()` (`model: "jev-latest"`) with `Backend::jev()`
(`model: Some("jev-latest")`), so the two values coincide by construction.

In `driver.rs`, the outgoing request takes the backend's model:

```rust
                let req = SystemOneRequest {
                    state: serde_json::Value::Object(state),
                    // The backend owns which model to ask. Reading cfg here
                    // while the cache key reads the backend would let a
                    // request go to one model and its answer be filed under
                    // another's key, in a cache meant to be committed. It is
                    // also already Option, so a backend with no model omits
                    // the field rather than sending a name the endpoint does
                    // not know.
                    model: self.backend.model.clone(),
                    questions: qs,
                };
```

And `apply_one`'s fallback label prefers the response, then the backend's
model, then the backend's name, so a judgment from a model-less backend is
labelled `laya` rather than an empty string:

```rust
        if judgment.model.is_empty() {
            judgment.model = if model.is_empty() {
                self.backend
                    .model
                    .clone()
                    .unwrap_or_else(|| self.backend.name.clone())
            } else {
                model.to_string()
            };
        }
```

Then delete `JudgeConfig`'s `model` field, its `d_model()` default function, and
its entry in the `Default` impl, in `config.rs`. The knob it provided is
superseded: `--model` sets it per run, and `[judge.backends.<name>] model` sets
it per backend. Update `defaults_match_the_spec_when_no_config_exists` to drop
its `c.model` assertion.

- [ ] **Step 6: Write the test that would have caught this**

Append to `driver.rs`'s tests:

```rust
    /// The request must name the model the cache key hashes. When these
    /// disagree, an answer from one model is filed under another's key, and
    /// the cache is meant to be committed, so the mismatch outlives the run.
    #[tokio::test]
    async fn the_request_carries_the_backends_model_not_the_configs() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "model": "jev-1.13.0",
                "answers": {},
                "usage": { "input_tokens": 10, "output_tokens": 0 }
            })))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let pinned = Backend {
            model: Some("jev-preview".into()),
            ..Backend::jev()
        };
        let d = Driver::new(
            Some(Client::new(server.uri(), Some("k".into()))),
            Cache::new(dir.path().into(), false),
            JudgeConfig::default(),
            pinned,
        );
        d.run(&mut report(1), &ctx()).await;

        let sent = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&sent[0].body).unwrap();
        assert_eq!(body["model"], "jev-preview", "the backend's model, not the config's");
    }

    /// A backend with no model omits the field entirely. Laya's documented
    /// request has no `model` key, and sending one names an endpoint it does
    /// not know.
    #[tokio::test]
    async fn a_backend_without_a_model_omits_the_field() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "model": "laya-1",
                "answers": {},
                "usage": { "input_tokens": 10, "output_tokens": 0 }
            })))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let d = Driver::new(
            Some(Client::new(server.uri(), None)),
            Cache::new(dir.path().into(), false),
            JudgeConfig::default(),
            Backend {
                url: server.uri(),
                ..Backend::laya()
            },
        );
        d.run(&mut report(1), &ctx()).await;

        let sent = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&sent[0].body).unwrap();
        assert!(body.get("model").is_none(), "no model key at all: {body}");
    }
```

- [ ] **Step 7: Correct the README**

`README.md`'s Cache section says the key hashes the configured model alias.
That is no longer true. It hashes the backend name and the backend's model, so
a Jev answer and a Laya answer never share a key. Say that instead.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
fix: put the backend in the cache key

The key hashed the model but not which endpoint answered. With two
backends that is insufficient, and with model optional for Laya it can
collide outright. The cache is designed to be committed, so a
cross-backend hit would serve verdicts from a model the user never ran.

The model is tagged rather than fed raw, so an absent model and an empty
string are different inputs.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: The context floor, and a ceiling from the backend

**Files:**
- Modify: `crates/stratify-judge-core/src/driver.rs`

**Interfaces:**
- Consumes: `Backend::token_ceiling` from Task 2, `Driver.backend` from Task 4.
- Produces: `pub enum RunError { ContextTooSmall { backend: String, state_tokens: usize, ceiling: usize, finding: String, cost: usize } }` implementing `std::fmt::Display`; `Driver::run(&self, &mut Report, &RepoContext) -> Result<RunStats, RunError>`; `Driver::plan(&self, &Report, &RepoContext) -> Result<(usize, usize), RunError>`. `TOKEN_CEILING` is deleted.

- [ ] **Step 1: Write the failing tests**

Append to `driver.rs` tests:

```rust
    /// Review Focus 3: a budget too small for one finding must say so by
    /// name rather than truncate the state or send a request that 422s.
    #[tokio::test]
    async fn a_backend_too_small_for_one_finding_is_a_named_error() {
        let dir = tempfile::tempdir().unwrap();
        // 512 is Laya's English checkpoint. One dead_code finding needs
        // roughly 1,200 tokens, so this cannot work and must not pretend to.
        let small = Backend {
            state_tokens: 512,
            ..Backend::laya()
        };
        let d = Driver::new(None, Cache::new(dir.path().into(), false), JudgeConfig::default(), small);
        let err = d.plan(&report(1), &ctx()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("laya"), "names the backend: {msg}");
        assert!(msg.contains("512"), "names the budget: {msg}");
        assert!(msg.contains("fn0"), "names the finding it could not fit: {msg}");
    }

    #[tokio::test]
    async fn a_zero_budget_is_the_same_named_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let zero = Backend {
            state_tokens: 0,
            ..Backend::laya()
        };
        let d = Driver::new(None, Cache::new(dir.path().into(), false), JudgeConfig::default(), zero);
        assert!(d.plan(&report(1), &ctx()).is_err());
    }

    #[test]
    fn the_ceiling_follows_the_backend_not_a_constant() {
        let dir = tempfile::tempdir().unwrap();
        let mk = |b: Backend| {
            Driver::new(None, Cache::new(dir.path().into(), false), JudgeConfig::default(), b)
        };
        // Laya's 6,144 fits fewer findings per batch than Jev's 24,576.
        let (laya_reqs, _) = mk(Backend::laya()).plan(&report(20), &ctx()).unwrap();
        let (jev_reqs, _) = mk(Backend::jev()).plan(&report(20), &ctx()).unwrap();
        assert!(
            laya_reqs >= jev_reqs,
            "a smaller budget cannot need fewer requests: laya {laya_reqs}, jev {jev_reqs}"
        );
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p stratify-judge-core driver`
Expected: FAIL, `no method named unwrap_err` (plan returns `usize`, not `Result`).

- [ ] **Step 3: Implement**

Delete `pub const TOKEN_CEILING`. Add the error type:

```rust
/// A failure that stops judging before any request is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// The backend cannot hold one finding's state plus its questions, so
    /// no batching strategy helps and truncating would ask the model about
    /// a function it cannot see.
    ContextTooSmall {
        backend: String,
        state_tokens: usize,
        ceiling: usize,
        finding: String,
        cost: usize,
    },
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::ContextTooSmall {
                backend,
                state_tokens,
                ceiling,
                finding,
                cost,
            } => write!(
                f,
                "backend `{backend}` holds {state_tokens} tokens of state \
                 ({ceiling} after headroom), but `{finding}` needs about \
                 {cost}. Raise `state_tokens` under [judge.backends.{backend}] \
                 if the endpoint can take more, or serve a larger checkpoint"
            ),
        }
    }
}

impl std::error::Error for RunError {}
```

Add a shared check used by both `run` and `plan`, so they cannot disagree:

```rust
    /// The largest single finding must fit, or nothing can. Checked before
    /// any request so a too-small backend costs nothing.
    fn check_fits(&self, items: &[(String, usize)]) -> Result<(), RunError> {
        let ceiling = self.backend.token_ceiling();
        for (name, cost) in items {
            if *cost > ceiling {
                return Err(RunError::ContextTooSmall {
                    backend: self.backend.name.clone(),
                    state_tokens: self.backend.state_tokens,
                    ceiling,
                    finding: name.clone(),
                    cost: *cost,
                });
            }
        }
        Ok(())
    }
```

In both `run` and `plan`, build `items` as `(function name or message, estimated cost)` per prepared finding, call `check_fits` before `plan_batches`, and pass `self.backend.token_ceiling()` where `TOKEN_CEILING` was. Change both signatures to return `Result`, and propagate with `?`.

The finding's name for the message comes from `judges::dead_code::function_name(&f.message).unwrap_or(&f.message)`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p stratify-judge-core`
Expected: PASS. Every existing `d.run(...)` and `d.plan(...)` in tests needs `.unwrap()` or `.await.unwrap()`; fix them here.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat: refuse to judge on a backend too small to hold one finding

The token ceiling was a constant sized for Jev. It now comes from the
backend, keeping the same 75% headroom, and a backend that cannot hold
one finding plus its questions is a named error before any request goes
out.

Truncating instead would ask the model five questions about a function
it cannot see, and an empty occurrence list is exactly what the resolver
question reads as proof of no caller. Laya's 512-token English
checkpoint fails this check; max_len=8192 passes.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: CLI flags and a visible resolved backend

**Files:**
- Modify: `crates/stratify-judge-cli/src/main.rs`
- Test: `crates/stratify-judge-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `resolve_backend`, `client_for`, `Backend`, `RunError` from Tasks 2 to 5.
- Produces: the `stratify-judge` binary with `--backend <NAME>` (default `jev`) and `--model <ID>` alongside the existing `--base-url`.

- [ ] **Step 1: Write the failing integration tests**

Append to `crates/stratify-judge-cli/tests/cli.rs`:

```rust
/// Review Focus 1: pointing --base-url at another model without
/// --backend keeps the default backend's rules. That is defensible, but
/// only if the tool says which backend it resolved.
#[test]
fn dry_run_names_the_backend_it_resolved() {
    Command::cargo_bin("stratify-judge")
        .unwrap()
        .env_remove("TYPESAFE_API_KEY")
        .args([
            "--root",
            fixtures().join("sample-repo").to_str().unwrap(),
            "--base-url",
            "http://127.0.0.1:8000",
            "--dry-run",
        ])
        .write_stdin(report_json())
        .assert()
        .success()
        .stdout(predicates::str::contains("backend jev"))
        .stdout(predicates::str::contains("http://127.0.0.1:8000"));
}

#[test]
fn the_laya_backend_needs_no_key_to_get_past_the_client_check() {
    // No LAYA_API_KEY, and laya does not require one, so this must reach
    // the request stage and fail there rather than passing through as
    // "no key configured".
    Command::cargo_bin("stratify-judge")
        .unwrap()
        .env_remove("LAYA_API_KEY")
        .args([
            "--root",
            fixtures().join("sample-repo").to_str().unwrap(),
            "--backend",
            "laya",
            "--base-url",
            "http://127.0.0.1:1",
            "--cache-dir",
            "/tmp/stratify-judge-test-cache",
        ])
        .write_stdin(report_json())
        .assert()
        .success()
        .stderr(predicates::str::contains("request(s) failed"));
}

#[test]
fn an_unknown_backend_fails_with_a_message_naming_both_places() {
    Command::cargo_bin("stratify-judge")
        .unwrap()
        .args([
            "--root",
            fixtures().join("sample-repo").to_str().unwrap(),
            "--backend",
            "mistral",
        ])
        .write_stdin(report_json())
        .assert()
        .failure()
        .stderr(predicates::str::contains("judge.backends.mistral"));
}

#[test]
fn a_missing_key_names_the_backends_own_env_var() {
    Command::cargo_bin("stratify-judge")
        .unwrap()
        .env_remove("TYPESAFE_API_KEY")
        .args(["--root", fixtures().join("sample-repo").to_str().unwrap()])
        .write_stdin(report_json())
        .assert()
        .success()
        .stderr(predicates::str::contains("TYPESAFE_API_KEY"));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p stratify-judge-cli --test cli`
Expected: FAIL, unrecognized `--backend`, and the dry-run output has no backend line.

- [ ] **Step 3: Implement**

Add to `Args`:

```rust
    /// Which model endpoint to ask: a built-in preset (jev, laya) or a
    /// name with a [judge.backends.<name>] table.
    #[arg(long, default_value = "jev")]
    backend: String,

    /// Model id to send. Jev requires one; Laya ignores it.
    #[arg(long)]
    model: Option<String>,
```

Resolve after loading config and before the dry-run check:

```rust
    let backend = match resolve_backend(
        &args.backend,
        &cfg,
        args.base_url.as_deref(),
        args.model.as_deref(),
    ) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("stratify-judge: {e}");
            return ExitCode::from(2);
        }
    };
```

An unresolvable backend is a configuration error before anything is read from the model, so it exits 2 rather than passing through. Everything downstream of a resolved backend keeps the pass-through rule.

Print it in the dry-run line:

```rust
    if args.dry_run {
        match Driver::new(None, cache, cfg, backend.clone()).plan(&report, &ctx) {
            Ok((requests, tokens)) => {
                println!(
                    "backend {} at {}: {requests} request(s) planned, \
                     {tokens} tokens estimated, nothing sent.",
                    backend.name, backend.url
                );
                return ExitCode::SUCCESS;
            }
            Err(e) => {
                eprintln!("stratify-judge: {e}");
                print!("{}", render(&args, &report));
                return exit_code(&args, &report);
            }
        }
    }
```

Replace `Client::from_env()` / `from_env_at` with `backend::client_for(&backend)`, and make the missing-key message name `backend.api_key_env`:

```rust
        None => {
            eprintln!(
                "stratify-judge: {} is not set, passing the report through unchanged",
                backend.api_key_env
            );
        }
```

Handle `RunError` from `driver.run` the same way: print it, print the report, return `exit_code`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS.

- [ ] **Step 5: Update the README**

Add a Backends section documenting both presets, the three flags, and the local Laya recipe:

```sh
pip install 'laya[serve]'
laya-serve --max-len 8192
stratify check . --format json | stratify-judge --root . --backend laya
```

State plainly that Laya's 512-token English checkpoint cannot hold a dead-code request, and that `max_len=8192` is required.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat(cli): --backend and --model, and say which backend resolved

--base-url alone keeps the default backend's auth rules and token
budget, which is defensible only if the tool says what it resolved, so
--dry-run now names the backend and its url before anything is spent.

An unknown backend exits 2 naming both the presets and the config table
it looked for. A missing key names that backend's own env var rather
than TYPESAFE_API_KEY unconditionally.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: Install via cargo-dist, and CI

**Files:**
- Create: `dist-workspace.toml`
- Create: `.github/workflows/test.yml`
- Generated: `.github/workflows/release.yml` (by `dist init`, do not hand-write)
- Modify: `README.md` install section

**Interfaces:**
- Consumes: the binary name `stratify-judge` from Task 1.
- Produces: `brew install stratify-dev/tap/stratify-judge` after the first tagged release.

- [ ] **Step 1: Write the test workflow**

`.github/workflows/test.yml`:

```yaml
name: test
on:
  push:
    branches: [master]
  pull_request:

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --workspace
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo fmt --all --check
```

- [ ] **Step 2: Write `dist-workspace.toml`**

Copying the engine's, with this repo's tap and formula name:

```toml
[workspace]
members = ["cargo:crates/stratify-judge-cli"]

[dist]
cargo-dist-version = "0.32.0"
ci = "github"
installers = ["shell", "homebrew"]
tap = "stratify-dev/homebrew-tap"
formula = "stratify-judge"
publish-jobs = ["homebrew"]
targets = ["aarch64-apple-darwin", "aarch64-unknown-linux-gnu", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"]
install-path = "CARGO_HOME"
pr-run-mode = "plan"
install-updater = false
```

- [ ] **Step 3: Generate the release workflow**

```bash
cargo install cargo-dist --version 0.32.0 --locked 2>/dev/null || true
dist init --yes
```

`dist init` writes `.github/workflows/release.yml`. Do not edit it by hand. If `dist` is unavailable, stop and report rather than hand-writing the workflow: a wrong release workflow publishes broken artifacts.

- [ ] **Step 4: Verify the plan builds**

```bash
dist plan
```

Expected: it lists five targets, a shell installer and a homebrew formula named `stratify-judge`, with no errors.

- [ ] **Step 5: Update the README install section**

Replace the build-from-source instructions with, in this order:

```sh
brew install stratify-dev/tap/stratify-judge
```

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/stratify-dev/stratify-judge/releases/latest/download/stratify-judge-installer.sh | sh
```

Then `cargo install --git`, matching the engine README's ordering and wording.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
build: release through cargo-dist, and run the suite in CI

One config yields brew install, the curl installer and prebuilt
binaries for five targets, pushing the formula to
stratify-dev/homebrew-tap the way the engine already does.

Also adds the test workflow. The final whole-branch review flagged that
nothing ran the suite, clippy or fmt on a push, which matters more once
four more judgments land.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 7: Report, do not release**

Cutting the first tag publishes artifacts and pushes a Homebrew formula to a shared tap. Report that the repo is release-ready and stop. The tag is the human's to cut.

---

## Self-Review

**Spec coverage:**

| Spec section | Task |
|---|---|
| Name, crate names, binary | 1 |
| `Backend` type, presets, resolution order, config tables | 2 |
| Optional `model` on the wire | 3 |
| Optional auth, `client_for` | 3 |
| Cache key includes the backend | 4 |
| Ceiling from `state_tokens`, context floor | 5 |
| `--backend`, `--model`, backend-aware messages, dry-run reports the backend | 6 |
| cargo-dist, brew, CI | 7 |
| Website | Deliberately a separate plan: different repo, different toolchain, no Rust reviewer can check it. |

**Deviation from the spec, recorded:** the spec put `for_backend` on `Client` in the client crate. That would require `systemone-client` to depend on `stratify-judge-core` for `Backend`, and `stratify-judge-core` already depends on `systemone-client`, so it is a dependency cycle. Task 3 puts `client_for` in `backend.rs` instead, next to the type whose rules it reads. Same behavior, no cycle.

**Placeholder scan:** clean. Two typos found on review and fixed in place rather than annotated: a mangled path in Task 4's file list, and a placeholder type name in Task 3 Step 5 that would have been transcribed literally.

**Type consistency:** `Driver::new` gains a fourth parameter in Task 4 and is used with it in Tasks 5 and 6. `plan` returns `(usize, usize)` from the previous plan's work and becomes `Result<(usize, usize), RunError>` in Task 5; Task 6's dry-run handles both arms. `cache_key` takes `&Backend` from Task 4 onward. `JevConfig` is `JudgeConfig` from Task 2 onward, including in Tasks 5 and 6's test fixtures.

**Review Focus coverage:** all five lines have a test in the task owning the code: 1 in Tasks 2 and 6, 2 in Task 2, 3 in Task 5, 4 in Task 4, 5 in Task 3.
