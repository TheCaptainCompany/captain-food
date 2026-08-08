# ADR-20260808-224500 — Unflake the ci gate: env-mutation purge + evidence step + flake policy; test-binary consolidation scoped infrastructure-only, sequenced after

**Status**: Accepted (ensemble consent — architect, dba, farley lenses, coordinator synthesis;
customer veto window open per ADR-20260808-155656) · **Date**: 2026-08-08 · **Tracking**:
[#388 "[watchdog] Flaky SIGSEGV in `infrastructure` lib-test binary reddens the `ci` build gate on `main`"](https://github.com/TheCaptainCompany/captain-food/issues/388) ·
[#335 "Decide whether to consolidate integration test binaries (~3.5G of link products)"](https://github.com/TheCaptainCompany/captain-food/issues/335)

## Context

The standing objective couples #388 (a one-off, locally non-reproducible SIGSEGV in the
`infrastructure` lib-test binary that reddened `ci` on `main`) with #335's link-product
hypothesis. Three lens consults ran in parallel on current `main`; their reports are the
evidence base (full texts in the run transcript; verdict substance below).

## Findings (evidence, not opinion)

1. **#388 root cause identified with high confidence (architect)**: two lib tests mutate the
   process environment — `ovh_sms.rs` `from_env_requires_the_full_credential_set` and
   `supabase_auth.rs` `from_env_gates_on_both_url_and_key` interleave `set_var`/`remove_var`
   with `getenv` reads while libtest runs the 33 tests on parallel threads. Concurrent
   `setenv`/`unsetenv` vs `getenv` is documented glibc UB (`unsetenv` shifts `environ` under a
   walking reader) — the canonical intermittent SIGSEGV in Rust test binaries, and the reason
   edition 2024 makes `env::set_var` unsafe (this workspace is edition 2021, so
   `unsafe_code = "forbid"` cannot see it). Every #388 symptom fits: reader-thread crash makes
   the failing test unidentifiable; clean at `--test-threads=1`; nanosecond window explains 40
   clean local runs; lib binary only (integration suites run `--test-threads=1` in `db-test`).
   The repo already names this hazard: `server/src/lib.rs` splits `parse_flag` from `env_flag`
   precisely so tests never mutate process env. These two tests are the ONLY
   `set_var`/`remove_var` call sites in `crates/**`.
2. **The link-product hypothesis is a null causal factor for #388 (architect)**: `cargo test`
   finishes linking all binaries before executing any, then runs binaries sequentially — no
   link was in flight at crash time; ENOSPC fails the link loudly, a truncated mmap presents as
   SIGBUS/loader error, not a clean SEGV after 7 passing tests. #335 remains a real
   disk/latency cost on its own merits.
3. **CI is blind (farley)**: no `df`/`free` logging, no core-dump capture, no failure
   artifacts anywhere in `.github/workflows/` — "non-reproducible" is partly "uninstrumented".
4. **Today's isolation is implicit (dba)**: the 27 infrastructure DB suites share one Postgres
   and one `public` schema with ~20 divergent hand-copied `reset_schema` blocks; their only
   cross-suite isolation is cargo running binaries one at a time. A mechanical merge deletes
   exactly that.

## Decision

1. **Fix #388 at the root (green lane, dispatched now)**: refactor the two tests to the
   established `server/src/lib.rs` pattern — pure core (`from_parts`/lookup-injected) tested
   without touching process env; delete every `set_var`/`remove_var`. The deferred
   single-threaded CI mitigation is **rejected permanently** — it would mask UB, not remove it.
2. **Gate the bug class**: `clippy` `disallowed-methods` for `std::env::set_var`/`remove_var`
   across the workspace, enforced where CI runs lints, negative-verified against a planted
   violation. Endgame is the edition-2024 migration (makes the mistake unspellable under the
   existing `unsafe_code = "forbid"` floor) — tracked as a later, separate change.
3. **Make failures diagnosable (farley's evidence step, same slice)**: `df -h` + `free -m`
   before/after the test steps in `build-test`/`db-test`, core-pattern + `ulimit -c unlimited`,
   and an `if: failure()` artifact upload of cores + crashing binary. Zero green-path risk.
4. **Flake policy (recorded on #388, the ledger)**: no automatic retry in workflows; one
   manual rerun of a red `main` run, only after the run URL/job/binary are recorded on the
   ledger; a second occurrence within ~50 `main` runs escalates to a blocking investigation
   with the captured core; ~50 clean runs after the fix closes #388. Quarantine only ever a
   named recurring test, never a binary, never the required aggregate.
5. **#335 scope decided: consolidate `crates/infrastructure/tests` ONLY — sequenced strictly
   AFTER the #388 fix merges.** Mechanical merge-everywhere is REJECTED (dba veto: it deletes
   the only existing isolation and industrializes the recorded eternal-retry incident);
   "non-DB crates only" is REJECTED as a decline in disguise (the 3.5G lives in the DB-coupled
   crates); `actor_runtime` is DECLINED (process-sensitive concurrency suites). Preconditions
   (dba): a `TestDb` witness whose constructor holds a binary-wide lock (compiler-enforced
   serialization, not invocation prose); ONE migration-derived `reset_schema` via
   `include_str!` replacing the divergent hand copies; no env mutation (now clippy-gated).
   Acceptance gates: the consolidated binary green ALONE against a FRESH database with
   `DB_TESTS_REQUIRED=1`, plus per-module filtered passes on fresh databases; before/after
   wall-clock + `deps` size + identical test counts in the PR body, durable delta to
   `sessions.md §2`. `cargo-nextest` (per-test process isolation with one link product) is the
   named alternative if the witness pattern proves awkward — its adoption would be its own
   small decision.

## Consequences

- The #388 fix slice (items 1–3) is one executor dispatch: `crates/infrastructure` test
  refactor + clippy config + `ci.yml` evidence step.
- #335 implementation waits for that merge; its issue carries this scope decision and the
  preconditions verbatim.
- Sequencing note for later slices: the scheduled `--shuffle` (seed-replayable) run the
  architect proposed is deferred to the #335 slice itself, where module order first becomes a
  live risk.
