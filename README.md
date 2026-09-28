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

## Cache

Answers cache under `.stratify/jev-cache/`, keyed on judge version, model,
state, and question set. Commit the directory: CI then runs without calling the
API, results stay deterministic, and a change to question wording shows up as a
reviewable diff of verdicts.

The cache directory is resolved against the current directory, not `--root`,
so it never lands as untracked files inside the repository being analysed.

The cache key hashes the configured model alias (`jev-latest` by default),
not the concrete model that actually answered, which the entry records
separately. This is deliberate for CI: a cached answer stays a hit even
after TypeSafe moves `jev-latest` to a newer version, so results do not
silently shift between runs. The cost is that moving the alias does not, by
itself, cause anything to be re-asked. To force a re-ask after a model or
question change, bump the judge's `version()`, which changes every cache key
for that judge.
