# stratify-jev

Judgment layer for [Stratify](https://github.com/stratify-dev/stratify). Reads a
Stratify JSON report, asks [Jev](https://docs.typesafe.ai) what static analysis
could not prove, and moves each finding along the confidence ladder the engine
already uses.

```sh
export TYPESAFE_API_KEY=...
stratify check . --format json | stratify-jev --root .
```

```
info  src/lib.rs:6  possibly unused function `helper`
      jev: reached by a framework (0.91)

73 findings, 9 shown, 64 dismissed. Re-run with --show-dismissed to see them.
```

## What it does

Stratify is precise about what it can prove. When cross-file resolution is
uncertain, it reports `possibly unused` rather than a false `unused`. The
residue is noise a reviewer dismisses in one second: framework hooks, test
helpers, and symbols reached only through reflection.

`stratify-jev` asks Jev about that residue. A finding is never deleted. A
dismissed finding drops below the display threshold and keeps its full
judgment, every raw probability included, in the JSON output.

## Without an API key

The report passes through unchanged and the exit code still follows
`--fail-on`. The same holds for a network failure. Judgment is additive, never
a dependency of the scan.

## Options

Run `stratify-jev --help`.

`--dry-run` reports how many requests a real run would send and sends
nothing. It counts through the same cache check a real run uses, so a
committed cache shows as zero requests. It always exits 0, since it is a
preview rather than a gate.

## Cache

Answers cache under `.stratify/jev-cache/`, keyed on judge version, model,
state, and question set. Commit the directory: CI then runs without calling the
API, results stay deterministic, and a change to question wording shows up as a
reviewable diff of verdicts.
