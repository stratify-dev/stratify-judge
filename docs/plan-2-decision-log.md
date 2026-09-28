# Plan 2 decision log

Every ruling made while making this tool model-agnostic, in the order made,
with what each one costs if it turns out wrong. Kept because the reasoning
behind a decision outlives the decision, and because several of these
rulings overturned the plan's own text.

Plan: `docs/plans/2026-09-27-model-agnostic-judge.md`
Spec: `docs/specs/2026-09-27-model-agnostic-judge-design.md`

---


Spec: docs/specs/2026-09-27-model-agnostic-judge-design.md (read, binding authority)
Repo: ~/dev/stratify-jev, branch `model-agnostic-backend` off `master`.

Ruling: work happens on a branch, not master. The skill forbids implementing
on master without explicit consent, and master is already published, so
branching is the compliant path rather than a question to ask.
Cost if wrong: one merge at the end instead of none.

Model policy this run (partner asked for lower models where possible):
implementers on the cheapest tier wherever the plan carries complete code,
standard tier for the two tasks needing multi-step judgment (5 and 7),
reviewers at the documented mid-tier floor. Reviewers are NOT lowered: a
cheap reviewer on logic changes would defeat the reason for choosing this
mode, and Plan 1's reviews caught five Criticals.

## Pre-flight conflict scan

### Cross-task pairs sharing a file or interface

| Pair | Produces → Consumes | Finding |
|------|---------------------|---------|
| 1 → all | crate names, binary name | Clean |
| 2 → 3 | `Backend` fields; `client_for` placed in backend.rs | Clean; the cycle-avoidance deviation is recorded in the plan's self-review |
| 2 → 4 | `Backend` as a `cache_key` parameter | Clean |
| 2 → 5 | `Backend::token_ceiling()` | Clean |
| 2 → 6 | `resolve_backend` signature | Clean |
| 2 → 5,6 | `JevConfig` → `JudgeConfig` rename reaches later test fixtures | Clean; Task 2 Step 4 sed covers the workspace and Tasks 5/6 use the new name |
| 3 → 4,5,6 | `Client::new(base, Option<String>)` breaks every call site | Clean; Task 3 Step 6 owns the fixes and runs before 4 |
| 4 → 5,6 | `Driver::new` gains a 4th parameter | Clean; Task 5 tests pass four args, Task 6 passes `backend.clone()` |
| 5 → 6 | `run`/`plan` become `Result` | Clean; Task 6's dry-run handles both arms |
| 6 → 7 | binary name drives the formula name | Clean |

### Per-task internal agreement

| Task | Tests vs code vs files | Finding |
|------|------------------------|---------|
| 1 | Baseline count vs the diff check | **F1**: says "four ok lines totalling 118"; the run emits six result lines, four carrying tests |
| 2 | Test TOML vs the type being deserialized | **F2**: tests parse `JudgeConfig` from TOML carrying a `[judge]` header, but `JudgeConfig` is the INNER type, so serde would look for a field named `judge` and fail |
| 3 | Test matcher vs the wiremock version in the lock file | **F3**: uses `header_exists(..).not()`; wiremock 0.6.5 has `header_exists` but no `not()` and no negation matcher at all, so it would not compile |
| 4 | Clean | |
| 5 | `check_fits` message vs what the fixture produces | Clean; `report(n)` messages carry `` `fn0` `` and `function_name` extracts it |
| 6 | Clean | |
| 7 | Clean; `dist init` writes the release workflow and the step forbids hand-writing it | |

### Rulings

Ruling (F1): reworded to name six result lines and to say plainly that the
`diff` is the check, not the prose count. A wrong count in a comment invites
an implementer to "fix" a passing baseline.
Cost if wrong: none.

Ruling (F2): test TOML drops the `[judge]` header, because these tests
exercise the inner type while production reads the header through the
wrapper in `JudgeConfig::load`. The alternative, deserializing the wrapper in
the test, would test serde's nesting rather than the resolution logic the
test is about.
Cost if wrong: the tests would not have covered the wrapper path, which
`JudgeConfig::load`'s own existing tests already cover.

Ruling (F3): the no-key test inspects `received_requests()` instead of
matching on an absent header. Verified against the vendored source of
wiremock 0.6.5: `header_exists` exists, `not()` does not, and there is no
`Negate` matcher. The captured-request approach is already used elsewhere in
this repo for the batch-shape test, so it is a proven pattern here rather
than a workaround.
Cost if wrong: none; it asserts the same property more directly.

All three fixed in the plan before Task 1 dispatch.

## Task log

Ruling (my Step 6 verification was broken, not the rename): the implementer
  correctly reported a non-empty diff, as instructed. The cause was my check,
  not the code: each `test result` line carries its own elapsed time, and cargo
  runs crate test binaries in a nondeterministic order, so raw output never
  matches twice even on identical code. Verified properly by stripping timing
  and sorting: the post-rename tree hashes a09448ff, and a worktree of master
  at the pre-rename commit hashes a09448ff too. The rename changed nothing.
  Plan Steps 1 and 6 corrected so a re-run does not hit the same false alarm.
  Cost if wrong: none; the corrected check is strictly stronger, since it
  compares against master rather than against an earlier run of the same tree.

Task 1: review spec ❌, Changes Requested, 1 Critical + 3 Important + 1 Minor.

Ruling (Critical, the CLI tests never exercised the renamed binary): finding
  stands, and it invalidates my own verification. 13 call sites in
  crates/stratify-judge-cli/tests/cli.rs name cargo_bin("stratify-jev"), which
  is no longer a build target. They pass only because a stale pre-rename binary
  from Step 1's baseline build (dated Sep 20) still sits in target/debug/.
  Verified myself: moving that artifact aside makes all 12 fail with
  "CARGO_BIN_EXE_stratify-jev is unset, available binary names are
  stratify-judge".
  Worse, I told the reviewer this was "independently verified identical to
  master". It was not. Both of my runs leaned on artifacts, and on master
  cargo_bin("stratify-jev") is legitimately correct, so the matching hash was a
  coincidence between two different mechanisms rather than evidence.
  Cost if wrong: none; a fresh clone or CI would have failed all 12.

Ruling (root cause, my Step 4 grep could not fail): the verification pattern
  ended its last alternative in a hyphen, `stratify-jev-`, so it could not see
  `"stratify-jev"`, `stratify-jev:` or `stratify-jev/`. Measured 26 surviving
  occurrences across those three shapes. This is the same defect class as
  Step 6's timing diff: a check that passes whether or not the work was done.
  Two of them in one seven-step task is a pattern in how I write verification,
  not two unrelated slips. Step 4 now greps without a trailing-character
  requirement and Step 6 requires `cargo clean` first.
  Cost if wrong: the grep now also matches the legitimate historical
  references, which the step's prose enumerates so they are not "fixed".

Ruling (3 Important, all the same cause): clap's #[command(name)], 10
  eprintln! prefixes, and json.rs's judged_by literal all self-identify as
  stratify-jev. The reviewer ran the built binary to prove each. All fixed by
  the added hyphenated sed pass.

Ruling (Minor, the repository URL): fixed rather than deferred, and the
  ambiguity is now resolved. The reviewer correctly noted the URL might still
  be accurate. I renamed the GitHub repo to stratify-dev/stratify-judge and
  updated the remote, which the partner's request explicitly authorised, so the
  URL is now stale and gets updated in this round. GitHub redirects the old
  path.

Ruling (my "amend rather than add a commit" instruction broke the scoped
  re-review): a scoped re-review needs FIX_BASE..HEAD where FIX_BASE is the
  head the previous review saw. Amending replaces that head, so the range
  stopped existing and review-package refused it. The reviewed object still
  lives in the reflog, so I produced the scoped diff with `git diff bdaa053
  0c87def`, which is well defined for any two commits regardless of ancestry.
  For the remaining six tasks: fix rounds ADD a commit, never amend.
  Cost if wrong: a slightly longer history on tasks that needed a fix round,
  which is a fair price for a reviewable fix diff.

Task 1: fix round 1/5 (5 addressed pending re-review — 13 cargo_bin call sites,
  clap command name, 10 eprintln prefixes, judged_by literal, repository URL;
  commit bdaa053 amended to 0c87def)
Task 1: controller verified by execution after cargo clean: judged_by is
  stratify-judge/0.1.0, diagnostics print "stratify-judge:", --version prints
  stratify-judge 0.1.0, no `stratify-jev` survives in crates/ or Cargo.toml,
  the stale artifact is gone, and 118 tests pass from a clean build.
Task 1: re-review — all 5 findings ADDRESSED, no new breakage. Re-reviewer
  confirmed the hyphenated sed did not overreach: jev-latest, [jev], JevConfig
  and the jev: reason-line label are all untouched, so Task 2's scope and the
  model ids on the wire are intact.
Task 1: note for Task 2 — the sed also renamed the config file from
  stratify-jev.toml to stratify-judge.toml, which Task 2 Step 4 expected to do
  itself. Already done; Task 2 should not be surprised to find it.
Task 1: complete (commits de770b4..0c87def, review clean)

Task 2: review spec ✅, quality Approved, 0 Critical/Important, 2 Minor, both
  plan-mandated (the reviewer diffed the code character-for-character against
  my brief and confirmed it is what I specified).

Ruling (Minor 2, --base-url cannot rescue a config table with no url): fixed,
  not deferred, even though Minors do not normally enter the loop. It is
  plan-mandated so it is mine to rule on, and it is a genuine limitation: a
  table setting only `state_tokens` is a reasonable thing to write, since an
  endpoint's capacity is stable while its address moves between environments.
  My code checked for an empty url before applying the flags, so that
  configuration was unusable. The check now runs after the flags and the
  message names --base-url as an option.
  Cost if wrong: a typo'd backend name with a url flag now resolves to a
  working custom backend rather than erroring. The CLI reports the resolved
  backend, so that is visible.

Ruling (Minor 1, the no-url error path was untested): closed by the same fix.
  Two tests added, one for the error and one for the flag rescuing it, so the
  reorder is pinned in both directions rather than just asserted.
Task 2: fix round 1/5 (2 addressed, 0 open — url check moved after the flags,
  two tests pinning the error and the rescue; commits 2b1a3d3..97dfdea)
Task 2: re-review — both ADDRESSED, no new breakage. The rescue test asserts
  the table's state_tokens, api_key_env and api_key_required all survive flag
  application, not just that the url took effect.
Task 2: complete (commits 0c87def..97dfdea, review clean)

Task 3: review spec ✅, quality Approved, 0 Critical/Important, 2 Minor.

Ruling (both Minors are one fix): the reviewer flagged that client_for's
  empty-key filter is untested, and separately that the same "trim-empty is
  not usable" rule is implemented twice, once as the private `usable_key` in
  the client crate and once inline in client_for. Those are the same defect.
  The untested one cannot be closed on its own: testing client_for's
  env-reading path needs env mutation, which this task's brief forbids and
  which a previous task had to delete a test over, because writing the process
  environment races reqwest's proxy-variable reads on a threaded runner.
  So: make `usable_key` public and re-export it, and have client_for call it.
  That gives one definition in one place, already covered by the client
  crate's existing direct tests for None, empty, whitespace and verbatim
  preservation, with no new env-touching test.
  Cost if wrong: one more public item on a crate the spec intends to publish
  standalone, which is defensible since the rule is part of its auth contract.

Ruling (from_key not yet redundant): the reviewer noted from_key/from_env may
  become dead once Task 6 wires the CLI to resolve_backend + client_for, and
  asked whether it is already a near-duplicate. Keeping it for now: it is the
  CLI's live construction path until Task 6 changes that, so deleting it here
  would break the build for three tasks. Task 6 should remove whatever it
  orphans, and the final review will catch it if Task 6 does not.
Task 3: fix round 1/5 (2 addressed, 0 open — usable_key made public and shared,
  inline duplicate removed; commits 489c1ba..5687bb6)
Task 3: re-review — both ADDRESSED, no new breakage. usable_key's body
  unchanged, only widened; the surviving trim().is_empty() in backend.rs is the
  doc comment explaining the dedup.
Task 3: complete (commits 97dfdea..5687bb6, review clean)
Task 3: note for Task 6 — from_key/from_env/from_env_at remain the CLI's live
  construction path and become orphaned once Task 6 wires resolve_backend +
  client_for. Task 6 should delete whatever it orphans.

Task 4: review spec ✅, Changes Requested, 1 Critical + 1 Minor.

Ruling (Critical, two sources for "which model"): finding stands, and it is a
  plan defect of mine, not an implementer error. Task 4 moved the cache key to
  self.backend but left driver.rs:250 sending Some(self.cfg.model.clone()) and
  driver.rs:343 labelling judgments from cfg.model. Before this task both read
  the same field and could not disagree.
  Reachable today with no flag: set [judge] model = "jev-preview" in config and
  the request asks jev-preview while the key hashes Backend::jev()'s
  jev-latest, so jev-preview's verdicts are filed under a jev-latest key in a
  cache the README tells you to commit. Unset the knob and re-run: cache hit,
  wrong model's answers served as the default's.
  Worse after Task 6: --backend laya resolves model None, meant to omit the
  field per Task 3's skip_serializing_if, but the request would still send
  Some("jev-latest") to Laya. That defeats Task 3 entirely.
  The reviewer read Tasks 5, 6 and 7 in full and confirmed my plan never
  migrates that line, so this was not "fixed one task later"; as written it was
  never fixed. Verified all four claims myself against the source.
  Fix: the request takes self.backend.model.clone() (already Option, so a
  model-less backend omits the field), apply_one's label prefers the response
  then the backend's model then the backend's name, and JudgeConfig.model is
  deleted outright since --model and [judge.backends.<name>] model supersede it.
  Cost if wrong: removing a documented config key. The tool is unreleased with
  no tags, so there is no compatibility burden, and the replacement is strictly
  more expressive.

Ruling (the invisible-by-construction point is the real lesson): every
  driver.rs test pairs JudgeConfig::default() with Backend::jev(), whose model
  values coincide, so 135 passing tests could not see the split. The fix
  therefore ships with two tests that inspect the actual request body: one
  pinning a backend model that differs from the config's, one asserting a
  model-less backend sends no model key at all.

Ruling (Minor, misleading comment indentation in cache.rs): deferred to the
  final review. It is a comment visually trail-aligning under the previous
  line while documenting the next one. Real but cosmetic, and fmt-clean, so it
  is not an artifact that will spread.
Task 4: fix round 1/5 (1 Critical addressed, 0 open — request and cache key both
  read self.backend, JudgeConfig.model deleted, two request-body tests, README
  corrected; commits cb244e5..c062bb8)
Task 4: re-review — C1 ADDRESSED, no new breakage. The re-reviewer traced the
  counterfactual rather than trusting the assertion: reverting only the
  request-line change makes the crux test fail, so it would have caught the
  original bug. Confirmed no remaining path by which the request's model and
  the cache key's model can differ.
Task 4: minor (deferred): a comment in cache.rs is indented so it visually
  trails the previous line while documenting the next one. Cosmetic, fmt-clean.
Task 4: complete (commits 5687bb6..c062bb8, review clean)

Ruling (the client crate still owns two vendor facts): added as Task 6 step 4b
  rather than deferred. Replacing the CLI's Client::from_env_at with client_for
  orphans from_env, from_env_at and from_key, and with them the last references
  to DEFAULT_BASE_URL ("https://api.typesafe.ai") and ENV_API_KEY
  ("TYPESAFE_API_KEY"). Both are already duplicated by Backend::jev(), which is
  the same two-sources-for-one-fact pattern the Task 4 review caught in
  driver.rs.
  They are also the last vendor-specific items in a crate this plan renamed
  precisely to be protocol-named. Leaving them would mean a crate called
  systemone-client hardcodes one vendor's endpoint as its default and one
  vendor's env var as THE api key, which contradicts the rename's whole reason.
  Noted in the brief that `pub const` produces no unused warning, so clippy
  will not flag them: the step carries an explicit grep.
  Cost if wrong: a downstream user of systemone-client loses two convenience
  constants. The crate is unpublished, and Backend::jev() is the honest home.

Task 5: review spec ✅, quality Approved, 0 Critical/Important, 2 Minor.
  Reviewer confirmed run and plan build their item lists differently but
  equivalently (both exclude cache hits, both cost via the same
  estimate_tokens), so --dry-run and a real run cannot disagree about the
  floor. Quoted the Display output for a 512-token backend and confirmed all
  four facts are present. Verified the fmt-reflow claim by diffing the brief's
  snippets against the diff: only line wrapping moved, every value identical.

Ruling (Minor 1, duplicated function-name extraction): deferred, not fixed.
  The three-liner appears verbatim in run and plan. It is the same
  two-sources shape I have fixed twice this plan, but both call sites are
  byte-identical today and the value is derived, not authoritative: a
  divergence would change an error message, not a verdict or a cache key.
  Recorded for the final review.
Task 5: minor (deferred): the function-name extraction three-liner is
  duplicated verbatim between run and plan.
Task 5: minor (deferred): with a second registered judge, a later judge's
  check_fits failure bails via `?`, discarding stats and skipping the CLI's
  --verbose block, so a user loses visibility into judging already done and
  tokens already spent. Unreachable today since registry() returns only
  dead_code; matters when Plan 2 adds four more judges.
Task 5: complete (commits c062bb8..a64228a, review clean)

Controller end-to-end verification after Task 6, against the real 73-finding
report from `stratify check .` on ~/dev/stratify:
  --dry-run                      -> "backend jev at https://api.typesafe.ai:
                                     2 request(s) planned, 17950 tokens"
  --base-url at Laya, no --backend -> "backend jev at http://127.0.0.1:8000:
                                     2 request(s)". The trap is real AND
                                     legible: Jev's rules at Laya's address,
                                     and the line says so.
  --backend laya                 -> "backend laya at http://127.0.0.1:8000:
                                     4 request(s)". Same url, different
                                     ceiling, different batching.
  --backend mistral              -> exit 2, names both the presets and the
                                     [judge.backends.mistral] table.
  context floor (state_tokens 512) -> "backend `tiny` holds 512 tokens of state
                                     (384 after headroom), but
                                     `sample_finding` needs about 1104", report
                                     still passed through with 73 findings,
                                     --fail-on never still exits 0.
  full run against a mock Laya-shaped endpoint, both key vars unset:
                                     15 judged, 0 from cache, 4 requests,
                                     0 failed. All 15 dead_code findings
                                     dismissed, which is the correct
                                     ground-truth answer for all 15. Judgment
                                     model recorded as laya-mm, the response's
                                     model, proving the Task 4 fix.
  the mock recorded: Authorization header None, and no `model` key in the
                                     request body. So no bearer and no null
                                     model, which is the wire shape Laya
                                     documents.
Laya compatibility is now measured rather than claimed.

Task 6: review spec ✅, quality Approved, 0 Critical/Important, 1 Minor.
  Reviewer upheld both of the implementer's judgment calls. The retry-hint fix
  was the same vendor-hardcode class the task targets, and no test pinned the
  old string, so nothing was weakened. The hardcoded /tmp cache path is a real
  hygiene gap but bounded: a stale entry there can only make the assertion fail
  loudly, never pass wrongly, since the test targets a port that always refuses
  and so can never populate the cache itself.
  Also confirmed only three ExitCode::from(2) sites exist in main(), two
  pre-existing, so no resolved-backend failure gained a hard exit.

Ruling (Minor, README reads as if --model does nothing on Laya): folded into
  Task 7 as step 5b rather than its own round, since Task 7 already edits
  README.md for the install section. The two sentences are individually true
  and jointly misleading: the CLI does serialize the field when --model is
  passed, and whether Laya's server ignores it is a fact about Laya.
  Cost if wrong: one clause of documentation.

Task 6: minor (deferred): the_laya_backend_needs_no_key_to_get_past_the_client_check
  uses a hardcoded /tmp/stratify-judge-test-cache rather than a tempdir, unlike
  the rest of the suite. Brief-mandated verbatim; risk is CI noise, not a
  masked bug.
Task 6: complete (commits a64228a..979689f, review clean)

Task 7: review spec ✅, quality Approved, 0 Critical/Important, 1 Minor.
  Both disclosed deviations upheld. The installer filename fix was necessary:
  the brief's stratify-judge-installer.sh would have 404'd forever, since
  cargo-dist derives it from the package name. Confirmed against the engine
  repo, which ships stratify-cli-installer.sh for a binary named stratify. The
  Cargo.toml metadata used repository.workspace = true, the tidier of the two
  options, matching the engine's own stratify-cli crate exactly.
Task 7: minor (deferred): the task report claims dist init required all three
  of repository, description and homepage, but its own transcript only shows a
  hard failure for repository. The values are right either way; the rationale
  overstates its evidence.
Task 7: complete (commits 979689f..12de82b, review clean)

Ruling (operational gap found by the controller, NOT fixable by me): the
  generated release.yml needs secrets.HOMEBREW_TAP_TOKEN at line 297 to push
  the formula into stratify-dev/homebrew-tap, because GITHUB_TOKEN is scoped to
  the current repo only. The engine repo has that secret configured since
  2026-06-14; stratify-dev/stratify-judge has no secrets at all. So the repo is
  release-ready in code and NOT release-working until that token exists.
  Not something I should create or handle: it is a credential with write access
  to a shared tap. Surfaced to the partner instead.
  Cost if wrong: the first tag builds all five targets successfully and then
  fails only at the homebrew publish step, leaving a usable GitHub release with
  no formula.

ALL 7 TASKS COMPLETE. 142 tests, clippy clean under -D warnings, fmt clean.

FINAL WHOLE-BRANCH REVIEW: merge with fixes. Report at
  .superpowers/sdd/2026-09-27-model-agnostic-judge/final-review.md
  (16 commits, d5a14d8..12de82b; 4 Important, 5 Minor, 0 Critical).

Fix wave (ONE dispatch, per the skill) dispatched at base 12de82b with brief
  .superpowers/sdd/2026-09-27-model-agnostic-judge/final-fix-brief.md covering
  I1, I2, I3, I4, M5, M6, M7, M8, M9, deferred-minors 1 and 4.

Ruling (I2, spec deviation): fix by feeding backend.url into the cache key, not
  by making Backend.name become "custom" as the spec's struct comment says.
  Finer mechanism, same stated goal, and the spec's Risks section already
  accepts that every committed entry misses once the backend enters the key.
  The deviation gets recorded in the spec rather than hidden.
  Cost if wrong: a capture proxy in front of real Jev stops hitting the cache.

Ruling (I4 shape): run returns Result<RunStats, RunFailure> with the stats
  inside the error, not the (RunStats, Option<RunError>) tuple the reviewer
  suggested. The tuple breaks all 16 .run().await.unwrap() test sites and
  gives up `?`; the struct keeps both.
  Cost if wrong: one more type in the public surface than strictly needed.

Ruling (I4 scope): floor-dropped findings now count in stats.unjudged, which
  the reviewer's failure scenario names. Cheap and correct; the field's doc
  comment widens to say so.
  Cost if wrong: unjudged conflates two loss causes in one counter.

Ruling (M6): --dry-run's floor arm returns SUCCESS, honouring README:66,
  rather than softening the README. A preview that fails a build is surprising.
  Cost if wrong: a dry run against a too-small backend passes CI silently,
  which is what a preview should do.

Ruling (deferred-minor 2): deferred as the reviewer triaged. The real fix is a
  subject_name method on the Judge trait, which belongs with the second judge.
  It only ever affects one error string.

Ruling (deferred-minor 5): closed. Process artifact in .superpowers/, not
  shipped documentation, and the thing that matters (dist plan emitting the
  README's artifact names) was verified directly.

Fix wave: DONE (commits 12de82b..d42f7aa, 11 commits, 153 tests, clippy and
  fmt clean). Report at final-fix-report.md.

Ruling (accepted implementer deviation): run's error is Box<RunFailure>, not
  bare RunFailure. An unboxed RunFailure trips clippy::result_large_err against
  a RunStats-only Ok type, which would fail the -D warnings gate the plan's
  Global Constraints require. Boxing is clippy's own suggested fix, keeps all 16
  .run().await.unwrap() sites compiling, and a partial move out of a Box field
  is stable Rust. My brief's literal sample was wrong; the implementer was right
  to deviate and to say so.
  Cost if wrong: one allocation on a path that already stopped judging.

Scoped re-review dispatched over 12de82b..d42f7aa with a per-finding checklist
  and instructions to reproduce I1, I2, I3, I4 and M6 live rather than read the
  diff. This is the ONE re-review; residuals get adjudicated, not a second wave.
