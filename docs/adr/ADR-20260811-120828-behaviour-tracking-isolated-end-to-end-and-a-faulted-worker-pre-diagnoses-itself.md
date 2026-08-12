# ADR-20260811-120828 — Behaviour tracking is isolated end to end, and a faulted worker pre-diagnoses itself in its health payload

- **Status**: Accepted
- **Date**: 2026-08-11
- **Source**: product-owner directives (verbatim below)
- **Extends**: [ADR-20260811-105024 "A database-rejected fold HALTS its projection group…"](ADR-20260811-105024-projection-halt-default-and-health-visibility.md) — Decision 2 here confirms its readiness-not-liveness finding and adds the payload requirement and the "any worker" scope. Nothing in it is reversed.
- **Realized by**: _(filled at completion)_

---

## Decision 1 — behaviour tracking is isolated end to end: its own database **and** its own worker

> *"The behaviour event tracking will be stored in another database not the business databases the
> behaviour event tracking will be completely isolated and projected by another projector worker to
> avoid dependencies between the behaviour event tracking and the business events."*

This goes **further than [PROP-20260811-000946](../proposals/PROP-20260811-000946-behaviour-event-tracking-in-the-screens-spec.md) D5**,
which asked only for a separate store. The isolation is now **end to end**: a separate database *and*
a separate projector worker, so a behaviour-tracking fault cannot stop a business projection and a
business-projection fault cannot stop behaviour tracking.

That matters more under [ADR-20260811-105024](ADR-20260811-105024-projection-halt-default-and-health-visibility.md)
than it did before it: now that a database-rejected fold **halts its group**, sharing a worker would
mean a malformed behaviour event could wedge a group in the same process as the order read models.
Separate workers make that failure unspellable rather than merely unlikely.

**It also settles the distinction cleanly**, which is worth stating because the two are easy to
conflate:

| | Store | Worker | Source |
|---|---|---|---|
| **Behaviour events** | its own database, time-partitioned | its own projector worker | UI writes through a `sink:` mutation — **not** `domain_events` |
| **Business metrics** | `bam` schema in read-models | the `bam` projector | a fold over `domain_events` — business data derived from business facts |

**C4 consequence**: a **new container** plus its edges is needed for the behaviour database and the
behaviour worker. `specs/architecture/*.yaml` is **source DSL, not generated**, so that is a spec
change the executor makes when the work lands — not a regeneration.

## Decision 2 — a faulted worker reports unhealthy and is NOT restarted; the payload is the deliverable

> *"In case of issue in any worker it must for now stop and say in the /health with a 500 unhealthy
> status to inform k8s. K8s does not need to restart the worker. In our monitoring app we will have to
> detect and fix the issue on production based on the pod logs or the /health error info by this way we
> don't need to go on the pods logs. It's a pre diagnostic."*

### 2.1 The convergence, recorded

*"K8s does not need to restart the worker"* is **independently the same conclusion** ADR-20260811-105024
reached from the failure analysis: a database-rejected fold is deterministic, so a restart re-reads the
same event from the same checkpoint and fails identically — liveness would produce CrashLoopBackOff
and take every sibling group down with it. Readiness reports; liveness restarts; the decision says
report and do not restart. **Confirmed, not merely compatible.**

### 2.2 The payload is the deliverable; the status code is the transport

*"we don't need to go on the pods logs. It's a pre diagnostic."* — this is a constraint on the **body**,
not on the status code. The non-2xx tells Kubernetes something is wrong; **the body has to tell a human
what broke well enough to act without opening a shell.**

So the per-group breakdown specified in ADR-20260811-105024 §3 — group, `haltedSince`, position,
`eventType`, stream, error — **stops being a nicety and becomes the point of the feature**. A health
endpoint that returns `{"status":"unhealthy"}` satisfies the status code and fails the requirement.

**This is [ADR-20260810-231300 "no polling, only pushing"](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)
applied one layer up**: the failure **pushes its own diagnosis** into a surface that is already being
watched, instead of a human polling pod logs to reconstruct what happened. That framing is the rule
worth remembering; the health-check mechanics are the implementation of it.

**On `500` specifically**: Kubernetes treats **any** non-2xx as a failed probe, so `500` and `503` are
identical to the cluster. The existing endpoints use `503 SERVICE_UNAVAILABLE`, which is also the
semantically correct code for "up but not serving". **Keep `503`** — the directive's intent is "a
non-2xx that k8s sees", not the specific integer, and nobody should "fix" `503` to `500` for literal
compliance.

### 2.3 ⚠️ Edge 1 — as stated, this would take the storefront down in the current topology

The directive says *"say in the /health"*. **In the monolith that is the wrong endpoint**, and the
consequence is customer-facing. Verified on `37642cd`:

| # | Fact | Evidence |
|---|---|---|
| F1 | **The monolith runs the API and the projection worker in ONE process**, gated by `RUN_PROJECTOR` (**default on**) | `crates/server/src/lib.rs:641-648` (ADR-0040); `:26` |
| F2 | The same process serves the storefront API — `/{role}/graphql` | `crates/server/src/lib.rs:14` |
| F3 | `/health` is the **deploy interlock** and the readiness probe target — *"point Render's Health Check Path here"*, `200` only when the DB is reachable and the schema is at/after the required version | `crates/server/src/lib.rs:1503-1526` (ADR-0043) |
| F4 | It knows **nothing** about projections today: every branch is DB reachability + schema version | `:1508-1525` |
| F5 | Unlike the projector bins, the monolith **has a `Service`** — traffic is routed to it | it serves the storefront |

So **making `/health` reflect projection state in the monolith would take the API unready when a read
model halts** — a degraded projection becomes a customer-facing outage, which is the opposite of the
intent. It would also fail the deploy interlock, so a halted projection would block deploys of the
thing that could fix it.

**The rule is therefore restated so the edge cannot occur:**

> **The endpoint that a pod's readiness probe points at returns non-2xx when a component THAT POD IS
> RESPONSIBLE FOR is faulted.** Not "`/health` returns 500".

Concretely:

| Deployable | Readiness probe points at | Non-2xx when |
|---|---|---|
| `projector-*`, `bam`, the behaviour worker | **`/projector`** (today: `/health`) | any group it hosts is halted |
| saga runner | `/saga` | the runner is faulted |
| the monolith (until cutover) | **`/health` — unchanged, API components only** | the DB is unreachable or the schema is behind |

The monolith's in-process projector stays observable at `/projector`, **which is not its readiness
probe** — so an operator and the monitoring app see it, and the storefront does not go down for it.

**The final shape, for when the cutover lands**: the bins are generated from the spec, so *which
components a deployable hosts* is already declared. Both the probe path and the health composition can
be **generated from that same declaration** — a process then cannot claim a component it does not host,
and cannot fail readiness for one it does not own. Until then the table above is the honest degradation.

### 2.4 ⚠️ Edge 2 — "any worker" does NOT apply unchanged to the actor-mailbox workers

The directive says *"any worker"*. Applied literally it is wrong for one class, and the reason is that
that class **already solved this problem the other way**.

| Worker | Status surface today | Does "halt and report" apply? |
|---|---|---|
| projection workers | `/projector` | **Yes** — this is ADR-20260811-105024 |
| saga runner | `/saga` | **Yes**, same shape |
| SIRENE sync | `/sirene` | **Yes**, already reports `poll_loop_not_started` |
| **actor-mailbox workers** | **none — there is no `/mailbox`, no `MailboxStatus`, nothing** | **No, and it should not** |

**Why not.** The mailbox **already quarantines**: a message that keeps failing hits the
delivery-attempts cap and is parked as poison (`specs/database/tables/journals.yaml:69`, *"poison
supervision count"*), the lane **keeps draining**, and an operator inspects and requeues it through
`poisonedMailboxMessages` / `requeueMailboxMessage` (`specs/common/api.yaml:158,170,202`). Making an
actor worker *stop* on a bad message would **remove** that property and turn a parked message into a
stopped lane — and a stopped order lane at 19:40 on a Friday is the platform's worst failure mode.

**This asymmetry is the real content of Edge 2, and it is worth stating as a principle**: halt is the
right answer **where there is no quarantine**, and quarantine is better wherever it exists. Projections
halt *because* they have no quarantine — which is precisely why quarantine remains the tracked
follow-up for them (ADR-20260811-105024, the role-revocation wedge).

**What actor workers do owe** is the other half of the directive — the *pre-diagnostic*. Today the
poison data is reachable only through the **admin GraphQL API**, not through any health surface, so the
monitoring app cannot see a poisoned lane without authenticating as an admin. A `/mailbox` endpoint
reporting lane depth, poisoned counts and the oldest poisoned message is the missing piece, and it is
**report-only — it must not gate readiness**, because a poisoned message is a normal, recoverable
state, not an unhealthy pod.

## Consequences

- `/projector` gains the per-group payload and returns non-2xx when halted; projector bins' readiness
  re-points to it. The monolith's `/health` is **unchanged**.
- A `/mailbox` status surface is owed, report-only, and does **not** gate readiness.
- The observability contract gap stands: `specs/observability.yaml` declares no projection contract.
- `deploy/**` is GENERATED — probe changes are emitter changes, not hand-edits.
- The behaviour database and behaviour worker need a C4 container and edges when the work lands.

## Refs

- `crates/server/src/lib.rs:641-648` — the in-process projector (ADR-0040); `:1503-1526` — `/health`, DB + schema only; `:5-20` — the endpoint list
- `specs/database/tables/journals.yaml:69` — mailbox poison supervision
- `specs/common/api.yaml:158,170,202` — `mailboxLanes`, `poisonedMailboxMessages`, `requeueMailboxMessage`
- [ADR-20260811-105024](ADR-20260811-105024-projection-halt-default-and-health-visibility.md) — the halt default and the health design this extends
- [ADR-20260810-231300 "No polling, only pushing"](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md) — the principle §2.2 applies one layer up
- [ADR-0043 "DB migration release strategy"](0043-db-migration-release-strategy.md) — what `/health` gates
- [PROP-20260811-000946](../proposals/PROP-20260811-000946-behaviour-event-tracking-in-the-screens-spec.md) D5 — the separate store this decision extends to a separate worker
