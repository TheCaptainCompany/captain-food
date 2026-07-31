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
| **C** | [PROP-170000 D3](PROP-20260726-170000-event-log-integrity-evolution-and-erasure.md) — **GDPR erasure strategy** | Every option is cheaper the fewer customers exist. May touch the event envelope itself, so it constrains the log's shape. Gates [#194](https://github.com/TheCaptainCompany/captain-food/issues/194) — and now also the `ExpireOrder` reminders pilot ([ADR-20260731-153000](../adr/ADR-20260731-153000-gdpr-expiry-as-scheduled-actor-message.md)): the scheduled-expiry machinery lands with a STUB handler until this is decided | Crypto-shredding |
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
| PROP-172500 D4 | Job-pool filtering | Filter by city, zone and `RiderStatus` |
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
| PROP-172500 D3 | Reclaiming an abandoned run | Rider release + stall sweep | A stalled `PICKED_UP` job means the food is with the rider — re-offering is wrong, it needs re-cooking |

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

## Maintenance

The `architect` reconciles this file on each daily run: new proposals add rows, answered decisions
move to §5, and a decision open for many runs gets flagged in the report with its age. A decision
nobody is making is the most expensive thing in the backlog, and it will never surface on its own.
