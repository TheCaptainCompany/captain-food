# ADR-20260811-105024 — A database-rejected fold HALTS its projection group, and a halted group must be visible to Kubernetes

- **Status**: Accepted
- **Date**: 2026-08-11
- **Source**: product-owner decision (verbatim below), overruling the team's recommendation to build quarantine first
- **Flips the default set by**: [#474](https://github.com/TheCaptainCompany/captain-food/issues/474) / [#478](https://github.com/TheCaptainCompany/captain-food/pull/478), which landed `DbFaultPolicy` **gated and inert** (`DbFaultPolicy::Skip` = the pre-existing behaviour)
- **Realized by**: _(filled at completion)_

---

## The decision

> *"A. The projector has to stop and indicates it in the health. So k8s will detect it and we will be
> informed."*
>
> — product owner, 2026-08-11

`DbFaultPolicy`'s default flips from `Skip` to **`Halt`**: a database-rejected fold leaves the
checkpoint exactly where it was rather than advancing past an event that did not land. This is the
gate-then-stabilize default flip CLAUDE.md requires be recorded separately from the gated form — the
gated form shipped in #478 with tests
(`crates/infrastructure/tests/main/projection_checkpoint_halt.rs`).

The team recommended building quarantine first and was **overruled**. That is recorded as a choice,
not a concession: `Skip` leaves a read model *permanently and silently wrong*, and for a
money-adjacent or authorization-bearing projection that is worse than being stuck.

## What the decision requires before it can land — this is not a caveat, it is a precondition

**Verified on `5fdc519`: flipping the default today produces a projector that wedges permanently and
reports itself completely healthy to Kubernetes on both probes.**

| # | Fact | Evidence |
|---|---|---|
| F1 | Under `Halt` the worker **does not stop**. The failing slice's transaction rolls back, `run_once` returns `Err`, and the loop continues — *"Errors are recorded on the status snapshot by run_once; the loop keeps going"*. The group retries the same failing slice **every tick** (1.5 s, or on wake) | `crates/infrastructure/src/projection/worker.rs:800-816`, `:688-700` |
| F2 | `ProjectionStatus.running` therefore stays **`true`** — it is set once at loop start and never cleared by a fault | `worker.rs:688` (`self.status_mut().running = true;`) |
| F3 | `/projector` returns **`200 OK` whenever `running` is true** — so a halted projector reports READY | `crates/server/src/lib.rs:1377-1392` |
| F4 | **Neither Kubernetes probe looks at projection status at all.** Projector bins probe `readinessProbe: /health` (the DB-reachable + schema-version gate, ADR-0043) and `livenessProbe: /ping` (*"process is up; touches nothing"*) | `deploy/generated/manifests/bins/projector-ordering.yaml:102-111`; identical in `bam.yaml:135-144` |
| F5 | **No observability contract covers projections.** `specs/observability.yaml` mentions projections once, in prose, and declares no contract, no `projection_halted` signal and no lag alert | `specs/observability.yaml:11` |

So *"indicates it in the health, so k8s will detect it"* is **not satisfied by the flip**. Landing the
flip alone converts a silent-wrong-answer failure into a silent-no-answer failure, which is not the
improvement the decision is for.

**Therefore the flip and the health surface land together**, and the health surface is specified here.

## The health design

### 1. Halt stays PER-GROUP; the process stays alive

Already true by construction (F1): the rollback is scoped to one group's slice and sibling groups in
the same tick are untouched. **It must not become process-level.** A `projector-ordering` bin hosts
every ordering group, so a process-level halt would convert one poisoned read model into a
**scope-wide projection outage** — trading a stuck `View_X` for a stopped cart, order tracking and
authorization index at once.

Note the wording mismatch worth keeping straight: *"the projector has to stop"* is satisfied in the
sense that **the checkpoint stops advancing**. The worker itself keeps ticking and re-failing. That is
correct — it means the group self-heals the moment the underlying schema fault is fixed, with no
restart — but it also means an ERROR per tick per halted group, which the payload below replaces as
the primary signal.

### 2. READINESS, not liveness

| Probe | What Kubernetes does | Verdict |
|---|---|---|
| **`readinessProbe`** ✅ | Marks the pod `0/1 READY`. **Projector bins have no `Service`** (verified: none in `projector-ordering.yaml`), so there is nothing to remove from rotation — readiness here is a **pure signal channel with no side effect**: visible in `kubectl get pods`, in Argo CD health, and to any `kube_pod_status_ready` alert | **Use this** |
| `livenessProbe` ❌ | kubelet **kills and restarts** the container | **Actively harmful.** A database-rejected fold is deterministic: the restart re-reads the same event from the same checkpoint and fails identically → **CrashLoopBackOff** with backoff to 5 minutes. The restart cannot fix a schema fault, and while it loops **every other group in that bin stops draining too**. It would manufacture the scope-wide outage §1 exists to prevent |

This *serves* the product owner's intent rather than narrowing it: readiness gives "k8s detects it and
we are informed" in full, and liveness adds only a restart loop that fixes nothing and takes siblings
down with it.

Concretely: point the projector bins' `readinessProbe` at **`/projector`** instead of `/health`, and
make `/projector` return `503` when any group is halted. Liveness stays on `/ping`.

### 3. The payload must name the group and the event

`ProjectionStatus` today is **per-worker, not per-group** — `checkpoint`, `lag`, `last_error` are
aggregates over all groups (`crates/infrastructure/src/projection/mod.rs:13-28`), so it structurally
cannot say *which* group halted. "unhealthy" is not an operator's answer. The status gains a per-group
breakdown, and each halted group reports what triage needs:

```json
{
  "running": true,
  "halted": true,
  "groups": [
    { "group": "ordering", "checkpoint": 918233, "lag": 0, "halted": false },
    { "group": "scope_membership", "checkpoint": 918107, "lag": 126, "halted": true,
      "haltedSince": "2026-08-11T19:42:07Z",
      "haltedAt": { "position": 918108, "eventType": "OrderPlaced", "stream": "Order-{id}" },
      "error": "column \"service_type\" of relation \"OrderFacts\" does not exist" }
  ]
}
```

`group`, `position`, `eventType` and `error` are already carried by the halt log line
(`worker.rs:807-814`) — this promotes them from a log to a queryable surface.

### 4. The signal does not exist and must be declared

F5: there is **no projection contract in `specs/observability.yaml`**. A halted group needs
`projection_halted` (a labelled gauge/counter by group) and a projection-lag signal, so the condition
pages rather than waiting to be noticed. Declared as a contract, not as a dashboard.

## Known consequence accepted by flipping now: the role-revocation wedge

`ScopeMembership` is *"the single index every read-side authorization question resolves against, for
every role and every surface"* (`specs/database/tables/projection_tables.yaml:801-810`) — and it is a
**projection**. If its group halts, **read-side authorization freezes at the last good position**:
grants stop arriving, and **revocations stop applying**. A removed staff member or a deactivated rider
keeps their access until a human clears the fault.

That directly touches a guarantee already recorded: the §6.4 claim-staleness closure
([ADR-20260810-194548](ADR-20260810-194548-six-decision-answer-sheet-claim-staleness-closed.md))
decided revocation must be *"explicit and immediate"* for rider deactivation and staff removal. A
halted `ScopeMembership` group breaks that silently.

This is **accepted, not solved, by flipping now** — under `Skip` the same event would be skipped and
the index left permanently wrong, which is worse in kind (wrong beats stale for an authorization
index). It is written down here so nobody rediscovers it as a surprise:

- the per-group health payload (§3) makes `scope_membership` halting **nameable**, which is the
  minimum;
- **quarantine remains the real fix** and stays a tracked follow-up — it is what lets an unrelated
  group keep draining while the poisoned record is parked;
- until then a halted `ScopeMembership` is an **incident**, not a ticket, and its alert should say so.

## Consequences

- The flip does not land alone: `/projector` per-group status, the readiness re-point, and the
  observability contract land with it.
- `deploy/**` manifests are GENERATED — the probe change is an emitter change, not a hand-edit.
- Quarantine is deferred, with the wedge above recorded as its justification rather than as a nice-to-have.

## Refs

- `crates/infrastructure/src/projection/worker.rs:800-816` — the halt branch; `:688-700` — the loop that keeps running; `:117-125` — `DbFaultPolicy`
- `crates/infrastructure/src/projection/mod.rs:13-28` — `ProjectionStatus`, per-worker not per-group
- `crates/server/src/lib.rs:1377-1392` — `/projector` returns 200 while `running`
- `deploy/generated/manifests/bins/projector-ordering.yaml:102-111` — readiness `/health`, liveness `/ping`
- `specs/database/tables/projection_tables.yaml:801-810` — `ScopeMembership`, the authorization index
- [ADR-20260810-194548](ADR-20260810-194548-six-decision-answer-sheet-claim-staleness-closed.md) — the immediate-revocation guarantee this wedge touches
- [ADR-0043 "DB migration release strategy"](0043-db-migration-release-strategy.md) — what `/health` currently gates (the schema-version readiness gate `crates/server/src/lib.rs:8` names)
- [#474](https://github.com/TheCaptainCompany/captain-food/issues/474) · [#478](https://github.com/TheCaptainCompany/captain-food/pull/478) — where `DbFaultPolicy` landed, gated and inert
