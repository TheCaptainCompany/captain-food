# Open decisions — the product-owner register

**Every decision the proposals are waiting on, in one place.** Proposals hold the reasoning; this
holds the queue. If a decision is not here, it is not blocking anything.

> **The gate:** implementation does not start from a proposal whose **Status is not `Approved`**.
> The `architect` agent enforces this — an issue whose proposal has unanswered questions is classified
> 🔴 RED and never dispatched. So this page is the throttle on the whole pipeline.

Last reconciled: **2026-07-28** · 12 proposals `Proposed` · **66 open decisions** (PROP-004616's six and PROP-120931's five both closed, both proposals `Approved`; PROP-032306 added 2026-07-30 with five open of seven — §11)

> **2026-07-30 — the actor-runtime set is `Approved`** (product owner, in-session:
> *"we are at the same page, we can build it now"*; ADR-20260730-231500):
> [PROP-20260728-135632 "Aggregate state as spec"](PROP-20260728-135632-aggregate-state-as-spec.md) (D1–D5),
> [PROP-20260728-152752 "The write path becomes an actor mailbox"](PROP-20260728-152752-actor-mailbox-write-path.md) (D1–D7),
> [PROP-20260730-230803 "Projection runtime"](PROP-20260730-230803-projection-runtime-batched-partitioned.md) (D1–D3)
> — all recommended options stand. The one approved-by-default flag (`messages.yaml` as the third
> payload catalog) was **VETOED 2026-07-31** (product owner, in-session): reminder messages are
> typed INSIDE the actor with a validator-proven `receives` handler, and deferred until the first
> real use case —
> [ADR-20260731-120825](../adr/ADR-20260731-120825-actor-messages-typed-inside-the-actor.md).
> The set is now fully decided. Build starts at
> [#242](https://github.com/TheCaptainCompany/captain-food/issues/242)'s foundation slice.

---

## How to decide

Three ways, in increasing formality. All three are fine; pick per decision.

1. **Answer in this file** — put the choice in the `Decision` column with the date. Cheapest, good for
   the batch-approvable set below.
2. **Comment on the proposal's tracking issue** — better when the answer needs reasoning that future
   readers will want.
3. **Write an ADR** — required for anything cross-cutting (`docs/adr/ADR-YYYYMMDD-HHMMSS-*.md`).

Then flip the proposal's `Status` to `Approved`, naming what recorded the decision. **Do not rewrite
the proposal to match the answer** — it is a historical record of what was on the table; the decision
lives in the header, the register, and the ADR.

A proposal can be **partially approved**: mark the decided rows here and note in the header which
decisions remain. That is often the right move — several proposals have one hard question and four
easy ones.

---

## 1. Decide these first — highest leverage

Six decisions gate roughly two thirds of the backlog. Everything else can wait.

| # | Decision | Why it is first | Recommendation |
|---|---|---|---|
| **A** | [PROP-165000 D1](PROP-20260726-165000-marketplace-economics-and-money-movement.md) — **payout posture**: Stripe Connect vs merchant-of-record | Determines who the seller is, who invoices whom, how VAT is declared, and Captain's legal standing while holding customer funds. Gates [#173](https://github.com/TheCaptainCompany/captain-food/issues/173), [#172](https://github.com/TheCaptainCompany/captain-food/issues/172), [#174](https://github.com/TheCaptainCompany/captain-food/issues/174). **Gets more expensive with every real order.** | Connect, separate charges & transfers |
| **B** | [PROP-165000 D2](PROP-20260726-165000-marketplace-economics-and-money-movement.md) — **capture timing**: authorize-then-capture vs capture-at-checkout | Changes what a rejection costs the customer, what the acceptance timeout releases ([#167](https://github.com/TheCaptainCompany/captain-food/issues/167)), and how far ahead orders can be scheduled ([#197](https://github.com/TheCaptainCompany/captain-food/issues/197)) | Authorize at checkout, capture on acceptance |
| **C** | [PROP-170000 D3](PROP-20260726-170000-event-log-integrity-evolution-and-erasure.md) — **GDPR erasure strategy** | **DECIDED for ORDERS 2026-07-31** ([ADR-20260731-160000](../adr/ADR-20260731-160000-order-erasure-tombstone-then-stream-deletion.md), product owner, diverging from the crypto-shredding recommendation): `OrderExpired` = deletion from the system — projections tombstone the order's rows, a technical worker later deletes the streams, an `OrderErasureProcess` PM owns the journey. REMAINING open: customer-account-level erasure (identity, files, Supabase) + the per-phase retention windows. Gates [#194](https://github.com/TheCaptainCompany/captain-food/issues/194) | Orders: tombstone + stream deletion (decided) · account scope: open |
| **D** | [PROP-165500 D1](PROP-20260726-165500-catalog-compliance-and-merchandising.md) — **allergen representation** | EU FIC 1169/2011 is a launch blocker, and the model must exist before imports can carry it. Gates [#184](https://github.com/TheCaptainCompany/captain-food/issues/184) | Controlled 14-category enum + explicit "not declared" state |
| **E** | [PROP-164500 D1+D2](PROP-20260726-164500-order-operational-safety.md) — **acceptance timeout policy and TTL** | Decides whether a customer can be left charged for an ignored order. Gates [#167](https://github.com/TheCaptainCompany/captain-food/issues/167); pairs with B | Auto-cancel + auto-approved refund; 5 min with per-restaurant override |
| **F** | [PROP-172000 D1](PROP-20260726-172000-spec-to-ui-contract-integrity.md) — **how a screen declares a runtime input source** | The one DSL addition needed before the write-side validator gate can fail closed. Gates the required-field half of [#169](https://github.com/TheCaptainCompany/captain-food/issues/169) and the fix for [#168](https://github.com/TheCaptainCompany/captain-food/issues/168) | Name the input source explicitly (`from:`) |

---

## 2. Batch-approvable — recommendation is the standard answer

These have a conventional right answer and little genuine trade-off. Reading the recommendation and
saying "yes to all" is a reasonable use of five minutes.

| Decision | Question | Recommendation |
|---|---|---|
| PROP-170000 D1 | Preventing skipped events ([#189](https://github.com/TheCaptainCompany/captain-food/issues/189)) | Snapshot / `xmin` guard — the only option that is correct rather than probabilistic |
| PROP-170000 D2 | Event evolution policy | Additive-only + validator gate; add `event_version` now (cheaper before the log grows) |
| PROP-170000 D4 | `$maxAge` / `expired_at` | Implement or delete — a specified-but-inert control is worse than none |
| PROP-170000 D5 | Spec-vs-code divergences (`version` 0- vs 1-based, `id` as idempotency key) | Correct the spec to match the code; the code is what has been running |
| PROP-170500 D3 | Where the workers run | Advisory lock now (in-process), dedicated service later |
| PROP-170500 D4 | GraphiQL / Voyager in production | Keep, gated to ADMIN |
| PROP-170500 D5 | Subscription fan-out at >1 instance | Postgres `LISTEN`/`NOTIFY` |
| PROP-171500 D1 | Where the write-side scope check runs | Dispatch layer, before journaling |
| PROP-171500 D2 | Validate the supplied id, or derive it | Derive where the role implies one scope; validate otherwise |
| PROP-171500 D3 | Sequencing against [#144](https://github.com/TheCaptainCompany/captain-food/issues/144) | Immediately after it lands |
| PROP-172000 D3 | The drifted product spec | Rewrite §4–§5 to match ADR-0034 |
| PROP-172000 D4 | Fix the four dead actions with the rule | Same PR — a rule landing red breaks "keep main green" |
| PROP-172500 D4 | Job-pool filtering | Filter by city, zone and `RiderStatus` — composes with [PROP-20260808-141817](PROP-20260808-141817-rider-delivery-write-surface.md) slice 4's per-rider decline exclusion on the same `myDeliveries` query (the rider write surface itself moved to that proposal, §20) |
| PROP-172500 D5 | Rider↔customer contact | Route through the order conversation, not phone numbers |
| PROP-165500 D6 | Menu scheduling | Defer, but record it — needed when combos land |

---

## 3. Genuine trade-offs — worth your time

| Decision | Question | Recommendation | The tension |
|---|---|---|---|
| PROP-165000 D3 | Rounding for fee splits | Buyer total first, residual cent to `captainNet` | Undefined today; any answer works, but it must be stated and tested or splits stop reconciling |
| PROP-165000 D4 | Delivery-fee dimension | Per-zone | Pairs with PROP-172500 D1; distance-banded is fairer but needs geocoding you do not have |
| PROP-165000 D5 | Do tips move money? | Yes, same transfer mechanism as D1 | Tips are recorded and displayed today but reach nobody |
| PROP-165500 D2 | Does Captain own stock consumption? | Re-validate at checkout; decrement only Captain-managed offers | HubRise restaurants have a POS as stock authority — double-counting is worse than not counting |
| PROP-165500 D3 | Per-service-type pricing | Optional price override on `Offer` | French practice prices delivery above counter; the model allows per-mode VAT but not per-mode price |
| PROP-165500 D4 | Catalog images on the [#134](https://github.com/TheCaptainCompany/captain-food/issues/134) framework | Confirm a **public** audience now, while it is on paper | #134 is designed around private per-order attachments; retrofitting public access later is the expensive version |
| PROP-165500 D5 | Merchandising order | Promo codes first | Highest acquisition value for a single-city launch; loyalty must reuse [#158](https://github.com/TheCaptainCompany/captain-food/issues/158)'s balance, not a second one |
| PROP-164500 D3 | V0 notification channel | In-app + sound, then SMS | Waiting for [#127](https://github.com/TheCaptainCompany/captain-food/issues/127)'s full cascade blocks the entire operational loop behind a post-V0 epic |
| PROP-164500 D4/D5 | Timed pause; opening-hours exception days | Yes to both | Weekly recurrence alone is wrong on all eleven French public holidays |
| PROP-164500 D6/D7 | Scheduling window; order modification scope | Same-day slots; address correction before `PREPARING` | Bounded by B — card authorizations expire in ~7 days |
| PROP-171500 D4 | ADMIN acting on behalf of a tenant | Explicit, logged bypass | Revisits ADR-0037's impersonation-only stance |
| PROP-172000 D2 | Rejection reasons: enum or free text | Controlled enum + optional note | Rejection reasons are the analytics that tell you which restaurants to coach |
| PROP-172500 D1 | Delivery-area model | Postal-code sets now, geocoding next | Geocoding unlocks distance fees and honest ETAs — sequence it deliberately |
| PROP-172500 D2 | Proof of delivery | Handover photo over [#134](https://github.com/TheCaptainCompany/captain-food/issues/134) | `NOT_DELIVERED` claims are unadjudicable today, and [#151](https://github.com/TheCaptainCompany/captain-food/issues/151) is already routing them |
| PROP-172500 D3 | Reclaiming an abandoned run | Rider release + stall sweep | A stalled `PICKED_UP` job means the food is with the rider — re-offering is wrong, it needs re-cooking. **Dependency**: the sweep's release must emit the SAME event as the manual release, so this now depends on [PROP-20260808-141817 D3](PROP-20260808-141817-rider-delivery-write-surface.md)'s naming decision (§20) — never a twin event |

---

## 4. Inherited — still `Proposed`, decisions still open

| Proposal | Decisions | Note |
|---|---|---|
| [PROP-20260725-185140](PROP-20260725-185140-read-side-per-instance-authorization.md) read-side authz | **D1–D11** | [#144](https://github.com/TheCaptainCompany/captain-food/issues/144) carries `status/in-progress`, so these are being answered in practice by the build. Worth reconciling the register with what was actually chosen when it lands |
| [PROP-20260725-120055](PROP-20260725-120055-generic-file-attachment-framework.md) file framework | **D1–D5** (D2b decided) | Blocks [#134](https://github.com/TheCaptainCompany/captain-food/issues/134); D2 (GDPR retention windows) is the substantive one |
| [PROP-20260724-133700](PROP-20260724-133700-runtime-screen-and-translation-delivery.md) · [PROP-20260724-144500](PROP-20260724-144500-admin-flag-translation-keys.md) | — | `Proposed` with no formal decision section; confirm whether they are still live or superseded |

---

## 5. Decided

| Date | Decision | Answer | Recorded in |
|---|---|---|---|
| 2026-08-02 | **PROP-20260802-130500 D1–D6** — isolation by construction | **All six answered** (D1 via PROP-20260728-152752 D9). D2 **(a) handler crates per actor** — aggregates AND process managers, domain value types stay one crate · D3 **cargo-deny capability allowlist in phase 1** (who may hold `sqlx`/`reqwest`) · D4 **one generic `ActorClient` with `get_operation_status(message_id)`** — operation status is generic to all operations, so neither a per-actor client method nor a separate `OperationStatusClient` type; per-actor typed clients stay write-side · D5 **`test-fixtures` feature + CI check** · D6 **later, separately — against the recommendation** (own change after phase 1). Scope directive: "per actor" includes the two process managers at every phase. | Product owner, this register (§14) + [PROP-20260802-130500](PROP-20260802-130500-isolation-by-construction.md), realized by [#290 "Actor-client crate isolation (PROP-20260728-152752 D9): compiler-enforced door, then per-actor crates"](https://github.com/TheCaptainCompany/captain-food/issues/290) |
| 2026-07-29 | **PROP-170500 D1 + D2** — telemetry backend and sampling | **D1 answered: Honeycomb**, over OTLP/HTTP, pinned to the **EU (`eu1`)** region — a GDPR constraint, not a default, since spans carry `customerId`/`orderId` and ADR-0042 pinned data to Frankfurt. `HONEYCOMB_API_KEY` supplied as a repo Actions secret and pushed to Render by CI. Telemetry **degrades, never gates**: no telemetry key is `required:`, so a missing ingest key drops the exporter and keeps structured logs rather than refusing to serve orders. **D2 answered but NARROWED — against the recommendation**: parent-based HEAD sampling at `1.0` (keep everything), not tail-based. Tail sampling needs Refinery, i.e. a service to run and pay for, which contradicts ADR-0042's minimal-ops-pre-PMF posture — and D2's own justification says the volume is not there yet. Revisit when ingest cost is measurable. | Product owner + [ADR-20260729-183000](../adr/ADR-20260729-183000-telemetry-is-honeycomb-eu-and-degrades-never-gates.md), realizing [#191](https://github.com/TheCaptainCompany/captain-food/issues/191) |
| 2026-07-28 | **PROP-004616 D1–D6** — slug lifecycle + SIRENE inbound events | **All six answered.** D1 `RestaurantSlugConfigured` + `RestaurantSlugReconfigured` (in session) · D2 slug chosen **between claim and activation**, gated by "no activation without a configured slug" · D3 **write-side reservation table** with a real `UNIQUE` (also holds released slugs) · D4 the ACL stages **`RestaurantRegistered` only** — *against the recommendation*, and stricter: no registry-fact event, no ACL branching, the **aggregate** decides record/ignore/update · D5 **null the slug on `NON_PARTNER` rows** · D6 **both** `IGNORED` and `DUPLICATE`. Partially supersedes ADR-0045. | Product owner, this register + [ADR-20260728-011344](../adr/ADR-20260728-011344-slug-lifecycle-and-sirene-inbound-events.md) |
| 2026-07-26 | **PROP-193000 D1–D4** — continuous development loop | **Deferred.** The daily architecture-review routine is sufficient for now; the dev loop stays off until the proposals are under control. `dev-loop.yml` remains `workflow_dispatch`-only with `dry_run` defaulting true. | Product owner, this register |

## 6. Newest — the daily cycle itself

[PROP-20260726-201500](PROP-20260726-201500-daily-decision-cycle.md) ([#211](https://github.com/TheCaptainCompany/captain-food/issues/211))
proposes closing the loop: audit → ask → record → implement, daily. Its **D5 is the one that decides
whether the cycle works at all** — whether DSL diffs join the daily approval ritual. Verified finding
behind it: all six §1 decisions above unblock work that needs `specs/**` changes, so answering them
moves items from 🔴 RED to 🟠 AMBER, **not** to 🟢 GREEN. Without D5 the cycle would ask the important
questions and still be unable to act on the answers.

| # | Decision | Recommendation |
|---|---|---|
| D1 | Where the ask lands | One standing pinned GitHub issue, rewritten daily |
| D2 | What counts as an answer | Free prose, re-stated by the next run before use (24h interpretation window) |
| D3 | How many questions per day | Up to 3, plus one batch block |
| D4 | When nothing is answered for days | Keep implementing GREEN; escalate the ask with its age |
| **D5** | **Do DSL diffs join the ritual?** | **Yes — it is what makes the cycle reach the work, and it keeps a human approving every DSL change** |
| — | Start with a week of ask-only? | Recommended |

---

## 7. Slug lifecycle + SIRENE inbound events — ✅ DECIDED 2026-07-28

[PROP-20260728-004616](PROP-20260728-004616-slug-lifecycle-and-sirene-inbound-events.md)
([#220](https://github.com/TheCaptainCompany/captain-food/issues/220)) came out of a Supabase disk-IO
alert that turned out to be a symptom. Three defects underneath it: a **failed INSERT is the
idempotency mechanism** for six creation handlers, **INSEE updates are silently dropped** (no
`UpdateRestaurant` exists in the SIRENE worker at all), and the write path resolves identity through an
**unindexed JSONB scan of the read model** once per staged SIRET. All three trace to deriving the slug
at seeding time.

**✅ CLOSED 2026-07-28 — all six answered, proposal `Approved`, implementation under way.** Kept here for
the audit trail; the answers are in §5 and in
[ADR-20260728-011344](../adr/ADR-20260728-011344-slug-lifecycle-and-sirene-inbound-events.md).

| # | Decision | Recommendation | **Answer** |
|---|---|---|---|
| D1 | Naming of the rename event | `Configured` + `Reconfigured` | ✅ as recommended (in session) |
| D2 | When is the slug chosen? | Between claim and activation, gated by "no activation without a configured slug" | ✅ as recommended |
| D3 | How slug uniqueness is enforced on the write side | Write-side reservation table with a real `UNIQUE` | ✅ as recommended |
| D4 | What the ACL stages as the inbound event | A registry fact (`RestaurantObservedInRegistry`) plus a policy | ⚠️ **against the recommendation** — `RestaurantRegistered` **only**, unconditionally, with the **aggregate** deciding record/ignore/update. Stricter than either option offered: no new event, no ACL branching, no domain decision in the adapter |
| D5 | Migration of the ~200k existing derived slugs | Null them on `NON_PARTNER` rows | ✅ as recommended |
| D6 | Is the `IGNORED` / `DUPLICATE` split worth two statuses? | Both | ✅ as recommended |

**Sequencing (binding):** the slug change lands *before* the SIRENE update path, or an INSEE rename
becomes a live-storefront rename. SIRENE stays paused at both halves until the whole chain is complete.

Reverses part of **ADR-0045** (SIRENE → `RegisterRestaurant` via the command path), so the realizing
change needs an ADR. The `ImportCatalog`-stays-a-command contrast in CLAUDE.md survives intact: the test
is whether the originator can be told no.

---

## 8. SIRENE mirror storage — ✅ DECIDED 2026-07-28

[PROP-20260728-120931](PROP-20260728-120931-sirene-mirror-payload-is-transient.md)
([#231](https://github.com/TheCaptainCompany/captain-food/issues/231)). Measured on production
2026-07-28: `external_sirene_restaurants` is **655 MB — 77% of the database** — at department 37 of 101,
on a **2 GB disk with ~580 MB free** (Free plan, already flagged *exceeding usage limits*). Full France
is ~2 GB for that one table. [#218](https://github.com/TheCaptainCompany/captain-food/issues/218) paced
the sweep correctly, but pacing does not create disk, so this is what actually gates national coverage.

The proposal: the payload is an input to translation with a **lifetime**, the hash is the
change-detection key that persists. Keep the payload only while a row is pending, drop it once
translated. ~1.8 kB/row → ~200 B/row; ~655 MB → ~90 MB today, ~250 MB at full France.

**D5 is the one that needs real thought** — everything else has a clear answer. It asks whether losing
replay/backfill from the mirror is acceptable, given INSEE is the system of record and (since #218) a
full re-fetch is a normal paced operation rather than a special one.

**✅ CLOSED 2026-07-28 — all five answered, proposal `Approved`, implemented in PR #234.** Kept here for
the audit trail; the reasoning is in
[ADR-20260728-143000](../adr/ADR-20260728-143000-sirene-mirror-payload-is-transient.md).

| # | Decision | Recommendation | **Answer** |
|---|---|---|---|
| D1 | What the mirror retains | Payload transient (NULL after successful processing), hash permanent | ✅ as recommended |
| D2 | Hash algorithm + encoding | Keep SHA-256, store as `bytea` not hex text. Keep the column named `payload_hash`, never `payload_md5` — naming a column after an algorithm pins the schema to it | ✅ as recommended, **sequenced after compaction**: `ALTER … TYPE` rewrites the whole table and needs ~655 MB free against ~580 MB. Cheap once live data is ~90 MB |
| D3 | Unmappable / failed rows | KEEP their payload — it is the only evidence of why the record was unusable | ✅ as recommended, with a limit: the CI compaction has no ACL, so **historical** ACL-unmappable payloads are dropped. Holds going forward via the worker |
| D4 | Migration on ~580 MB free | Batched `UPDATE … SET payload = NULL` with `VACUUM` interleaved; a single whole-table UPDATE would likely hit `No space left on device` again | ✅ as recommended |
| **D5** | **Replay/backfill posture** | **Accept re-fetch from INSEE when a new field is needed** — the mirror is a cache, INSEE is the system of record | ✅ accepted, and **cheaper than the proposal argued**: the hash covers the TYPED projection, so adding a field to the wire types invalidates every digest and the next ordinary paced sweep re-translates the whole mirror by itself. The backfill is not an operation to build |
| — | Where the compaction runs | Server-side (it has the ACL, so D3 holds for historical rows too) | ⚠️ **against the recommendation** — the **CI `sirene_ingest` job**. Cost recorded under D3 |

---

## 9. Configuration is declared and validated at startup — PROP-20260729-004500

Tracking issue: [#246 "Declare the app's configuration in specs/, validate it at startup, and refuse to boot when a required key is missing"](https://github.com/TheCaptainCompany/captain-food/issues/246).
Product-owner directive of 2026-07-29 (the *what* is decided; these are the *how* questions it raises).

Context in one line: configuration is the only part of the system with no source of truth — which is
why `RUN_SIRENE_WORKER` had no home, `API_SECRET` is configured and read by nothing, and a missing
`STRIPE_WEBHOOK_SECRET` would silently produce the worst failure mode in the product.

| Decision | Question | Recommendation |
|---|---|---|
| PROP-004500 D1 | Required-ness model | **Per-profile** (`required: [production, staging]`) — production cannot boot misconfigured, dev/CI still start on a partial secret set |
| PROP-004500 D2 | Which currently-degrading keys become HARD requirements in production | `STRIPE_WEBHOOK_SECRET`, `AUTH_SESSION_KEY`, `DATABASE_URL`, `SUPABASE_URL`/`SUPABASE_PUBLISHABLE_KEY`, `STRIPE_SECRET_KEY`. **Behavioural change** — see D5 for sequencing |
| PROP-004500 D3 | Where the profile comes from | An explicit `APP_PROFILE` key defaulting to `development` — not inferred from the host or from key prefixes |
| PROP-004500 D4 | A `/config` readiness endpoint | Yes, **presence-only** (never values) — the same one-curl answer `/sirene` just proved worth having |
| PROP-004500 D5 | Rollout sequencing | **Warn-only for one deploy, then enforce** — the first deploy reports what production is missing without taking anything down |

---

## 10. CI owns the Render service configuration — PROP-20260729-014500

Tracking issue: [#248 "CI owns the Render service configuration: sync specs/configuration.yaml + repo secrets to the service, never the dashboard"](https://github.com/TheCaptainCompany/captain-food/issues/248).
Product-owner directive of 2026-07-29 (*"the settings must be done by the CI itself not by my manual
configuration on render"*) — the *what* is decided; these are the *how*.

[#246](https://github.com/TheCaptainCompany/captain-food/issues/246) gave configuration a declaration;
this gives it an owner. Until it lands, `RUN_SIRENE_WORKER=true` remains a dashboard field nobody can
see from the repo — and the production boot log confirms it was never set, which is why 6,649
department-37 rows are still `PENDING`.

| Decision | Question | Recommendation |
|---|---|---|
| PROP-014500 D1 | Write mode | **Upsert only** to start (never deletes); revisit replace-all once the drift report has been empty for several deploys |
| PROP-014500 D2 | Dry-run first? | **Yes** — the workflow cannot be tested outside CI (no local `RENDER_API_KEY`), so its first real run would otherwise rewrite production config untested |
| PROP-014500 D3 | Secret bootstrap ordering | **Non-secrets first** — unblocks `RUN_SIRENE_WORKER` immediately with zero secret-handling risk |
| PROP-014500 D4 | `RENDER_API_KEY` as a write credential | **Reuse the existing account key** — the exposure already exists; Render issues no narrower token |
| PROP-014500 D5 | Where non-secret values live: **baked into the image** or service env? | **Hybrid — bake non-secrets, sync secrets.** Render has no per-deploy env override (the deploy API takes only `clearCache`/`commitId`/`imageUrl`/`deployMode`), so baking is the only way to attach config to the artifact. It makes the digest determine behaviour, so a rollback restores the config that shipped with that build. Secrets can never be baked — the GHCR image is public |

---

## 11. Uber Eats Marketplace + per-surface Uber Direct credentials — PROP-20260730-032306

Tracking issue [#260 "Epic: Uber Eats Marketplace integration (order centralization + menu sync) and per-surface Uber Direct credentials"](https://github.com/TheCaptainCompany/captain-food/issues/260).
**Partially approved**: D2 and D6 were decided in the 2026-07-30 session and are recorded by
[ADR-20260730-032306](../adr/ADR-20260730-032306-uber-integration-topology-two-orgs-and-asymmetric-app-auth.md).
Three remain, and **D7 is not an engineering decision** — it needs whoever advises on company/tax law.

| Decision | Question | Recommendation |
|---|---|---|
| PROP-032306 D1 | Build the Eats integration directly, or layer on HubRise (which already syncs menus to Uber Eats and Deliveroo)? | **Direct** — effectively chosen by registering the app. Reaches restaurants with no POS at all, which is the segment Captain targets; and allergen relay is contractually ours whether or not we own the pipe |
| PROP-032306 D2 | Which Uber org is billed for a Direct dispatch? | ✅ **DECIDED 2026-07-30: two orgs, split by acquisition surface, storefront first.** (C — one org plus internal attribution — was recommended; A was chosen) |
| PROP-032306 D3 | Where does the acquisition surface live? | **A field on `OrderPlaced`.** Not derivable at dispatch: acceptance-first (ADR-20260720-015500) means the saga runs long after the `Host` header is gone |
| **PROP-032306 D4** | How is a marketplace-originated order represented, given it carries **no Captain PaymentIntent**? | **A distinct `ExternalOrderReceived` event.** Making the payment fields nullable on `OrderPlaced` would weaken a money invariant for every order to accommodate a minority. **Pairs with §1 A/B (payout posture, capture timing) — decide together** |
| **PROP-032306 D5** | Menu ownership across Captain / HubRise / Uber, and per-channel price parity | **HubRise authoritative when connected, else Captain**, one-way push. Parity is the sharp edge: restaurants mark Uber prices up to absorb Uber's commission, and ADR-0024's comparison coefficients are calibrated on that — pushing Captain prices unchanged undercuts the restaurant *and* invalidates `basis: REAL` |
| **PROP-032306 D7** | Is the Provider entity on the signed Uber agreement (**Caring Hope Foundation**, RNA W372020229 — a loi-1901 association) the entity that will operate the platform? | **Needs legal input, not a recommendation.** An Uber API licence follows the entity; if the association holds it while another entity operates and earns commission, access sits outside the licence. Also interacts with the payout posture in §1 A |


---

## 12. The batched send's signature — ✅ DECIDED 2026-08-02 (deferred)

[PROP-20260728-152752](PROP-20260728-152752-actor-mailbox-write-path.md) is `Approved`, but the
product owner raised ONE new decision while reaffirming §2.1 (typed actor clients — realization
tracked by [#284 "Typed actor clients (PROP-20260728-152752 §2.1): one generated client per actor"](https://github.com/TheCaptainCompany/captain-food/issues/284)):
what the TYPED form of the batched send looks like. The interim untyped
`enqueue_inbound_facts` ([#283 "batch the SIRENE drain"](https://github.com/TheCaptainCompany/captain-food/pull/283))
fixed a 6x producer bottleneck and must be absorbed, not kept. Blocks only `send_many` — the
singular typed client can land first.

| # | Decision | Recommendation | **Answer** |
|---|---|---|---|
| **D8** | `send_many` signature + compile-time checks | (a) Homogeneous generic batch | ⚠️ **Deferred — no `send_many` for now** (product owner, 2026-08-02). Build the client as §2.1 always specified it first — per-actor, `send` + `schedule`, compile-time checked — then discuss parallelisation separately. The #283 batching stays infrastructure-internal (`enqueue_inbound_facts`), outside the client's public surface, until that discussion |


---

## 13. Client isolation by crate — ✅ DECIDED 2026-08-02

[PROP-20260728-152752 D9](PROP-20260728-152752-actor-mailbox-write-path.md): make the typed-client
door COMPILER-enforced. Product owner: **Option B** — a dedicated `actor-client` crate between
`application` and `infrastructure` (private-field `MailboxEntry` + constructors + generated clients
in one crate, so bypassing the client does not compile), with **per-actor crates as the target
topology** ("improve the isolation with crates everywhere" — the C# assembly-per-client /
assembly-per-actor practice, crate as the boundary). Phased: one client crate first (the payoff),
per-actor client crates second, per-actor implementation crates gated separately. Tracked by
[#290 "Actor-client crate isolation (PROP-20260728-152752 D9): compiler-enforced door, then per-actor crates"](https://github.com/TheCaptainCompany/captain-food/issues/290).


---

## 14. Isolation by construction — PROP-20260802-130500 — ✅ DECIDED 2026-08-02

[PROP-20260802-130500](PROP-20260802-130500-isolation-by-construction.md)
([#290](https://github.com/TheCaptainCompany/captain-food/issues/290)). The product owner's threat
model, first-class: most code here is written by AI sessions, and a rule an agent can violate
silently is a review burden forever — so buy compile-time enforcement wherever it is for sale.
Measured finding behind it: the typed-client door is level-4 enforced while **nine crates hold
`sqlx`** and can bypass every door with one query. Scope (product-owner directive, 2026-08-02):
**"per actor" includes the process managers** — one crate per PM and one per PM client at every
phase, symmetric with aggregates (16 actors = 14 aggregates + `PlaceOrderProcess` +
`RefundProcess`). All six decisions answered:

| # | Decision | Answer |
|---|---|---|
| D1 | The client door becomes a crate | Dedicated `actor-client` crate (decided as PROP-20260728-152752 D9) |
| D2 | Per-actor IMPLEMENTATION crates (phase-3 endpoint) | (a) handler crates per actor — aggregates AND process managers; domain types stay one crate — ✅ as recommended |
| D3 | `Cargo.toml` as capability allowlist (cargo-deny: who may hold `sqlx`/`reqwest`) | Adopt in phase 1 — ✅ as recommended |
| D4 | The read door | One generic `ActorClient` with `get_operation_status(message_id)` — status is generic to all operations, so neither a per-actor client method nor a separate `OperationStatusClient` type; in the client crate, phase 1 |
| D5 | Cross-crate test fixtures | `test-fixtures` cargo feature + CI check that no release artifact enables it — ✅ as recommended |
| D6 | Lint floor | **Later, separately — against the recommendation**: lands as its own change after phase 1, tracked on #290's checklist |

---

## 15. Push-driven mailbox — PROP-20260802-223522 — ✅ DECIDED 2026-08-02

[PROP-20260802-223522](PROP-20260802-223522-push-driven-mailbox.md)
([#313 "Push-driven mailbox: pg_notify on inbound_messages, idle lane gate, poison policy (PROP-20260802-223522)"](https://github.com/TheCaptainCompany/captain-food/issues/313)).
Extends [#301](https://github.com/TheCaptainCompany/captain-food/pull/301)'s NOTIFY approach to the
last polling surface: the actor mailbox (post-audit: it out-polled what #301 removed, ~8×; interim
width-5 mitigation merged as ADR-20260802-220402). Also closes the up-to-10 s adapter→worker wake
gap on the money path, and bounds the silent infinite-retry poison mode found 2026-08-02.
**Approved as recommended, D1–D5** (product owner, in-session, 2026-08-02; ADR-20260802-224532);
unresolved questions live on the tracking issue's checklist.

| # | Decision | Recommended | Answer |
|---|---|---|---|
| D1 | Wake transport | `pg_notify` in the enqueue transaction (one door: `PgMailbox`) | ✅ as recommended (2026-08-02) |
| D2 | Channel topology | One channel, payload = `actor_type` (per-type coalescing) | ✅ as recommended (2026-08-02) |
| D3 | Idle gate | One lanes-with-work query per pass (partial index exists) | ✅ as recommended (2026-08-02) |
| D4 | Poison policy | `attempts` cap (default 5) → terminal `FAILED` + error on the row | ✅ as recommended (2026-08-02) |
| D5 | Gating | Own toggle (worker-toggle pattern) + `MAILBOX_MAX_DELIVERY_ATTEMPTS` (0 = today) | ✅ as recommended (2026-08-02) |

Unresolved questions — **all four decided 2026-08-03**
([ADR-20260803-002712](../adr/20260803-002712-mailbox-poison-follow-ups-decided.md)): admin
requeue mutation ([#315](https://github.com/TheCaptainCompany/captain-food/issues/315)) ·
exponential backoff ([#316](https://github.com/TheCaptainCompany/captain-food/issues/316)) ·
page on EVERY poison ([#317](https://github.com/TheCaptainCompany/captain-food/issues/317)) ·
fleets default-off until DB-persisted posture ([#318](https://github.com/TheCaptainCompany/captain-food/issues/318)).

---

## 16. Who owns the OVH host — PROP-20260805-181926 — ⚠️ MOSTLY MOOT 2026-08-06

> **The destination changed to Clever Cloud** ([ADR-20260806-151122](../adr/ADR-20260806-151122-hosting-destination-is-clever-cloud-not-ovh.md),
> product owner: *"Instead of OVH"*). A PaaS means **no host OS of ours**, so **D1–D6 below have no
> subject** — they stay as the costed record of the option space, not as decisions anyone owes an
> answer to. **Only D7 is still open**, in reduced form. **D3 (SaltStack) is settled by construction**:
> there is no machine for it to configure. The one live question moved to the ADR's follow-up —
> **whether Clever Cloud meters egress the way Render did**, which gates any spend, because egress
> exhaustion is one of the incidents that started this migration.

[PROP-20260805-181926](PROP-20260805-181926-host-provisioning-and-configuration-ownership.md)
([#349 "Who owns the OVH host: provisioning IaC + host configuration (SaltStack evaluated)"](https://github.com/TheCaptainCompany/captain-food/issues/349)).
Raised by the product owner as *"SaltStack seems to be an interesting solution"*. It is live because
the OVH cutover ([#271](https://github.com/TheCaptainCompany/captain-food/issues/271),
ADR-20260731-061609) gives us a **host OS of our own for the first time** — on Render nothing about
the machine was ours. Today no file says which OVH resources exist or what is installed on the box,
which is the Render-dashboard failure mode (`RUN_SIRENE_WORKER` set in no file, `API_SECRET` read by
no code) one layer deeper. The proposal splits the question into **provisioning** (what resources
exist — Salt does not address this at all) and **host configuration** (what runs on the box), and
notes that **application** configuration is already owned by `specs/configuration.yaml` and must stay
that way.

| # | Decision | Recommended | Answer |
|---|---|---|---|
| D1 | Layer A — provisioning | OpenTofu + the official `ovh/ovh` provider — the instance, network, firewall, managed PG plan and DNS become reviewed files | _(open)_ |
| D2 | Layer B — host configuration | cloud-init `user_data` from the repo (~80 lines, no agent, no daemon); **Ansible named as the escape hatch** at 3+ hosts. NixOS deferred on **bootstrap risk** (OVH has no first-class NixOS image) and D7 — no longer on authoring cost | _(open)_ |
| D3 | **SaltStack: adopt or reject** | **Reject** — its advantage needs ~1,000 nodes and we have one, it adds a listening root-equivalent control plane to the box terminating payment traffic, its pillars become a second config store, its convergence model contradicts the immutable-artifact doctrine, and its stewardship is consolidating into Broadcom's VMware suite. Revisit only for restaurant-side hardware fleets | _(open)_ |
| D4 | Host posture | Disposable — rebuild, never converge (affordable only because the managed PG is a separate resource, PROP-20260731-061609 D2) | _(open)_ |
| D5 | OpenTofu state | OVH Object Storage S3 backend + committed `.terraform.lock.hcl`; **never** the repo (public — it would leak the PG credential) | _(open)_ |
| D6 | Sequencing | cloud-init now, cut over, **then** `tofu import` the live resources — IaC must not block restoring production | _(open)_ |
| D7 | **Is host config generated from the DSL?** (product owner, 2026-08-05: *"based on the spec in YAML you can generate it… encapsulated in the codegen"*) | **Derive from the specs that ALREADY exist** — compose file, firewall ports and collector config from `configuration.yaml` / `observability.yaml` / `services.yaml` / C4 — **not** a new `specs/host.yaml`, which would be a single-target passthrough with none of the fan-out that earns the repo's other emitters. Target-independent, so it works for cloud-init now and NixOS later | _(open)_ |

Concern registered and unchecked, so this cannot be approved as-is: **cutover-not-blocked** — prod is
down today and nothing here may delay [#271](https://github.com/TheCaptainCompany/captain-food/issues/271).
D6 is the mechanism; checking the concern means confirming that ordering.

---

## 17. Kubernetes as the deployment substrate — PROP-20260806-223656 — ✅ DECIDED 2026-08-07

> **Fully approved** (product owner, D1–D7 across 2026-08-06/07, closed with *"D3 and D5 yes, start
> clean, move the NS to OVH"*), recorded by
> [ADR-20260807-002705](../adr/ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md): **OVH MKS +
> in-cluster CNPG + GitOps-only operations + generated manifests + `Recreate` until #242 + straight
> to the cluster, starting from an EMPTY schema (no dump restore) + NS hosting → OVH DNS (Dynadot
> stays registrar)**. All four concerns checked. The rows below record the option space as decided.

[PROP-20260806-223656](PROP-20260806-223656-kubernetes-as-the-deployment-substrate.md)
([#271](https://github.com/TheCaptainCompany/captain-food/issues/271)). Reopens
[ADR-20260806-151122](../adr/ADR-20260806-151122-hosting-destination-is-clever-cloud-not-ovh.md)
(Clever Cloud), which is **no longer in force**, at the product owner's direction.

**Why**: that ADR's decisive argument was *"a team of one product owner plus agents should not be
operating a PostgreSQL server"* — a premise about the OPERATOR that was **wrong**. The product owner
has run Kubernetes professionally, so the heaviest weight in the decision was mis-specified. Three
further arguments were raised and none appeared in the ADR: **ingress as a light API gateway**
(wildcard TLS is needed on every destination anyway), **lock-in** (previously dismissed as "a
Dockerfile and env vars", which under-weighted Tasks/Cellar/add-ons compounding), and **manifests as a
codegen target** — a cluster can consume generated deployment descriptors, a PaaS cannot, which makes
this the best available home for PROP-20260805-181926's surviving D7.

| # | Decision | Recommended | Answer |
|---|---|---|---|
| D1 | Kubernetes, or the PaaS decided yesterday? | **OVH MKS** if k8s (free control plane, **free egress**, GA — vs CKE still in public beta); Clever Cloud PaaS retained as the costed fallback | ✅ **OVH MKS** (product owner, 2026-08-07: *"MKS of course"*) |
| D2 | **Where does PostgreSQL live?** — the hard one | Managed alongside the cluster was recommended; the option table also carries a vRack-instance shape and in-cluster CNPG | ✅ **In-cluster CNPG** (product owner, 2026-08-06: *"Postgres on Kubernetes"*) — with the operability conditions as part of the answer: ≥3 nodes, required anti-affinity, WAL archiving to object storage, scheduled executed restore drills |
| D3 | Deploy strategy while [#193](https://github.com/TheCaptainCompany/captain-food/issues/193) caps us at one instance | **`Recreate`** — a RollingUpdate runs two write paths at once, exactly what [#242](https://github.com/TheCaptainCompany/captain-food/issues/242)'s leases and fencing exist to prevent | ✅ As recommended (2026-08-07) |
| D4 | Ingress + wildcard TLS | ingress-nginx + cert-manager, DNS-01 for `*.captain.food` | ✅ **As recommended** (product owner, 2026-08-07: *"Ingress yes!"*) — with a zone-host correction: **DNS is at DYNADOT, which has NO cert-manager solver**. Sub-decision open: move zone hosting to OVH DNS (NS change only, Dynadot stays registrar — recommended), CNAME-delegate just the ACME challenge, or write a custom webhook |
| D5 | Manifests generated from the specs? | **Yes** — the strongest argument for a cluster, and PROP-20260805-181926 D7 with a target that fits | ✅ As recommended (2026-08-07) |
| D6 | Sequencing, with prod DOWN | Restore service on the simplest path first, build the cluster deliberately after — the digest-pinned image runs unchanged on either, so it is a redeploy, not a second migration | ✅ **Build the cluster now, cut over once** — AGAINST the recommendation (product owner, 2026-08-07: *"I don't care about prod on Render and Supabase, it was a crash test"*). Opens the data question: restore the dump into CNPG, or start clean? |
| D7 | How does the agent operate the cluster? | GitOps as the only change path + read-mostly RBAC + per-incident break-glass; PVC/StatefulSet/namespace deletes outside every standing role | ✅ **GitOps** (product owner, 2026-08-06: *"Of course gitops"* — diagnostics via cluster + Postgres read access, fixes as repo changes). Practices in the proposal's §2b |

All four concerns ✅ checked. D6's data question is answered — **start clean, no dump restore** — and
D4's zone-host sub-decision is answered — **NS hosting moves to OVH DNS**. Realization proceeds under
[#271](https://github.com/TheCaptainCompany/captain-food/issues/271) per the ADR's consequences.

---

## 18. One decomposition axis: spec folders, schemas, projectors — PROP-20260807-174246 — ✅ DECIDED 2026-08-07

> **Approved as recommended** (product owner, 2026-08-07 — D1–D8, with D2 and D8 in their revised
> forms; the critical-path-growth concern explicitly accepted), recorded by
> [ADR-20260807-183024](../adr/ADR-20260807-183024-one-decomposition-axis.md). The rows below stand
> as the record of the option space; every `_(open)_` cell reads **✅ as recommended (2026-08-07)**.
> Realization order is the ADR's consequences list; unresolved questions live on
> [#374](https://github.com/TheCaptainCompany/captain-food/issues/374)'s checklist.

[PROP-20260807-174246](PROP-20260807-174246-one-decomposition-axis-specs-schemas-projectors.md)
([#374](https://github.com/TheCaptainCompany/captain-food/issues/374)). Product-owner directive
(screaming architecture): spec folders per business domain + common, per-domain storage, per-domain
`configuration.yaml`, per-domain projectors, admin cross-scope queries preserved. Completes the
one-axis chain begun in §17: `specs/{scope}/` → `domain-{scope}` crate → `actor-{scope}` image →
`{scope}` schema → `projector-{scope}`.

| # | Decision | Recommended | Answer |
|---|---|---|---|
| D1 | Spec folders per scope + `common/` | Yes — with placement, cross-scope-DAG and kernel-purity validator rules | _(open)_ |
| D2 | **Storage level** | **REVISED after product-owner pushback** (*"I don't like too many responsibilities on one database"* — the integration-database antipattern): split by RESPONSIBILITY — **`captain-core`** (event log + mailbox only; all backup/PITR budget) and **`captain-views`** (per-scope schemas of projections; rebuildable by replay, EXCLUDED from backups) in the one CNPG cluster. No native cross-DB join is ever needed; cross-scope exposure via projections/GraphQL; per-scope lifts later are connection-string changes | _(open)_ |
| D3 | The event log | Stays SINGLE in a `core` schema — global ordering, PM causality, one PITR timeline, the GDPR erasure path | _(open)_ |
| D4 | Projectors | Per scope over the single log, independent checkpoints; admin/BAM are consumer schemas — scope views never join across schemas | _(open)_ |
| D5 | Configuration | Splits per scope + common; each bin's generated `Config` reads only its own keys | _(open)_ |
| D6 | Admin cross-scope queries | Via **projections + GraphQL composition** (the admin surface reads its own consumer schema); `admin_ro` cross-schema SQL demoted to INCIDENT tooling, never an application path | _(open)_ |
| D7 | Sequencing | Everything pre-cutover — **start-clean makes the storage split FREE** (schemas created, nothing migrated); this window does not recur | _(open)_ |
| D8 | GraphQL per domain (product owner: *"merge them in one graphql"* — closest name: **schema stitching**) | **REVISED after product-owner pushback** (an over-responsible graph = the integration-DB antipattern at the API layer): **`graphql-{scope}` services** (one domain, one graph, one GRANT) + a **thin generated gateway per role** — no DB access, no logic, top-level-field routing from a codegen-emitted composition table (static stitching, no query planner). Cheap here because CQRS denormalization puts composition in the PROJECTOR, so entity resolution/N+1 never arise; a validator rule keeps nested types intra-scope | _(open)_ |

Concern registered and unchecked: **critical-path-growth** — prod is down and this grows the
pre-cutover program again; approving accepts that explicitly.

---

## 19. Build in public — PROP-20260807-190936

[PROP-20260807-190936](PROP-20260807-190936-build-in-public-transparency.md)
([#377](https://github.com/TheCaptainCompany/captain-food/issues/377)). Product-owner directive:
platform transparency — *"Kubernetes completely open"* — for recruitment, press, marketing, branding.
**The line: transparency exposes INFORMATION, never CONTROL** — "Kubernetes open" is a generated,
sanitized public view OF the cluster (from the already-public GitOps state), never network reach INTO
it. Most of L1 already exists as a side effect of the operating model (public repo, ADRs, the git
deploy ledger, public CI). Decisions D1–D4 open (levels · initial aggregate-only metric set · L4 as a
static generated page · L2–L4 after cutover); concerns **pii-and-gdpr** and **attack-surface**
registered. Related, no decision needed: [#378](https://github.com/TheCaptainCompany/captain-food/issues/378)
emits JSON Schemas FROM the validator model (generated, never hand-written) for authoring-time
feedback — the validator stays the semantic authority (`REF_CONTRACT` already gates
$ref-kind-appropriateness).

---

## 20. The rider/delivery write surface — PROP-20260808-141817

[PROP-20260808-141817](PROP-20260808-141817-rider-delivery-write-surface.md)
([#348 "Epic: the rider/delivery write surface does not exist"](https://github.com/TheCaptainCompany/captain-food/issues/348)).
`Proposed`, all decisions **open**. Derives the four delivery persona journeys and answers the
epic's vocabulary question (the wired offer/accept vocabulary is canonical); decomposes into 8 V0
slices (+3 V1). Absorbs the rider-write-surface half of PROP-20260726-172500 (whose D1/D2/D3/D4/D5
rows above remain that proposal's). **Two unchecked Concerns mechanically block `Approved`**: the
D3 rename (event vocabulary is a PO call) and the slice-2 validator-credit semantics (a PM-send
credit must require a resolvable PM edge, never an annotation).

| # | Decision | Recommendation |
|---|---|---|
| D1 | `AssignDeliveryToPartner` family: retire vs keep for manual dispatch | **Retire** — no journey pushes a job at a partner; an assignment no courier agreed to carry is the oversell failure mode as an event type |
| D2 | `UpdateDeliveryPartnerStatus`: retire vs keep as a command-wrapped fact | **Retire** — a command wrapping an external fact (ADR-0004); the ACL already records it as inbound `DeliveryStatusUpdated` |
| D3 | `Unassign…` naming: keep as-is vs generalize to `DeliveryAssignmentReleased` | **Generalize/rename** — one release step for both courier kinds; cheapest now, before production events exist (held as an unchecked Concern; decide before slice 6) |
| D4 | Issue model | **One open issue per job** (V0) — the honest model for `issueId`-less commands; history stays in the log |
| D5 | `ConsumeCustomerCredit` shape | **`PlaceOrder` payload flag + PM step** — consume atomic with payment (ADR-20260726-163737 §checkout-consume) |
| D6 | How does `PlaceReplacementOrder` get spec-checkable dispatch coverage? (no PM step sends it — wrapper-seam dispatch) | **A declared `sends:` on the wrapper-seam receive** — parallel to the existing declared `emits:` precedent (`ordering/processmanager.yaml:194-199`), checkable both ways; alternatives: extend the step DSL (bigger), or leave it in the warning baseline (erodes the diff discipline) |

---

## 21. Disappearance is a designed state — PROP-20260808-142532

[PROP-20260808-142532](PROP-20260808-142532-disappearance-terminal-states.md)
([#398 "Decide the API contract for tombstoned rows before the #194 projection sweep"](https://github.com/TheCaptainCompany/captain-food/issues/398)
+ [#347 "Decide the last annotated read-model hole: Restaurant fed by RestaurantListingOptedOut"](https://github.com/TheCaptainCompany/captain-food/issues/347)).
`Proposed`, all decisions **open**. One principle, two faces: disappearance is always a designed
state; physical row removal is reserved for legal erasure. **Three unchecked Concerns mechanically
block `Approved`**: D2 is THREE artifacts (`OrderPlaced` + `CheckoutSnapshot`/`PaymentIntentCreated`
+ the replacement-order emitter) needing PO event sign-off; the resolver-policy change lands in the
emitter with NO generic seam (the `Option<_>` type flip + one shared hydration helper, never a
source-text scanner); the OPTED_OUT guard errors carry ADR-0032 completeness.

| # | Decision | Recommendation |
|---|---|---|
| D1 | API contract for dangling/tombstoned references | **The scoped mix** — projector-/event-carried composition for money-history surfaces + a thin pinned dangling policy (silent drop and join hard-errors banned) |
| D2 | `OrderTracking` restaurant name/phone | **Event-carried on `OrderPlaced`** — survives projection rebuild after restaurant stream deletion; three artifacts, per the header Concern |
| D3 | [#347](https://github.com/TheCaptainCompany/captain-food/issues/347): tombstone vs `listing_status` fold vs vestigial removal | **Fold to a new `OPTED_OUT` value** — a tombstone is self-defeating under SIRENE re-import; also closes the live cold-email exposure (`ProspectionPipeline` does not fold the opt-out today) |
| D4 | `OPTED_OUT` shape | **Enum value + BOTH write-side guards** (`OptOutRestaurantListing` rejected for ACTIVE_PARTNER, AND `ChangeRestaurantListingStatus` rejecting `OPTED_OUT` as source and target — the guard closes two doors, not one); the orthogonal `delisted` boolean is materially strengthened by the two-door finding and stands ready if the PO prefers unspellable over guarded |
| D5 | Erased-restaurant storefront host | **Parked "closed" page** — never the claim-landing fall-through (invites resurrection of a dead business's address), better than a bare 404 |

---

## Maintenance

The `architect` reconciles this file on each daily run: new proposals add rows, answered decisions
move to §5, and a decision open for many runs gets flagged in the report with its age. A decision
nobody is making is the most expensive thing in the backlog, and it will never surface on its own.
