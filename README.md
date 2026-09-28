# stratify-judge

Judgment layer for [Stratify](https://github.com/stratify-dev/stratify). Reads a
Stratify JSON report, asks [Jev](https://docs.typesafe.ai) what static analysis
could not prove, and moves each finding along the confidence ladder the engine
already uses.

```sh
export TYPESAFE_API_KEY=...
stratify check . --format json | stratify-judge --root .
```

```
info  src/lib.rs:6  possibly unused function `helper`
      jev: reached by a framework (0.91)

73 findings, 9 shown, 64 hidden. Re-run with --show-dismissed to see them.
```

## Install

**Homebrew** (macOS and Linux):

```sh
brew install stratify-dev/tap/stratify-judge
```

**One-line installer** (macOS, Linux):

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/stratify-dev/stratify-judge/releases/latest/download/stratify-judge-cli-installer.sh | sh
```

**From source** (needs a [Rust toolchain](https://rustup.rs)):

```sh
cargo install --git https://github.com/stratify-dev/stratify-judge stratify-judge-cli --locked
```

The binary is `stratify-judge`. Run `stratify-judge --help` to see every command.

## What it does

Stratify is precise about what it can prove. When cross-file resolution is
uncertain, it reports `possibly unused` rather than a false `unused`. The
residue is noise a reviewer dismisses in one second: framework hooks, test
helpers, and symbols reached only through reflection.

`stratify-judge` asks Jev about that residue. A finding is never deleted. A
dismissed finding drops below the display threshold and keeps its full
judgment, every raw probability included, in the JSON output.

## Without an API key

The report passes through unchanged and the exit code still follows
`--fail-on`. The same holds for a network failure. Judgment is additive, never
a dependency of the scan.

## Options

Run `stratify-judge --help`.

`--dry-run` reports how many requests a real run would send, its total
token estimate, and sends nothing. It counts through the same cache check a
real run uses, so a committed cache shows as zero requests. It always exits
0, since it is a preview rather than a gate.

`--base-url` points the client at a different API base URL, such as a
capture proxy or a local mock, for diagnosing what a live run is actually
sending and receiving.

## Backends

`--backend <name>` picks which model endpoint to ask. Two presets ship
built in:

- `jev` (the default): TypeSafe's hosted model at `https://api.typesafe.ai`.
  Requires `TYPESAFE_API_KEY` and a model id, which defaults to
  `jev-latest`.
- `laya`: Convai's Laya, served locally under Apache 2.0 at
  `http://127.0.0.1:8000`. Needs no key unless `LAYA_API_KEY` is set, in
  which case it requires one. The preset sends no model id unless `--model`
  or a config table supplies one.

`--model <id>` overrides the model sent in the request. `--base-url`
overrides the endpoint. Both flags overlay whichever backend `--backend`
resolved: they change where the request goes and what it asks for, not the
backend's name, auth rules, or token budget. Pointing `--base-url` at a
different model without also passing `--backend` keeps the original
backend's rules, which is why `--dry-run` always names the backend it
resolved before spending anything.

A name that is not a preset needs a `[judge.backends.<name>]` table in
`stratify.toml` or `stratify-judge.toml`, setting at least `url`.

To run against a local Laya:

```sh
pip install 'laya[serve]'
laya-serve --max-len 8192
stratify check . --format json | stratify-judge --root . --backend laya
```

`max_len=8192` is required. Laya's default checkpoint is the 512-token
English one, and 512 tokens of state cannot hold even one `dead_code`
request; serving it that way trips the context floor on the first finding.

## Cache

Answers cache under `.stratify/jev-cache/`, keyed on judge version, model,
state, and question set. Commit the directory: CI then runs without calling the
API, results stay deterministic, and a change to question wording shows up as a
reviewable diff of verdicts.

The cache directory is resolved against the current directory, not `--root`,
so it never lands as untracked files inside the repository being analysed.

The cache key hashes the backend name and the backend's model. An answer from
Jev and an answer from Laya never share a key, so switching `--backend` does
not cause a stale cache hit. The entry records the concrete model that actually
answered separately, in case that differs from the request.

The model is tagged in the hash so an absent model (Laya) and an empty string
are different inputs. This is deliberate for CI: a cached answer stays a hit
even after TypeSafe moves `jev-latest` to a newer version, so results do not
silently shift between runs. The cost is that moving the alias does not, by
itself, cause anything to be re-asked. To force a re-ask after a model or
question change, bump the judge's `version()`, which changes every cache key
for that judge.
