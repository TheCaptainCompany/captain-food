# PROP-20260811-093000 — Storage boundaries and least-privilege database users: the write-side transactional unit, the five-database split, and the last five `View_*`

- **Status**: Proposed
- **Date**: 2026-08-11
- **Tracking issue**: [#494 "Storage boundaries and least-privilege database users: the write-side transactional unit, the five-database split, and the last five View_*"](https://github.com/TheCaptainCompany/captain-food/issues/494)
- **Realized by**: _(filled at completion)_
- **Refines**: [PROP-20260807-174246](PROP-20260807-174246-one-decomposition-axis-specs-schemas-projectors.md) D2/D3/D4 (Approved, [ADR-20260807-183024](../adr/ADR-20260807-183024-one-decomposition-axis.md)) · [ADR-20260807-002705](../adr/ADR-20260807-002705-self-hosted-postgres-on-ovh-mks-with-cloudnativepg.md) as amended by [ADR-20260807-114122](../adr/ADR-20260807-114122-mks-starts-at-one-node.md) · [ADR-20260731-160000](../adr/ADR-20260731-160000-erasure-is-a-journey-tombstone-then-stream-deletion.md) (erasure) · ADR-0039/ADR-0040 (fold views / hybrid projectors)
- **Adjacent, and each assumes the other**: [PROP-20260811-150242](PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md)
  (**the domain boundaries**, [#493](https://github.com/TheCaptainCompany/captain-food/issues/493), register §31 BND-1…BND-5)
  decides **which units exist** — the boundary set, its name, and which app belongs to which. This
  proposal decides **what shares a recovery posture and a buffer pool**, and defers the unit question
  to it entirely: nothing here changes if the answer is 4 boundaries, 5, or the 8 scopes. The reverse
  deferral is explicit too — BND-3 asks whether storage follows the boundary one-to-one and answers
  **no**, for the reasons §4 gives; see **§4.2**, which records that conclusion and the stop condition
  that keeps the deviation honest.
- **Concerns**:
  - [ ] **erasure-fail-open**: the deletion engine's scan bound is `MIN(position) FROM projection_checkpoint` and `COALESCE(…, i64::MAX)` — a write database with **zero** checkpoint rows bounds at log head and erases **without** the fold verification ADR-20260731-160000 §4 requires. The split creates exactly that database. §5.3's `projection_watermark` must land **before** any database is split, and this concern stays unchecked until it has.
  - [ ] **connection-ceiling**: the split puts the post-cutover pod fleet at ~235 backends against `max_connections: 220` on a 1 Gi instance (§8.3). A session-mode pooler is a **prerequisite**, not a follow-up; transaction mode silently kills `LISTEN` and would break the push-driven mailbox and `event_wake`.

---

## 1. Origin — the product owner's text, verbatim

Product owner, 2026-08-11:

> We should have one database for
>
> DomainEventLogDb <== domain_events
>
> DomainCommonDb <== customer, restaurant, rider
> CatalogDb
> OrderDb
>
> BehaviorEventTrackingDb <== events table
>
> ——
> Every app/worker that need to access a database must have a dedicated user in the database with the most restricted access based on the spec
>
> Normally
> - the reading of the read side is done only by graphql queries and projectors to know the current state of the rows to update them
> - the writing of the read side is done only by the projectors
> - the writing of the write side is done only by the actors
> - the reading of the read side is done by actors to load the events and the projectors

**Reading of the last bullet, stated so it can be corrected.** Taken literally the fourth bullet
repeats the first. Read as written it also contradicts the second and third: an actor that reads the
*read* side would be taking a decision on a projection, and the whole point of the write model is
that an actor decides from its **own stream**. We therefore read the fourth bullet as *"the reading
of the **write** side is done by actors (to load their stream) and by projectors (to fold the log)"*
— which is exactly what the code does today (`EventStore::load` for actors,
`worker.rs:667` `SELECT … FROM domain_events WHERE position > checkpoint` for projectors) and which
makes all four bullets a consistent, complete, correct access model. **This proposal is built on that
reading.** If the intent was the literal one — actors reading read models — say so, because that is a
different and much more expensive design (it re-imports the weak-isolation anomaly catalogue onto the
order path: a decision taken on a projection is a decision taken on stale state, and at Friday peak
that is oversell).

**The access model is accepted as the strong default and is correct.** What follows completes it,
prices it, and names the three places it does not close.

---

## 2. The dominating fact, verified — and the number that changes the recommendation

`specs/database/projection_views.yaml` does emit every `View_*` as `CREATE OR REPLACE VIEW … FROM
domain_events` (ADR-0039), and Postgres has no cross-database queries. So moving `domain_events` out
of the database that serves reads does break those views. Verified. But the blast radius is not "every
`View_*` and every GraphQL query" — it is **measurably smaller and points the other way**:

| Measured on `main`, 2026-08-11 | Count |
|---|---|
| `View_*` SQL fold views (`specs/database/projection_views.yaml`) | **5** |
| Materialized projection **tables** (`specs/database/tables/projection_tables.yaml`) | **11** |
| Generated fold SQL (`specs/generated/views.generated.sql`) | 138 lines |
| GraphQL queries total (8 × `specs/{scope}/api.yaml`) | 32 |
| Queries that break if `domain_events` leaves the read database | **9 (28%)** |
| Queries that survive untouched (they read materialized tables) | **23 (72%)** |
| Queries on the Friday-peak **money path** that break | **0** |

The five views and the nine queries:

| `View_*` | Queries it serves | Surface |
|---|---|---|
| `View_DeliveryJob` | `delivery`, `myDeliveries`, `restaurantDeliveries` | rider job list, restaurant delivery board |
| `View_DeliveryPartnerAvailability` | `deliveryPartnerAvailabilities` | admin/external partner list |
| `View_Reclamation` | `myReclamations`, `restaurantReclamations`, `reclamation` | claims |
| `View_PendingRefunds` | `pendingRefunds` | restaurant refund queue |
| `View_DeliverySatisfaction` | `restaurantDeliverySatisfaction` | restaurant timeliness insight |

Everything the money path and the menu path touch — `Cart`, `OrderTracking`, `Catalog`, `Restaurant`,
`Customer`, `OrderConversation`, `CustomerCreditBalance`, `ScopeMembership`, `SlugAlias`,
`ProspectionPipeline` — is **already a materialized table**. The direction of travel is 11:5 and the
five stragglers are all off the checkout path.

### 2.1 The second finding, which flips the sign of the cost

The five views declare **8 secondary indexes** in the spec — `View_DeliveryJob` declares
`[restaurant_id, status]` ("restaurant delivery board") and `[rider_id, status]` ("rider's
assigned/available jobs"); `View_Reclamation` declares two; and so on. `specs/generated/views.generated.sql`
contains **zero `CREATE INDEX`**, against 64 in `schema.generated.sql`. A Postgres view cannot be
indexed. **Those eight indexes do not exist and never have.**

What that means at Friday peak, with the arithmetic shown:

`View_DeliveryJob`'s outer row source is `FROM domain_events c WHERE c.event_type =
'DeliveryRequested'` — one row per delivery job **ever created** — and each outer row fires **8
correlated subqueries** against `domain_events`. The filter (`rider_id = $1 AND status = 'ASSIGNED'`)
cannot be pushed down, because `rider_id` and `status` are *derived by* those subqueries. So
`myDeliveries` folds every delivery job in history to return the two or three a rider currently holds.

- Tours V0 target ~200 orders/day, ~60% delivered → ~120 delivery jobs/day
- month 3 ≈ 10,800 jobs; month 6 ≈ 21,600; year 1 ≈ 43,800
- one `myDeliveries` call at month 6 ≈ 21,600 × 8 ≈ **173,000 index probes** to return ~3 rows
- riders poll; 15 riders at 10 s intervals during 19:00–21:30 ≈ **260,000 probes/second-equivalent**

**Visible how**: the rider app's job list slows first, then times out, then riders stop accepting —
and the restaurant delivery board goes with it, on the same view, in the same two hours.
**Cheapest instrument that catches it early**: the `event.consume.projection` sibling does not cover
this path, so add a p95 latency SLO on `myDeliveries` / `restaurantDeliveries` and alert at 500 ms.
That alert would fire around month 2–3 on the numbers above.

**Consequence for this proposal.** Converting the five views to materialized tables is *not* a tax the
database split imposes. It is a **defect fix that is due anyway**, and the split merely forces us to
schedule it. That reframes the whole cost discussion below.

---

## 3. D1 — The way out of the cross-database view problem

**Final vision first**: the end state is that every read model in this system is a materialized table
written by a projector, and no read model is a live SQL fold over the log. That is where ADR-0040
(hybrid projectors) has been walking for eleven read models already, and it is the only end state
compatible with *any* separation of the log from the reads. The options below are ordered final-shape
first.

### Option A — Convert the 5 `View_*` to materialized projection tables ✅ **recommended**

| Pros | Cons |
|---|---|
| **The product owner's own access rule already requires it**: *"the writing of the read side is done only by the projectors"* is vacuous for a SQL VIEW, which nobody writes. The access model and the storage split independently point at the same change — strong evidence it is right | Real migration work (§3.5), and it must be done before any database is split |
| Fixes §2.1: the 8 declared indexes become real, and the rider board stops folding history | **Introduces projection lag where a view was always current** — the one genuine regression, §3.6 |
| Mostly **generated**: `classify_column` in `tools/codegen-rs/src/emit/projectors.rs` already handles `derive`, `occurrence` and `scalar-latest`; the YAML block moves nearly verbatim from `projection_views.yaml` to `tables/projection_tables.yaml` and the DDL + fold dispatch fall out | The non-mechanical columns need hand-written `…Compute` hooks (§3.5) |
| Makes the split a **connection-string change** afterwards: projectors read the log on one pool, write read models on another — no SQL crosses a database | Behaviour tests must be added per converted read model (ADR-0032 completeness gate) — correct, and not free |
| Restores the property Kleppmann's derived-data chapter (DDIA ch. 11–12) is built on: a projection is a materialized view whose correctness comes from **deterministic re-derivation**, and whose recovery is replay. A live SQL fold has no recovery story to *have* — it is not derived state, it is a query | — |

### Option B — `postgres_fdw` / `dblink` ❌ rejected

| Pros | Cons |
|---|---|
| Keeps the views textually unchanged; smallest diff | **Does not just move the join across a network boundary — it removes the only thing making the fold survivable.** `postgres_fdw` cannot push a correlated subquery of this shape to the remote; it fetches the foreign rows and folds locally. §2.1's 173,000 probes become 173,000 probes **plus** shipping the delivery slice of the log over a socket, per call, per rider |
| | A foreign table has no local index — the 8 phantom indexes stay phantom |
| | One extra remote connection **per backend**, on a plan whose ceiling is already crossed (§8.3) |
| | Re-introduces the cross-database join that PROP-20260807-174246 D2 explicitly designs around (*"No native cross-database join is ever needed in this shape"*) — it converts an approved property into a dependency |
| | `IMPORT FOREIGN SCHEMA` joins the migration chain, in the one place a migration chain must not be clever |

### Option C — Logical replication of `domain_events` into each read database ❌ rejected

| Pros | Cons |
|---|---|
| Views work unchanged; no application change at all | **Inverts the recovery posture the split exists to create.** The whole design case is that the log is irreplaceable (PITR, rehearsed restores) and read models are rebuildable (excluded from backups). Copying the log into 3–4 rebuildable databases makes the irreplaceable asset live in four places with four postures |
| Read databases become independently readable at full log fidelity | 4× the log's storage and 4× its WAL on a 20 Gi volume that also has to hold behaviour tracking (§9.4) |
| | **A broken subscription is the worst failure mode in the catalogue**: the views keep serving, they look current, and they are silently stale. Nobody notices until a restaurant asks why a delivery it completed still shows PENDING |
| | Breaks GDPR erasure: ADR-20260731-160000's tombstone-then-**stream-deletion** must now propagate `DELETE`s to N subscribers, on a table whose `REPLICA IDENTITY` covers a large `jsonb` payload |
| | Still no index on the fold — §2.1 is untouched. You pay 4× to keep the defect |
| | Kleppmann's framing, applied: replicating the log to N consumers is normal and good; replicating it so N consumers each re-derive **the same fold N times** is paying N× for one answer that a projector computes once |

### Option D — Keep everything in one database and abandon the split ❌ rejected

Recorded because it is the honest alternative to all of the above. It loses on the product owner's own
argument — *"I know from experience that having a database with multiple purposes ends badly"* — and on
the resource form of it: at Friday peak a HubRise menu-import burst and the order-write burst arrive in
the same two hours, and a single buffer pool means the import evicts the pages checkout needed. That is
not hypothetical here; it is the SIRENE lesson (655 MB, 77% of the database, from one department) with
a different table name.

### 3.5 — Honest cost of Option A, measured against existing code

| Work item | Evidence / estimate |
|---|---|
| Move 5 blocks `projection_views.yaml` → `tables/projection_tables.yaml` | Near-verbatim: same `columns` map, same `from` lineage, same `fedBy`, same `indexes` (which now *emit*) |
| Generated DDL + fold dispatch | **Free** — `emit_projection_tables_sql` + `emit_projectors` already consume this shape |
| Hand-written `…Compute` hooks | Only for columns `classify_column` marks `Complex`: `Money` composites (`View_Reclamation` ×2, `View_PendingRefunds` ×3), `jsonb`/composite and `timestamptz`-value columns (`View_DeliveryJob`: `courier`, `pickup_address`, `dropoff_address`, `estimated_pickup_at`, `estimated_dropoff_at`), and the one `occurredWhen` column (`View_DeliveryJob.delivered_at` — `ColMode` has no `OccurredWhen` variant). **`View_DeliverySatisfaction` and `View_DeliveryPartnerAvailability` appear fully mechanical.** Comparable existing hooks: `projectors/restaurant.rs` 38 lines, `slug_alias.rs` 21, `prospection_pipeline.rs` 44 |
| 5 store modules | Existing ones run 57–195 lines, median ~75 → ~400 lines |
| 5 registry arms in `projection/worker.rs` | Mechanical, one `ReadModelProjector` variant + one `apply_inner` arm each |
| Rewrite 5 read repos | `crates/infrastructure/src/persistence/{delivery,reclamation,refund_queue,delivery_satisfaction,delivery_partner_availability}.rs` = 490 lines total; the change is `FROM View_X` → `FROM x` with **identical column names**, because the generator emits the same columns |
| Behaviour tests + `rules:` links | ADR-0032 completeness gate; the real, irreducible cost |

**This is one to two sessions of patterned work, not a re-platforming.** The brief's framing —
*"it changes the projection worker's job from 'maintain some tables' to 'maintain all read state'"* —
is correct in principle and modest in practice: the worker already maintains 11 of 16 read models.

### 3.6 — The one genuine regression: projection lag

Today `myReclamations` is transactionally current with the append — the customer opens a claim and the
view *has* it. After conversion it is checkpoint-current, so there is a read-your-writes window.

This is not a new problem class; it is the problem `OrderTracking` already has and already solves
(acceptance-first `PENDING` + the operation-status read). It must be named per surface rather than
waved at:

| Surface | Exposure | Answer |
|---|---|---|
| `reclamation` / `myReclamations` after opening a claim | High — the customer looks immediately | The mutation payload already returns the accepted operation; the claim detail screen reads it until the projection catches up (same pattern as checkout) |
| `pendingRefunds` after a restaurant approves | Medium — the restaurant re-reads its queue | Same pattern |
| `myDeliveries` after a rider accepts | **High and load-bearing** — a rider who accepts and does not see the job accepts it again | Same pattern, and it deserves a behaviour test, because this is the oversell failure mode wearing a delivery hat |
| `deliveryPartnerAvailabilities`, `restaurantDeliverySatisfaction` | Low — admin/insight surfaces, human-paced | No special handling |

**Instrument**: the existing projection-lag gauge, per group, alerting at a lag the acceptance-first
pattern can absorb (a few seconds). This is cheaper than the current situation, where the "always
current" view is current and *slow*, and slowness has no ceiling.

---

## 4. D2 — Does the split match the domain boundaries?

**No, and it should not — the product owner's own deviations are right, for a reason worth stating
explicitly, and the list is incomplete.**

The correct axis for a **database** boundary is not *"is this a different domain"*. It is **does this
share a fate at peak, and does it share a recovery posture**. Domain boundaries answer the ownership
question (who may change this); they do not answer the resource question (whose buffer pool does this
evict at 20:30). Applying the resource axis to the product owner's list:

| His grouping | Verdict | The reason (which is *not* the domain reason) |
|---|---|---|
| `CatalogDb` separate | ✅ **right** | HubRise catalog imports arrive as **bursts** (whole menus at once), and Friday 19:00–21:30 is simultaneously an order-**write** burst and a menu-**read** burst. Menu-import churn, its bloat and its autovacuum must not share a buffer pool with checkout writes |
| `OrderDb` separate | ✅ **right** | The money path is the one path whose latency is revenue. It gets its own resources on that basis alone |
| `customer` + `restaurant` + `rider` grouped in `DomainCommonDb` | ✅ **right** | Not "they are small" and not "they are one domain" — **none of the three is on the write-burst path at peak**. They are read-mostly, human-paced-churn read models (a restaurant edits hours; a rider registers). Grouping read-mostly low-churn read models is a resource decision and it is correct |
| `BehaviorEventTrackingDb` separate | ✅ **right** — see §9 | Opposite access pattern (analytics vs OLTP, DDIA ch. 3) and, on the arithmetic, **13× the log's growth rate** |
| `DomainEventLogDb <== domain_events` **alone** | ❌ **the one that does not close** | The log is not the transactional unit. §5 |

So: storage follows the domain boundary for the *business read models*, and deviates deliberately in
two places, both defensible from resource coupling and neither defensible from domain ownership.

### 4.1 What the list does not assign — and the object that decides the shape

The five-database list is stated in domain nouns. The actual objects are **~65**: 11 projection tables,
8 referential tables, 3 integration-connection tables, 6 integration-staging tables, ~30 `ref_*` enum
tables, 4 process-manager tables, `slug_reservations`, `projection_checkpoint`,
`inbound_messages`, `mailbox_partitions`, `domain_events`, `domain_stream`. **The list assigns one of
them by name.**

Concretely unresolved, and each is a real question: `Cart` (ordering, but pre-money and high-churn),
`OrderConversation` (comms scope, order-keyed), `CustomerCreditBalance` (payments), `SlugAlias` +
`slug_reservations` (network read model + write-side uniqueness — and they must **not** be co-located,
§5), the SIRENE mirror (655 MB once — #231), and the Stripe/HubRise staging tables.

**`ScopeMembership` is the one that decides the shape.** It is the authorization index, read on
**every authenticated query in every scope**. Three ways it can go:

| Option | Pros | Cons |
|---|---|---|
| **Projected into EVERY read database by each scope's projector** ✅ recommended | Each subgraph is self-sufficient — one database, one grant, no cross-database read on the authorization path; it is **derived data**, so N copies cost nothing conceptually and are re-derivable; failure is isolated per database | N copies to keep folding; a projector bug can now desynchronise authorization per-database (mitigated: the fold is the same generated code, and the existing `read-authorization` lag gauge already exists per group) |
| One shared database every subgraph also connects to | One copy | **Destroys the property the split is being bought for**: every `graphql_{scope}` role gains CONNECT to a second database, and "one domain, one grant" is gone |
| Duplicated into the JWT / principal | No database read at all | Membership changes would not take effect until token refresh — an authorization stale-window, which is the wrong thing to make eventually-consistent |

The recommended answer is Kleppmann's *"composition happens in the projector, not the query"*
(PROP-20260807-174246 D8's validator rule) applied to authorization, and it is the same shape the
`ref_*` enum tables want: **small, derived, replicated per database.**

### 4.2 The deviation, stated as a decision — and the stop condition that keeps it honest

The same question is asked from the other side by
[PROP-20260811-150242](PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md)
(register row **BND-3**, §31), and the two records agree — recorded here so the deviation is a
*decision* rather than something inherited from the shape of the product owner's list:

> **Storage deliberately does NOT follow the domain boundary one-to-one.** Storage groups by
> **operational profile**; the boundary is the **schema plus the per-app role**, not the database.
> `DomainCommonDb` holds three boundaries and `CatalogDb` stands alone, and both are right for
> resource reasons that have nothing to do with ownership.

What still makes it a boundary, concretely: three boundaries share a database and **still cannot read
each other**, because no app's role is granted another's schema (§6.1). That is ADR-20260807-183024 D2
refined one level down — truth-vs-serving kept, the serving side subdivided by read profile.

**The stop condition, which should become a validator rule** (BND-3's recommendation, restated here
because this proposal owns the grant emitter that would carry it — §6.2):

> **If any app's `GRANT` spans two boundaries' schemas outside the declared exceptions
> (`admin_ro`/`claude_ro` incident tooling, `bam`, `worker-erasure`), the shared database has silently
> become an integration database.**

It is checkable from exactly the inputs §6.2 already derives the grants from, so it costs a rule and
not an investigation. Without it the deviation is indistinguishable, six months on, from never having
had a boundary at all — which is the failure mode the product owner named on 2026-08-07.

---

## 5. D3 — Atomicity: what is written in one transaction today, and what survives

Three transactions exist. All three verified in code.

### 5.1 (a) The fenced completion transaction — **breaks catastrophically if the log and the mailbox are separated**

`crates/actor_runtime/src/completion.rs:71-100`. One `BEGIN … COMMIT` carries **all** of:

1. the handler's event appends to `domain_events`,
2. process-manager state rows (`payment_process_manager`, `refund_process_manager`, `cart_binding_process_manager`, `delivery_dispatch_process_manager`),
3. scheduled reminder inserts into `inbound_messages`,
4. the `inbound_messages` terminal flip — **guard 1**, matching only `status = 'RECEIVED'`,
5. the fenced `mailbox_partitions` checkpoint advance — **guard 2**, matching only `claimed_by = me AND ownership_version = mine`.

Either guard matching zero rows **rolls the whole transaction back, handler appends included**. That is
the file's stated §3.1 guarantee: *dual BELIEF is tolerated because dual AUTHORITY is impossible.* It is
precisely Kleppmann's fencing token (DDIA ch. 8) — the storage layer rejects the stale writer by
monotonic token, and the rejection is atomic with the writer's effects.

**If `domain_events` and `inbound_messages` land in different databases, this transaction cannot
exist.** And the failure is not "we lose some atomicity". It is:

> **A paused actor pod wakes at 20:40 on a Saturday, its lease long stolen and its
> `ownership_version` stale, appends `OrderAccepted` to a stream a live worker already advanced —
> and the append commits, because the fence that would have rolled it back is in another database.**

That is the exact defect the mailbox exists to prevent, re-introduced by a storage decision.
**Two-phase commit is not an acceptable answer**, and specifically not at peak: a `PREPARE
TRANSACTION` holding locks on `domain_events` across a network stall during Friday checkout is an
outage, and an orphaned prepared transaction blocks vacuum on the log — the one table that must never
bloat.

**What replaces it: nothing. Do not break it.** The following are **one transactional unit and must be
one database**:

```
domain_events · domain_stream · inbound_messages · mailbox_partitions
· payment_process_manager · refund_process_manager · cart_binding_process_manager
· delivery_dispatch_process_manager · slug_reservations
```

Recommendation: `DomainEventLogDb` is **widened and renamed `captain-write`** — it holds the log **and
everything that commits with the log**. This is the single hardest constraint in the proposal, it is
the one place the product owner's list must change, and it is not negotiable by 2PC.

```mermaid
sequenceDiagram
    autonumber
    participant W as actor-order worker
    participant WDB as captain-write<br/>(log + mailbox + PM state)
    Note over W,WDB: ONE transaction — the fence and the appends share a fate
    W->>WDB: BEGIN
    W->>WDB: handler effects: append OrderAccepted, PM row, reminders
    W->>WDB: UPDATE inbound_messages SET status=... WHERE status='RECEIVED'  [guard 1]
    W->>WDB: advance mailbox_partitions WHERE claimed_by=me AND ownership_version=mine  [guard 2]
    alt both guards match
        W->>WDB: COMMIT
        Note over WDB: durable; post-commit fan-out runs
    else either guard matches 0 rows
        W->>WDB: ROLLBACK
        Note over WDB: the stale worker's appends are GONE — this is the whole point
    end
```

### 5.2 (b) The projector batch transaction — **survives the split cleanly**

`crates/infrastructure/src/projection/worker.rs:691-742`. One `BEGIN … COMMIT` carries every read-model
upsert in the batch **and** the `projection_checkpoint` advance ("the unit-of-work boundary"), with a
per-event `SAVEPOINT` so one poison record cannot wedge the group. This is the transaction the product
owner separately required — *projection state "saved with the checkpoint transactionally"*.

**It survives, and cleanly**, on one condition: `projection_checkpoint` is **co-located with the read
models it checkpoints** — i.e. **one `projection_checkpoint` table per read database**, not one global
one. Then:

- the projector reads `domain_events` from `captain-write` on pool A (a plain `SELECT`; no transaction needed),
- and commits fold + checkpoint on pool B, in the read database.

If the commit fails, the batch replays; the folds are idempotent by construction (`*Updated` carries
replace semantics, ADR-0039). **No 2PC, no correctness loss.** This is Kleppmann's
*exactly-once = at-least-once delivery + idempotent processing*, and it is the good news of the split:
the transaction the product owner cares most about is the one the split does not touch.

```mermaid
sequenceDiagram
    autonumber
    participant WDB as captain-write<br/>domain_events
    participant P as projector-ordering
    participant RDB as OrderDb<br/>read models + projection_checkpoint
    P->>RDB: SELECT position FROM projection_checkpoint WHERE projector='Order'
    P->>WDB: SELECT ... FROM domain_events WHERE position > $cp ORDER BY position LIMIT 500
    Note over P: fold in memory (deterministic, idempotent)
    P->>RDB: BEGIN
    P->>RDB: upsert read-model rows (SAVEPOINT per event)
    P->>RDB: upsert projection_checkpoint = last_position
    P->>RDB: COMMIT
    Note over P,RDB: crash anywhere -> replay the batch; idempotent folds absorb it
    P-->>WDB: heartbeat: UPSERT projection_watermark (GREATEST) -- see 5.3
```

### 5.3 (c) The GDPR deletion engine — **its bound becomes cross-database, and it FAILS OPEN**

`crates/infrastructure/src/deletion.rs`. Its `tx2` (delete the stream's rows from `domain_events` and
`domain_stream`, append the receipt to `DeletionLedger-<Actor>`, advance its cursor) is entirely
write-side: **survives.** Its **scan bound** does not:

```sql
-- deletion.rs:229-233
SELECT COALESCE(MIN(position), 9223372036854775807) FROM projection_checkpoint
 WHERE projector <> 'DeletionEngine'
```
then `let bound = bound.min(head);`

That `MIN` is how the engine proves *every read model has already folded the fact* before erasing the
stream — ADR-20260731-160000 §4's phase-1 tombstone verification, expressed as a scan bound. After the
split that `MIN` would have to span 3–4 read databases.

**And here is the sharp edge.** The `COALESCE(…, i64::MAX)` then clamped to `head` means: **a database
with zero `projection_checkpoint` rows bounds at the log head and erases everything reachable,
immediately, with no verification that anything was ever folded.** Today this is harmless — all
checkpoints live in the same database as the engine. **After the split, `captain-write` has exactly
zero `projection_checkpoint` rows**, and the engine silently loses its only safety bound. Fail-open, on
the legal path, with no error, no log line and no test that would notice.

**Replacement — and it must land before any split:**

A `projection_watermark` table **in the write database**. Each projector, on the tick after its batch
commits, upserts its position there monotonically (`SET position = GREATEST(position, EXCLUDED.position)`).
The engine's bound becomes `MIN(position) FROM projection_watermark`, and — critically — the
`COALESCE` default is replaced by **fail-closed**: no watermark rows means bound `0`, i.e. erase
nothing, rather than bound `head`, i.e. erase everything.

Why this is safe rather than clever: the heartbeat may lag, and lag only makes the bound **more
conservative** — it delays deletion, never deletes early. That is the only direction a legal guarantee
may err in. One table, one statement, one changed default.

**This is worth landing even if the split never happens**, because the current fail-open is one empty
table away from erasing unverified — and "one empty table away" includes a fresh production database,
which is what start-clean creates.

### 5.4 Two smaller things a rename would miss

- `crates/infrastructure/src/mailbox/pm_delivery.rs:296,406` reads and writes `projection_checkpoint`
  rows keyed `pm:PlaceOrderProcess` / `pm:RefundProcess`. Despite living in the projection-checkpoint
  table, these are **write-side PM backfill cursors**, not projections. They move to `captain-write`.
- `crates/infrastructure/src/mailbox/activation.rs:133` (`guard_freshness_in_tx`) reads
  `MAX(version) FROM domain_events` **inside** the completion transaction. Write-side: survives —
  and is a second reason the log cannot leave `captain-write`.

---

## 6. D4 — Least-privilege users: how far Postgres actually reaches

Honest inventory of the mechanisms, strongest wall first:

| Mechanism | What it actually buys here | Verdict |
|---|---|---|
| **`REVOKE CONNECT` on a database** | A role that cannot connect to `CatalogDb` cannot read it. Full stop. No `search_path` accident, no slipped GRANT, no clever join | **The strongest wall, and the split buys it for free.** This is the argument that makes database-per-domain better than schema-per-scope *for the read side*: with schemas, a role with a wrong `search_path` can still `SELECT * FROM other.table` if one GRANT slipped |
| **Table-level `GRANT`** | "projector-ordering writes only ordering read models"; "a subgraph gets SELECT only" | Sufficient, and fully generatable (§6.2) |
| **Column privileges** | Available | **Do not use.** Nothing today needs it; it is maintenance burden against no current threat |
| **RLS `WITH CHECK` on `domain_events`** | This is where *"an actor writes only its own aggregate's streams"* lives, and it genuinely works: `INSERT … WITH CHECK (stream_name LIKE 'Order-%')` rejects a cross-stream append | **Recommended, gated and benchmarked.** Cost: a policy evaluated on every append and every fold read, on the money path. The predicate is a `LIKE` against an indexed text column — probably small, definitely unmeasured. §6.3 |
| **`SECURITY DEFINER` append function** | The alternative row-level boundary: `REVOKE INSERT ON domain_events FROM PUBLIC` and one `append_event(...)` function checking `current_user` | **Rejected.** It forks the most correctness-critical code in the system — the multi-event transaction, `pg_notify`, version-conflict semantics — into PL/pgSQL, and then that fork has to stay in step with `PgEventStore::append` forever |

### 6.1 The model, one row per role class

| Role | CONNECT | `domain_events` | mailbox / PM / journal | its scope's read models | other scopes' read models | Enforced by |
|---|---|---|---|---|---|---|
| `actor_{Actor}` (×16) | `captain-write` only | INSERT + SELECT, **own stream prefix** | SELECT/UPDATE its own lane rows | — | — | CONNECT + GRANT + **RLS `WITH CHECK`** |
| `projector_{scope}` (×8) | `captain-write` (read) + its read DB (write) | **SELECT only** | — | INSERT/UPDATE/DELETE + its `projection_checkpoint` + its `projection_watermark` heartbeat | — | CONNECT + GRANT |
| `graphql_{scope}` (×8) — **query** path | **its read DB only** | **no CONNECT** | — | **SELECT only** | **no CONNECT** | **CONNECT — the wall** |
| `graphql_{scope}` (×8) — **mutation** path (§6.1.1) | its read DB **+ `captain-write`** | **no access** | `inbound_messages`: **INSERT + SELECT**, nothing else | (as above) | **no CONNECT** | CONNECT + GRANT (+ optional RLS on `actor_type`) |
| `deletion_engine` | `captain-write` only | SELECT + DELETE + INSERT (ledger streams) | — | — | — | GRANT |
| `tracking_projector` | tracking DB only | **no CONNECT** (§9.2) | — | its own tables | — | CONNECT |
| `admin_ro` / `claude_ro` | all, SELECT-only | SELECT | SELECT | SELECT | SELECT | GRANT — **incident tooling only**, never an application path |
| `migrator` | all, DDL | owner | owner | owner | owner | CI-only credential |

The load-bearing line is `graphql_{scope}`: **no CONNECT to any other read database.** That is what
the product owner is buying, and it is exactly what the currently-approved schema-per-scope shape
does *not* give.

### 6.1.1 Two corrections the matrix needs before it becomes a `GRANT`

Both are correctness checks, not design choices — an ambiguous line in a permission matrix becomes a
wrong `GRANT`, and a wrong `GRANT` is either a boot failure or a silent breach. The second was caught
on the independent pass and is the reason this subsection exists at all; it also appears as
**BND-4** in the register (§31), and the two records must stay in step.

**(i) The fourth bullet is a transcription slip, and must be CONFIRMED before it becomes a role.**
The directive's fourth bullet reads *"the reading of the **read** side is done by actors to load the
events and the projectors"*. Loading events is reading the **write** side; read as written the bullet
duplicates the first and contradicts the second and third. §1 states the reading this proposal is
built on — *"the reading of the **write** side is done by actors (their own stream) and by projectors
(the log fold)"*. **This is a reading, not a fact**: it must come back as a yes before it is emitted
as a grant, because the literal alternative — actors deciding from projections — is a different and
far more expensive design (a decision on stale state, i.e. oversell at peak).

**(ii) The matrix omitted the mailbox, and the omission is load-bearing: taken literally, it makes
every mutation fail at runtime.** *"The writing of the write side is done only by the actors"* is
true of `domain_events` and **false of `inbound_messages`**. GraphQL mutation resolvers are the
primary writer of the mailbox: the generated resolver takes `Arc<dyn Mailbox>` out of the request
context and enqueues before returning acceptance
(`crates/server/src/graphql/generated/mutation.rs:42`, every mutation; the enqueue itself is
`crates/actor_client/src/enqueue.rs:462` → `MailboxStore::insert`). A grant script generated from the
matrix as written would revoke that write and turn **acceptance-first PENDING** — the contract every
mutation returns — into `permission denied` on the first order of the evening.

The exact privileges the enqueue needs, read off the statements rather than assumed
(`crates/infrastructure/src/persistence/mailbox_store.rs:96-145`):

| Statement | Privilege |
|---|---|
| `INSERT INTO inbound_messages (…) ON CONFLICT (message_id) DO NOTHING RETURNING actor_type` | **INSERT** on the table — **and SELECT on `actor_type`**: Postgres requires SELECT on every column named in `RETURNING`, so `INSERT`-only is a `permission denied` on the happy path, not on an edge |
| the duplicate arm: `SELECT status, payload_hash FROM inbound_messages WHERE message_id = $1` | **SELECT** on those two columns — this is the idempotent-retry path, so it fires on exactly the requests a client retries |
| `SELECT pg_notify('inbound_messages', actor_type)` | EXECUTE, granted to `PUBLIC` by default — no action, but it must not be revoked, or the push-driven mailbox degrades to poll-only |

So the mutation-resolver row is **CONNECT to `captain-write` + INSERT + SELECT on `inbound_messages`
and nothing else** — no `domain_events`, no PM tables, no `mailbox_partitions`. Note honestly what
this costs: it is a **door through the CONNECT wall on the write database**, so the wall's guarantee
is precisely *"no subgraph can read another scope's read models, and no subgraph can read or write the
log"* — not *"no subgraph connects to `captain-write`"*. Narrowing it further is possible and should
be evaluated with RLS: `WITH CHECK (actor_type = ANY(<the scope's actors>))` restricts a compromised
subgraph to enqueueing for its own actors, and the actor list per scope is already derivable
(`actors.yaml` + the scope folder), so it is emitted, not hand-written.

**`schedule()` is deliberately NOT in that row.** It is an `ON CONFLICT … DO UPDATE`
(`mailbox_store.rs:161-173`) and therefore needs UPDATE — it belongs to actors and process managers
(reminders), never to a resolver, and keeping UPDATE off the subgraph role is what stops a subgraph
rewriting a queued message's payload.

### 6.1.2 The third correction — `operationStatus` reads a table the matrix does not grant, and the product owner is right that two journals exist

**Product-owner concern, 2026-08-11, verbatim:** *"Ok for the event log, but I'm concerned about the
word « journal », we have replace journal with unified mailbox inbound messages make sure we don't do
both."*

**The concern was correct, and it is now RESOLVED in the code.** When this section was written both
tables existed on `main`: `inbound_events` was backfilled and dropped (migration `20260731143000`),
but `command_journal` survived as the PM legs' door. **It no longer does.** Product-owner direction,
2026-08-11 — *"Remove inbound events and command journal from the dsl, the only tables that must
remain is inbound messages"* — landed as
[#242](https://github.com/TheCaptainCompany/captain-food/issues/242) **Runtime D**
([ADR-20260812-000000](../adr/ADR-20260812-000000-the-pm-mailbox-flip-rides-the-journal-retirement.md)):
`command_journal` is dropped by `20260812000000`, the `PM_MAILBOX_DELIVERY` gate is deleted with it,
and `inbound_messages` is the only write-path journal. **This section is kept, rewritten, because its
second finding is NOT resolved and would otherwise disappear with the first.**

#### (A) What was on `command_journal`, and where each role went

| Role | Who | Where it went |
|---|---|---|
| **Writer — the PM legs' legacy arm** | `placeOrder`, `approveRefund`, `denyRefund` when `PM_MAILBOX_DELIVERY` was **false** — the seeded default, so `PlaceOrder`'s acceptance really did live here | **Deleted.** The three PM commands are addressed to their mailbox lanes like every other command; the emitter now FAILS GENERATION on a wired mutation with no addressing rather than falling back |
| **Reader — the acceptance poll** | `operationStatus` read the mailbox, then fell back to the journal | **Mailbox only.** An unknown messageId resolves null, as it always did for non-owned ids |
| **Reader — the acceptance push** | `operationStatusChanged`'s snapshot | **Mailbox only** |
| **Reader — cross-arm duplicate identity** | the mailbox arm consulted the journal before enqueueing | **Gone with the second arm.** The mailbox pk is the whole dedupe; the PM-specific duplicate/Conflict property is still asserted (`graphql_write_path.rs`) |
| **Writer — stale-RECEIVED sweep** | `worker-journal-sweep` | **Retired** — the app count goes 57 → 56. The mailbox's own attempt cap + backoff is its liveness backstop |
| **Writer — retention sweep** | terminal rows aged 90 days | **Leg removed** from `sweep_retention()`; `inbound_messages` keeps its identical window |
| ~~Worker channel~~ | ~~`dispatch_journaled` for SIRENE / HubRise~~ | Already retired before this change; **the stale module docs it left behind are now corrected too**, and `application::dispatch` is deleted |

#### (B) The API-lens claim — the half that STANDS

The claim was that `operationStatus` reads the mailbox and then `command_journal`, and that §6.1 grants
subgraphs `inbound_messages` but not `command_journal`, so *"every acceptance poll in the product
returns null at 19:30 while the writes themselves succeed."*

The `command_journal` half is moot: there is no journal to grant. **The other half is not, and it is
the more serious one:**

1. **`operationStatus` is a QUERY, and §6.1's query row grants the write database *no `CONNECT` at
   all*.** The mailbox read is missing too. Read literally, the matrix breaks the acceptance poll
   outright. The pod serving it is `graphql-common` (`operation_scopes.rs:13` —
   `("query", "operationStatus", "common")`), which owns no read models, so under §6.1 as written that
   role connects to nothing and every acceptance poll in the product fails.
2. **The blast radius is the whole product, not one screen.** Mutations are acceptance-first: the
   client receives PENDING and polls up to **30 times at 1 s** (`crates/web/src/actions.rs:30-40`).
   Every checkout, every restaurant acceptance, every rider transition. **The failure mode is the
   acceptance contract silently reporting "we never heard of your order" while the order is in fact
   being processed** — this domain's named worst failure mode, seen from the customer's side.

#### (C) The decision

The option space that used to sit here — *grant `command_journal` with an expiry, or retire it first?*
— **is closed by the retirement.** Option (b) ("retire `command_journal` first, so the grant is never
needed") is what happened, earlier than this proposal expected, because the window made it cheap: the
flip it was blocked on rode the retirement rather than preceding it
([ADR-20260812-000000](../adr/ADR-20260812-000000-the-pm-mailbox-flip-rides-the-journal-retirement.md)).
**No permission is owed for a table that no longer exists**, and nothing here should be read as
licensing one.

**What the matrix still owes, and it is not optional:** the platform graph needs
`CONNECT captain-write` + `SELECT` on `inbound_messages` and `mailbox_partitions` (plus the
ADMIN-guarded `UPDATE on inbound_messages` for `requeueMailboxMessage`). Without it the acceptance
poll fails on the one remaining door. This is a **grant with no expiry** — the mailbox is the
acceptance surface permanently — so the "residual that outlives its window" worry that shaped the old
recommendation does not apply.

**Corrected §6.1 rows.** Replace the single `graphql_{scope}` pair with these three, because the
platform graph is genuinely different from a boundary subgraph and the difference is a `CONNECT`:

| Role | CONNECT | `domain_events` | mailbox / journal | its boundary's read models | other boundaries' read models |
|---|---|---|---|---|---|
| `graphql_{B}` (×5) — **query** path | **its read DB only** | **no CONNECT** | — | **SELECT only** | **no CONNECT** |
| `graphql_{B}` (×5) — **mutation** path | its read DB **+ `captain-write`** | **no access** | `inbound_messages`: **INSERT + SELECT**, nothing else | (as above) | **no CONNECT** |
| **`graphql_platform` — the acceptance surface** (`operationStatus`, `operationStatusChanged`, the mailbox lane monitor) | **`captain-write` only** — it owns no read models | **no access** | **SELECT** on `inbound_messages` and `mailbox_partitions`; **UPDATE on `inbound_messages`** only for the ADMIN-guarded `requeueMailboxMessage`. No expiry — the mailbox is the acceptance surface permanently (#242 Runtime D) | — | **no CONNECT** |

**And the standing rule that keeps this from recurring**, because the same omission has now been made
twice in one matrix — the mailbox in (ii), the journal here: **every table a resolver touches is read
off the resolver, never off the architecture.** The generator in §6.2 should derive each subgraph's
grant from the ports its resolvers actually pull out of the request context — `Arc<dyn Mailbox>`,
`Arc<dyn CommandJournal>`, each `…ReadRepository` — so that adding a port to a resolver adds a grant,
and a resolver reaching a store nobody granted is a **build** failure rather than a Friday-evening
`permission denied`. That is compiler-first applied to the permission matrix, and it is cheaper than
the two review passes that found these by eye.

#### (D) The retirement landed — what the programs in flight must not undo

This section used to ask whether the three programs in flight were adding NEW dependencies on
`command_journal` (they were not). The table is gone; what survives is the one constraint that was
called *"the one thing to watch"*, restated for the mailbox:

**`Mailbox` is a PLATFORM write-path acceptance port and must stay one.** It must not acquire a
`ports-{B}` / `read-{B}` / `projections-{B}` home under
[PROP-20260811-173223](PROP-20260811-173223-repository-crates-and-the-infrastructure-split.md) REP-2,
and `inbound_messages` must not be split out of `captain-write` under STO-1: the mailbox row's terminal
flip and the `domain_events` append **commit in one transaction**, so a boundary that owns one and not
the other has no way to be correct. The boundary reshape
([PROP-20260811-150242](PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md), §31)
leaves it in `platform` on both partitions, which is right.

### 6.2 Compiler-first: what can be GENERATED so a wrong grant cannot be hand-written

Per ADR-20260803-234035, ask what makes the mistake unspellable before writing a gate. Here almost all
of it is derivable from the spec the validator already parses:

| Derived thing | Source already in the model |
|---|---|
| The role list | `actors.yaml` + `processmanager.yaml` (16 actors) · the projector split (8 scopes) · `specs/{scope}/api.yaml` (8 subgraphs) |
| A projector's **writable** set | the read models whose `fedBy` events belong to its scope — already computed for the `view-fedby-unused` rule |
| A subgraph's **readable** set | the read models bound by its scope's api.yaml `reads:` — already computed for the api↔model cross-validation |
| An actor's **stream prefix** | its `actors.yaml` key (`Order` → `Order-%`) |
| Which database an object lives in | the placement map this proposal adds to the DSL (§4.1) |

**Deliverable: `specs/generated/grants.generated.sql`**, emitted from those five, plus two validator
rules that turn the mistake that actually happens into a build failure:

- **every read-model table has ≥ 1 writer role** — catches "a new read model was added and no projector owns it";
- **every api.yaml `reads:` binding has a matching SELECT grant** — catches "a query was added against a table its subgraph cannot see".

Grants then are not written by hand, so a wrong grant cannot be written by hand. That is the
compiler-first answer, and it is level-4-or-better in PROP-20260802-130500's hierarchy.

**What must stay runtime:** the RLS policy's *evaluation* (a row check is a row check), and tenant
`Host` scoping, which is application-level and stays where it is.

**What the compiler cannot reach, and needs a gate anyway:** **role → pod binding.** A perfect
generated grant script is worthless if `projector-ordering`'s pod mounts the `migrator` credential.
That is a manifest fact, and it gets the treatment the platform manifests already get — a codegen test
asserting every generated Deployment's DB secret name matches the role its bin is declared to use
(`tools/codegen-rs/src/tests.rs`, alongside `platform_*`).

### 6.3 Gating RLS (gate-then-stabilize)

RLS on `domain_events` ships **behind a flag**, benchmarked at ≥ 200 appends/s against the same
workload without it, and the default flip is a separate one-line ADR after the gated form has been
smoked. Enabling a row policy on the hottest table in the system in the same change that introduces it
is exactly the move that directive exists to prevent.

---

## 7. D5 — Does this subsume the missing capability witness on `EventStore::append`?

**The citation, resolved.** The brief cites *"ISO-3 (DECISIONS §29)"*, and it resolves: **§29 "Scope
isolation is nominal — PROP-20260811-090000"**, row **ISO-3**, which records that `EventStore::append`
is the one row in PROP-20260802-130500 §5's audit table marked *"❌ hole"* that nobody has filed.
(An earlier reading of this proposal reported the citation as dangling; that was a stale tree — §29
and §30 had landed. §31's closing note confirms ISO-3 is **unchanged and orthogonal**: the witness is
missing whatever the boundary set turns out to be.) **The underlying fact is verified independently
and stands**: `crates/application/src/ports.rs:54-60` —

```rust
async fn append(
    &self,
    stream_name: &str,
    expected_version: i64,
    events: &[DomainEvent],
    actor: &Actor,
) -> Result<i64, DomainError>;
```

— takes no capability witness, so any holder of `&dyn EventStore` can append any event to any stream.

**Answer: complements, and is strictly weaker where it overlaps. It is not a substitute.**

| | Capability witness (type level) | Per-actor database role + RLS (storage level) |
|---|---|---|
| Catches at | `cargo build` | runtime, in production |
| Failure looks like | a compile error | `permission denied for table domain_events` inside a handler at 20:40 Friday, surfacing as a mailbox retry storm |
| Catches | *our* code appending to the wrong stream | **any** code — a migration script, an ad-hoc `psql`, a future bin, an agent-written one-off |
| Cost | a `pub(crate)` constructor in the actor's own handler crate — a shape §14/D2's per-actor crates **already create** | a generated GRANT + a benchmarked row policy |

ADR-20260803-234035 ranks the type-level answer above the gate, and here the type-level answer is the
cheaper of the two. **Recommendation: both, witness first.** And explicitly: **this proposal must not
be used to close the witness item.** A storage role that turns a compile error into a Friday-evening
runtime error is not an improvement on a compile error; it is a second, different wall that happens to
catch a class the first one cannot.

---

## 8. D6 — Operational cost on CloudNativePG

Baseline, from `deploy/platform/cnpg/cluster.yaml` (ADR-20260807-002705 as amended by
ADR-20260807-114122): **one cluster, `instances: 1`**, `requests/limits memory 1Gi`,
`shared_buffers 256MB`, `max_connections 220`, `storage 20Gi`, `retentionPolicy 30d`, WAL archiving to
OVH Object Storage as the **only** recovery path, one bootstrap database `app`.

### 8.1 One cluster with five databases, or five clusters?

| Option | Pros | Cons |
|---|---|---|
| **One cluster, five databases** ✅ recommended | **One WAL timeline, one base backup, one PITR** — a restore to time T brings all five databases to the *same* point, which is what makes a cross-database incident recoverable at all; fits the memory budget; one operator surface, one drill | Shared buffer pool and shared WAL — the split buys *ownership* isolation (CONNECT walls, blast radius, independent migration) but not full *resource* isolation until a cluster split |
| Five clusters | True resource + failure isolation | **Not affordable**: ~5.5 Gi of ~6.3 Gi allocatable on the single d2-8 node is already spoken for, of which ~1 Gi is this cluster. Five clusters at even 512 Mi = 2.5 Gi the node does not have. Also five WAL streams, five base backups, five drills, and **five independent timelines whose cross-restore is not mutually consistent** |
| **Two clusters: business (4 databases) + tracking (1)** ✅ recommended **when tracking ships** | Puts the only database whose *size* makes the physical base backup expensive (§9.4: ~17 GB/yr) outside the business cluster, where it can have its own posture and its own retention | A second operator surface and a second drill; only pays once tracking is real |

### 8.2 Backup / WAL / restore drill — the limitation that must be said out loud

**CNPG's `barmanObjectStore` backup is PHYSICAL, so a database cannot be excluded from a base backup.**
That partially defeats the "backup budget goes to the log only" argument that PROP-20260807-174246 D2
rests on: the base backup takes the whole cluster whatever we intend. Two honest responses: accept it
while the read models are V0-small (11 tables of Tours data — the saving is not worth a second
cluster), and move **tracking** out to its own cluster at the moment its size makes the base backup
expensive, which the arithmetic in §9.4 puts at roughly month 3.

**The drill must grow a second leg.** Today `deploy/platform/restore-drill/` proves `app` restores.
The split makes replay the read side's *only* recovery path, and an unrehearsed replay is a hope, not a
plan. New leg: **restore `captain-write` → run projectors from checkpoint 0 → assert a known row count
in each read database.** That leg is what converts "read databases are excluded from recovery budget"
from a gamble into a decision. It is also the cheapest possible test of the claim that every fold is
deterministic — the claim the entire derived-data design rests on.

### 8.3 Connection math at peak — the split crosses the ceiling

The manifest's own budget comment: today 1 monolith pod × 5 = 5 of 220; post-cutover ~37 db-needing
bins × 5 = 185, leaving ~30 spare.

The split multiplies exactly one class of pod — **projectors need two pools** (read `captain-write`,
write their read DB):

```
  8 projectors x 2 pools x 5 =  80   (was 40)
 ~29 other db-needing bins x 5 = 145
 drill / diagnosis psql / operator headroom ~= 10
                          total ~= 235   vs   max_connections: 220
```

Over the ceiling, before anything else grows. Three ways out, and only one is safe:

| Option | Verdict |
|---|---|
| Raise `max_connections` to 300 | ❌ 300 backends on a **1 Gi** instance with 256 MB `shared_buffers` — per-backend overhead alone eats the remainder. This is how a 1 Gi pod OOMs at 20:30 |
| Drop `DATABASE_POOL_MAX_CONNECTIONS` to 4 for subgraph bins | ⚠️ buys ~29 connections; a stopgap, and it lowers headroom for the exact burst it needs to absorb |
| **PgBouncer in SESSION mode** ✅ recommended, as a **prerequisite** | The only option that survives the next growth step. **Session mode, not transaction mode** — transaction mode silently registers `LISTEN` and delivers nothing, which would break the push-driven mailbox and `event_wake`; `RUN_EVENT_PUSH`'s own configuration note in `specs/common/configuration.yaml:513` records this exact trap. Cost: one more component |

And the unglamorous reason it is a prerequisite rather than a follow-up: **37+ pods reconnecting after
a `Recreate` deploy is a connection storm**, and the split makes that storm larger before it makes
anything else better.

### 8.4 The migration chain

Today: **45 migrations**, one `sqlx migrate run --source migrations`, one `_sqlx_migrations`, one
`REQUIRED_SCHEMA_VERSION: i64 = 20260810113000` gating `/health`.

After: **five chains** (`migrations/{write,ordering,catalog,common,tracking}/`), five
`_sqlx_migrations` tables, five `DATABASE_URL`s, five `db-migrate` jobs, and **`REQUIRED_SCHEMA_VERSION`
becomes a per-database map** that `/health` reports as such.

The trap, stated because it is the one that bites: **migration order ACROSS databases is not
expressible.** A change that adds a column to a read model and a column to the log has no combined
ordering. That is fine **if and only if** the two sides are independently deployable — which is exactly
what the split is supposed to guarantee, so it is a property to *enforce*, not to assume:

> **New validator rule**: a migration file targets exactly one database, declared in its path, and the
> declaration is checked against the objects the file touches.

And a reminder from this repo's own history: [#264](https://github.com/TheCaptainCompany/captain-food/issues/264)
had to split a routine migration to fit production's disk. Five chains means five places that trap can
spring.

---

## 9. D7 — `BehaviorEventTrackingDb`

**Confirmed: it fits this model, and it fits it better than the currently-approved two-database
shape.** Under PROP-20260807-174246 D2 REVISED, behaviour tracking would have landed in `captain-views`
beside the business read models — sharing a buffer pool with the order-path reads, which is the exact
BAM-evicts-checkout coupling that revision exists to prevent. Its own database is right.

**It has zero footprint in the repo today** — no `events` table, no spec, no proposal, no projector.
That makes it fully greenfield, which is the cheapest possible moment to get four things right:

### 9.1 It is a third recovery posture, not one of the two existing ones

Neither irreplaceable (unlike the log) nor replayable (unlike the views): behaviour data lost is lost,
but its business value decays in days. Recommended posture: **excluded from PITR; daily logical dump,
30-day retention; and say plainly that a restore loses up to a day.** That is the honest posture, and
it keeps the backup budget on the log where it belongs.

### 9.2 Its projector must not read the business log

The strongest form: `tracking_projector` has **no CONNECT to `captain-write` at all**, and behaviour
tracking is fed by its own ingestion path (client beacons → its own mailbox → its own tables). If it
must derive from `domain_events`, it gets a strictly rate-limited reader role and its own checkpoint,
and it is the **first** thing paused when the write database is under pressure. A runaway analytics
backfill scanning the log at Friday peak is the named failure.

### 9.3 The BAM/analytics separation this makes real

DDIA ch. 3 (OLTP vs analytics) and ch. 12 (unbundling), applied: the tracking database is where the
long, scan-heavy, unpredictable queries live, and putting them behind a CONNECT wall means a badly
written analytics query cannot evict the pages checkout needs. That is the concrete cash value of the
whole split, and it is worth stating in one line so it does not get lost in the plumbing.

### 9.4 Growth arithmetic — because nobody does it, and this repo has the scar

| | events/day | bytes each | /day | /year |
|---|---|---|---|---|
| `domain_events` (business log) | 200 orders × ~12 | ~1.5 KB | ~3.6 MB | **~1.3 GB** |
| behaviour tracking | 3,000 sessions × ~40 | ~400 B | ~48 MB | **~17.5 GB** |

**Behaviour tracking is ~13× the business log and, unretained, consumes most of a 20 Gi volume in the
first year.** The SIRENE mirror hit 655 MB — 77% of the database — from one department before
[#231](https://github.com/TheCaptainCompany/captain-food/issues/231) reclaimed it. Behaviour tracking
is that hazard with a bigger denominator.

**Therefore, non-negotiably: a declared retention policy ships WITH the first tracking table, not
after** — a `sweep_retention`-style rule on the tracking database, and the numbers above re-checked
against reality **monthly**. Unbounded growth is a business fact before it is a storage fact; the
arithmetic is one line and nobody ever does it in advance.

---

## 10. D8 — The empty-log window: what is cheap only today

`docs/STATUS.md:833` — *"Start-clean makes the storage split FREE at cutover — the window that does not
recur."* True, and narrower than it sounds. Split honestly:

| Item | Cheap only today? | Why |
|---|---|---|
| Creating the databases and placing ~65 objects | **YES — sharply** | Today they are `CREATE TABLE`s in a fresh chain. Later it is a live data migration with the write path down |
| Splitting `projection_checkpoint` per read database | **YES** | Every row is `0` today. Later it is a coordinated cutover across N databases |
| Per-database migration chains | **YES** | Re-cutting 45 applied migrations later means reconciling five `_sqlx_migrations` histories against one applied history |
| Converting the 5 `View_*` to tables | **Mildly** | The DDL is free either way (a projection table rebuilds by replay). What the empty log saves is the **backfill** — bounded, but it has to be scheduled against peak. **This one is the least window-dependent, and it is justified on its own merits (§2.1) regardless** |
| Generated GRANTs + roles | **NO** | Grants apply to a live system; role changes are online |
| RLS on `domain_events` | **Slightly** | Window-independent in principle; enabling a policy later on a hot table is a brief lock event |
| `projection_watermark` + the erasure fix | **NO — and it must land regardless** | It is a correctness precondition, not a migration |
| The behaviour-tracking database | **No window at all** | Fully greenfield; what matters is the arithmetic and the retention policy, not the timing |

### 10.1 Sequencing that follows from that table

Final-vision-first: **no shims anywhere below.** Step 2 is not an intermediate step toward the split —
it is the final shape of those five read models, and a defect fix that stands alone.

1. **`projection_watermark` + the deletion-engine bound (fail-closed).** Correctness precondition,
   independent of everything else, worth landing even if the split is never approved.
2. **Convert the 5 `View_*` to materialized projection tables**, with their 8 declared indexes made
   real. Justified on §2.1 alone; makes the split possible as a side effect.
3. **Session-mode pooler + re-derived connection budget.** The split crosses the ceiling; this is a
   prerequisite.
4. **The split itself**, at cutover, inside the window: eleven databases (five business +
   ADP-1's six adapter databases -- five partner adapters + sirene), the placement map,
   `grants.generated.sql` + its two validator rules, per-database migration chains, the restore drill's
   replay leg.
5. **RLS on `domain_events`**, separately gated and benchmarked; default flip is its own one-line ADR.
6. **When tracking ships**: its own cluster (§8.1), its own retention, its arithmetic re-checked monthly.

---

## 11. Screen mockups

**No end-user screens.** The operator surface is the placement map and the grants:

> **Live source since [#494](https://github.com/TheCaptainCompany/captain-food/issues/494) slice 1:**
> the map below is the decision record; the LIVE, validator-enforced resolution is
> [`specs/generated/databases.generated.md`](../../specs/generated/databases.generated.md)
> (declared in `specs/database/databases.yaml` — underscored Postgres names, `k8sName` bindings,
> recovery postures, per-table placement `$ref`s).

```
captain-db  (CNPG cluster, one WAL timeline, one PITR)
├── captain-write     domain_events · domain_stream · inbound_messages · mailbox_partitions
│                     · 4x *_process_manager · slug_reservations
│                     · projection_watermark · auth_sessions
│                     · DeliveryChannelCatalog · CityDeliveryRanking · RestaurantDispatchConfig
│                     · RuntimePosture (write-side-read config -- STO-2 closure)
│                                                                  [actor_* · projector_* (SELECT)
│                                                                   · deletion_engine
│                                                                   · graphql_* -- inbound_messages
│                                                                     INSERT+SELECT ONLY, 6.1.1]
├── read_order        Cart · OrderTracking · OrderConversation · CustomerCreditBalance
│                     + ScopeMembership + replicated referentials + projection_checkpoint
│                                                    [projector_order · graphql_order (SELECT)]
├── read_catalog      Catalog + ScopeMembership + replicated referentials + projection_checkpoint
│                                                    [projector_catalog  · graphql_catalog (SELECT)
│                                          !! mailbox worker (captain_write) reads Catalog on the
│                                             WRITE path -- oversell guard + checkout repricing;
│                                             UNRESOLVED, register row STO-7]
├── read_common       Customer · Restaurant · SlugAlias · ProspectionPipeline · City
│                     · Rider read models (View_* -- placement follows their conversion)
│                     + ScopeMembership + replicated referentials + projection_checkpoint
│                                         [projector_{customer,network,delivery}
│                                          · graphql_{...} (SELECT)
│                                          · gateway tenant-host-router (Restaurant + SlugAlias,
│                                            every request's hot path, c4-l3.yaml:33-35)
│                                          !! mailbox worker (captain_write) reads Customer on the
│                                             LOGIN path, + Restaurant / ProspectionPipeline
│                                             write-side guards; UNRESOLVED, register row STO-8]
│
│   replicated into EVERY read database (recovery: replay -- STO-2(a) class):
│     ScopeMembership · PricingPolicy · UberEstimationPolicy · UberSplitPolicy
├── adapter-stripe        external_stripe_events                                  [adapter_stripe ONLY]
├── adapter-hubrise       external_hubrise_callbacks · hubrise_connections
│                         · hubrise_connection_locations                          [adapter_hubrise ONLY]
├── adapter-uber-direct   external_uber_direct_events                             [adapter_uber_direct ONLY]
├── adapter-coopcycle     external_coopcycle_events                               [adapter_coopcycle ONLY]
├── adapter-avelo37       external_avelo37_events                                 [adapter_avelo37 ONLY]
└── adapter-sirene        external_sirene_restaurants (655 MB mirror, #231)       [worker-sirene-sync ONLY]

captain-tracking  (own cluster when it ships -- 8.1/9.4)
└── BehaviorEventTrackingDb   events + its own checkpoint    [tracking_projector -- no CONNECT above]
```

**The `adapter-*` block is a DECISION, not a recommendation** (founder directive 2026-08-12,
[ADR-20260812-115930](../adr/ADR-20260812-115930-each-adapter-owns-its-own-completely-isolated-database.md),
register row **ADP-1**): each adapter's owned state — the staging mirrors and HubRise's
connection/credential tables — is completely isolated in a database of its own, reachable by that
adapter's role and nothing else; the adapter's one outward grant is the `inbound_messages` front door
(ADP-1 leg 1, **closed as (a)**: an outbox+relay would put a bidirectional platform grant INSIDE each
adapter database, and `LISTEN`/`NOTIFY` being per-database would need an inward connection to all six).

**Two corrections to this map's first version, both verified against the tree.** `adapter-avelo37` is
**in** the set — `external_avelo37_events` is declared (`integration_staging.yaml:178`) and already
retention-swept (`sweep_retention.sql:60`), so the earlier *"avelo37 owns no table today"* was false and
would have left the delivery partner as the one mirror still inside `captain-write`. And there is **no
`adapter-identity` database**: `auth_sessions` stays platform on `captain-write` (ADP-1 leg 2 closed as
(b)) — it is AES-256-GCM encrypted under `AUTH_SESSION_KEY` where `hubrise_connections.access_token` is
plaintext, no such adapter crate or bin exists, and its users are the actor path and the BFF login route,
so the database would have been named for a non-existent adapter with a non-adapter `CONNECT` list on the
sign-in path. The count is unchanged at six adapter databases; the **membership** changed.

**The map above is now a DECISION, not a recommendation** — register row **STO-2 CLOSED 2026-08-14**
([DECISIONS §32, "STO-2 closure"](DECISIONS.md#32-storage-boundaries-and-least-privilege-database-users--prop-20260811-093000)):
the 17-table remainder is declared in the spec with per-table port evidence, and the closure
**corrects this section's first version in four places** — the pricing referentials
(`PricingPolicy`/`UberEstimationPolicy`/`UberSplitPolicy`) are **replicated** into every read
database (their declared readers span three read databases: the `OrderTracking` `uber_*` fold, the
`Catalog` `uberPrice` derivation, the `Cart` read-time breakdown, the admin queries); the
dispatch-config trio (`DeliveryChannelCatalog`/`CityDeliveryRanking`/`RestaurantDispatchConfig`)
and `RuntimePosture` are **`captain_write`** (their only consumers are write-side apps; a
replay-restore would silently revert an admin-flipped posture); `CustomerCreditBalance` stays in
the order boundary's `read_order` per §31's `CustomerCredit → order`. The `ref_*` family this
section's first version replicated no longer exists (ADR-20260728-170000). The
`graphql_*` line on `captain-write` is **not** a recommendation: without it every mutation fails
(§6.1.1 (ii)).

**The `!!` lines in the tree are the part that is decided-but-not-yet-buildable, and they point the
other way to every other reader annotation here.** Each `CONNECT` list above reads *"which roles may
reach INTO this database"*; the two `!!` lines record a reach that **has no legal role yet**: the
`captain_write` mailbox worker holds four read-repository ports (`CommandDeps` in
`crates/infrastructure/src/generated/command_router.rs`) into read models this map places on the far
side of a wall — `Catalog` (the add-to-cart oversell guard and `place_order`'s repricing, both
fail-closed) and `Customer`/`Restaurant`/`ProspectionPipeline` (the login path's new-vs-returning
decision plus write-side guards). **Placement is unaffected; the physical split is BLOCKED on
register rows STO-7 and STO-8.** The generalisable lesson, and the one this section is the evidence
for: this map's reader annotations were derived from `api.yaml` resolvers plus hand-added special
cases, which cannot see a write app's port set — **an app's `CONNECT` set must be derived from its
DECLARED reads, write-path apps included** ([#513](https://github.com/TheCaptainCompany/captain-food/issues/513)'s
grant emitter), never from a tree drawn by hand.

---

## 12. Drawbacks — why we might regret the whole thing

- **It grows the pre-cutover program again**, which PROP-20260807-174246's registered
  `critical-path-growth` concern already flagged and the product owner already accepted once. Steps 1–3
  of §10.1 are real sessions before a single database is created.
- **The write database becomes the thing everything depends on.** Widening `DomainEventLogDb` into
  `captain-write` is correct, but it means the split's isolation applies to the *read* side only. An
  incident in `captain-write` is still a total outage. The split does not buy availability; it buys
  blast radius on reads and an enforceable access model. Say so rather than let it be misread.
- **Eleven databases is eleven of everything** (five business + ADP-1's six adapter databases: five
  partner adapters + the sirene mirror —
  though an adapter chain is one or three tables): migration chains, checkpoint tables, sets of
  grants, a `REQUIRED_SCHEMA_VERSION` map, a drill with more legs. Every one of those is a place a
  future session can get it subtly wrong, and the generated-grants emitter is the only counterweight.
- **Projection lag arrives on five surfaces that never had it** (§3.6). The pattern is known and
  already used on checkout, but it is five more places to apply it correctly.
- **Physical backups do not honour the logical split** (§8.2) — the neat "backup the log, replay the
  views" story is partly aspirational on CNPG until tracking gets its own cluster.
- **The placement map becomes a new kind of spec**, and a wrong placement is a migration, not an edit.
  It should be DSL (so the grant emitter can consume it), which means a new validator surface.

---

## 13. Unresolved questions

Copied to [#494](https://github.com/TheCaptainCompany/captain-food/issues/494)'s checklist on approval.

1. ~~Placement of the ~65 unnamed objects — specifically `Cart`, `OrderConversation`,
   `CustomerCreditBalance`, `SlugAlias`/`slug_reservations` (STO-2)~~ — **answered in two steps**:
   the staging/connection leg 2026-08-12 by per-adapter isolated databases (ADP-1,
   [ADR-20260812-115930](../adr/ADR-20260812-115930-each-adapter-owns-its-own-completely-isolated-database.md)),
   and the remaining 17 tables 2026-08-14 by the **STO-2 closure**
   ([DECISIONS §32](DECISIONS.md#32-storage-boundaries-and-least-privilege-database-users--prop-20260811-093000))
   — the real remainder was 17, not ~65: the `ref_*` family was already deleted
   (ADR-20260728-170000).
2. Does the placement map live in the DSL (recommended — the grant emitter needs it) or in the deploy
   layer?
3. ~~`ScopeMembership` replicated per read database (recommended) — confirm, and decide whether the
   `ref_*` enum tables follow the same rule~~ — **answered**: `ScopeMembership` is declared
   `replicated: read-databases` (#494 slice 1); the `ref_*` enum tables no longer exist
   (ADR-20260728-170000), and the STO-2 closure applies the same replicated class to the pricing
   referentials (`PricingPolicy`/`UberEstimationPolicy`/`UberSplitPolicy`), whose declared readers
   span three read databases.
4. Does the capability witness on `EventStore::append` land **before** or **with** the per-actor roles?
5. Does the restore drill's replay leg assert row counts, or a stronger property (a full read-model
   hash) that would also prove fold determinism?
6. **Confirm the fourth bullet** (§6.1.1 (i)): *"the reading of the **write** side is done by actors
   and by the projectors"* is a reading, and it must be a yes before any role is emitted from it.
7. Does the mutation-resolver mailbox grant (§6.1.1 (ii)) stay a plain `INSERT + SELECT` on
   `inbound_messages`, or does it get the RLS `WITH CHECK (actor_type = ANY(<scope's actors>))`
   narrowing from day one? (The grant is not optional — without it every mutation fails.)
8. Does `admin_ro` keep cross-database reach at all after the split (it would need CONNECT to
   everything, which is the one role that undermines the CONNECT wall — recommended: yes, but
   SELECT-only, break-glass, and time-boxed like the existing superuser practice).

---

## 14. Alternatives considered (whole-proposal level)

| Alternative | Why it lost |
|---|---|
| Keep the approved schema-per-scope-in-one-database shape (PROP-20260807-174246 D2) | It does not give the CONNECT wall (§6), which is the property the product owner's directive is actually buying; a slipped GRANT plus a wrong `search_path` re-opens the boundary silently |
| Split the event log per scope | Unchanged from D3: global ordering underpins projector checkpoints, cross-scope PM causality, the completion transaction and the erasure path. Splitting it now re-derives Kafka's hardest problems on day one of a one-city launch |
| Split only the read side; leave everything else as one database | Very close to what is recommended — and it *is* the recommendation, correctly stated: `captain-write` is one database, the read side is N. The framing difference matters only because the product owner's list named the log alone |
| Do the split and skip the `View_*` conversion, using FDW | §3 Option B: it keeps a defect and pays a network hop for it |
| Do nothing until after the first real orders | Loses the free window (§10) on the placement work, and leaves the §2.1 rider-board defect and the §5.3 erasure fail-open in production |
