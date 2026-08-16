# ADR-20260816-213000 — The birth-gap detector reads the saga's own run state, not an anti-join of two aggregates

**Status**: Accepted · **Date**: 2026-08-16 ·
**Deciders**: the mob, on the [#608](https://github.com/TheCaptainCompany/captain-food/issues/608)
dispatch — `vernon` raised it, `observability` independently verified the deciding fact, both
converged; `young` set the fold constraint, `beck` the proof shape, `business` the response routing ·
**Realized by**: [#608 "Nothing detects an authorized payment with no order birth"](https://github.com/TheCaptainCompany/captain-food/issues/608),
PR [#610](https://github.com/TheCaptainCompany/captain-food/pull/610) ·
**Context**: [ADR-20260810-231300 "No polling, only pushing"](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)
(the monitoring carve-out this sweep lives in) ·
[ADR-20260811-014129 "A business metric IS a projection"](ADR-20260811-014129-a-business-metric-is-a-projection-and-every-reference-is-a-ref.md)
(why this one is NOT one) ·
[ADR-20260808-195315 "The customer answers the decision brief"](ADR-20260808-195315-customer-brief-answers.md)
§1.2 (authorize on checkout, capture on fulfilment — why an AUTHORIZED payment can sit unsettled at
all) ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

## Context

Nothing in the system could answer *"is a customer's card authorized right now with no order behind
it?"*. The one declared signal for the class,
`payment_authorized_unsettled_age_seconds`, is computed over the **OrderTracking read model** — and
an order that was **never born** has no OrderTracking row. The gauge was structurally blind to
exactly the case its own contract header cited it for, and it had **zero emit sites in
`crates/**`**: a declared contract with no runtime, which reads on a dashboard exactly like a
working one.

The dispatch proposed detecting the gap by anti-joining the **Payment** aggregate's authorization
facts against the **Order** aggregate's births.

## The objection, and the fact that settled it

`vernon` refused the anti-join: two aggregates have two write paths and two clocks, so "authorized
and not born" is a statement about a simultaneity that is not observable, and it makes another
consumer's projection an input to the money path.

He and `observability` then independently verified the deciding fact, which nobody had stated:

> **The `payment_process_manager` row is created at CHECKOUT, not at PM processing.**
> `crates/application/src/commands.rs` — the `PlaceOrder` handler appends `PaymentIntentCreated` and
> then opens the run at `AWAITING_PAYMENT_RESULT`/`PENDING`. The PM legs only `expect` an existing
> row (`generated/process_managers.rs`).

So an authorization the saga **never processed still has a run row**. The #596 unseeded-lane case is
visible, not blind. That collapses the whole main population onto one table with one owner and one
write path.

## Decision

**1. The main population is the saga's own run state, joined to the durable record of its own
trigger.** A `PaymentAuthorized` hop on the `PlaceOrderProcess` lane whose
`payment_process_manager` run is still `AWAITING_PAYMENT_RESULT`. That is not a cross-aggregate
anti-join: both sides are the saga's runtime state.

**2. ONE gauge, `payment_authorized_no_order_birth_age_seconds{reason}`, over a DECLARED bounded
label set** — never one gauge per reason:

| `reason` | Meaning | Threshold |
|---|---|---|
| `retry_pending` | hop still deliverable (`RECEIVED`/`SCHEDULED`), run unresolved | lane-derived (below) |
| `delivery_exhausted` | hop terminal, run still unresolved — nothing will retry | **0** |
| `no_run` | a `PaymentAuthorized` in `domain_events` with **no run row at all** | **0** |

**3. `no_run` is a third reason member, not a second gauge.** It is the residue the run-state source
cannot cover: `PlaceOrder` performs two sequential unfenced durable writes — the Stripe intent
create, then the run-row upsert — and a crash between them leaves **funds held with no run row**.
The webhook leg then finds nothing by intent and skips, while the Payment aggregate still records
`PaymentAuthorized`. Visible only in `domain_events`. It is the same question, so it is the same
gauge.

**4. Zeros are the contract.** Every member is emitted on EVERY sweep, plus
`payment_birth_gap_sweep_heartbeat_total` **after** a complete pass. An absent series must never
read as zero, and an early return on an empty population is the specific defect designed against.

**5. Thresholds are lane-derived, and their antecedents are `$ref`s.** *A threshold citing a number
no contract owns is not a threshold.* `retry_pending`'s bound is the whole exponential retry
schedule — `MAILBOX_HEARTBEAT_SECONDS × (2^MAILBOX_MAX_DELIVERY_ATTEMPTS − 1)` = 310 s — plus slack,
declared as 600 s with both keys named by `$ref` into `configuration.yaml`.

**6. The fold is over OPENNESS, never AGE** (`young`, binding wherever a fold appears). Nothing
materialises `ageSeconds` or `isOrphaned`; the sweep reads the open set and its stored timestamps
and applies `now()` at query time. A stored age would be rewritten by rebuild-time `now`.
Falsification: truncate, replay, diff — the open set and its timestamps must be identical.

**7. Operational, not BAM.** This is a dead-man's switch on the money path: it must work when the
business read models are broken, so it stays on OTLP and is not a `bam` fold
(ADR-20260811-014129's own operational/BAM line). It reads Postgres because the durable record IS
Postgres; when Postgres is down the series stops, which is itself the alarm.

**8. The existing gauge is AMENDED, not deleted, and rides the same sweep.**
`payment_authorized_unsettled_age_seconds` is correct for born-but-never-**captured**; only its
header's claim to be THE switch for the never-born case was false. The claim is deleted, the signal
is kept, and #608 is the first thing that ever emitted it — shipping a second declared-but-silent
money-path contract is the failure this chunk exists to stop.

**8bis. "Emitted" is not "no longer silent" — the corollary this chunk learned on itself.** The
first cut of §8 shipped the gauge emitted but UNPROVEN: no projector ran in its test binary, so
`ordertracking` was empty for the whole suite and the gauge's single `== 0.0` assertion was
satisfied by a query that could not return anything else. Mis-spelling its predicate
(`'AUTHORIZED'` → `'AUTHORISED'`) left the suite green. **A gauge wired to a permanently-empty
population is not distinguishable on a dashboard from the declared-but-silent state it replaces**,
so the §20 rule reporting it clean would have been asserting a runtime nobody had seen work. The
claim in §8 is true only with the positive control that now stands beside it (two distinct
projected ages ⇒ the older; a second value ⇒ a different number), and the mis-spelling mutant is
red. Generally: `obs-metric-no-emitter` proves a name can be SPELLED at a call site, never that the
call site is reached with a value — that half is always a test that looks at the series.

**9. Response routing is IN scope; remediation automation is NOT.** An alert with no named response
is a control that renders and does nothing, so
[`docs/runbooks/authorized-payment-no-order-birth.md`](../runbooks/authorized-payment-no-order-birth.md)
lands with the signal (check Stripe → cancel the PaymentIntent **by hand** → contact the customer →
note the reclamation), linked from the contract. Automating the void is money movement and is a
separate decision.

## Consequences

- A `PaymentAuthorized` hop that is delivered but whose leg `Skip`s without resolving the run reads
  as `delivery_exhausted`, which is the honest classification: no further delivery will occur.
- **`delivery_exhausted` is reachable in the current runtime without any infrastructure fault**, and
  the branch's earlier claim to the contrary is retracted (see the dispatch card's `## Findings`).
  The route is the injected `EventStore` port failing the PM leg's Order-stream read plus a poison
  cap of 1; it is exercised as a positive control, and it answers
  [#611](https://github.com/TheCaptainCompany/captain-food/issues/611)'s reachability question.
- **The checkpoint MISS this chunk produced** — the ~50 s threshold derived from a linear reading of
  an exponential backoff, in the dispatch card itself — is the FIRST answer to
  [DECISIONS §44 MOB-COST-1a](../proposals/DECISIONS.md), and it **reverts the HIGH-CONSEQUENCE
  reversibility class to the whole roster at briefing AND checkpoint** (ADR-20260816-134352).
- The gauge reports an AGE, never an identity — a payment intent id is not a metric label. The
  runbook carries the queries that recover the identities.
- **Known gap, recorded, not invented**: there is **no alert-route wiring anywhere in this repo**,
  so no artifact names a human or a rota. This is the same gap as the
  `ROUTE_ORDER_BIRTH_THROUGH_LANE` flip's "the rollback trigger has no observer" obligation, which
  is founder-gated. It is recorded in the contract and the runbook and cross-linked; inventing a
  route here would have been worse than naming the hole.
- A new validator rule, `obs-metric-no-emitter` (§20), makes "declared but silent" a ratcheted
  warning: 41 existing metrics are frozen in the baseline and a 42nd is a hard gate failure.
