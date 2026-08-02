# ADR-20260802-200416 — Drain loops are woken by Postgres NOTIFY, not by a 1.5 s poll

## Status

Accepted

## Context

The projection worker (ADR-0040) and the saga runner (ADR-20260719-193500) each polled
`domain_events` every 1.5 s, paying `1 + 2 × groups` queries per pass — 17 and 11 respectively, plus
the inbound drain (2 s) and the health heartbeat (30 s). That is **≈70,900 queries per hour, 1.7
million a day, on a platform with zero orders**.

Every one is an outbound round trip to Supabase. Render's monthly breakdown showed 3.97 GB
service-initiated against 210 MB of HTTP responses: **background polling cost roughly 19× all
customer-facing traffic combined**, and put the 5 GB free allowance at 70 % consumed. At ~300 bytes
sent per round trip the two loops account for ~21 MB/hour, matching the observed baseline. The write
payloads are not the driver — the entire SIRENE write path is under 250 MB.

Polling also set a latency floor on the money path: up to 1.5 s before `PlaceOrderProcess` reacts to
a captured payment, and a GraphQL subscription resolver that receives its push instantly then polls
the read model up to 12 × 250 ms waiting for the projector to catch up.

The forces:

- `domain_events` + `projection_checkpoint` is **already a durable queue with a consumer offset**, so
  the loops need a *wake signal*, not durable delivery. A missed signal costs latency, never work.
- ADR-0040 keeps projection and business logic **out of the database** — no SQL triggers.
- The projector is planned to graduate to its own process (`crates/server/src/bin/projector.rs`), so
  an in-process fan-out would be a dead end.
- A background worker that silently stops is the worst failure mode in the domain lens: a paid order
  that nobody is told about.

## Decision

**`PgEventStore::append` raises `pg_notify('domain_events', '')` inside the append transaction, and
one dedicated `LISTEN` connection wakes both drain loops.**

1. **App-side, not a trigger.** The notify lives in the Rust adapter, so ADR-0040's "no SQL triggers,
   no business logic in the DB" rule stays intact. Supabase Database Webhooks / `pg_net` were
   rejected for exactly this reason (see Alternatives).
2. **Inside the transaction.** Postgres delivers at COMMIT, so a rolled-back append notifies nobody
   and there is no post-commit window in which a crash could leave events no listener heard about.
3. **Empty payload.** Postgres coalesces identical notifications within a transaction, so a
   multi-event append wakes the drains once. The drains re-read from their own checkpoint, so the
   signal carries nothing.
4. **The safety-net poll is mandatory.** `NOTIFY` is fire-and-forget with no replay. Each loop still
   drains on its own interval, and a freshly (re)connected listener signals once unconditionally so
   whatever landed while it was deaf is drained immediately.
5. **The fallback is adaptive.** The safety interval is 60 s **only while the listener is confirmed
   live**; whenever it is down the loops revert to the 1.5 s cadence. Losing push therefore degrades
   to exactly the pre-change behaviour and never past it.
6. **An idle head-gate** skips the per-group queries when `MAX(position)` has not moved since the last
   *fully drained* pass, so even the fallback path costs 1 query per tick instead of 17 or 11. Only a
   pass that drained every group arms the gate, so an errored group is retried rather than skipped.

**Deployment constraint (hard):** `LISTEN` requires a **session-mode** connection. It works on
Supabase's session pooler (port 5432, what `render.yaml` specifies) and on a direct connection, and
silently delivers nothing through the transaction pooler (6543). Moving the service behind a
transaction pooler would disable push without an error — the adaptive fallback keeps that mistake
expensive rather than fatal, but it must not be made silently.

`RUN_EVENT_PUSH=false` forces the unassisted polling path as an escape hatch.

## Alternatives considered

- **Head-gate only, keep polling** — ~20 lines and no new failure mode, but leaves a
  ~2,400 queries/hour/loop floor and does not improve latency. Adopted as a *complement* on the
  fallback path, not as the answer.
- **Subscribe the loops to the existing in-process `EventBus`** — free and works today, but only sees
  appends made by *this* process. It would break silently the moment the projector moves out of
  process (already the documented plan) or a second instance runs. Rejected as a dead end.
- **`pg_net` / Supabase Database Webhooks (the database calls the app over HTTP)** — the intuitive
  reading of "push from the database", and rejected on five counts: it requires an `AFTER INSERT`
  trigger (violating ADR-0040); it is fire-and-forget too, so it still needs the safety net and gives
  no reliability gain; it costs a full HTTP request per event (~13/sec during the SIRENE sweep)
  against a few bytes for `NOTIFY`; the database would have to hold a credential to call the app; and
  it inverts the dependency direction. Its one genuine advantage — waking a *sleeping* instance —
  cuts both ways, since defeating spin-down is the cost driver this ADR exists to remove.
- **`pgmq` durable queue** — real durable delivery, but rebuilds a queue we already own.
  `domain_events` is the log and `projection_checkpoint` is the offset.
- **Position as the notify payload** — marginally more debuggable, but distinct payloads do not
  coalesce, so a 3-event append would queue 3 wakes.

## Consequences

### Positive

- Idle queries drop from ~70,900/hour to ~120 — the outbound bandwidth that triggered this work
  effectively disappears, and the Supabase egress and connection budget benefit equally.
- **Lower latency on the money path**: sagas and projections react as fast as the commit lands rather
  than up to 1.5 s later. The subscription resolver's 12 × 250 ms wait largely disappears.
- Works **out of process**, so the planned graduation of the projector to its own worker needs no
  redesign — the standalone `projector --loop` binary is wired the same way.
- Losing push is a no-op regression: the worst case is the status quo ante.
- ADR-0040 is preserved rather than amended away — no trigger, no DB-side logic.

### Negative

- One dedicated connection is held open indefinitely for `LISTEN`, outside the pool's
  `max_connections(5)`. One listener serves both loops to keep this to a single connection.
- A new hard deployment constraint (session-mode pooler) that fails **silently** if violated. Recorded
  here, in the module docs, and mitigated by the adaptive fallback.
- A `pg_notify` failure now fails the append. The statement has already poisoned the transaction so
  there is no "ignore and carry on"; and the realistic cause (a full async queue behind a wedged
  listener) is precisely when a silent stall would be most expensive.
- The 60 s safety net means that in the narrow window where push is *believed* live but a signal was
  genuinely lost, a drain can be up to 60 s late. The reconnect-drain closes the common case
  (listener actually dropped); this residual is accepted.

### Follow-up actions

- Add an **egress observability contract** to `specs/observability.yaml`: bytes sent per destination
  and DB round trips per idle hour, with a budget the gate fails on. The reason this ran unnoticed
  for a month is that the application measures nothing about what it sends — the diagnosis had to be
  reconstructed from source arithmetic and table sizes.
- Fix the **SIRENE re-pend**: `crates/sirene_ingest/src/staging.rs` bumps `last_seen_at` on every row
  on every run, so a weekly re-ingest replays all 227k staged rows through the write path even when
  nothing changed. Needs an ACL-version constant so a deliberate bump can still force a full
  re-drain — its own proposal.
- Decide the **Supabase storage plan**: 570 MB against a 500 MB free limit with 33 of ~101
  departments loaded; full France projects to ~1.7 GB.
- Consider extending the same wake to `InboundEventsDrainWorker` (2 s poll, on the payment path).

## References

- Proposal: [PROP-20260802-200416](../proposals/PROP-20260802-200416-push-driven-drain-loops.md)
- Tracking issue: [#300 "Push the drain loops from Postgres NOTIFY instead of polling every 1.5s"](https://github.com/TheCaptainCompany/captain-food/issues/300)
- Amends the polling model of [ADR-0040](0040-materialized-read-model-tables-projectors.md)
  (the "no SQL triggers" rule is preserved, not relaxed).
