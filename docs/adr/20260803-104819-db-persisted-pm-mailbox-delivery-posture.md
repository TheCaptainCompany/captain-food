# ADR-20260803-104819 — PM_MAILBOX_DELIVERY lives in one database row

## Status

Accepted — realizes [#318 "DB-persisted PM_MAILBOX_DELIVERY posture — precondition for adapter
worker fleets (ADR-20260803-002712 Q4)"](https://github.com/TheCaptainCompany/captain-food/issues/318),
decided by [ADR-20260803-002712](20260803-002712-mailbox-poison-follow-ups-decided.md) Q4.

## Context

The Runtime D1 money-path gate (`PM_MAILBOX_DELIVERY`, ADR-20260801-023000) was a per-process
environment read: the monolith through its generated `Config`, a standalone adapter fleet through
its own env parse plus an unset-refusal guard on the money lanes
(`infrastructure::mailbox::standalone`). Per-process env is per-deploy state — a drifted value on
one of five adapter deploys delivers Payment facts WITHOUT the PM chain hop while the monolith's
saga triggers are retired, and a paid order sits un-reacted until a restart: the silent
paid-order stall. Operator discipline ("set it explicitly, matching the monolith") was the only
defense. ADR-20260803-002712 Q4 keeps adapter worker fleets default-off UNTIL the posture is one
database row all processes read, making the drift structurally impossible.

## Decision

1. **One seeded row is the posture.** A `RuntimePosture` reference/config table (ADR-0037
   family, `specs/database/tables/referential.yaml`) holds one row per process-wide runtime
   posture; the migration seeds `('PM_MAILBOX_DELIVERY', false)`. Every process that can deliver
   mailbox rows — the monolith composition root and every standalone adapter fleet — reads that
   row at startup. The `PM_MAILBOX_DELIVERY` key is REMOVED from `specs/configuration.yaml` and
   the generated `Config`: no env override path remains, because any override path is a drift
   path.

2. **Resolution is fail-closed, distinguished by cause** (the monolith and the adapters share
   the same read, `infrastructure::persistence::runtime_posture`):
   - **Row read** → that value is the posture, process-wide, until the next restart.
   - **Table or row missing** (schema behind this binary, or an unseeded database) → the posture
     resolves **deterministically to the legacy arm**: gate off in the monolith, money lanes
     refused in an adapter fleet. This is safe WITHOUT retry because it is consistent by
     construction — no process can read `true` from a database state in which the row does not
     exist, so the saga runner stays active and handles every recorded fact, exactly the
     pre-flip topology. `/health` independently reports `schema_behind` (ADR-0043).
   - **Transient read error** (DB unreachable, timeout) → the value is UNKNOWABLE and a peer
     process may have read `true`; guessing is the exact failure this ADR exists to remove. The
     **monolith retries briefly (four retries, 2 s apart) and then refuses to start** (a failed deploy keeps the previous
     version serving — the ADR-0043/#246 posture: what cannot be proven does not boot). A
     **standalone fleet retries until the read resolves** before spawning ANY worker — a fleet
     that cannot reach the database has nothing to deliver anyway, and spawning non-money lanes
     early would only race the answer.

3. **A flip is `UPDATE RuntimePosture SET enabled = …, updated_at = now() WHERE posture =
   'PM_MAILBOX_DELIVERY'` plus a full restart of EVERY mailbox-delivering process, in a
   prescribed order.** The read is startup-time, not per-request: processes converge at the
   pace of the restart, so DURING the flip window postures legitimately differ and the restart
   ORDER is what keeps the money path safe (independent review finding, 2026-08-03):
   - **Flip ON (false → true): adapter fleets first, monolith last.** A fleet that chains
     early is harmless — the still-legacy monolith runner double-reacts at worst, and the
     cross-arm acceptance dedupe absorbs it; the runner's Stripe-fact triggers retire only
     when the monolith restarts, LAST, after every fleet already chains.
   - **Flip OFF (true → false): monolith first, adapter fleets last.** The monolith restarting
     into `false` reactivates the runner BEFORE any fleet stops chaining. A fleet restarted
     first would record captures unchained while the runner is still retired — and the gate-ON
     startup backfill would never recover them (a monolith restarting into `false` skips it):
     the one ordering that loses a saga hop durably.
   Restarting only the monolith after a flip is an operator error the row cannot absorb —
   restart them all. (Live re-read / an admin flip mutation is a possible follow-up if
   operators need it; the steady-state guarantee #318 wanted does not depend on it.)

4. **The fleet-guidance flip stays its own one-line ADR.** Per ADR-20260803-002712 Q4 and
   gate-then-stabilize, `RUN_MAILBOX_WORKERS` guidance flips to opt-out only AFTER this
   DB-persisted posture has been smoked in a real deployment — never in the change that
   introduces it.

### Options considered

- **Keep env, add a startup cross-check against the DB row** — still two sources of truth; the
  check can only detect drift at boot, not prevent an operator writing a new env value.
- **Per-request read (no restart to flip)** — puts a DB read on the money path's hot loop and
  makes MID-FLIGHT posture change possible, which the flip semantics (backfill on gate-on
  startup) were never designed for. Startup read matches the existing gate contract.
- **Refuse to start on a missing row too** — rejected: a binary deployed ahead of its migration
  must still boot and serve `/health schema_behind` (ADR-0043); missing-row is deterministic
  across processes, so the legacy arm is provably consistent, unlike a transient error.

## Consequences

- `standalone.rs` loses `pm_gate_posture()` (env) and the unset-refusal guard; the money lanes
  are now refused only while the posture is UNPROVABLE from the shared database.
- The monolith composition root wires the request-time PM resolver arm, the saga-runner
  retirement, the B2 chaining and the gate-on backfill from the SAME row read.
- At STEADY STATE the drifted silent paid-order stall is structurally impossible: there is no
  per-process posture state left to drift, and identical database state resolves to mutually
  consistent arms everywhere. The FLIP WINDOW is the remaining exposure, governed by the
  restart order prescribed in Decision 3 — an ordering runbook, no longer a per-deploy value
  that can silently rot.
- The monolith's transient-error refusal (four retries, 2 s apart, then refuse to start)
  deliberately NARROWS ADR-0043's "an unavailable dependency still boots and 503s" doctrine
  for exactly this one read: a non-deploy restart during a database outage crash-loops until
  the read answers, because a guessed money posture is worse than a refused boot. (A missing
  table/row still boots — gate off, `/health` reports `schema_behind`.) Recorded here rather
  than amending ADR-0043.
- Migration `20260803104819_runtime_posture.sql`; `REQUIRED_SCHEMA_VERSION` bumped.
