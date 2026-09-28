# stratify-judge: A Model-Agnostic Judgment Layer

**Date:** 2026-09-27
**Status:** Approved (design), pending implementation plan
**Supersedes:** the vendor-specific framing in `docs/plan-1-decision-log.md`
**Repo:** `stratify-dev/stratify-judge` (renamed from `stratify-dev/stratify-jev`)

## Goal

Plan 1 shipped a working judgment layer, and named it after one vendor's model. That was a mistake. The thing it actually talks to is a protocol, `POST /v1/systemone`, and at least two models speak it: TypeSafe's hosted Jev, and Convai's Laya, which runs locally under Apache 2.0.

This change makes the tool model-agnostic, renames everything that assumed otherwise, and gives it a real install path.

Nothing about the judgment design changes. The five dead-code questions, the confidence ladder, the structural guard against strengthening a hedged finding, and the cache all stay exactly as reviewed.

## Why Laya is a drop-in, and where it is not

From Laya's own model card: `laya-serve` "exposes the `Router` on the same `POST /v1/systemone` request and response shape as TypeSafe Jev, so existing TypeSafe clients work by changing their base URL." It accepts every question shape the Jev API does, ignores unknown fields, and returns a 422 naming the problem for a malformed question. It binds `0.0.0.0:8000` with no authentication unless `LAYA_API_KEY` is set.

So on the wire it is a drop-in, and the `--base-url` flag already added covers most of it.

Three things are not drop-in:

1. **Authentication is optional.** The client currently always sets a bearer header, and `Client::from_env` returns `None` without a key, which would refuse to run against a local Laya entirely.
2. **`model` is optional.** Jev requires it. Laya's documented example omits it.
3. **Context is far smaller.** Jev holds 64k with 32k for state plus the longest question. Laya's English checkpoint holds 512 tokens; its multilingual checkpoint holds 1,024, or 8,192 with `max_len=8192`.

Measured on the real corpus, one `dead_code` finding costs roughly 1,200 input tokens: about 400 of state and 796 of question set. So Laya's 512-token English checkpoint cannot hold a single finding's request, and no batching strategy fixes that.

## Decisions

| Decision | Choice | Reason |
|---|---|---|
| Name | `stratify-judge` | `Judge` and `judge()` are already the code's core abstraction, so the rename is mostly deleting "jev" from names that otherwise stay. Model-neutral. |
| Client crate name | `systemone-client` | It implements the `/v1/systemone` protocol, which both models speak. Naming it after the protocol rather than a vendor makes it independently useful. |
| Small-context backends | Declare a context floor and error below it | Keeps the cache-completeness gate and the batching invariants exactly as reviewed. The failure is legible instead of silent truncation. |
| Rejected: split questions across requests | No | A finding's answers would arrive across several responses, so the cache would have to accumulate them before the completeness gate could judge whether the set is whole. That gate was a Critical fix; this reopens it. |
| Rejected: a compact question set for small contexts | No | A second question design to write, review and eval separately. The criteria's specificity is exactly what stops the model dismissing real dead code, so a compact variant is where accuracy would quietly regress. |
| Install | cargo-dist, copying the engine's config | One config yields `brew install`, the curl installer, prebuilt binaries for five targets, and a CI workflow. A hand-written formula yields only the formula. |

## The `Backend` type

Replaces the hardcoded endpoint, model and token ceiling.

```rust
pub struct Backend {
    /// Preset name, or "custom" when built from flags. Part of the cache key.
    pub name: String,
    pub url: String,
    /// Jev requires a model; Laya omits it. Skipped from the request when None.
    pub model: Option<String>,
    pub api_key_env: String,
    pub api_key_required: bool,
    /// Budget for state plus the longest single question, in tokens.
    pub state_tokens: usize,
}
```

Two presets:

```rust
Backend::jev()   // https://api.typesafe.ai, model jev-latest,
                 // TYPESAFE_API_KEY required, state_tokens 32_768
Backend::laya()  // http://127.0.0.1:8000, no model,
                 // LAYA_API_KEY optional, state_tokens 8_192
```

The `laya` preset's 8,192 assumes the multilingual checkpoint served with
`max_len=8192`, which is the only Laya configuration that can hold a
`dead_code` request. Someone serving the 512-token English checkpoint sets
`state_tokens = 512` in config and then hits the context floor below, which is
the intended outcome rather than a workaround to find.

Resolution order, narrowest wins: `--base-url` / `--model` flags, then `[judge.backends.<name>]` in config, then the built-in preset named by `--backend`, then `jev`.

Config gains:

```toml
[judge]
backend = "laya"

[judge.backends.laya]
url = "http://127.0.0.1:8000"
state_tokens = 8192
api_key_env = "LAYA_API_KEY"
api_key_required = false
```

A backend named by `--backend` with no preset and no config entry is an error naming both places it looked.

## Consequences in `systemone-client`

- `SystemOneRequest.model` becomes `Option<String>` with `skip_serializing_if`. A request without a model must serialize without the key, not with `null`.
- `Client::new(base, key: Option<String>)`. The bearer header is set only when a key is present. An absent key against a local URL is normal operation, not a degraded mode.
- `Client::for_backend(&Backend) -> Option<Client>`: `None` when the backend requires a key and none is set, which stays a pass-through exactly as today. `Some` with no bearer header when the backend does not require one. Deliberately not a `Result`: a missing key is a supported mode, not an error, and the existing pass-through behavior depends on it staying that way.
- `DEFAULT_BASE_URL` stays as `Backend::jev()`'s url rather than a client-level constant.

## Consequences in `stratify-judge-core`

**The cache key must include the backend name.** Today it hashes `cfg.model`. With two backends that is insufficient and, with `model` optional for Laya, can collide outright. An answer from Laya is not interchangeable with one from Jev, and the cache is designed to be committed, so a silent cross-backend hit would serve verdicts from a model the user did not run. Add `backend.name` to the key ahead of the model, and treat an absent model as a distinct value rather than an empty string.

**`TOKEN_CEILING` stops being a constant.** It becomes `backend.state_tokens * 3 / 4`, preserving the 75% headroom the reviewed code already used (24k of 32k).

**A context floor, checked before any request.** After preparing, compare the largest single finding's estimated cost against the ceiling. If one finding plus its question set does not fit, return an error naming the backend, its `state_tokens`, the offending finding, and its estimated cost. Do not truncate, do not silently drop the finding, and do not send a request that will 422.

Laya's 512-token English checkpoint fails this check by construction. `max_len=8192` passes and fits roughly five findings per batch.

## Consequences in `stratify-judge-cli`

- `--backend <NAME>` (default `jev`), alongside the existing `--base-url` and the new `--model`. `--model` sets the field whatever the backend is: against Laya that sends a key Laya ignores, which is harmless and beats special-casing the flag away.
- The missing-key message names the backend's own env var rather than `TYPESAFE_API_KEY` unconditionally.
- `--dry-run` prints the resolved backend, so a run against the wrong endpoint is visible before it costs anything.
- The context-floor error exits non-zero with the report still passed through, consistent with every other failure path.

## Install and CI

`dist-workspace.toml`, copying the engine's:

```toml
[dist]
cargo-dist-version = "0.32.0"
ci = "github"
installers = ["shell", "homebrew"]
tap = "stratify-dev/homebrew-tap"
formula = "stratify-judge"
targets = ["aarch64-apple-darwin", "aarch64-unknown-linux-gnu",
           "x86_64-apple-darwin", "x86_64-unknown-linux-gnu",
           "x86_64-pc-windows-msvc"]
install-path = "CARGO_HOME"
pr-run-mode = "plan"
install-updater = false
```

Yielding, after the first tagged release:

```sh
brew install stratify-dev/tap/stratify-judge
```

A `test.yml` workflow runs `cargo test`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` on push and pull request. The final whole-branch review flagged the absence of CI as worth fixing before more judges land; this closes it.

## Website

`stratify-dev/stratify-site` builds stratify.dynaum.com from `content/*.md`, `templates/`, and `src/index.html`.

- A section on `src/index.html` presenting the tool by what it does for the reader: the engine is precise about what it can prove, and this reads the residue.
- `content/judge.md`, matching the existing pages' shape, covering install, the two backends, the pipe, and the auditability guarantee that a dismissed finding keeps its full judgment in the JSON.
- A nav entry in `templates/nav.html`.

The claim worth leading with is the measured one: on the engine's own repository, all 15 dead-code findings are false positives, and the judgment supports dismissing all 15 while strengthening none.

## Out of scope

- The other four judgments (duplication, complexity, cycles, layers).
- Making the occurrence index package-aware, which the final review called the single change most likely to move eval numbers.
- Tagging the `Answer` enum on `type` so one unmodelled answer shape does not cost a whole batch.
- The labelled corpus and `eval` subcommand.
- Any change to the dead-code question design.

## Risks

- **The rename touches every file.** Mitigated by doing it as one mechanical commit with no behavioral change, verified by the suite passing before and after.
- **Laya is unverified end to end.** The protocol compatibility is documented, not measured. The first real run against `laya-serve` may surface shape differences the docs do not mention, and `ClientError` now reaches the user, so those will be legible.
- **A committed cache predates the backend in its key.** Every existing entry was written without a backend component and will miss once the key changes. That is correct, not a regression: those answers came from Jev and should be re-asked or re-keyed rather than silently reused.

## Implementation deviations

1. `Backend.name` never becomes `"custom"`. The cache key includes
   `backend.url` instead, which meets the same goal.
2. `Client::for_backend` became `backend::client_for` in
   `stratify-judge-core`, because the client crate must not depend on the
   core crate.
3. The context-floor error passes the report through and lets `--fail-on`
   decide, rather than exiting non-zero unconditionally. The spec's own
   next clause ("consistent with every other failure path") argues for
   this.
