#!/usr/bin/env bash
#
# The DB pre-flight for `make test-crates` (#830) -- the half the skip receipt cannot have.
#
# WHY THIS EXISTS
#
# `crates/db_test_gate` decides on whether `DATABASE_URL` is **set**. It never asks whether the
# server at the other end is **reachable**, because it cannot: it runs inside libtest, once per
# suite, after the whole workspace has already been built. So with a `DATABASE_URL` pointing at a
# stopped Postgres:
#
#   * every DB-gated suite fails with a connection error, ~12 minutes after the run started, and
#     those failures read as regressions in the diff under test (measured 2026-08-30: one executor
#     spent that long chasing `actor_runtime`/`infrastructure` reds unrelated to its change);
#   * the skip receipt `target/db-test-skips.log` stays EMPTY -- nothing skipped, the gate did
#     exactly what it promises -- so `grep`ping the run for `DB-GATED SUITES SKIPPED` reports the
#     same thing on a live database and on a dead one.
#
# That second point is the real defect, and CLAUDE.md already names its class: "a monitoring path
# that can only fire when a signal ARRIVES -- a threshold alert goes quiet exactly when it should
# scream; liveness needs a dead-man's-switch." The receipt is precisely that shape. It can only
# speak when suites SKIP, so its silence carries no information at all.
#
# This script is the dead-man's-switch. It runs BEFORE the workspace suite and is TWO-SIDED:
# it fails loudly when the database is unreachable, and it prints a POSITIVE line when it is
# reachable. The positive line is what makes the receipt's silence meaningful, because the two
# together are a complete claim:
#
#     DB PRE-FLIGHT OK  +  empty skip receipt  +  exit 0   =>  the DB-gated suites RAN, live.
#
# Any one of the three alone proves nothing.
#
# COMPILER-FIRST NOTE (ADR-20260803-234035). The floor asks whether the type system can make the
# mistake unspellable before a check is written. It cannot reach here: "is a server process on the
# other end of this socket accepting connections" is a property of the world at a moment in time,
# not of the program text, and it is re-established on every run. Level 4 -- an executable check --
# is the honest ceiling for this one. What the check DOES buy over prose is that it fails before
# the build rather than 12 minutes into it, and that it cannot be forgotten.
#
# WHAT THIS DELIBERATELY DOES NOT DO
#
# It does not restate the `DB_TESTS_REQUIRED` opt-out table. That decision lives in exactly one
# place (`crates/db_test_gate/src/lib.rs`, #474, pinned by the
# `only_the_db_test_gate_spells_the_database_skip_polarity` codegen test) and a shell copy of it
# would be the eighteenth hand-written spelling of the polarity -- the exact defect #474 removed.
# It does not need one: a NON-EMPTY `DATABASE_URL` means `Verdict::Run` unconditionally, whatever
# `DB_TESTS_REQUIRED` says, so the single condition below is complete for the case that bites.
# When `DATABASE_URL` is empty the gate crate already handles both branches loudly and correctly
# (a panic naming the remedy, or a skip with a receipt), and this script stands aside.

set -eu

TIMEOUT="${DB_PREFLIGHT_TIMEOUT:-5}"

# `DATABASE_URL` may carry a password. Never echo it raw: this output lands in PR bodies, CI logs
# and session transcripts. Strip any userinfo between `://` and the last `@` of the authority.
redact() {
  printf '%s' "$1" | sed -E 's#(://)[^@/]*@#\1***@#'
}

url="${DATABASE_URL:-}"

if [ -z "$url" ]; then
  # Not this script's branch. `crates/db_test_gate` owns it, and owns it loudly.
  echo "test-crates: DB PRE-FLIGHT SKIPPED -- no DATABASE_URL; crates/db_test_gate decides this run"
  echo "test-crates: (it PANICS unless DB_TESTS_REQUIRED names an explicit opt-out, which leaves a receipt)"
  exit 0
fi

if ! command -v pg_isready >/dev/null 2>&1; then
  # A DECLARED degraded mode, not a silent one. Failing the build because a client utility is
  # missing would be worse than the defect: it would block a run whose database is perfectly fine.
  # But the degradation is announced, so a reader knows the positive half of the evidence is absent
  # for this run and must not read an empty skip receipt as proof the suites ran.
  echo "test-crates: DB PRE-FLIGHT UNAVAILABLE -- pg_isready is not on PATH, reachability NOT checked."
  echo "test-crates: this run has NO positive database evidence; an empty skip receipt proves nothing here."
  echo "test-crates: install it with 'apt-get install -y postgresql-client' to restore the check."
  exit 0
fi

if pg_isready -d "$url" -t "$TIMEOUT" >/dev/null 2>&1; then
  echo "test-crates: DB PRE-FLIGHT OK -- $(pg_isready -d "$url" -t "$TIMEOUT" 2>&1 | head -1)"
  echo "test-crates: DATABASE_URL=$(redact "$url") -- the DB-gated suites will RUN against it."
  exit 0
fi

status_line="$(pg_isready -d "$url" -t "$TIMEOUT" 2>&1 | head -1 || true)"
cat >&2 <<EOF
test-crates: DB PRE-FLIGHT FAILED -- the configured database is NOT accepting connections.

  DATABASE_URL : $(redact "$url")
  pg_isready   : ${status_line:-(no output)}

Nothing has been built or run yet, ON PURPOSE. Without this check the workspace suite would build
for minutes and then fail inside every DB-gated suite at once, with connection errors that read as
regressions in the diff under test. That misattribution cost one executor ~12 minutes on
2026-08-30, and it is the reason this check exists (#830).

Fix ONE of these:

  1. Start the database (it is NOT up by default in this container):
         service postgresql start
     then re-run. The ~40s initdb recipe for a fresh container is in
     docs/claude/sessions/gates.md.

  2. Point DATABASE_URL at a database that IS running.

  3. If you genuinely have no database, say so EXPLICITLY and accept the receipt:
         env -u DATABASE_URL DB_TESTS_REQUIRED=0 make test-crates
     That run exercises NO database behaviour, prints which suites were skipped, and must not be
     reported as evidence for a change that touches migrations/, crates/infrastructure or
     crates/actor_runtime.

Do NOT unset DATABASE_URL to get past this: that turns a loud, specific failure into 50 skipped
suites, which is the false signal #474 exists to remove.
EOF
exit 1
