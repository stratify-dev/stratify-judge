# Plan 1 decision log

Every ruling made while building this repo, in the order made, with what
each costs if wrong. Preserved from the build session's working ledger,
which lived outside version control.

The plan and spec this argues from are not published. They live in a working
copy of the engine repo as
`docs/superpowers/specs/2026-09-20-stratify-jev-design.md` and
`docs/superpowers/plans/2026-09-20-stratify-jev-p1-skeleton.md`. Rulings below
that cite a task number or a brief refer to those files. The rulings themselves
stand on their own: each says what was decided, why, and what it costs if wrong.

---

# SDD ledger — plan: docs/superpowers/plans/2026-09-20-stratify-jev-p1-skeleton.md

Spec: docs/superpowers/specs/2026-09-20-stratify-jev-design.md (read, binding authority)
Target repo: ~/dev/stratify-jev (new, created by Task 1)

## Workspace ruling

Ruling: no git worktree is created — the plan creates an entirely new repo at
~/dev/stratify-jev and touches zero files in ~/dev/stratify. The new repo's own
`main` is isolated by construction. Cost if wrong: none; ~/dev/stratify stays
clean and is verifiable with `git status`.

## Pre-flight conflict scan

### Cross-task pairs sharing a file or interface

| Pair | Produces → Consumes | Finding |
|------|---------------------|---------|
| 1 → 2 | `model::Span`, `lib.rs` module list | Clean |
| 1 → 5 | `Severity::step_down`, `Finding.extra` | Clean |
| 1 → 9,10,11 | `Report`, `Finding` must derive `Clone` | Clean (Task 1 derives Clone) |
| 2 → 8 | `RepoContext::{file_text, function_source, root}` | **F1**: `root()` exists in Task 2's code block but is absent from its Interfaces line; Task 8's test calls it |
| 3 → 4 | `SystemOneRequest`, `SystemOneResponse` | Clean |
| 3 → 6 | `Question: Serialize`, `Answer: Serialize + Deserialize`, `Usage: Copy` | Clean |
| 6 → 9 | `Cache::{get, put}` signatures | **F2**: `get` returns only answers, so a cache-hit judgment records the wrong model |
| 7 → 9 | `JevConfig{model,concurrency,batch_findings,thresholds}`, `Clone` | Clean |
| 8 → 9 | `Judge` trait surface, canonical question names, slot prefixing | Clean (inherent vs trait `judge` called out in Task 8 Step 3) |
| 9 → 10 | `RunStats` is public and derives `Default` | **F3**: `human::render` takes `&RunStats` and never reads it |
| 10 → 11 | `human::visible`, `human::render`, `json::render` | Clean |
| 2,7 → 11 | `--root` drives both `RepoContext::new` and `JevConfig::load` | Clean |

### Per-task internal agreement

| Task | Tests vs code vs files | Finding |
|------|------------------------|---------|
| 1 | Clean | |
| 2 | Clean | |
| 3 | Clean | |
| 4 | Clean | |
| 5 | Clean | |
| 6 | Clean apart from F2 | |
| 7 | Clean | |
| 8 | `attributes_above` matches `#` after already matching `#[` | **F4**: bare `#` captures Python and Ruby comments as attributes |
| 9 | Imports vs use | **F5**: `Finding` and `RetryPolicy` imported but unused; `pub use ... as ClientRetryPolicy` is dead |
| 10 | Clean apart from F3 | |
| 11 | Clean; `div_ceil` needs Rust >= 1.73 | **F6**: toolchain is `stable` unpinned, so note the floor |

### Rulings

Ruling (F1): add `root()` to Task 2's Interfaces block. The code block already
writes it, so this is a documentation gap only. Cost if wrong: none.

Ruling (F2): change `Cache::get` to return `Option<Entry>` rather than
`Option<BTreeMap<String, Answer>>`, and have the driver read `entry.model`.
Spec says every judgment records the model that produced it; a cache hit
reporting the config default instead of the recorded model violates that.
Cost if wrong: one extra field threaded through two call sites.

Ruling (F3): drop the `&RunStats` parameter from `human::render`. The plan's
own rubric treats an unused parameter as a YAGNI defect, and Task 11 would
pass `&Default::default()` into a value nobody reads. Plan 2 reintroduces it
when the token and cost summary line lands. Cost if wrong: one signature
change in Plan 2.

Ruling (F4): rewrite `attributes_above` to collect only lines starting with
`#[` or `@`, skipping blanks and comments without collecting them. As written
it labels every Python and Ruby comment above a function an "attribute",
poisoning the strongest framework signal in the state. Cost if wrong: a
decorator syntax neither prefix covers is missed; no language in scope uses one.

Ruling (F5): remove the unused `Finding` and `RetryPolicy` imports and the
dead `pub use ... as ClientRetryPolicy` from `driver.rs`; the driver test
imports `RetryPolicy` directly from `jev_client`. Cost if wrong: none.

Ruling (F6): pin `rust-toolchain.toml` to `1.75` rather than bare `stable`,
since `usize::div_ceil` stabilized in 1.73 and a pin makes the floor explicit
the way the engine repo pins its own. Cost if wrong: a contributor on an older
toolchain gets a clear rustup prompt instead of a confusing error.

All six rulings applied to the plan file before Task 1 dispatch.

## Task log

Task 1: dispatched (impl-task-1, haiku) — new repo has no BASE, so the review package uses the empty-tree hash.

Ruling (F6 REVERSED): `rust-toolchain.toml` keeps `channel = "stable"`, not the
1.75 pin I set pre-flight. The pin made the workspace unbuildable: transitive
deps (hashbrown 0.17.1) require edition2024, which needs Cargo >= 1.85, and the
installed toolchain is 1.98.1. The engine repo pins `stable` for the same
reason. The implementer deviated from the brief correctly and flagged it.
`div_ceil` (Rust >= 1.73) is satisfied regardless. Plan amended so later tasks
do not reintroduce the pin. Cost if wrong: none; a floor can be pinned later
once a real minimum is known.

Task 1: DONE_WITH_CONCERNS (2d342dc), 3/3 tests pass, concern was F6 and is resolved above.
Task 1: review clean (spec ✅, quality Approved, 0 Critical/Important).
Task 1: ⚠️ items resolved by controller — TDD RED documented in report ("cannot
  find type Report", the expected failure); Co-Authored-By trailer verified via
  git log -1; ~/dev/stratify untouched (only my own plan amendment is dirty).
Task 1: minor (deferred): Report.extra has no round-trip test, only Finding.extra
  does. Same #[serde(flatten)] mechanism, inherited from the brief's own test.
Task 1: minor (deferred): Cargo.lock committed though absent from the brief's
  file list. Standard for a workspace that ships a binary; not a defect.
Task 1: complete (commit 2d342dc, review clean)

Ruling (F7): Task 8's F4 regression test was weak. It asserted that no captured
attribute starts with "//" against a fixture containing zero comments, so it
passed whether or not the fix worked. Amended Task 8 to create
tests/fixtures/sample-repo/src/app.py (a decorated function preceded by two "#"
comments) and to assert the decorator is captured while the comments are not,
plus a case where only prose sits above. This exercises the exact bug F4 fixed.
Cost if wrong: one extra fixture file in a directory already holding fixtures;
adding a .py file does not affect the Task 2, 9, or 11 tests that read this repo.

Task 2: review ❌ spec, Changes Requested (2 Critical, both plan-mandated — my
  plan text carried both bugs; the implementer transcribed them faithfully).

Ruling (Critical #1): finding stands, plan corrected. `function_source` snapped
  line_start/line_end to char boundaries, which are already boundary-safe since
  they come from searching '\n' (ASCII). The span's own start/end were sliced
  unsnapped, so `text[..start]` panics on a mid-character offset. Engine spans
  are raw byte offsets, so a stale report or a file edited since the scan
  reaches this. Added floor_boundary/ceil_boundary applied to the span's own
  offsets, plus a non-ASCII fixture and an off-boundary test.
  Cost if wrong: two small helpers and one fixture file.

Ruling (Critical #2): finding stands, plan corrected. Verified against
  crates/stratify-analysis/src/ignore.rs myself: the engine uses
  GlobBuilder::new(p).literal_separator(true), documented as "`*` does not
  cross `/`, `**` does". Plain Glob::new leaves the flag false, so `build/*.log`
  would match `build/sub/a.log` here but not in the engine. The spec's whole
  reason for reading the engine's ignore table is that the two tools agree on
  scope, so this is a direct spec violation. Extracted compile_ignore_globs and
  added a single-star regression test.
  Cost if wrong: none; this matches the engine exactly.

Ruling (Minor, io::Result decorative): fixed rather than deferred. new() now
  returns NotFound when root is not a directory. This makes the signature
  meaningful and wires Task 11's "cannot read <root>" path, which would
  otherwise have silently scanned an empty inventory on a typo'd --root.
  Cost if wrong: a caller passing a file path gets an error instead of an
  empty result, which is the better failure either way.

Controller re-audit of remaining plan code (prompted by the two Task 2
Criticals being plan-mandated). Checked every later task's code for the same
class of defect. Findings:

- Task 9 `Driver::run` holds `&RepoContext` across await points, and
  RepoContext contains a RefCell, so the future is !Send. This is FINE as
  written: Task 11 awaits it directly under #[tokio::main] and never spawns
  it, and #[tokio::test] defaults to current_thread. Recorded because a later
  task that reaches for tokio::spawn on the driver will hit a confusing
  compile error. Plan 2 should swap RefCell for a Mutex if spawning is ever
  wanted. No change now: YAGNI, and the current shape is correct.
- Task 4: the 429|529|500..=599 arm overlaps on 529. Harmless, one arm.
- Task 9: all async blocks in the join_all vec come from one syntactic block,
  so they share an anonymous type. Compiles.
- Task 3: Question derives Clone (Task 9 clones per batch slot) and Answer
  derives Clone + Serialize + Deserialize (driver clones, cache round-trips).
  Both confirmed present.
- Task 6: after F2, Entry derives Serialize + Deserialize and the test reads
  got.model. Consistent.
- Task 11: exit_code intentionally filters by the display threshold, so a
  dismissed warning does not fail a build. That is the product's whole point
  and matches the spec.
No further plan corrections needed from this audit.

Task 2: fix round 1/5 (3 addressed, 0 open — char-boundary snap, glob
  literal_separator, new() NotFound; commits 67558ed..a9db1ed)
Task 2: minor (deferred): #[derive(Debug)] added to RepoContext, unrequested.
Task 2: minor (deferred): compile_ignore_globs is `pub fn` where `pub(crate)`
  would suffice; widens the crate API more than the fix needed.
Task 2: complete (commits 2d342dc..a9db1ed, review clean)

Task 3: review spec ✅, quality Approved with 1 Important.
Task 3: ⚠️ items resolved by controller — Co-Authored-By trailer verified on
  4e1940d via git log; all six changed files live under ~/dev/stratify-jev and
  ~/dev/stratify's working tree is clean.

Ruling (Task 3 Important, no round-trip test): finding stands, entering the fix
  loop even though the reviewer chose not to block. The rule is mechanical: an
  Important finding enters the loop. It is also correct on the merits. Task 6
  round-trips answers through a JSON disk cache, so serialization is a contract
  another task depends on, and nothing exercises it. A future change that breaks
  it would pass every test in jev-client and surface as a cache bug, which is a
  much worse place to debug. Added
  `every_answer_variant_survives_a_json_round_trip` to the plan.
  Cost if wrong: one cheap test covering a path another task relies on.

Ruling (Task 3 Minor, 255-option and 2-10-level limits unenforced): deferred,
  not fixed. Every question in Plan 1 is a hardcoded constant built by a judge,
  never assembled from untrusted or variable-length input, so a runtime check
  would guard an unreachable case. Revisit if Plan 2 builds Choice options from
  configured layer names, which is variable-length and user-supplied.
  Cost if wrong: a malformed request returns a 422 from the API, which the
  client already surfaces as ClientError::Validation with the body attached.

Controller integration check (real engine output vs the Task 1 model), run
against a live `stratify check . --format json` of this repo, 73 findings:
  top-level keys   = {findings, schema_version}        matches Report
  finding keys     = {rule, severity, message, span, confidence}  matches Finding
  span keys        = {file, start_byte, end_byte, start_line}     matches Span
  severity values  = info, warning (lowercase)         matches the serde rename
  confidence values= likely, certain (lowercase)       matches the serde rename
  schema_version   = 1 (integer)                       matches KNOWN_SCHEMA_VERSION
  dead_code message= "possibly unused function `helper`"  parses with Task 8's
                     function_name backtick extraction
No model change needed. Task 11's end-to-end path is de-risked against real
data rather than only against the hand-written fixture.

Task 3: fix round 1/5 (1 addressed, 0 open — Answer JSON round-trip test covering
  all three variants with populated maps and an equality assertion;
  commits 4e1940d..f6c40e0)
Task 3: complete (commits a9db1ed..f6c40e0, review clean)

Ruling (expected test counts): replaced every "Expected: PASS, N tests" line in
the plan with a named-outcome assertion ("these specific cases pass, workspace
green"). Review rounds added three tests to Task 2, one to Task 3, and turned
Task 8's single attributes test into two, so every downstream count had drifted.
A stale exact count invites a false spec failure and wastes an implementer's
time reconciling arithmetic that proves nothing. Naming the cases that must
appear is both more robust to further additions and a stronger check, since a
count can be met while the wrong tests run.
Cost if wrong: none; the suite still has to be fully green either way.

Task 4: review spec ✅, Changes Requested (1 Important, 2 Minor).
Task 4: ⚠️ resolved by controller — Co-Authored-By trailer verified on 3c9ce33.
  The reviewer independently confirmed the retry test is deterministic by
  reading wiremock's own source (stable priority sort, and an exhausted
  up_to_n_times mock stops matching before matchers run), and traced the
  attempt bound to exactly 4 requests for max_attempts 4. Both sound.

Ruling (Task 4 Important, empty-key branch untested): finding stands. The
  branch existed only because it was in my brief verbatim, never driven by a
  red test, so a future edit to `key.is_empty()` or a dropped check would pass
  the whole suite while sending `Bearer ` and earning a 401 on every request.
  Fixed by extracting `usable_key(Option<String>) -> Option<String>` and
  testing that directly, rather than by adding more env-var mutation. This also
  answers the reviewer's separate fragility note: the env-setting test is no
  longer the only coverage of the rule, so a later task adding a second test
  that touches TYPESAFE_API_KEY cannot silently erase it.
  Cost if wrong: one small private function and three tests.

Ruling (Task 4 Minor, Duration overflow in backoff): folded into this same fix
  round rather than deferred. Normally a Minor never enters the loop, but the
  round was already happening, the fix is one method call (saturating_mul), and
  the constraint I wrote is absolute: "ask must never panic", with no
  caller-misconfiguration carve-out. Making an absolute constraint literally
  true beats deferring it. Added a test constructing RetryPolicy with
  Duration::MAX.
  Cost if wrong: a pathological base_delay now sleeps a saturated duration
  instead of panicking, which is the better failure.

Task 4: minor (deferred): the implementer's report said the pre-existing tests
  split 4+4 across question.rs and answer.rs; the real split is 5+3. The total
  of 8 was right. Bookkeeping slip in prose, no code impact.

Ruling (Task 4, my own saturating-backoff test was vacuous): caught by the
  controller before re-review. The test I wrote mounted a 200 responder, so
  `ask` returned on attempt 1 and `backoff` was never called. It passed
  identically with `*` or `saturating_mul` and proved nothing about the
  overflow it claimed to guard. Testing this through `ask` is impossible in
  principle: reaching a retry means the test then sleeps for the very duration
  under test. Extracted `backoff_delay(base, attempt) -> Duration` as a pure
  function and test the arithmetic directly, including the factor cap at 1<<6
  and two saturation cases. Fix round 2 for Task 4.
  Cost if wrong: one small pure function, and the async path loses a test that
  was never testing anything.

Task 4: fix round 1/5 (usable_key extraction + tests, saturating_mul; 63518c1)
Task 4: fix round 2/5 (backoff_delay extracted, vacuous test deleted, two real
  tests added; 1e5f129) — round 2 existed only because MY round-1 test was
  vacuous, not because the implementer erred.
Task 4: re-review of 3c9ce33..1e5f129 — both findings ADDRESSED, no new
  breakage. Re-reviewer confirmed by mutation analysis that the cap test
  (attempts 7 and 20 both 640ms) catches dropping or changing .min(6), which a
  1/2/3-only test would not, and that both saturation assertions panic rather
  than mismatch under plain Mul.
Task 4: minor (deferred): from_env_is_none_without_a_key still uses
  std::env::remove_var, a process-global mutation on a threaded test runner.
  No live race today (it is the only test touching that var, verified twice),
  and usable_key's direct tests mean it is no longer the sole coverage of the
  rule. Flag for the final review if a later task adds a second env-touching test.
Task 4: complete (commits f6c40e0..1e5f129, review clean)

Tasks 5-7: batched into ONE dispatch. They are three small independent modules
  (verdict, cache, config) in the same crate, each with complete code in the
  plan and no dependency on one another. All three touch
  crates/stratify-jev-judge/src/lib.rs and Task 6 also touches that crate's
  Cargo.toml, so one implementer avoids the edit conflicts three would create.
  Three separate TDD cycles and three commits, one review surface.

Ruling (Task 11 fixture byte spans): corrected before dispatch. My
report-dead-code.json used start_byte 44 for `helper`, but the real offset is
43, so the span sliced "n helper(" rather than the function. Harmless today
because function_source widens to whole lines, which is exactly why no test
would ever have caught it, and precisely the kind of quietly-wrong fixture that
misleads whoever reads it next. Now the real spans: helper 43..72, used 0..41,
verified against the fixture file. Cost if wrong: none; the test asserts finding
counts, and the spans are now honest either way.

Tasks 5-7: review spec ✅ on all three, Changes Requested (1 Important, 2 Minor).
Tasks 5-7: ⚠️ resolved by controller — Co-Authored-By trailer present on all
  three commits; ~/dev/stratify working tree clean. The reviewer's third ⚠️
  (batch-prefix stripping) correctly belongs to Task 9 and is deferred there.

Ruling (Important, cache test never varies `judge`): finding stands. cache_key
  does hash judge today, but nothing proves it, so a refactor dropping that line
  passes the whole suite while letting two judges collide on one key and serve
  each other's verdicts. Added an assert_ne! varying judge.
  Cost if wrong: one assertion.

Ruling (Minor, NUL-delimiter framing): folded in, with a caveat. The reviewer
  is right that single-byte delimiters are weaker than length prefixes and that
  `model` is user-supplied from TOML. I could not construct a genuinely
  reachable collision, and I deliberately did NOT write a test claiming to
  demonstrate one: a test that cannot fail under the old code is precisely the
  vacuous-test defect this plan has already shipped twice. Changed to
  length-prefixed framing anyway, on a different argument: the cache is designed
  to be committed to git, so the hash input format can never be changed cheaply
  again. No entries exist yet, so this is the last free moment.
  Cost if wrong: none; the coverage is the judge/version/model/state/questions
  variation tests, which are honest about what they prove.

Ruling (Minor, verdict tests always passed an empty answers map): folded in.
  The raw probabilities in that map are the product's auditability guarantee
  (spec: never delete a finding, keep the judgment attached), so a test suite
  that never puts content in it leaves the spec's central promise unexercised.
  Judgment fixtures now carry two real answers and a new test asserts they
  reach the finding.
  Cost if wrong: one richer fixture.

Ruling (⚠️ escalated to a real gap): `Severity::step_down` has NO direct test
  anywhere. Task 1 never tested it and Task 5 reaches only Error->Warning and
  the Info floor through apply(), so Warning->Info is entirely uncovered. That
  is the common case in practice, since duplication findings are Warning. The
  reviewer named this as unverifiable-from-diff; checking the whole repo myself
  showed it is not merely unverifiable, it is untested. Added a direct
  three-tier step_down test plus a Weaken-from-Warning case.
  Cost if wrong: two small tests on the function Weaken entirely rests on.

Tasks 5-7: fix round 1/5 (4 addressed, 0 open — judge-variation assertion,
  length-prefixed feed() framing, real answers in verdict fixtures plus
  every_raw_probability_reaches_the_finding, step_down three-tier test plus
  weaken_moves_a_warning_finding_to_info; commit 04b35fa)
Task 5: complete (commit b92159e, amended by 04b35fa, review clean)
Task 6: complete (commit 12e0a69, amended by 04b35fa, review clean)
Task 7: complete (commit 6f5c969, review clean)
Batching verdict: worthwhile. Three tasks, one implement cycle and one review
  cycle instead of three of each, and the review still found a real Important.

Task 8: review spec ✅, Changes Requested, 2 Critical + 3 Important + 5 Minor.
  The strongest review of the session. It walked real findings through the
  pipeline by hand rather than reading for style.

Ruling (C1, Strengthen promotes live code): finding stands, fixed now, not in
  Plan 2. The `explanation` Choice opens by asserting "Nothing in this
  repository calls the function" as fact. For a cross-crate call the resolver
  missed, a model correctly rules out framework, test and public-API, leaving
  genuinely_unused as the literally correct answer to the question as posed.
  All three Nouls then sit below low_at and Strengthen fires, taking a function
  called at run.rs:72 from Info/Likely to Warning/Certain. The tool would tell
  a user to delete working code with more confidence than the engine had, on
  4+ of the 15 findings in the only labelled corpus we have.
  The diagnosis underneath is the valuable part: each Noul is individually
  correct, and together they say "no special reason this looks unused", which
  the policy reads as "therefore dead". The truth is "therefore the analyzer
  failed", and no question asks about analyzer failure, so that hypothesis has
  nowhere to collect probability and it flows into genuinely_unused.
  I had predicted a weaker version of this in the plan's own verification
  section and deferred it to Plan 2. That was wrong: it is not a wording
  nicety, it is the difference between a useful tool and a harmful one.
  Cost if wrong: a genuinely dead public library function can no longer be
  promoted past the engine's hedge. That is the right trade, since the state
  carries no evidence capable of outranking the engine's own uncertainty.

Ruling (C2, Weaken inert on the ground truth): finding stands, fixed now. The
  external_api branch fires precisely where it cannot help (public symbols
  already at the Info floor, where step_down is a no-op and confidence is
  already Likely) and cannot fire where it would (private helpers at
  Warning/Certain). Now Dismiss at the Info floor, Weaken above it.
  Cost if wrong: a public-API answer on an Info finding hides it rather than
  leaving it visible. That is the intended behavior for the engine's known
  weak spot.

Ruling (scope, fix now vs Plan 2): doing the safety half now and the evidence
  half as a second round, rather than deferring either. The stated purpose of
  Plan 1 is a skeleton the user tests live against this very repo. A skeleton
  that is either harmful (C1) or observationally inert (C2 plus everything
  landing on Keep) delivers nothing testable, so deferring would defeat the
  plan's own goal. Round 1 = stop being harmful. Round 2 = start being useful.
  Cost if wrong: Task 8 takes two more rounds and Plan 1 lands later.

Ruling (my plan drifted from my own spec): the spec's Judge trait at
  specs/...-design.md:99 already reads
  `fn judge(&self, finding: &Finding, answers, cfg)`. The plan dropped the
  finding parameter, which is the direct cause of C2's root cause (decide had
  no access to the engine's own verdict). The spec was right and the plan
  diverged. Plan now matches the spec again.

Task 8 fix round 1 covers: the Confidence::Likely guard on Strengthen, Dismiss
  at the Info floor for public_api, entrypoint wired to a verdict (M10, it
  previously mapped to nothing), engine_confidence added to the state, I3's
  mutation-catching test, M7's boundary test plus corrected threshold docs,
  M6's package.json parsing, M8's empty-backtick guard, M9's grammar.

Task 8: fix round 1/5 (both Criticals closed; e051e02). Implementer ran the
  mutation check: reverting the genuinely_unused guard to Some((_, conf))
  produced exactly one failure, the new test, with 45 others passing.

Task 8 fix round 2 scope: the evidence gap. The state carried no caller
  information of any kind, so a private helper called four times inside its own
  file was indistinguishable from dead code, and every question was correctly
  answered "no". Round 2 adds:
  - RepoContext::occurrences(name), a lazily-built identifier index. Built once
    per run, not per finding: N findings against M files would otherwise be N*M
    scans. Returns file, line and the trimmed source line.
  - RepoContext::in_test_context(file, line). #[cfg(test)] sits on the
    enclosing module, not the function, so nothing about the function reveals
    it. This is the decisive fact for 6 of the 15 findings.
  - A workspace marker in project_markers. `publish = false` almost never
    appears in a Cargo workspace, so external_api was reading an empty array on
    exactly the repo that produced the ground truth.
  - A fourth Noul, resolver_missed_a_call, anchored on the occurrence list
    rather than on generic analyzer fallibility. The reviewer warned that a
    generic "could an analyzer have missed a call?" is honestly "yes, somewhat"
    for every function including dead ones, and would absorb probability
    indiscriminately.
  - explanation reworded so the premise is a claim that may be wrong, plus a
    resolver_limitation option wired to Dismiss. M10 warned that adding an
    option without a verdict repeats entrypoint's mistake.
  - version() bumped to 2 so every version-1 cache entry, produced without any
    of this evidence, is invalidated.

Ruling (absent resolver answer): neither Dismiss nor Strengthen. My first draft
  used unwrap_or(1.0) to stop a stale cache arming Strengthen, but that would
  make a model omitting the answer dismiss every finding. Caught it before
  dispatch. Absence is now unknown: Dismiss needs a real high answer,
  Strengthen needs a real low one, and absence falls through to Keep.
  Cost if wrong: a finding whose resolver answer failed to parse stays as the
  engine reported it, which is the correct default everywhere else in this tool.

Ruling (round 2 brief was incomplete): the implementer stopped and asked rather
  than reconstructing the identifier scanner and brace-depth heuristic from my
  prose. Correct call, and the right instinct given both round-1 Criticals were
  precise-logic defects. Cause: I patched the context.rs code into the plan's
  Task 2 section, because that is where context.rs is defined, so extracting
  Task 8's section alone never picked it up. Wrote
  task-8-context-supplement.md with the exact code and tests rather than
  restructuring the plan, since the plan's organization by file is correct and
  it is only the brief extraction that is per-task.
  Cost if wrong: one extra file the implementer reads alongside the brief.

Task 8: fix round 2/5 (evidence dossier; d603918). 72 tests workspace-wide.
  Implementer ran my own described first-draft bug as a real mutation
  (is_some_and -> unwrap_or(0.0)) and got exactly one failure, the test written
  for it. Also hand-traced the resolver-hit test's reason-text assertion to
  confirm it pins that specific branch rather than any path to Dismiss, since
  the resolver_limitation Choice branch also Dismisses but with other text.
Task 8: accepted deviation: `type IdentifierIndex` alias, introduced because
  clippy::type_complexity fired on the raw RefCell<Option<HashMap<..>>>.
  Spelling only.
Task 8: accepted deviation: config.rs `resolver_at` written by the implementer
  from my prose, since neither document carried exact code for it. Its own
  judgment call, correctly distinguishing a scalar mirroring four existing
  fields from the context.rs gap, which was real algorithm design it refused to
  guess at. That distinction is exactly right.

Task 8: re-review of 3351577..d603918 — all 10 findings ADDRESSED. Ground-truth
  walk, reconstructed over the real ~/dev/stratify index (41 in-scope files):
  all 15 findings support Dismiss, 0 reach Strengthen. The 4 cross-crate ones
  are blocked structurally by the Confidence::Likely gate; the other 11
  evidentially. in_test_context is true for all six test helpers, and the
  cascade shape now arrives with two literal `.with_resource(resource(config))`
  call sites. That is the number that decides whether this judge works, and it
  was measured rather than argued.
  Fresh: no Critical, 3 Important, 12 Minor.

Ruling (F1, opens_test_block false positives): fix now. Measured true for
  `#[cfg(test)] use tempfile::tempdir;`, `mod tests;`, `mod testsuite;` and a
  Python `class TestRunner:` sitting above production functions. The flag armed
  at the opener's depth and only disarmed on a closing brace at that depth,
  which is the NEXT function's own brace, so that function read as test code
  and got dismissed. This is the original Critical reversed: silently deleting
  a real finding instead of promoting real code. Now an opener must actually
  open a braced block, and `mod tests` matches as a whole word so `mod
  testsuite` no longer qualifies. Python openers dropped entirely: its blocks
  are indentation-delimited so brace counting can never close them, and
  path_is_tests covers the usual layouts.
  Cost if wrong: a Python test class not under a test-shaped path is missed,
  and its helpers stay visible. Visible beats silently deleted.

Ruling (F2, samples chosen by path order): fix now. Measured: `span` has 183
  occurrences and all eight samples were struct-field lines from files sorting
  before lib.rs, not one a call. The resolver question asks whether the listed
  entries read like calls, so showing eight that do not, while real calls
  exist, steers the answer to the one that satisfies the Strengthen
  precondition. Round 2 added evidence; without ranking it was systematically
  the wrong evidence. Now ranked by call-likeness, then in-file first.
  Cost if wrong: looks_like_call is a substring hint, so a call written in an
  unusual form ranks below a coincidental match. It reorders, never filters.

Ruling (F3, F5, F7, F8, F9, F11, F12, F13, F14): folded into round 3. All are
  small and several are correctness, not polish: F5 reported token counts as
  site counts, F7 made non-ASCII identifiers unfindable so the state asserted
  "no caller" for functions that have them, F9 had the weaker public-API branch
  shadowing the stronger resolved-call evidence.

Deferred to Plan 2 with reasons recorded: F4 (index holds the whole repo;
  32,118 token entries on 41 files, linear in repo size, needs a keyword or
  length filter or on-demand indexing), F6 (two in_test_context false
  negatives from brace counting inside string literals; the doc comment already
  accepts this as the heuristic's cost), F10 (the Choice gate reads the
  distribution's confidence rather than the winner's margin, so a thin
  plurality can clear 0.70; needs probabilities plumbed through jev-client),
  F15 (exact-line declaration filter costs one sample slot when a span starts
  at a doc comment).

Task 8: fix round 3/5 (dd443b2). 77 tests workspace-wide.

Ruling (my round-3 opens_test_block was wrong, implementer corrected it): the
  `following` fallback I supplied applied to every test-naming line, not only
  to `#[cfg(test)]`. The implementer ran my code as its RED step, hit the
  supplement's own b.rs case (`mod tests;` above a production function whose
  `{` armed the flag through the fallback), and scoped the fallback to the
  attribute branch. Correct: `#[cfg(test)]` is an attribute applying to the
  next item, while `mod tests;` and `describe(...)` are complete statements
  that never need a lookahead. The function's own doc comment already said the
  fallback existed for the attribute case; my code just did not enforce it.
  Plan text corrected to match the shipped code.
  That is three rounds in a row where running the supplied code rather than
  trusting it caught something.
Task 8: accepted deviation: F12's config assertions written by the implementer
  from prose, matching the existing test's style. Same judgment call as
  resolver_at in round 2, and the same correct line between a scalar assertion
  and real algorithm design.

Task 8: round-3 re-review — all 11 findings ADDRESSED, Approve, no new breakage.
  Reviewer independently re-derived the for_each_identifier panic-safety
  argument (a continuation byte is always is_ident, so it can never become a
  run start or end) rather than accepting it, and hand-traced my literal
  supplement code against its own b.rs case to confirm it returns true and
  arms the very false positive F1 existed to kill. The implementer's narrower
  scoping is endorsed as correct, not a shortcut.
Task 8: deferred (noted by re-review, both fail safe): `mod tests` and
  `describe(` with the opening brace on the following line are no longer
  detected. Cost is a genuine test helper getting Keep instead of Dismiss,
  never a production function losing a true positive. `describe(` blocks live
  almost exclusively in files path_is_tests already matches. Worth a follow-up,
  not a blocker.
Task 8: complete (commits 04b35fa..dd443b2, three fix rounds, review clean)
  Measured outcome: 15 of 15 ground-truth findings support Dismiss, 0 reach
  Strengthen.

Task 9: review spec ✅, Changes Requested, 1 Critical.

Ruling (C1, batched questions bind to nothing): finding stands, fixed now. The
  driver wrapped each finding's state under `finding_{slot}` and prefixed
  question NAMES, but never touched question TEXT. So a ten-finding batch sent
  a state whose only top-level keys were finding_0..finding_9 and fifty
  questions in ten byte-identical groups, each asking about a bare `function`
  key that does not exist at that level. Nothing on the wire bound
  s7__framework_invoked to finding_7. Answers would return by slot, the driver
  would faithfully apply each to the right finding, and a verdict formed from
  the wrong function would silence the wrong function. No crash, no log.
  This fell exactly between two tasks: Task 3's brief showed the example
  question as "`finding_0.function`", Task 8's report noted it left text
  unqualified "as instructed for Task 9 to add", and Task 9's brief specified
  only the name prefix. I wrote all three.
  No test caught it because all four async tests use a single finding, so
  `s1__` never appears anywhere in the suite and nothing inspects a request
  body.
  Fix: judges emit `{root}.field` placeholders, Question::with_state_root
  renders `finding_{slot}` per slot, driver calls it alongside the name prefix.
  Criteria deliberately not rendered: they say what counts as an answer, never
  where evidence lives.
  Cost if wrong: batch_findings would have to drop to 1, costing roughly ten
  times the requests.

Ruling (a second bug found while fixing C1): the `test_only` question never
  referenced `in_test_context`. My round-2 edit that was meant to add it used a
  conditional expression that silently no-opped, because the M9 grammar fix had
  already rewritten the line it was matching on. So round 2 added the decisive
  evidence for 6 of 15 findings to the state and then never asked about it.
  Found only because C1 forced me to read all five question texts literally.
  Now fixed in the same round.
  Cost if wrong: none; the question now names evidence the state already
  carries.

Task 9 fix round 1 also adds the send-side test the review asked for
  (`a_batch_binds_each_question_to_its_own_finding`, two findings, inspecting
  the actual request body through wiremock's received_requests). Every prior
  async test hard-codes `s0__` in the mock response, so they prove the strip
  and say nothing about what went out.

Ruling (I3, partial response cached as complete): finding stands, folded in.
  The only guard was `answers.is_empty()`, so a response missing one of five
  answers wrote four under the key computed from the full five-question set.
  In the moment that is harmless, because decide refuses to act on a missing
  answer. The durable part is not: the cache is designed to be committed, so a
  truncated body is replayed forever and the finding is never asked about
  again, escapable only by a version bump that discards every other entry too.
  Now gated on the judge's canonical question set being fully present. Applying
  a partial set stays allowed; persisting one does not.
  Cost if wrong: a flaky partial response costs one repeated request per run
  instead of being silently frozen into the repo.

Ruling (M5, token estimate ignored the questions): folded in. The driver ships
  the whole question set once per slot, roughly 4 KB for dead_code, so a
  ten-slot batch carries about 10k tokens the ceiling never saw. A batch
  measured at 23k against a 24k ceiling was well past the real 32k API limit,
  and the doc comment claimed headroom. Estimate now counts state plus one
  question-set copy per slot, which is exactly what each slot ships.
  Cost if wrong: batches get smaller, so more requests and a little more
  latency. Overshooting the API's state limit returns 422 for the whole batch.

Ruling (I4, failure isolation untested): folded in. The existing test put both
  findings in ONE batch, so it proved "the request failed", not "a failed batch
  leaves its peers alone", which is the reason `failed` is its own counter. New
  test runs batch_findings = 1 with a one-shot success mock and a catch-all
  500, asserting on the pair (one judged, one byte-identical) since the two
  tasks race.

Ruling (M6, M7): folded in. The `.max(1)` in plan_batches is load-bearing and
  reachable from user config, since JevConfig validates nothing: without it an
  empty batch becomes a request with no state and no questions. And apply_one
  counted the judgment before writing it, a shape where run can report a
  judgment it never applied and the CLI prints a number with nothing behind it.

Reviewer's warnings for the {root} fix, all checked against what I wrote:
  - Do not search-replace the bare word `function`: the test_only text says
    "the function in `{root}.function`", prose and key kept distinct. OK.
  - Render Choice instructions too: with_state_root covers all three variants.
  - Do not prefix Choice criteria keys, decide compares them as literals:
    with_state_root renders instructions only. OK.
  - Render on the cloned question in the send loop only, or the cache key
    picks up the slot: cache_key uses the canonical `questions` map, rendering
    happens when inserting into `qs`. OK.
  - Nothing else on the wire is ambiguous across slots. Confirmed, stop looking.

Task 9: fix rounds 1-2 (bbd1307, abff904). 91 tests workspace-wide.
Task 9: implementer found a defect in my own test fixture while implementing:
  the cache test's mock response carried four of the five canonical answers, so
  under the new completeness gate nothing was ever cached and the second run
  broke the mock's .expect(1). It chased the failure to its cause rather than
  loosening the assertion. Fixed in the brief too.
Ruling (the completeness gate turned out to be testable after all): I told the
  implementer the I3 gate probably had no cheap test and asked it to think of
  one. Its .expect(1) failure proved the opposite: a response missing a
  canonical key changes whether a second run hits the network, which is
  directly observable. Added a_partial_answer_set_is_applied_but_never_cached,
  which the implementer then TDD'd against the live mutation: written while the
  gate was removed, watched fail with from_cache 1 instead of 0, gate restored,
  watched pass. Three independent assertions catch its removal.
  Cost if wrong: one more test on a path that is otherwise invisible.
Task 9: accepted correction to my mutation prediction: dropping with_state_root
  fails on the `contains("finding_0.")` assertion, not on `assert_ne!(q0, q1)`,
  because total removal leaves both slots holding the literal `{root}`. The
  assert_ne still earns its place against the subtler mutation where both slots
  render with one fixed root. The implementer's reading is better than mine.

Task 9: re-review of 7a3510c..abff904 — all 7 findings ADDRESSED, all 4 traps
  cleared, no new breakage. Reviewer noticed HEAD had moved past the stated head
  and verified both versions of the one changed test rather than reviewing a
  stale tree. It also checked forward: registry() holds only DeadCodeJudge, so
  no sibling judge is left un-rendered and silently broken in a batch. That
  becomes a real hazard in Plan 2 when four more judges land.
Task 9: minor (deferred): with_state_root rebuilds every JSON node even when no
  placeholder exists. Behaviorally correct and negligible for single short
  instruction strings.

Ruling (my cross-fixture port was wrong, implementer corrected it): I folded
  the implementer's 4-of-5 fixture into the brief but carried the
  `verdict == "keep"` assertion over from my 1-of-5 version. Wrong on the new
  data: decide's early-return-to-Keep fires only when framework_invoked,
  test_only or external_api is ABSENT, and in the 4-of-5 fixture all three are
  present with framework_invoked at 0.95, so it legitimately dismisses. The
  implementer ran it, got `left: "dismiss", right: "keep"`, and corrected the
  expectation rather than mutating the fixture's probabilities, which is the
  right way round: the fixture's realism is why we chose it over the
  degenerate one.
  That is four defects I introduced into this single small test across two
  rounds: the missing canonical key in the cache mock, replacing a better
  fixture with a worse one, the miscredited `judged` assertion, and this
  mis-ported verdict. Each was caught by an implementer running the code
  rather than transcribing it. The pattern is the lesson, not the test.
  Cost if wrong: none; the assertion now matches what the fixture actually
  exercises.
Task 9: complete (commits dd443b2..bd0c0c2, four fix rounds, review clean)

Controller end-to-end verification of the release binary at 5e1f8dc, run
against the real 73-finding report from `stratify check .` on ~/dev/stratify:
  --dry-run                -> "2 request(s) planned, nothing sent."
                              (15 dead_code findings at batch_findings 10)
  no TYPESAFE_API_KEY      -> 73 findings pass through unchanged, exit 0,
                              one stderr line
  --fail-on warning        -> exit 1 (the 40 duplication warnings survive)
  --format json            -> judged_by "stratify-jev/0.1.0", 73 findings kept,
                              schema_version 1 preserved
  --root /no/such/dir      -> exit 2, "cannot read /no/such/dir: not a
                              directory". This path works only because of the
                              Task 2 ruling that made RepoContext::new return
                              NotFound instead of silently scanning an empty
                              inventory.
  schema_version 99        -> pass-through with a warning, exit 0
  garbage on stdin         -> exit 2, "the input is not a Stratify JSON report"
The binary works end to end. What remains is the Tasks 10-11 review, the final
whole-branch review, and a live run with a real key.

Tasks 10-11: review Task 10 ✅, Task 11 ❌, Changes Requested, 1 Critical.

Ruling (Critical, an unreadable --root swallowed the report): finding stands,
  fixed now. The report is already parsed and validated at that point, and the
  branch threw it away with a hardcoded exit 2, ignoring --fail-on entirely. A
  CI job with a misconfigured checkout path and --fail-on never would hard-fail
  anyway, and a downstream SARIF converter reading the pipe would get nothing.
  My own brief and my own commit message both claimed this path passes the
  report through; the code did not.
  Worse: I tested this exact invocation an hour earlier, saw `exit=2` with a
  clean message, and recorded it in this ledger as correct. I verified that
  something happened rather than that it matched the guarantee I had written.
  That is the same failure mode as a test asserting the code ran rather than
  that it worked, which this review chain has caught four times in others'
  work and now once in mine.
  Now mirrors the missing-key path: warn on stderr naming that nothing was
  judged, print the report, return exit_code(). Two integration tests pin it,
  one on stdout content and one on --fail-on never.
  Cost if wrong: a typo'd --root produces a passing gate plus a stderr warning
  rather than a hard failure. The Task 2 NotFound ruling still ensures the
  warning exists, so the typo is not silent.

Ruling (Important, --dry-run ignored the cache): fixed. plan_only counted every
  dead_code finding and divided by batch size, with no view of the cache at
  all, while the README recommends committing the cache so CI sends nothing.
  The preview and reality diverged precisely in the documented steady state.
  Replaced with Driver::plan, which runs the same prepare and cache-split path
  as run and counts only misses. Two tests, one of them answering a finding for
  real and then asserting the plan drops to zero.
  Cost if wrong: dry-run now builds state for every finding, so it is slower
  than an arithmetic estimate. It is a preview run by hand, not a hot path.

Ruling (Important, the exit-code guarantee was untested): fixed. "A dismissed
  warning does not fail a build" is the single behavior the CLI exists to
  prove, and the only test touching it ran without an API key, so nothing was
  ever judged and the finding survived because judging never happened. Added a
  test asserting two findings identical in severity and opposite in visibility,
  separated by confidence alone.

Ruling (Minor, the JSON test was vacuous): fixed. It built an empty report and
  asserted findings was an array, which passes whether or not anything is
  dropped. Now builds a genuinely dismissed finding and asserts it survives
  with its verdict, its raw probabilities and its original severity intact.
  The reviewer noted the guarantee holds by construction since json::render
  takes no threshold argument. True, and a test that demonstrates it still
  earns its place, because the next person may add one.

Ruling (Minor, tempfile): dropped, against the implementer's "use verbatim"
  reading. Verbatim is the right default for values encoding a decision, such
  as thresholds and wire strings. It is not a reason to keep a dependency that
  provides no behavior and no coverage.

Ruling (Minor, --dry-run always exits 0): kept, documented in the README rather
  than changed. It is a preview, not a gate.

Tasks 10-11: fix round 1/5 (6 addressed, 0 open; commits e47c963, b4c525d).
  Re-review verified the bad-root path by running the binary, not only reading
  it: --fail-on never exits 0 with the report on stdout, --fail-on warning
  exits 1. Controller confirmed the same independently against the real
  73-finding report.
Tasks 10-11: minor (deferred): Driver::plan duplicates run's prepare and
  cache-check loop rather than sharing a helper, so the two can drift. Mitigated
  today by a_cached_finding_is_not_counted_in_the_plan, which round-trips a real
  cache write through both paths and breaks under most divergences, but that
  protection relies on someone running that test after editing either loop.
  Deferring rather than refactoring the driver's core loop at the end of the
  plan; flagged to the final review for triage.
Tasks 10-11: minor (deferred): the visibility test's two findings differ in
  confidence and also in whether `extra` carries a judgment. Inert today since
  visible() reads only confidence, but a latent ambiguity if it ever grows
  judgment-awareness.
Task 10: complete (commit 7c57de3, amended by b4c525d, review clean)
Task 11: complete (commit 5e1f8dc, amended by e47c963 and b4c525d, review clean)

ALL 11 TASKS COMPLETE. 108 tests, clippy clean under -D warnings.

FINAL WHOLE-BRANCH REVIEW: ship with fixes. Two Critical, three Important, ten
Minor. Reviewer verified against a fresh engine scan and dumped the real
state_for output for all 15 findings: resource and header_map arrive with their
two in-file call sites, sample_finding with in_test_context true and four real
callers, run_stdio with main.rs:117 and :124 ranked first. The evidence layer
works on real data.

Ruling (C1, every ClientError discarded): fix now. `Err(_) =>` drops the error
  entirely, and ClientError is referenced nowhere outside client.rs. A typo'd
  key 401s every batch and the user sees "2 request(s) failed, those findings
  are unchanged", with no mention of authentication, no --base-url to point at
  a proxy, and --verbose adding only counters. The tool's own failure mode is
  invisible by construction, and this is the likeliest first-run mistake.
  Cost if wrong: a few lines of error plumbing and one new flag.

Ruling (C2, a wrong --root produces uniformly empty state): fix now. Measured:
  every finding comes out with source "", empty imports, repo_wide 0, and no
  warning. repo_wide 0 is the exact input the resolver question's "no"
  criterion describes as proof of no caller, so the model is asked five
  questions about a function it cannot see, lands on genuinely_unused, and on
  an engine-Certain finding that is a confident Strengthen derived from an
  empty file read. Then it is cached, and the README says to commit the cache.
  RepoContext::new already rejects a root that is not a directory; the gap is a
  root that exists but is the wrong one, which is the common mistake.
  Cost if wrong: a warning fires on a repo where most findings legitimately
  reference generated or moved files.

Ruling (I2, --show-dismissed changes the exit code): fix now. Measured, same
  report: exit 0 without the flag, exit 1 with it. A CI job adding it for
  fuller logs starts failing on findings the model dismissed. exit_code will
  gate on confidence directly rather than through a presentation-layer
  predicate that also honors the display override.

Ruling (deferred #4, remove_var): fix now, as the reviewer triaged. It races
  reqwest's own proxy-environment reads on a threaded runner, which is worse
  than the test-interference risk originally recorded. usable_key is tested
  directly, so the env-mutating test is deletable outright.

Ruling (I1, one unmodelled answer shape kills the whole batch): Plan 2. Real
  and well-argued: untagged Answer requires every derived field, so one odd
  answer fails the whole SystemOneResponse, becomes Transport, becomes a lost
  batch of ten. The spec says a missing answer is per-answer Keep, and the
  implementation escalates it to per-request. Deferring because the fix is a
  wire-format change to jev-client that wants its own review, and C1's error
  reporting makes it diagnosable in the meantime rather than silent.

Ruling (I3, occurrence index is name-only and repo-wide): Plan 2. Measured: the
  `span` test helper collects 128 repo-wide hits, six of eight samples being an
  unrelated walk::span in another crate, and looks_like_call ranks those first.
  Systematic false-Dismiss pressure on common names. Fails safe (nothing is
  deleted) but it is the mechanism most likely to produce a wrong verdict on a
  real repo, and the reviewer calls it the single change most likely to move
  eval numbers. That makes it Plan 3's eval harness work, not a blind fix now.

Ruling (deferred #10, Choice gate reads distribution confidence): CLOSED, and I
  was wrong to accept it. The reviewer checked the TypeSafe docs: Choice
  confidence is distribution concentration, so a thin plurality scores LOW, not
  high. The original finding's premise was inverted and I carried it forward
  without checking.
