# Open decisions — the product-owner register

**Every decision the proposals are waiting on, in one place.** Proposals hold the reasoning; this
holds the queue. If a decision is not here, it is not blocking anything.

> **The gate:** implementation does not start from a proposal whose **Status is not `Approved`**.
> The `architect` agent enforces this — an issue whose proposal has unanswered questions is classified
> 🔴 RED and never dispatched. So this page is the throttle on the whole pipeline.

Last reconciled: **2026-07-26** · 11 proposals `Proposed` · **62 open decisions**

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
| **C** | [PROP-170000 D3](PROP-20260726-170000-event-log-integrity-evolution-and-erasure.md) — **GDPR erasure strategy** | Every option is cheaper the fewer customers exist. May touch the event envelope itself, so it constrains the log's shape. Gates [#194](https://github.com/TheCaptainCompany/captain-food/issues/194) | Crypto-shredding |
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
| PROP-170500 D2 | Telemetry sampling | Tail-based: 100% of errors and money paths, sample successes |
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
| PROP-170500 D1 | Telemetry backend and monthly ceiling | Hosted OTLP, EU region pinned | ADR-0042 chose Frankfurt for GDPR; traces carrying `customerId` are personal data |
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

## Maintenance

The `architect` reconciles this file on each daily run: new proposals add rows, answered decisions
move to §5, and a decision open for many runs gets flagged in the report with its age. A decision
nobody is making is the most expensive thing in the backlog, and it will never surface on its own.
