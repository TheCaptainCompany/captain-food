# Open decisions — the product-owner register

**Every decision the proposals are waiting on, in one place.** Proposals hold the reasoning; this
holds the queue. If a decision is not here, it is not blocking anything.

> **The gate:** implementation does not start from a proposal whose **Status is not `Approved`**.
> The `architect` agent enforces this — an issue whose proposal has unanswered questions is classified
> 🔴 RED and never dispatched. So this page is the throttle on the whole pipeline.

**Last reconciled: 2026-08-18** — the founder's rulings of 2026-08-18 landed (§49), STAFF-AUTH
closed for two of three roles, and the page was **restructured from an archive back into a queue**
([ADR-20260818-193000](../adr/ADR-20260818-193000-the-register-is-a-queue-and-a-closed-row-collapses-to-its-record.md)).
**Closed rows now collapse to their outcome and the record that holds the reasoning** — the argument
lives in the ADR and the proposal, the history lives in git, and this page carries what is still
owed. Earlier reconciliation headers are in `git log docs/proposals/DECISIONS.md`.

> **Customer decisions: see [BRIEF-20260808-customer-decisions.md](BRIEF-20260808-customer-decisions.md) (ten decisions).**
> Everything only the product owner can decide — the five-decision money posture, account-level
> erasure scope, admin act-as, the operating entity, transparency levels, and who funds promotions —
> is argued there, lens by lens. Answers land back in this register.

---

## The queue — everything still owed, on one screen

This table is an **index, not a second home**: each row's full argument stays in its section below,
and nothing is decided here. A row leaves this table only by being answered in its section.

### Owed by the founder, or gated on counsel

| Row | The question, in one line | Full row |
|---|---|---|
| **Q-L1** | The publishable identity block: postal address, phone, a named *directeur de la publication*, and a consumer mediator — named nowhere. A legal precondition for distance selling, not a backlog item. | [§35](#35-the-founder-answer-sheet-of-2026-08-12---all-rows-closed-on-answers) |
| **LOSS-1** | When a post-delivery capture permanently fails, who absorbs the loss, and what does the operator do? Real money, and it commits Captain to absorbing it. | [§38](#38-capture-on-delivered-review-carry-forwards--pr-545-five-lens-review-2026-08-14) |
| **STAFF-AUTH-AM** | How does an **account manager** sign in? The 2026-08-18 rulings answered rider (phone/SMS) and restaurant (email link); account managers were not mentioned and are not ruled. | [§49](#49-the-founder-rulings-of-2026-08-18---three-rulings-plus-a-cleared-queue) |
| **D5** (Uber) | Menu ownership and per-channel price parity across Captain / HubRise / Uber. Open since 2026-08-08 and not in the 2026-08-14 delegated batch. | [§11](#11-uber-eats-marketplace--per-surface-uber-direct-credentials--prop-20260730-032306) |
| **D7** (Uber) | Is the entity on the signed Uber agreement the entity that will operate the platform? Needs counsel, not a recommendation. | [§11](#11-uber-eats-marketplace--per-surface-uber-direct-credentials--prop-20260730-032306) |
| **CAP-READY-LEGAL** | Capturing a COLLECTION order at READY takes payment before possession transfers — disclosure and VAT tax-point constraints on the unbuilt receipt engine. Counsel-gated, not a decision blocker. | [§41](#41-collection-captures-at-ready-not-at-pickup--refinement-of-12-founder-directive-2026-08-14) |
| **TRK-scope** | Does a pseudonymous journey identifier fit the audience-measurement exemption? **With legal, not with the founder** — the proposals are deliberately not amended until legal reports. | [§27](#27-business-metrics-for-every-feature-and-every-persona--prop-20260810-234225) |
| **PMW-3** | Actor queries as a mailbox/transport message. The founder floated it; **nothing authorises building it**. 🔴 | [§42](#42-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side-founder-directive-2026-08-15) |
| **Consumer-mediator registration** | ⏸️ Deferred by the founder to the first real consumer order, against the recommendation. Listed so the deferral stays visible. | [§22](#22-new-rows-from-the-2026-08-08-sweep) |
| **Solida rebrand** | Waiting on an external trademark process. Gates only [#411](https://github.com/TheCaptainCompany/captain-food/issues/411). | [§22](#22-new-rows-from-the-2026-08-08-sweep) |

### Team-owned — no answer is owed from outside the team

| Row | The question, in one line | Full row |
|---|---|---|
| **BUS-1** 🔴 | `operationStatusChanged` is a declared product subscription served by a **process-local bus**, so the client polls — the founder's own no-polling rule, already broken in shipped code, on the money path. | [§43](#43-opening-hours-and-stock-are-checked-server-side-on-place-order-and-a-big-catalog-snapshots-every-100-events-founder-directive-2026-08-15) |
| **FEN-1** 🔴 | `expectedTotal` is **optional**, so a client can simply omit the price fence on the money path. | [§43](#43-opening-hours-and-stock-are-checked-server-side-on-place-order-and-a-big-catalog-snapshots-every-100-events-founder-directive-2026-08-15) |
| **CHK-1** 🔴 | A shipped comment calls the restaurant fold *"authoritative, race-free"*. It is false. | [§43](#43-opening-hours-and-stock-are-checked-server-side-on-place-order-and-a-big-catalog-snapshots-every-100-events-founder-directive-2026-08-15) |
| **RSO-1 · RSO-2** | Where *"is this restaurant open right now?"* is derived, and re-validating each line's orderability at checkout. Directed by the founder; only the mechanism is open. | [§43](#43-opening-hours-and-stock-are-checked-server-side-on-place-order-and-a-big-catalog-snapshots-every-100-events-founder-directive-2026-08-15) |
| **STK-1 · SNAP-1 · CAT-1 · BSY-1 · DSC-1 · PAN-1 · HRS-1** | The 2026-08-15 audit's remaining rows: the oversell arbiter, snapshot residency, `restaurantId → catalogId`, `BUSY` as a word that changes nothing, seven silently-dropped discovery filters, a latent panic, and the accept branch's owed instrumentation. | [§43](#43-opening-hours-and-stock-are-checked-server-side-on-place-order-and-a-big-catalog-snapshots-every-100-events-founder-directive-2026-08-15) |
| **STO-7 · STO-8 · STO-9** | Three questions that must be answered **before or with** the physical database split. STO-9 came back into scope on 2026-08-18. | [§32](#32-storage-boundaries-and-least-privilege-database-users--prop-20260811-093000) |
| **STO-10** | ⏸️ Parked by founder answer until the walk lands. Reported blocked — never re-ranked to look dispatchable. | [§32](#32-storage-boundaries-and-least-privilege-database-users--prop-20260811-093000) |
| **IDOR-1** | Per-instance authorization across 83 of 118 operations. Deadlined fast-follow / hard V0-launch blocker. | [§39](#39-per-instance-authorization--a-cross-tenant-idor-on-both-sides-83-of-118-operations--178-write-side-per-instance-authorizationhttpsgithubcomthecaptaincompanycaptain-foodissues178--618-read-surfaces-missing-readscope--the-read-half-of-the-write-path-authorization-gap-178httpsgithubcomthecaptaincompanycaptain-foodissues618--prop-20260726-171500-architect-run-2026-08-14-scope-corrected-2026-08-17) |
| **IDOR-DEADLINE-GAP** | The three IDOR deadline triggers are all things the **team** does, while CUSTOMER signup is self-service — so *production restored with signup open* trips none of them. | [§45](#45-the-founder-answer-sheet-of-2026-08-17---all-six-rows-answered) |
| **ISO-3** | `EventStore::append` takes no capability witness and no issue tracks it. | [§29](#29-scope-isolation-is-nominal--prop-20260811-090000-product-owner-directive-2026-08-11) |
| **PMW-2** | Cross-aggregate activation residency, and the staleness fence it needs. | [§42](#42-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side-founder-directive-2026-08-15) |
| **RDR-1** | What may an `EVENT_STREAM` `read:` step's `model:` point at? Wants deciding **before** the PR2 emitter, not after. | [§48](#48-reader-set-derivation-carry-forwards--564httpsgithubcomthecaptaincompanycaptain-foodissues564-nine-lens-mob-checkpoint-2026-08-15) |
| **ENF-1** | Extend the capability allowlist to `jsonwebtoken` and `aes-gcm`. | [§40](#40-capability-allowlist-coverage--extend-the-manifest-gate-to-security-sensitive-dependencies-founder-insight-2026-08-14) |
| **REP-1** | The read-model "inherit" refinement — recorded as a confirm-or-redirect, not a blocker. | [§33](#33-repository-crates-and-the-dissolution-of-infrastructure--prop-20260811-173223-product-owner-direction-2026-08-11--closes-iso-1-and-iso-2) |
| **avelo37 threshold** · **Geocoding** | Deferred by design pending real order data; and the team owes a proposal. | [§22](#22-new-rows-from-the-2026-08-08-sweep) |

---

## How to decide

Four ways, in increasing formality. All are fine; pick per decision.

1. **Answer in this file** — put the choice in the `Decision` column with the date. Cheapest, good for
   the batch-approvable set below.
2. **Comment on the proposal's tracking issue** — better when the answer needs reasoning that future
   readers will want.
3. **Write an ADR** — required for anything cross-cutting (`docs/adr/ADR-YYYYMMDD-HHMMSS-*.md`).
4. **The interactive decision form** (product-owner directive, 2026-08-08: *"I liked what you did
   with the html file to let me answer in my tempo… keep this approach for other sessions"*) —
   when a BATCH of decisions goes to the customer, publish the brief as an interactive artifact:
   one card per decision (question, per-lens arguments, recommendation) with tap-choices
   **Approve as recommended / Different choice / Let's discuss** plus a free-text box, progress
   saved locally, and a "Copy my answers" button producing a markdown answer sheet the customer
   pastes back into the session. The 2026-08-08 ten-decision brief closed same-day this way.
   Mechanics for rebuilding it: [docs/claude/sessions.md](../claude/sessions.md).

Then flip the proposal's `Status` to `Approved`, naming what recorded the decision. **Do not rewrite
the proposal to match the answer** — it is a historical record of what was on the table; the decision
lives in the header, the register, and the ADR.

A proposal can be **partially approved**: mark the decided rows here and note in the header which
decisions remain. That is often the right move — several proposals have one hard question and four
easy ones.

---

## 1. Decide these first — highest leverage

✅ **No open row.** The six keystone decisions that gated roughly two thirds of the backlog are
closed, and the rows below are kept for their identifiers, which other records cite.

| # | Decision | Answer | Recorded in |
|---|---|---|---|
| **A** | Payout posture — Stripe Connect vs merchant-of-record | **Connect, separate charges & transfers** | [ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md) |
| **B** | Capture timing | **Authorize at checkout, capture on acceptance** — per service type; refined by §41 for COLLECTION | [ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md) |
| **C** | GDPR erasure strategy | Orders: **tombstone + stream deletion**; account scope decided 2026-08-08 | [ADR-20260731-160000](../adr/ADR-20260731-160000-order-erasure-tombstone-then-stream-deletion.md) · [ADR-20260808-203443](../adr/ADR-20260808-203443-tips-voluntary-contributions-funding-model.md) |
| **D** | Allergen representation (EU FIC 1169/2011) | **Controlled 14-category enum + explicit "not declared"** | [ADR-20260808-171056](../adr/ADR-20260808-171056-register-sweep-consent-decisions.md) |
| **E** | Acceptance timeout policy and TTL | **Auto-cancel + auto-approved refund**; 5 min with per-restaurant override | [ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md) |
| **F** | How a screen declares a runtime input source | **Name it explicitly (`from:`)**, with the naming collision resolved by §22 | [ADR-20260808-171056](../adr/ADR-20260808-171056-register-sweep-consent-decisions.md) |
| **G** | Cart price — LIVE vs LOCKED at add-to-cart | **Option B / LIVE** — priced fresh on read via `price_cart`; the projection stays a money-free fold | Product owner, 2026-08-10 · unblocked [#429](https://github.com/TheCaptainCompany/captain-food/issues/429) |
| **H** | The recorded boundary set | **Five business boundaries + `platform` + the kernel**, verbatim *"I'm ok for the 5 / Customer / Order / Catalog / Restaurant / Delivery"* | Product owner, 2026-08-11 · §31 |

---

## 2. Batch-approvable — recommendation is the standard answer

✅ **CLOSED 2026-08-08.** All fifteen rows were decided by ensemble consent with the customer veto
window open, or retired as stale. They covered skipped-event prevention (`xmin` guard), event
evolution policy (additive-only + `event_version`), `$maxAge`/`expired_at`, the spec-vs-code
divergences, where the workers run, GraphiQL/Voyager in production (kept, ADMIN-gated, self-hosted),
subscription fan-out (`LISTEN`/`NOTIFY` + reconcile-on-reconnect), the write-side scope check,
supplied-vs-derived ids, sequencing against [#144](https://github.com/TheCaptainCompany/captain-food/issues/144),
the drifted product spec, the four dead screen actions, job-pool filtering, rider↔customer contact
(through the order conversation, with a masked-call fallback) and menu scheduling (deferred until
combos). Reasoning and the per-row cures: [ADR-20260808-171056](../adr/ADR-20260808-171056-register-sweep-consent-decisions.md)
and the proposals it names.

---

## 3. Genuine trade-offs — worth your time

✅ **CLOSED 2026-08-08.** Fourteen rows: fee-split rounding (buyer total first, residual cent to a
stated leg, pinned by an odd-total test), the per-zone delivery-fee dimension, tips
([ADR-20260808-203443](../adr/ADR-20260808-203443-tips-voluntary-contributions-funding-model.md) —
rider tips as recommended, restaurant tip per-restaurant opt-in, platform contribution on the
HelloAsso model), stock ownership (re-validate at checkout, decrement only Captain-managed offers),
per-service-type pricing, catalog images on a public audience, promo codes first, the V0 notification
channel (in-app + sound **and** SMS in the same slice, escalation at ~60–90 s), timed pause and
opening-hours exception days, the scheduling window and order-modification scope, ADMIN explicit
act-as (superseding ADR-0037's impersonation-only stance), rejection reasons as a controlled enum +
note, the postal-code delivery-area model, proof of delivery, and reclaiming an abandoned run.
Records: [ADR-20260808-171056](../adr/ADR-20260808-171056-register-sweep-consent-decisions.md) ·
[ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md) ·
[ADR-20260808-203443](../adr/ADR-20260808-203443-tips-voluntary-contributions-funding-model.md).

---

## 4. Inherited — ✅ swept 2026-08-08 (what remains open moved to §22)

✅ **CLOSED.** The inherited backlog of undated rows was swept on 2026-08-08; everything still live
moved to §22, where it is tracked with an owner.

---

## 5. Decided

| Date | Decision | Answer | Recorded in |
|---|---|---|---|
| 2026-08-15 | **§42 PMW-1 — the PM `read:`-step grammar: how do you SPELL "fold the aggregate's stream"?** The row gating the typed request/reply design for PM actor asks | **CLOSED as (a) + the additive §8 decision grammar**, verbatim: *"I'm ok for the dsl for process manager"* — [PROP-20260815-142349 "Actor `answers:` + the PM `ask:`/`branch:` decision grammar"](PROP-20260815-142349-actor-answers-block-and-the-ask-step.md) Approved after three founder-directed design rounds. `read:` stays exactly as [PR #566 "A process-manager read step declares its SOURCE, not only its shape (#564 PR1)"](https://github.com/TheCaptainCompany/captain-food/pull/566) lands it; the richer grammar arrives as the **additive `ask:`/`branch:` step kinds**, replies typed by the answering actor's declared state. **PMW-3 (the transport) stays parked** — the PROP consumes only its two buildable items. Build tracked in [#582 "Actor `answers:` block + PM `ask:` step — typed request/reply for actor queries, transport stays parked"](https://github.com/TheCaptainCompany/captain-food/issues/582), sequenced behind #566 | Founder + this register (§42) + [PROP-20260815-142349](PROP-20260815-142349-actor-answers-block-and-the-ask-step.md) |
| 2026-08-11 | **§31 BND-1 — THE BOUNDARY SET.** The row that had been top of this register across several runs, blocking [PROP-20260811-090000](PROP-20260811-090000-scope-isolation-runtime-decomposition.md) slices 1–5 and 15 of REP-2(a)'s 28 crates | **CLOSED as recommended (a)**, verbatim: *"I'm ok for the 5 / Customer / Order / Catalog / Restaurant / Delivery"* — **five business boundaries** `customer` · `order` · `catalog` · `restaurant` · `delivery`, plus the **`platform`** bucket and the **`common`** kernel (a linkage concept with no pod, never a boundary). `catalog` stays a boundary; **`comms` and `payments` dissolve into `order`**; `public` stays a role of `customer`, not a member. The two partitions ([#493](https://github.com/TheCaptainCompany/captain-food/issues/493)) become one. **Still owed before that file can be marked approved**: the superseding ADR on [ADR-20260807-183024](../adr/ADR-20260807-183024-one-decomposition-axis.md) D1's named scope list (the DECISION-REVERSAL concern) | Product owner, this register (§31) + [PROP-20260811-150242](PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md) §0, [#493](https://github.com/TheCaptainCompany/captain-food/issues/493) |
| 2026-08-11 | **§31 BND-2 — the boundary's NAME** | **(a) `delivery`, not `rider`**, verbatim: *"Considering that you prefer delivery to rider because we have restaurants and admin commands in it make sense"* — the reasoning endorsed, not merely the outcome. `RIDER` stays a role (`c4-l2.yaml:68`) | Product owner, this register (§31) + [PROP-20260811-150242](PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md) §0 |
| 2026-08-11 | **§31 BND-6 + BND-7 — the ETA**, both answered *before* the freeze onto `OrderPlaced` is specified, which is what made them cheap | **BND-6 = (b)**, verbatim: *"Prep time only + labelled"* — when the travel leg cannot resolve, show the prep-time estimate and **label it for what it is**. The label is the whole condition, and it is exactly the defect D13.1 fact 4 measured shipped. **BND-7 = (a)**, verbatim: *"Estimate for now"* — the frozen number is an **estimate**, not a promise with a remedy; it exists as the internal promised-vs-actual signal. **Consequence to carry into D13.5 step 4**: the `OrderPlaced` payload field is named and documented as an estimate, no remedy semantics attach, and the upcasting story (old events carry no estimate — the reader says so rather than defaulting) is still owed because the event is already stored | Product owner, this register (§31) + [PROP-20260811-150242](PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md) §0 + D13.5 |
| 2026-08-11 | **§31 BND-4(i) — read side or write side?** The transcription slip in the permission directive, flagged rather than assumed because a wrong `GRANT` is a boot failure or a silent breach | **Confirmed: it is the WRITE side** that actors and projectors read to load events, verbatim: *"I agree it was the write side of course my mistake"*. [PROP-20260811-093000](PROP-20260811-093000-storage-boundaries-and-least-privilege-database-users.md) §6.1 may now be emitted on that reading. BND-4(ii) — the omitted mailbox row — was never a question and is already corrected in that file's §6.1.1 | Product owner, this register (§31, §32) + [PROP-20260811-093000](PROP-20260811-093000-storage-boundaries-and-least-privilege-database-users.md) §6.1.1 |
| 2026-08-11 | **§30 APP-1 — sequencing of the per-app folder work against the §29 enforcement slices** | **DELEGATED to the team**, verbatim: *"I believe creating the apps will do a cleaner split between and help the split process / Do the way you think it's better"* — with one deliverable demanded and **not** delegated: *"I need to know the app list and all dependencies we need to make sure we have a clean split."* The team's recommendation (b) stands as the sequencing; the app-list-plus-dependencies artifact is the acceptance criterion for [#491](https://github.com/TheCaptainCompany/captain-food/issues/491) slice A1, and [PROP-20260811-150242](PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md) §5 (all 57 apps homed, 39 + 18) is its boundary-side input | Product owner, this register (§30) + [PROP-20260811-141654](PROP-20260811-141654-per-app-declaration-folders.md), [#491](https://github.com/TheCaptainCompany/captain-food/issues/491) |
| 2026-08-11 | **In-between units for translating process managers** — a concession the product owner offered unprompted, not a queued row | **GRANTED**, verbatim: *"I'm ok if we create in between boundaries for process managers that are making the translation between 2 boundaries thanks to the fact that we have one crate per actor client type it's perfectly fine."* The team has **bounded** it rather than banked it: **§31 BND-8** records the `CONNECT` test, the three brakes and the classification of all five PMs (**zero units today, one candidate**), and **BND-9** records the precondition — the premise *"one crate per actor client type"* **is not true of process managers today**, which the code shows in two places | Product owner + [PROP-20260811-150242](PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md) §0, §D15 |
| 2026-08-11 | **§29 ISO-1 and ISO-2** — does `projection_runtime` own the LISTEN/`EventWaiter` plumbing, and do the `View_*` write repositories move per boundary? Both were blocking [PROP-20260811-090000](PROP-20260811-090000-scope-isolation-runtime-decomposition.md) slice 1 | **Both (a)**, the team's recommendation, closed by a product-owner direction that answers them without naming them, verbatim: *"The infrastructure has to be split in multiple crates to be able to regulate permissions of apps based on what they need nothing more."* Both (b) options end with a bin linking a crate carrying every other boundary's code — ISO-1(b)'s own wording is *"the bin keeps linking `infrastructure`"*, and ISO-2(b) is one shared crate every projector links, so every projector gains every boundary's write access. Under [PROP-20260811-173223](PROP-20260811-173223-repository-crates-and-the-infrastructure-split.md) D3 there is no `infrastructure` to stay in. **Caveat recorded, not hidden**: this does not by itself make slice 1 deliverable — the harder coupling is `DomainEvent`, one enum over all 8 scopes defined in the facade (**REP-4**, §33), which ISO-1 did not name | This register (§29, §33) + [PROP-20260811-173223](PROP-20260811-173223-repository-crates-and-the-infrastructure-split.md), [#497](https://github.com/TheCaptainCompany/captain-food/issues/497) |
| 2026-08-10 | **Is `specs/**` touchable by the team?** — never a queued row; a directive that arrived in-session, and the largest constraint removal in the operating model to date | **The freeze is LIFTED**, verbatim: *"I'm surprise that I read that the spec was untouchable now that we have the team working together we don't need to have this constraint anymore"* / *"We can perhaps have a discussion if the team is willing to change the structure of the specs. But I'm pretty sure the team will ensure the right naming and scope. Just keep me informed."* Execution loops may add and amend DSL **content and structure**. The boundary is **not** content-vs-structure (rejected: a scope-folder move rewrites no refs and is free, while a one-word type change on an emitted event is irreversible — the split is anti-correlated with risk in both directions). It is **three questions in order**: (1) does it contradict/create a **recorded decision**? → stop, file a row; (2) is the shape already **emitted, stored or promised**? → it is a **migration**, record the versioning story first; (3) otherwise it is the team's, **`specs/common/` included** — the kernel is high-fan-out, not off-limits, and freezing it would freeze the one place "one name = one dedicated scalar" is enforced. **Structure needs no separate gate**: proportionality already routes any real option space to a proposal + register row, which *is* the "perhaps a discussion" that was offered. **Reporting replaces the freeze**: one sentence per landed spec change in [docs/SPEC-LOG.md](../SPEC-LOG.md), same commit, gated — no cadence, no digest to send. **NOT delegated**: this register, external/legal/admin-gated matters, and the binding value-first method | [ADR-20260810-221840](../adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md) |
| 2026-08-10 | **Who prioritises the backlog** — never a queued row; a directive that arrived in-session | **Delegated to the team**, verbatim: *"Don't care about the project field anymore the team decides without me."* The `Priority` bucket and **row order** pass to the team (`Type`/`Value Size`/`Impact`/`Effort` were already team-set at triage, so only two things actually change hands). **NOT delegated**: this register, external/legal/admin-gated matters, `specs/**` approval, and the **value method itself** — which is promoted from description to binding constraint, standing in for the judgment that left the loop. New prohibition: an agent may never re-bucket or reorder an item **to make it dispatchable or to legitimise its own recommendation**; a blocked top item is reported blocked, never re-ranked. The product owner keeps a silent, immediate, per-item override. Consequence for THIS file: with the board no longer under product-owner eyes, **the register's ordering by leverage is now the main surface they work from** | [ADR-20260810-215503](../adr/ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md) |
| 2026-08-10 | **The six-decision answer sheet** (interactive artifact) | **Four approved as recommended** — 451-B `currency_mismatch` (spec window now OPEN), 451-C #451 retitled (executed), the `from:` collision and geocoding rows both to recommendation (c) = **team picks and records**. **§6.4 claim staleness CLOSED** on a legal+business convergence the PO sent the card back for: keep the ~1h Supabase default, explicit immediate revocation for rider deactivation and staff removal, [#194](https://github.com/TheCaptainCompany/captain-food/issues/194) erasure scrubs `app_metadata` AND revokes refresh tokens. **#474 answered with measurements** (990 tests / 182 binaries / 34s warm; `-p application` = 324 tests in 0.04s linking 9 crates) — the crate split already gives per-part testability, the hole is local-only, and the fix is `make test-crates` invoked from the Stop hook. **Process lesson recorded**: consult the standing lenses before escalating a card — a question a lens can answer is not a decision | [ADR-20260810-194548](../adr/ADR-20260810-194548-six-decision-answer-sheet-claim-staleness-closed.md) |
| 2026-08-08 | **The register sweep — 30 rows by ensemble consent** | The five-lens sweep decided 30 open rows with cures folded (per-row notes throughout §§1–4, 9, 11, 19), escalated one to the customer (PROP-165500 D5 → brief ch. 6), retired 7 stale blocks, and added the §22 rows. Customer veto window open on every consent decision | [ADR-20260808-171056](../adr/ADR-20260808-171056-register-sweep-consent-decisions.md) + [BRIEF-20260808-customer-decisions.md](BRIEF-20260808-customer-decisions.md) |
| 2026-08-02 | **PROP-20260802-130500 D1–D6** — isolation by construction | **All six answered** (D1 via PROP-20260728-152752 D9). D2 **(a) handler crates per actor** — aggregates AND process managers, domain value types stay one crate · D3 **cargo-deny capability allowlist in phase 1** (who may hold `sqlx`/`reqwest`) · D4 **one generic `ActorClient` with `get_operation_status(message_id)`** — operation status is generic to all operations, so neither a per-actor client method nor a separate `OperationStatusClient` type; per-actor typed clients stay write-side · D5 **`test-fixtures` feature + CI check** · D6 **later, separately — against the recommendation** (own change after phase 1). Scope directive: "per actor" includes the two process managers at every phase. | Product owner, this register (§14) + [PROP-20260802-130500](PROP-20260802-130500-isolation-by-construction.md), realized by [#290 "Actor-client crate isolation (PROP-20260728-152752 D9): compiler-enforced door, then per-actor crates"](https://github.com/TheCaptainCompany/captain-food/issues/290) |
| 2026-07-29 | **PROP-170500 D1 + D2** — telemetry backend and sampling | **D1 answered: Honeycomb**, over OTLP/HTTP, pinned to the **EU (`eu1`)** region — a GDPR constraint, not a default, since spans carry `customerId`/`orderId` and ADR-0042 pinned data to Frankfurt. `HONEYCOMB_API_KEY` supplied as a repo Actions secret and pushed to Render by CI. Telemetry **degrades, never gates**: no telemetry key is `required:`, so a missing ingest key drops the exporter and keeps structured logs rather than refusing to serve orders. **D2 answered but NARROWED — against the recommendation**: parent-based HEAD sampling at `1.0` (keep everything), not tail-based. Tail sampling needs Refinery, i.e. a service to run and pay for, which contradicts ADR-0042's minimal-ops-pre-PMF posture — and D2's own justification says the volume is not there yet. Revisit when ingest cost is measurable. | Product owner + [ADR-20260729-183000](../adr/ADR-20260729-183000-telemetry-is-honeycomb-eu-and-degrades-never-gates.md), realizing [#191](https://github.com/TheCaptainCompany/captain-food/issues/191) |
| 2026-07-28 | **PROP-004616 D1–D6** — slug lifecycle + SIRENE inbound events | **All six answered.** D1 `RestaurantSlugConfigured` + `RestaurantSlugReconfigured` (in session) · D2 slug chosen **between claim and activation**, gated by "no activation without a configured slug" · D3 **write-side reservation table** with a real `UNIQUE` (also holds released slugs) · D4 the ACL stages **`RestaurantRegistered` only** — *against the recommendation*, and stricter: no registry-fact event, no ACL branching, the **aggregate** decides record/ignore/update · D5 **null the slug on `NON_PARTNER` rows** · D6 **both** `IGNORED` and `DUPLICATE`. Partially supersedes ADR-0045. | Product owner, this register + [ADR-20260728-011344](../adr/ADR-20260728-011344-slug-lifecycle-and-sirene-inbound-events.md) |
| 2026-07-26 | **PROP-193000 D1–D4** — continuous development loop | **Deferred.** The daily architecture-review routine is sufficient for now; the dev loop stays off until the proposals are under control. `dev-loop.yml` remains `workflow_dispatch`-only with `dry_run` defaulting true. | Product owner, this register |

---

## 6. The daily decision cycle — ⚠️ SUPERSEDED 2026-08-08

⚠️ **SUPERSEDED.** The five-row daily-cycle design (a standing pinned issue, free-prose asks, up to
three questions plus a batch block, keep-implementing-green, and a human approving every DSL change)
was overtaken by the mob operating model and by the lifting of the `specs/**` freeze
([ADR-20260810-221840](../adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md)).
Design record: [PROP-20260726-201500](PROP-20260726-201500-daily-decision-cycle.md).

---

## 7. Slug lifecycle + SIRENE inbound events — ✅ DECIDED 2026-07-28

✅ **DECIDED**, D1–D6, recorded in
[ADR-20260728-011344](../adr/ADR-20260728-011344-slug-lifecycle-and-sirene-inbound-events.md).
Five rows went as recommended; **D4 went against the recommendation** — `RestaurantRegistered`
**only**, unconditionally, with the aggregate deciding record/ignore/update. Design record:
[PROP-20260728-004616](PROP-20260728-004616-slug-lifecycle-and-sirene-inbound-events.md).

---

## 8. SIRENE mirror storage — ✅ DECIDED 2026-07-28

✅ **DECIDED**, D1–D5, all as recommended, with two conditions worth keeping: D2 is **sequenced after
compaction** (`ALTER … TYPE` rewrites the whole table and needs ~655 MB against ~580 MB free), and
D3 holds only going forward — the CI compaction has no ACL, so historical ACL-unmappable payloads are
dropped. Design record: [PROP-20260728-120931](PROP-20260728-120931-sirene-mirror-payload-is-transient.md).

---

## 9. Configuration is declared and validated at startup — PROP-20260729-004500 — ✅ DECIDED (D1–D3/D5 2026-07-29 · D4 2026-08-08)

✅ **DECIDED.** Configuration is declared in the DSL and validated at process start, so a missing or
malformed key fails the boot rather than the first request that needs it. Design record:
[PROP-20260729-004500](PROP-20260729-004500-configuration-is-declared-and-validated-at-startup.md).

---

## 10. CI owns the Render service configuration — PROP-20260729-014500 — ⚠️ SUPERSEDED 2026-08-08

⚠️ **SUPERSEDED** by the move off Render to Kubernetes on OVH MKS (§17,
[ADR-20260806-223656](PROP-20260806-223656-kubernetes-as-the-deployment-substrate.md) and the GitOps
decision D7). The row is kept because the *principle* survived the platform change: the deployment
substrate's configuration is owned by a pipeline over committed manifests, never by a console.

---

## 11. Uber Eats Marketplace + per-surface Uber Direct credentials — PROP-20260730-032306

Design record: [PROP-20260730-032306](PROP-20260730-032306-uber-eats-marketplace-and-per-surface-direct-credentials.md) ·
Tracking issue [#260 "Epic: Uber Eats Marketplace integration (order centralization + menu sync) and per-surface Uber Direct credentials"](https://github.com/TheCaptainCompany/captain-food/issues/260) ·
Record: [ADR-20260730-032306](../adr/ADR-20260730-032306-uber-integration-topology-two-orgs-and-asymmetric-app-auth.md).

**Nine of eleven rows are closed. Two remain open: D5 and D7.**

✅ **Closed:** **D1** build the Eats integration directly rather than layering on HubRise · **D2** two
Uber orgs, split by acquisition surface, storefront first (against the recommendation) · **D3** the
acquisition surface is a field on `OrderPlaced`, because acceptance-first means the saga runs long
after the `Host` header is gone · **D4** a marketplace order is a distinct `ExternalOrderReceived`
event, never nullable payment fields on `OrderPlaced` · **D8** the onboarding wedge is
**bootstrap-then-flip** · **D9** Uber is merchant-of-record on a pre-paid external order,
informational record only · **D10** the wedge is **post-V0**, with the aggregator *shape* designed now ·
**D11** either side in test ⇒ the ORDER is test, ticket unmistakably marked and off the live kitchen
flow — which **unblocked [#257](https://github.com/TheCaptainCompany/captain-food/issues/257)**.
D8–D11 were adopted on their recommendations under the founder's 2026-08-14 delegation (*"You don't
need me for that"* / *"Go ahead team!!"*), governed by
[ADR-20260812-143619](../adr/ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md).

| Decision | Question | Recommendation |
|---|---|---|
| **PROP-032306 D5** | Menu ownership across Captain / HubRise / Uber, and per-channel price parity | **HubRise authoritative when connected, else Captain**, one-way push. Parity is the sharp edge: restaurants mark Uber prices up to absorb Uber's commission, and ADR-0024's comparison coefficients are calibrated on that — pushing Captain prices unchanged undercuts the restaurant *and* invalidates `basis: REAL` — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056 addendum; veto open; business CONFIRM: uplift preserved as a RATIO, push never defaults to overwrite, pinned by spec test) |
| **PROP-032306 D7** | Is the Provider entity on the signed Uber agreement (**Caring Hope Foundation**, RNA W372020229 — a loi-1901 association) the entity that will operate the platform? | **Needs legal input, not a recommendation.** An Uber API licence follows the entity; if the association holds it while another entity operates and earns commission, access sits outside the licence. Also interacts with the payout posture in §1 A — ✅ **decided by the customer 2026-08-08** ([ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md)): *"association (now) → SASU (operations, brand pending) → SCIC per area + federation, like CoopCycle"*; Connect onboarding waits for the SASU; the Uber agreement's entity questions become transfer-to-SASU questions in the counsel packet |

---

## 12. The batched send's signature — ✅ DECIDED 2026-08-02 (deferred)

⚠️ **Deferred — no `send_many` for now** (product owner, 2026-08-02). The actor client is built as
`PROP-20260728-152752` §2.1 always specified it: per-actor, `send` + `schedule`, one message at a
time. A batched send is revisited when a real use case asks for one.

---

## 13. Client isolation by crate — ✅ DECIDED 2026-08-02

✅ **DECIDED.** Actor clients live in their own crates so a caller links the clients it needs and
nothing more. Realized by [#290 "Actor-client crate split"](https://github.com/TheCaptainCompany/captain-food/issues/290);
the capability-allowlist gate that keeps it honest is §40.

---

## 14. Isolation by construction — PROP-20260802-130500 — ✅ DECIDED 2026-08-02

✅ **DECIDED**, D1–D6. Handler crates per actor (aggregates and process managers), domain types stay
one crate, adopted in phase 1; one generic `ActorClient` with `get_operation_status(message_id)`; a
`test-fixtures` cargo feature with a CI check that no release artifact enables it. **D6 went against
the recommendation** — it lands as its own change after phase 1. Design record:
[PROP-20260802-130500](PROP-20260802-130500-isolation-by-construction.md); §1 of it ranks the
compiler-first levels that [ADR-20260803-234035](../adr/ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md)
made binding. **ISO-3, the one row of its own audit table that never became work, is still open — §29.**

---

## 15. Push-driven mailbox — PROP-20260802-223522 — ✅ DECIDED 2026-08-02

✅ **DECIDED**, D1–D5, all as recommended: the mailbox is driven by push, and a poll is a declared,
observable, exit-having degraded mode. Generalised into the founder directive
[ADR-20260810-231300](../adr/ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md).
Design records: [PROP-20260802-223522](PROP-20260802-223522-push-driven-mailbox.md) ·
[PROP-20260802-200416](PROP-20260802-200416-push-driven-drain-loops.md).

---

## 16. Who owns the OVH host — PROP-20260805-181926 — ⚠️ SUPERSEDED 2026-08-08

⚠️ **SUPERSEDED** by §17: the question was who owns a single host's configuration, and the answer
became "no single host" — the substrate is Kubernetes on OVH MKS, operated GitOps-only over generated
manifests. D1–D7 were never answered and are not owed; the ownership question they asked is answered
by §17 D7. Design record: [PROP-20260805-181926](PROP-20260805-181926-host-provisioning-and-configuration-ownership.md).

---

## 17. Kubernetes as the deployment substrate — PROP-20260806-223656 — ✅ DECIDED 2026-08-07

✅ **DECIDED**, D1–D7. **OVH MKS** (*"MKS of course"*) · **in-cluster CloudNativePG** (*"Postgres on
Kubernetes"*), with the operability conditions as part of the answer — ≥3 instances, WAL archiving,
executed restore drills · ingress **yes**, with the correction that DNS is at Dynadot, which has no
cert-manager solver · **D6 went against the recommendation**: build the cluster now and cut over
once (*"I don't care about prod on Render and Supabase"*) · **GitOps** (*"Of course gitops"*) —
diagnostics via cluster and Postgres read access, fixes as repo changes. Records:
[ADR-20260807-002705](../adr/ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md) ·
[PROP-20260806-223656](PROP-20260806-223656-kubernetes-as-the-deployment-substrate.md) ·
[PROP-20260731-061609](PROP-20260731-061609-ovh-migration.md). The money and sizing consequences are
tracked as §35 **DB-HA**; the cutover itself is [#358](https://github.com/TheCaptainCompany/captain-food/issues/358).

---

## 18. One decomposition axis: spec folders, schemas, projectors — PROP-20260807-174246 — ✅ DECIDED 2026-08-07

✅ **DECIDED**, recorded as [ADR-20260807-183024](../adr/ADR-20260807-183024-one-decomposition-axis.md):
`specs/{scope}/{kind}.yaml`, `$ref`s stay kind-logical so moving an item between scopes rewrites no
refs, and `specs/common/` is the kernel. Its **D7** is the sentence the whole cutover window rests on
— *"start-clean makes the storage split free — the window that does not recur"*. Design record:
[PROP-20260807-174246](PROP-20260807-174246-one-decomposition-axis-specs-schemas-projectors.md).

---

## 19. Build in public — PROP-20260807-190936

✅ **FULLY DECIDED 2026-08-08.** **D1 went to the customer and came back a DIFFERENT choice —
radical transparency**: public accounting on Open Collective, public Kubernetes and technical usage,
public incidents and postmortems on GitHub, a public status page
([ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md)). Two guardrails stand
and compose with it: **transparency exposes INFORMATION, never CONTROL** — a generated, sanitized
public view *of* the cluster, never network reach *into* it — and **D2, platform-wide aggregates
only**, no per-restaurant / per-postcode / per-rider dimension without consent (a sole trader's
metrics are personal data, and a partner's published volume is an adoption killer), k ≥ 10 per cell
if slicing ever starts. **D3**: a static generated status page that **renders its own generation
timestamp and goes visibly stale** — a frozen "all green" during the outage that killed its publisher
is worse than no page. **D4**: levels L2–L4 after the cutover. Design record:
[PROP-20260807-190936](PROP-20260807-190936-build-in-public-transparency.md),
[#377](https://github.com/TheCaptainCompany/captain-food/issues/377).

---

## 20. The rider/delivery write surface — PROP-20260808-141817 — ✅ FULLY DECIDED 2026-08-08

✅ **FULLY DECIDED**, D1–D6. Two commands **retired** — no journey pushes a job at a partner (an
assignment no courier agreed to carry is the oversell failure mode as an event type), and a command
wrapping an external fact is an inbound ACL event, not a command (ADR-0004). The release step is
**generalized across both courier kinds**, one open issue per job in V0, a `PlaceOrder` payload flag
plus a PM step for atomic consume, and a **declared `sends:`** on the wrapper-seam receive. Design
record: [PROP-20260808-141817](PROP-20260808-141817-rider-delivery-write-surface.md) ·
[PROP-20260808-221424](PROP-20260808-221424-rider-delivery-slices-1-2-spec-diff.md).

---

## 21. Disappearance is a designed state — PROP-20260808-142532 — ✅ FULLY DECIDED 2026-08-08

✅ **FULLY DECIDED**, D1–D5. Money-history surfaces use projector- and event-carried composition with
a thin pinned dangling policy; the restaurant is **event-carried on `OrderPlaced`** so it survives a
projection rebuild after stream deletion; opt-out folds to a new **`OPTED_OUT`** value rather than a
tombstone (a tombstone is self-defeating under SIRENE re-import, and this closes the live cold-email
exposure); **both** write-side guards; and a parked "closed" page rather than a bare 404 or a
claim-landing fall-through. Design record:
[PROP-20260808-142532](PROP-20260808-142532-disappearance-terminal-states.md).

---

## 22. New rows from the 2026-08-08 sweep

Four rows of the nine are still live; the other five closed. ✅ **Closed**: the identity-bridge home
(**JWT claims**, [ADR-20260809-050000](../adr/ADR-20260809-050000-morning-brief-eight-decisions.md)
CARD-11 — **note that §46 IDENT-1 reverses its read-scope half**) · the PROP-185140 §6.4
**claim-staleness** policy ([ADR-20260810-194548](../adr/ADR-20260810-194548-six-decision-answer-sheet-claim-staleness-closed.md)) ·
the **`from:` naming collision** (the product owner picked (a), rename the screens input-source key) ·
**business-signal observability contracts**, closed by subsumption into §27 · the **D6 endpoint**
([ADR-20260809-002500](../adr/ADR-20260809-002500-quick-wins-approved-d6-dsl-extension-chosen.md)).

| Decision | Question | Status / owner |
|---|---|---|
| **Consumer-mediator registration** | France mandates médiation de la consommation registration before trading with consumers — a **launch precondition** that sat on no register row until now | ⏸️ **DEFERRED to first real order** (product owner, 2026-08-10) — the PO chose to register at the first real consumer order rather than now, **against the team's "start now" recommendation**. Recorded as the PO's decision. Still a tracked launch precondition (must complete before the first real consumer order clears); pairs with the entity/counsel packet |
| **Rebrand Captain → Solida** | Class-42 trademark opposition on "Solida" — external, only the customer/opposer resolves it; rename sweep pre-scoped in [#411 "Rebrand Captain → Solida (solida.food): rename sweep, BLOCKED on class-42 trademark confirmation"](https://github.com/TheCaptainCompany/captain-food/issues/411) | Waiting on external — **customer** ([ADR-20260808-212741](../adr/ADR-20260808-212741-solida-studio-strategic-frame.md) §4). **2026-08-10 — still PENDING**: the PO confirms the class-42 trademark is unresolved and **no company/entity name is chosen yet**, so [#411](https://github.com/TheCaptainCompany/captain-food/issues/411) stays blocked. "No entity name yet" **also gates the entity-path/rebrand work** (SASU naming per [ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md) ch. 4 — brand and entity land together) |
| **avelo37 partnership threshold** | At what orders-per-week does the avelo37 partnership conversation start — a number to set from real order data, not a guess | Open — needs the #400 order-volume contract; decision deferred by design ([ADR-20260808-212741](../adr/ADR-20260808-212741-solida-studio-strategic-frame.md) §1) |
| **Geocoding vs postal-code zones** (final-vision audit A6) | PROP-172500 D1 recorded "postal-code sets now, geocoding next — sequence it deliberately"; zones may BE the Tours final (river-crossing note) or geocoding needs an owner ("geocoding unlocks distance fees and honest ETAs — and the ETA is the product") | **Open — now TEAM-OWNED** (PO 2026-08-10, "Approve as recommended" on recommendation (c): team first, bring a proposal). No longer an unowned row waiting on a product-owner answer it never needed — the team owns the analysis and returns with a proposal |

---

## 23. Process-manager step-DSL conditional branching — PROP-20260809-003000 — ✅ FULLY DECIDED 2026-08-09

✅ **FULLY DECIDED**, D1–D7, confirmed as recommended by the customer's eight-decision answer sheet
([ADR-20260809-050000](../adr/ADR-20260809-050000-morning-brief-eight-decisions.md)). `match:` on an
enum discriminant — the only shape where *"is every case handled?"* is machine-answered — with **no
catch-all** (every member gets an arm; an intentionally empty one carries a `note:`, because a
catch-all is how a new member silently does nothing), a typed `from_resolver` returning a declared
enum, `present:`/`absent:` conditions, accepted duplication in v1, and a `derived_id:` value form
that slice 1 cannot retire the wrapper without. Design record:
[PROP-20260809-003000](PROP-20260809-003000-process-manager-step-dsl-conditional-branching.md).
Extended by [PROP-20260815-142349](PROP-20260815-142349-actor-answers-block-and-the-ask-step.md)
(§42 PMW-1).

---

## 24. The public demo — PROP-20260809-021351 — ⏸️ DEFERRED 2026-08-09

⏸️ **DEFERRED** (product owner, answer sheet). The demo is not next, and its production-critical
remainder was **re-filed on its own** rather than shipped under a marketing epic — the outcome two
lenses independently recommended. The three customer-owned rows were answered on the way out, so the
design is complete when it returns. Design record:
[PROP-20260809-021351](PROP-20260809-021351-public-demo-one-continuous-walk.md).

⚠️ **The gap table in that proposal was corrected 2026-08-12** and the correction is the part worth
carrying: G5, G6 and G7 are fixed; **C1 is only half fixed** (totals live on read, the competitor
comparison is still never computed); and **G7b, G8 and C2 are live** — G8 being *nobody is told about
a paid order*, with `crates/application/src/ports.rs` declaring four traits and zero notification
anything. That is the domain lens's worst failure mode, still open, tracked outside this register.

---

## 25. New rows from the 2026-08-10 #451 keystone adjudication

✅ **ALL THREE CLOSED 2026-08-10.** **451-A** closed by the
[#460](https://github.com/TheCaptainCompany/captain-food/pull/460) merge, standing position (a) held ·
**451-B** approved as recommended — the `currency_mismatch` reason joins the `cart-price` contract's
canonical reason set · **451-C** executed — [#451](https://github.com/TheCaptainCompany/captain-food/issues/451)
retitled. Record: [ADR-20260810-194548](../adr/ADR-20260810-194548-six-decision-answer-sheet-claim-staleness-closed.md).

---

## 26. New rows from the lifted `specs/**` freeze — 2026-08-10

✅ **BOTH CLOSED.** **SPEC-1** — the reporting gate that replaces the freeze is
[docs/SPEC-LOG.md](../SPEC-LOG.md): one sentence per landed spec change, in the same commit, saying
what the product now promises differently. It was chosen as the only option both readable by a
non-engineer and impossible to forget. **SPEC-2** — the `from:` rename stands as decided; a recorded
decision is not the team's to reverse. Record:
[ADR-20260810-221840](../adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md).

---

## 27. Business metrics for every feature and every persona — PROP-20260810-234225

Design records: [PROP-20260810-234225](PROP-20260810-234225-business-metrics-for-every-persona.md) ·
[#484](https://github.com/TheCaptainCompany/captain-food/issues/484) ·
**[ADR-20260811-014129](../adr/ADR-20260811-014129-a-business-metric-is-a-projection-and-every-reference-is-a-ref.md)**
(which supersedes [ADR-20260810-234225](../adr/ADR-20260810-234225-business-metrics-for-every-feature-and-every-persona.md)
**in part** — clauses 1–3 carried forward, clause 4 and the enforcement table reversed).

**The fact that earned the section**: `specs/observability.yaml` declared 29 `business_metrics` and
**26 had zero occurrences anywhere** in `crates/`, `tools/` or `deploy/` — the slot the directive
asked us to fill was already 90% fiction, and the gate that should have noticed covered 3 of 14
contracts and only checked that a string constant existed.

✅ **ALL ROWS CLOSED EXCEPT TRK-scope.** The design: a business metric **is a projection** — a
declared `fold:` over `domain_events` maintained by the `bam` projector, read through a
tenant-scoped GraphQL query, never a counter at a call site, because a fold **replays** and ratios
and distinct-identity denominators are inexpressible as counters. The unit is the **persona
ACTIVITY**, never the step. Attributes are **bounded declared populations**; ids belong on spans.
**MET-R** was a decision reversal the team filed rather than executed, and the product owner
confirmed it (*"Confirm the reversal, go with the projections"*) — what decided it was not deference
but that the earlier instrument design **forfeited replay by construction**. **MET-T** made
*"no strings in the spec"* precise as three categories. **MET-S dissolved** — a grain error, not a
missing field, so the versioning story was withdrawn. **MET-G**: the `DbFaultPolicy` default **flips
to `Halt`** — the team recommended quarantine first and was **overruled**
([ADR-20260811-105024](../adr/ADR-20260811-105024-projection-halt-default-and-health-visibility.md)),
with **MET-G2** recorded as a known accepted consequence: `ScopeMembership` is the single read-side
authorization index and is a projection, so a halted group freezes **revocations**. **MET-Q7 / Q7**:
no hosted analytics SDK, ours and server-side, and behavioural data lives in a database separate
from the business data. **COOP**: all three cooperative properties designed into the first slice.
**MET-W**: a named catalog of approved retention windows, `$ref`'d. **TRK-ISO**: behaviour tracking
gets its own database **and** its own projector worker. **HEALTH-2**: a faulted worker reports
unhealthy and is **not** restarted — with two edges recorded rather than discovered later, that
*readiness*, not liveness, is the probe (**HEALTH-2a**: a literal reading would take the storefront
down, because the monolith runs API and projector in one process), and that **actor-mailbox workers
are excluded** (**HEALTH-2b**: they already quarantine, so halting them would turn a parked message
into a stopped order lane). The principle: **halt is right where there is no quarantine; quarantine
is better wherever it exists.**

| # | Decision | Recommendation |
|---|---|---|
| **TRK-scope** | ⬅️ **OPEN — with legal, not with the product owner.** Product owner: *"we don't care the person info, just the behaviour"*, and the substantive part — *"using a generated identifier uncorrelated to the person so we will have the tracking without the need to know the person is doing what but a persona."* Plus a clarification that **changes an earlier legal finding**: the "help AI agents" sentence was **internal**, explaining to the team why the data is wanted — **not** a user-facing personalisation feature | **Do not design against this yet.** Legal is working out whether a pseudonymous journey identifier fits the **audience-measurement exemption**, or whether per-journey continuity exceeds it. **The mechanical half, thought about but NOT committed**: if the answer is *"lawful provided the join never happens"*, then **"never joined" must be structural rather than promised** — a pseudonym and a customer id in one database with correlatable timestamps is not a guarantee. Candidate constructions, compiler-first: the **separate database is already decided** (MET-Q7) and does most of the work; **no foreign key** to any business table; **no shared column name** the validator would refuse; and an **`identifierClass` that cannot be `CUSTOMER`** for an anonymous-funnel event — i.e. the join is *unspellable in the DSL* rather than *forbidden in review*. Note this pulls against D8 option A (authenticated, `identifierClass: CUSTOMER`), so the two are alternatives per event kind, not a single answer. **The proposals are deliberately NOT amended until legal reports** |

---

## 28. Behaviour event tracking declared inside the screens spec — PROP-20260811-000946

✅ **ALL ROWS CLOSED.** Design record:
[PROP-20260811-000946](PROP-20260811-000946-behaviour-event-tracking-in-the-screens-spec.md),
[#485](https://github.com/TheCaptainCompany/captain-food/issues/485).

**The fact that earned it** is not the absence of tracking, which is expected — it is that
`SetCustomerPreferences.dietaryTags` was `array<Tag>` with `Tag` a free-form string, `maxLength: 80`,
no enum, persisted to `View_Customer.preferences` jsonb: **`halal` and `kosher` were spellable values
today**, no screen bound it, and no review caught it because no artifact existed that would make
anyone look.

The design: a root `specs/behaviour_events.yaml` holds the definition so a DPIA reads one file, with
a `tracking:` `$ref` binding in the screens; events are **authored, not derived**; `kind:` is
**`VIEW | INTERACTION` and nothing else** — `IMPRESSION` and session replay are absent from the
grammar, not merely unused; separate catalogs share exactly one thing, the `activity:` `$ref` into
`stories.yaml`; the store is **its own database, RANGE-partitioned by day, retention is a partition
DROP**, and **never `domain_events`**; ten ERROR rules, of which **R5 `tracking-on-sensitive-node`**
is load-bearing. Sequencing: **the mechanism plus ZERO live events**, then the DPIA, then the first
three events — instrumentation before a DPIA is the wrong order, and validator rule R10 makes that
sequencing a build failure rather than a promise. **Q1** closed as authenticated **server-side only**
— no new client identifier and no analytics read of the existing one; **Q2** closed as yes in
principle, built after the DPIA. Both by
[ADR-20260812-214021](../adr/ADR-20260812-214021-the-founder-answer-sheet-of-2026-08-12.md).
**D10** records that the founder's own tracking design and the team's **converged independently**.

---

## 29. Scope isolation is nominal — PROP-20260811-090000 (product-owner directive, 2026-08-11)

Product-owner directive, verbatim: *"The enforcement is required before working on any other
functional subject"*. Design record:
[PROP-20260811-090000](PROP-20260811-090000-scope-isolation-runtime-decomposition.md),
[#423](https://github.com/TheCaptainCompany/captain-food/issues/423).

✅ **ISO-1 and ISO-2 are CLOSED as (a)** by the product-owner direction of 2026-08-11 (*"The
infrastructure has to be split in multiple crates to be able to regulate permissions based on what
they need nothing more"*), recorded in §5 and worked through in §33: both (b) options ended with a
bin linking a crate that carries every other boundary's code, which is exactly what the directive
forbids. **ISO-3 is still open**, and it is the sharpest row here — the point of it is that it
stopped being a *decision* and became a *silence*.

| # | Decision | Options & the trade-off | Recommendation / status |
|---|---|---|---|
| **ISO-3** | ⏳ **STILL OPEN — and now cheaper.** **`EventStore::append` has no capability witness** and is untracked — the one row in PROP-20260802-130500 §5's audit table marked "❌ hole (phase-3 territory)" that nobody has filed. `crates/application/src/ports.rs:50-60`: anyone holding `Arc<dyn EventStore>` may append any event to any stream. The mailbox got `MailboxAccess` ([ADR-20260803-172654](../adr/ADR-20260803-172654-mailbox-port-demands-a-capability-witness.md)); the event log — the actual system of record — did not | **(a) File and cost it now, as part of the enforcement program** the directive just prioritised: it is the same class and arguably the more consequential door. **(b) Ride slice 3 / [#307](https://github.com/TheCaptainCompany/captain-food/issues/307)** — it is genuinely adjacent to per-actor handler crates, so folding it in avoids touching `ports.rs` twice. **(c) Leave it** — the current position, which is an unrecorded acceptance rather than a decision | ✅ **Recommended: (a) file now, sequence with (b).** The point of this row is that it stopped being a *decision* and became a *silence*: the audit table names it, no issue tracks it, and the directive says this class comes first. **2026-08-11 update**: [PROP-20260811-173223](PROP-20260811-173223-repository-crates-and-the-infrastructure-split.md) D1 splits `EventStore` into `EventStreamReader` + `EventStore: EventStreamReader` in its slice 1 — the witness rides the **same signature edit** instead of touching `ports.rs` twice, which is option (b)'s efficiency without waiting for slice 3. The crate split **does not subsume it** (D6): the crate boundary is per-*boundary* ("which apps may hold an appending type at all"), the witness is per-*aggregate* ("which streams that type may append to"), and under BND-1(a) `handlers-order` holds six aggregates |

---

## 30. One folder per app — PROP-20260811-141654 (product-owner request, 2026-08-11)

✅ **ALL THREE ROWS CLOSED** (recommended options, team-owned by delegation). Design record:
[PROP-20260811-141654](PROP-20260811-141654-per-app-declaration-folders.md),
[#491](https://github.com/TheCaptainCompany/captain-food/issues/491).

**The headline is a "no" inside a "yes"**: the app list already exists as source
(`specs/architecture/c4-l2.yaml` `containers:`), and a per-app folder **cannot** make a scope
boundary real — only the crate graph does, which is §29's job. What the folder **can** carry is the
per-app knowledge currently written in Rust inside the generator, and the **grants** — the measured
finding being that `adapter-stripe`, the pod whose stated reason to exist is *"holds ONLY this
partner's secrets"*, carried **13** secrets including `AUTH_SESSION_KEY`, `SUPABASE_SECRET_KEY` and
the four `OVH_*` SMS credentials, while `gateway-public` (*"no DB access, no logic, no state"*)
carried **10**. The narrowing mechanism exists and works — `worker-erasure` carries **2** — it was
applied to one family. **APP-3 is independent of §29**: that fixes a code boundary, this fixes a
credential boundary; neither blocks the other.

---

## 31. The domain boundaries — PROP-20260811-150242 (product-owner proposal, 2026-08-11) · **upstream of §29**

✅ **ALL NINE ROWS CLOSED.** Design record:
[PROP-20260811-150242](PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md),
[#493](https://github.com/TheCaptainCompany/captain-food/issues/493).

**BND-1** — the recorded boundary set is **five business boundaries + `platform` + the kernel**
(§1 H), verbatim *"I'm ok for the 5 / Customer / Order / Catalog / Restaurant / Delivery"*, with
**BND-2** naming the fifth `delivery` rather than `rider`. The finding underneath: `boundedContexts:`
in `c4-l2.yaml` and `specs/{scope}/` were **two different partitions of the same 20 actors**, 6 of 20
homed differently, with nothing reconciling them — they had diverged since the 2026-08-07 reorg with
every gate green. The fix was nearly free: the context partition is a strict coarsening of the scope
partition on 7 of 8 scopes; exactly one scope splits, on exactly one member. One part of the request
was **declined with measured reasons**: `catalog`↔`network` coupling is **zero of every kind**, so
folding catalog into restaurant internalizes nothing and deletes a compiler-enforced boundary.

**BND-3** storage deliberately does **not** follow the boundary one-to-one, with a stop condition
that should be a validator rule · **BND-4** the write side confirmed and the two missing rows added
(read literally as a `GRANT`, the original matrix made **every mutation fail at runtime**) ·
**BND-5** notification policy stays in `order`, **refined into three parts** — the recipient contract
is restaurant-boundary data and was absent entirely · **BND-8** the `CONNECT` classification adopted
with three mechanical brakes · **BND-9** the premise fixed as a correctness matter.

**BND-6 and BND-7 are the ETA rows, both answered by the founder 2026-08-12**
([ADR-20260812-214021](../adr/ADR-20260812-214021-the-founder-answer-sheet-of-2026-08-12.md)):
**(B)** what the customer sees pre-order is **prep time only, labelled "ready"**, and **(A)** the
frozen ETA is an **estimate with no remedy**. The design behind them, worth keeping because it named
a mechanism the architecture was missing: **the ETA is a READ-SIDE COMPOSITION owned by `order`,
frozen onto `OrderPlaced` at checkout — not a projection and not a process manager.** Young's fold
rule kills the projection answer outright, since the estimate depends on *now* and a replay cannot
reproduce it. That makes a **read-time query contract** the THIRD sanctioned cross-boundary
mechanism, beside the projection fold and the PM bridge. **D14** states a property nobody had written
down: **ONE event log, boundaries write-isolated and read-shared on it** — two projection groups fold
across boundaries on the global `position`, so a per-boundary log would break replay determinism.

⚠️ **The ETA is still not computed anywhere.** CLAUDE.md's lens opens with *"The ETA is the
product"*, and at the time of the audit nothing computed one, no pre-order estimate existed at all,
and **two shipped surfaces already promised one** — an `eta_bar` labelled *"Estimated arrival"* bound
to the kitchen ready time, and a `delivery_time_asc` sort option over a query with no sort argument.
A wrong ETA outranks a missing one. Tracked outside this register as screen-spec defects.

---

## 32. Storage boundaries and least-privilege database users — PROP-20260811-093000

Design record: [PROP-20260811-093000](PROP-20260811-093000-storage-boundaries-and-least-privilege-database-users.md),
[#494](https://github.com/TheCaptainCompany/captain-food/issues/494). §31 decides **which units
exist**; this section decides **what shares a recovery posture and a database role**.

**Five databases plus a per-app least-privilege user, priced and accepted as the strong default** —
with the one thing the directive had to change stated plainly: **`DomainEventLogDb` cannot hold the
log alone**, because `crates/actor_runtime/src/completion.rs` commits the appends, the PM state, the
`inbound_messages` flip and the fenced `mailbox_partitions` advance in ONE transaction. Separating
log from mailbox does not weaken atomicity — it **deletes the fencing token**.

Two defects fell out that are neither about the split nor decisions: the erasure engine **fails
OPEN** in any database holding zero `projection_checkpoint` rows (exactly the database the split
creates), and the five remaining `View_*` declared **8 secondary indexes emitted nowhere**, so the
rider job board folded the whole log on every poll.

✅ **Closed rows.** **STO-1** (a), with a rename so the contents are not surprising — the hardest
constraint in the proposal · **STO-2** (a) for `ScopeMembership` and the `ref_*` tables —
*"composition happens in the projector, not the query"*, applied to authorization; **closed
2026-08-14, the remainder being a 17-table placement map now held in the proposal** · **STO-3** (a)
now, (c) when tracking ships, on a concrete written trigger · **STO-4** (a), with its **sequencing
WITHDRAWN and re-targeted** by the DBA lens rather than silently edited — the pooler is a
precondition of the bin-fleet flip, not of the split · **STO-5** (a), with RLS **gated and
benchmarked** (≥ 200 appends/s with and without) before its default flips · **STO-6** (a), with the
caveat said out loud that CNPG's `barmanObjectStore` backup is **physical**, so a single database
cannot be restored from it in isolation · **JRN-1** resolved as (b), its founder-owed leg answered
2026-08-12 — take the flip inside the empty-log window with the L4 smoke as the release gate, so
`command_journal` is DROPPED and the gate deleted rather than defaulted ON · **ADP-1** fully decided
2026-08-12, leg 1 (a) and leg 2 (b), on the one-app test every row of the set passes.

⚠️ **Four rows are still open**, and three of them **must be decided before or with the physical
split**. **STO-9 came back into scope on 2026-08-18** with the RLS-SEQ ruling (§46).

| # | Decision | Options & the trade-off | Recommendation / status |
|---|---|---|---|
| **STO-7** ⚠️ **OPEN — must be decided BEFORE OR WITH the physical split of `read_order`/`read_catalog`** (raised 2026-08-14 by the STO-2 closure's mob checkpoint; **WIDENED the same day by the post-ready independent review**, which found the CHECKOUT WRITE PATH crossing the same wall — a path option (a) does not reach) | **Who is the catalog's pricing-and-orderability AUTHORITY once the walls are physical?** TWO paths cross this wall, and a decision answering one while leaving the other fail-closed is not a decision. **(i) The cart READ path** (`read_order` → `read_catalog`): `Cart` is a deliberately MONEY-FREE fold (ADR-20260810-112836) — names and prices resolve AT QUERY TIME via `price_cart` against the LIVE `Catalog` projection, which the STO-2 closure places in `read_catalog` while `Cart` sits in `read_order`. Post-split as mapped, the order boundary's read path holds no CONNECT on `read_catalog`, so the cart screen cannot resolve names or prices at all — the [#424 "post-hoc UX pass found the built checkout state could not render"](https://github.com/TheCaptainCompany/captain-food/issues/424) defect class, on the CHECKOUT path, at Friday peak. **(ii) The checkout WRITE path** (`captain_write` → `read_catalog`): the mailbox worker's GENERATED composition root declares `CommandDeps.catalogs: CatalogReadRepository` (`crates/infrastructure/src/generated/command_router.rs`), backing `require_orderable_line` (`commands.rs:796,1006` — the availability+stock **OVERSELL GUARD** on every add-to-cart, fail-closed `OfferNotFound`/`OfferUnavailable`/`InsufficientStock`), `pricing::price_cart` from `place_order` (`commands.rs:2458` → `pricing.rs:56` — the `ServerPriceAuthority` repricing on every checkout, fail-closed `PriceUnresolvable`) and `configure_catalog_slug`'s `slug_taken` (`:2799`). Post-split as mapped, **every add-to-cart and every checkout dies the moment the wall becomes physical, and the oversell guard never runs** — losing both sides of the marketplace at once. (i) was found at the mob checkpoint and (ii) only after ready, neither by the closure's own hand-enumeration of readers (which also missed `Restaurant.cuisine_category` twice) — the empirical case for the declared-reads⇒CONNECT follow-up | **(a) Fold-local price snapshot** — the Cart projector folds names/prices into its own database. No cross-wall read, replay-correct; but it REVERSES the recorded money-free-cart decision — stale prices are exactly what ADR-20260810-112836 removed, and the fold would re-couple to catalog events. **AND, post-widening, it is INCOMPLETE ON ITS OWN**: it repairs the cart SCREEN in `read_order` and does nothing for `captain_write`, so taking (a) alone would unblock the physical split while add-to-cart and checkout still fail closed — precisely the trap this widening exists to prevent. **(b) A catalog-boundary pricing/orderability PORT** — D13's third sanctioned cross-boundary mechanism (a read-time query contract): the asking side (the order read path AND the mailbox worker) asks the catalog boundary to price and validate the lines. No copy, no staleness, `Cart` stays money-free, and it is the only option that answers both paths with ONE authority — the shared authority (b) previously only gestured at, now explicit. Cost: a synchronous cross-boundary hop on the cart read at peak (the cost BND-9 declined for a PM — a cart READ is not inside the checkout saga, but `place_order`'s repricing IS, so the write leg pays exactly the cost BND-9 refused, and that trade must be made with eyes open). **(c) Replicate `Catalog` into every read database** — mechanically available (the STO-2(a) grammar); destroys the isolation the split buys (catalog-import bursts land in `read_order`'s buffer pool, the exact head-of-line coupling `OrderTracking`'s placement exists to prevent) and **does not reach `captain_write` at all**, since replication targets `recovery: replay` databases only. **(d) A recorded cross-wall CONNECT grant for the write app** — zero design work, honest about today's code; and it hands the write role CONNECT on a read database on the hottest path, i.e. removes the wall the split is bought for (BND-9: *"two exceptions on the two hottest paths is not an exception list, it is the design"*) | ⚠️ **OPEN, deliberately NOT resolved by the STO-2 closure** — a placement slice must not silently decide a pricing-architecture question. The ux lens LEANS **(b)** ((a) reverses a recorded decision and is now known incomplete, (c) re-imports the coupling and misses the write path, (d) removes the wall) — recorded as a lean, not a decision. **The widening adds one binding constraint before any option is chosen: whatever is decided must cover BOTH the cart read path and the checkout write path, and a decision recorded for only one of them does not unblock the split.** Until decided: OPEN comments on `Cart`'s AND `Catalog`'s declarations (`projection_tables.yaml`) plus `read_catalog`'s (`databases.yaml`), and the physical split of `read_order`/`read_catalog` is BLOCKED on this row |
| **STO-8** ⚠️ **OPEN — must be decided BEFORE OR WITH the physical split of `read_common`** (raised 2026-08-14 by the post-ready independent review of the STO-2 closure) | **May a `captain_write` app read `read_common`, or does each write-side invariant get a source inside its own wall?** Same DIRECTION as STO-7(ii) (`captain_write` → a read database), different target and a genuinely different option space — which is why this is its own row rather than a leg of STO-7. Nine handlers use three read ports into `read_common` (`CommandDeps.customers` / `.restaurants` / `.prospection`). **Head of the row, and why it is not "lower stakes": `verify_phone` `by_phone` (`commands.rs:3257`) resolves NEW-vs-RETURNING on the LOGIN PATH** from the `Customer` projection. Post-split as mapped it fails — or, if that read were ever degraded to a "not found" fallback, it SILENTLY RE-REGISTERS a returning customer as new: a second `Customer` stream, an orphaned order history, and a customer who signs in to an empty account. Riding the same wall: `request_email_verification` / `request_phone_change` / `confirm_phone_change` (`:3363`, `:3414`, `:3453` — the `EmailAlreadyInUse`/`PhoneAlreadyInUse` identity-collision guards); `Restaurant` reads by `create_catalog` / `add_product` / `update_product` / `mark_restaurant_as_favorite` (`:2763`, `:2737`, `:3491` — `RestaurantNotFound` and the ADR-0016 `CurrencyMismatch` guard); and `ProspectionPipeline.last_contacted_at` by `record_prospect_contact` (`:2626` — the ≥ 7-day B2B anti-spam interval, whose ONLY possible source is the projection, because the contact TIME is envelope metadata invisible to the fold). **What forecloses the cheap answer: `Customer` cannot simply move to `captain_write`** — it is a genuine read model with subgraph readers (`specs/customer/api.yaml:15`, the `me` query), so moving it would park a replay-rebuildable projection on the `pitr` write database AND hand the customer subgraph CONNECT on `captain_write` | **(a) A fold-local write-side index per invariant** — the write side maintains its OWN minimal lookup inside `captain_write`, folded from the same events (`phone`/`email` → `customerId` from `CustomerRegistered`/`CustomerPhoneChanged`/`CustomerEmailVerified`; `restaurantId` → `{exists, default_currency}` from `RestaurantRegistered`/`RestaurantUpdated`; `restaurantId` → `last_contacted_at` from `ProspectContacted`). No cross-wall CONNECT, replay-correct, and the SAME shape this closure already chose for `Restaurant.cuisine_category` — *composition happens in the projector*, STO-2(a) applied to the write side. For the identity guards it is strictly stronger than what exists today: a uniqueness invariant read from an EVENTUALLY-CONSISTENT projection is already a race (two concurrent `VerifyPhone`s on one phone can both read "not found"), whereas a `captain_write`-local index can carry a UNIQUE constraint in the same transaction as the append — compiler-first applied to storage, an invariant made unspellable rather than checked. Cost: several folds plus a migration — the final-vision answer, not the cheap one. **(b) A recorded cross-wall CONNECT grant** — `captain_write` apps hold `CONNECT`+`SELECT` on `read_common`, emitted from a declaration folder with a named owner and a removal condition ([#491](https://github.com/TheCaptainCompany/captain-food/issues/491)/REP-5(a)'s exception mechanism). Zero design work; but the grant sits on the LOGIN path and the wall stops being a wall for the app class it most needs to exclude. **(c) A customer-boundary identity PORT** — STO-7(b)'s shape applied here; consistent if (b)-of-STO-7 is chosen, but it puts a synchronous cross-boundary hop on sign-in and does not fix the uniqueness race, it relocates it | ⚠️ **OPEN, deliberately NOT resolved by the STO-2 closure** — same discipline as STO-7: a placement slice must not silently decide an identity-architecture question. **Lean, recorded as a lean and not a decision: (a)**, on two grounds — it is the class this register already chose for the identical cross-wall shape (`Restaurant.cuisine_category`), and for the identity guards it is strictly BETTER than today rather than merely wall-compatible, because a `captain_write`-local unique index closes a duplicate-identity race the current projection read cannot. **The `ProspectionPipeline` leg may be settled separately and cheaply** (B2B outreach; no money, no customer-visible failure) but is recorded here rather than in a row of its own because it is the SAME wall with the SAME option set — a third row would fragment one decision. Until decided: OPEN comments on the `Customer`, `Restaurant` and `ProspectionPipeline` declarations (`projection_tables.yaml`) and on `read_common` (`databases.yaml`), and the physical split of `read_common` is BLOCKED on this row |
| **STO-9** ⚠️ **OPEN — must be decided BEFORE OR WITH the physical split of `read_order`, INDEPENDENTLY of STO-7** (raised 2026-08-14 by ROUND 2 of the post-ready independent review of the STO-2 closure) | **May a `captain_write` app read `read_order` — and what settles a Stripe capture when it cannot?** The THIRD wall direction (`captain_write` → `read_order`), and the one that touches money directly. Four write-side process managers read the order boundary's read models while running on the mailbox worker, through `OrderReadRepository` (`SELECT ... FROM ordertracking`, `crates/infrastructure/src/persistence/order.rs:28`) and `CartReadRepository`. **Head of the row: `SettlementHooks` reads `OrderTracking` for `payment_intent_id` on ALL FOUR legs, immediately BEFORE every Stripe capture and every release** (`crates/application/src/process_managers/payment_settlement.rs:53-99`; its own doc comment already says *"the intent of the row `read_order` admitted"*, so the code knew it crossed the wall before the map did). Post-split as mapped that read ERRORS rather than skipping — the `HookOutcome::Skip` arm covers a genuinely absent row, not an unreachable table — so the settlement lane stalls in mailbox retry: **capture-at-DELIVERED never runs and the authorization ages out (food delivered, money never collected — the worst-failure class CLAUDE.md names), and release-on-reject never runs, so a rejected customer's hold stays held**. Riding the same wall at lower stakes: `DispatchOpenHooks.orders` (`delivery_dispatch.rs:83`, the `OrderMarkedReady` birth leg), `on_reclamation_resolved`'s `orders` (`reclamation.rs:91`), and `CartBindingHooks.carts::open_by_session` (`cart_binding.rs:17-37`) — whose failure loses a guest's cart at the moment he signs in to pay. **Why its own row and not a leg of STO-8**: same DIRECTION, different target database and a different option set — the settlement datum is a payment fact the write side itself authored, which is exactly what makes the cheap answer here different from `read_common`'s identity guards | **(a) Fold the datum into `captain_write`** — a write-side `order → payment_intent_id` (+ status) index folded from the payment events the write side already appends; no cross-wall CONNECT, replay-correct, and the same class the register chose for `Restaurant.cuisine_category` and leans to in STO-8. Strongest version of the argument: settlement reads a PAYMENT fact out of an ORDER read model built for customer tracking — the read model was never the right source, so this is a correction rather than a workaround. Cost: one fold + a migration, per PM datum (dispatch and reclamation need their own, and the cart-binding leg needs a session→open-carts index, so it is four folds, not one). **(b) A recorded cross-wall CONNECT grant** for the mailbox worker on `read_order` — zero design work, honest about today's code, and it puts the write role on a read database for the settlement path, i.e. removes the wall on the money lane (BND-9's objection, in its sharpest form). **(c) An order-boundary read PORT** (STO-7(b)'s shape applied here) — consistent if STO-7 goes that way; but it puts a synchronous cross-boundary hop INSIDE the settlement saga, between the delivery fact and the capture, adding a failure mode to the money path to remove a grant. **(d) Carry the datum on the trigger events** (`OrderDelivered` & co. gain `paymentIntentId`) — no read at all, the PM becomes pure; but these events are **already emitted and stored**, so it is an event-shape MIGRATION with an upcasting story (CLAUDE.md question 2), not a spec edit, and it widens payment identifiers across every consumer of an order event. **(e) ASK THE ACTOR — added 2026-08-15 by the founder's write-side-PM directive ([ADR-20260815-030206](../adr/ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md), §42 below), and this row never enumerated it.** The PM folds the aggregate streams in-process through the `EventStore` port it already holds: **zero new tables** — `domain_events` is already inside `captain_write`, `OrderPlaced` already carries `paymentIntentId`, and `domain::payment::fold` already exists and returns a TYPED `PaymentStatus` where the projection hands back a `String`. Cheapest closure sequence (dba): fold `payment_intent_id` onto `OrderState` from the `OrderPlaced` the Order aggregate already owns — no event migration, one fold field — then the leg is *ask Order → intent, ask Payment → status*, two by-key folds on terminating streams. Under **final-vision-first** ([ADR-20260808-235113](../adr/ADR-20260808-235113-final-vision-first-no-intermediate-steps.md)) it **displaces the recorded lean (a)**, because (a) builds a **new write-side index folded from the log for query** — that is a projection wearing a write-side badge, and it would be the second authority on payment state alongside `domain::payment::fold` (the exact defect this row's own CRITICAL-1 history is made of). Cost: 2 folds where there was 1 read on the money path, and the residency cache does not serve cross-aggregate loads today (§42 PMW-2) | ⚠️ **OPEN, deliberately NOT resolved by the STO-2 closure** — the placement of `OrderTracking`/`Cart` is unchanged and correct; what is open is which mechanism feeds the write side once the wall is physical. **Lean, recorded as a lean and not a decision: (a)** — ⚠️ **the lean is DISPLACED by (e) as of 2026-08-15, on the founder's write-side-PM directive**; the original reasoning stands and now argues for (e) rather than for (a), on the register's own precedent (fold-local is the class already chosen for the identical shape) and on the observation that a settlement PM reading a customer-facing tracking projection is a source-of-truth mismatch independent of any wall — (e) is the same argument taken one step further, to the stream instead of to a second index over it. **Closing this row is the ONE grant this rule buys**: all of `read_order`'s `captain_write` readers are PM legs, so `captain_write` drops CONNECT on `read_order` entirely — while STO-7 and STO-8 are untouched, because their readers are aggregate command handlers, not process managers (§42). **BINDING CONSTRAINT, the reason this row exists separately: deciding STO-7 and STO-8 — even both, even well — does NOT unblock the physical split of `read_order` while this row is open**, because settlement's pre-capture read still fail-closes. Until decided: OPEN comments on the `OrderTracking` and `Cart` declarations (`projection_tables.yaml`) and on `read_order` (`databases.yaml`) |
| **STO-10** ⏸️ **PARKED 2026-08-17 BY FOUNDER ANSWER — until the walk lands** (§45 **STO-10-PARK**; the walk now runs on ONE database per §45 **SEQ-1**, so the split band this row belongs to is not on the path to the first reading). It stays **OPEN and BLOCKED, and is reported blocked** — never re-ranked to look dispatchable — and the standing prohibition below is unaffected: **#513 must not emit the CONNECT that would decide option (c) by default.** ⚠️ **OPEN — and it is NOT an open question of STO-7/STO-8's kind: it CONTRADICTS A CLOSED ROW.** The HubRise adapter already reads `read_common` in shipped code, which **reopens ADP-1** (decided 2026-08-12 BY DIRECTIVE, this table above). Under CLAUDE.md's question (1) — *does it contradict a recorded decision?* — that makes it AMBER: founder-owned, not a team call, and it is filed as its own row rather than absorbed into STO-7 or STO-8 precisely so the reopening is visible | **The HubRise adapter bin reads `read_common` today, and ADP-1 says it holds exactly ONE outward grant.** ADP-1 ([ADR-20260812-115930](../adr/ADR-20260812-115930-each-adapter-owns-its-own-completely-isolated-database.md)): each adapter owns a completely isolated database, and its one outward grant is INSERT into `inbound_messages`. A CONNECT+SELECT on `read_common` is a SECOND outward grant. The code: `crates/adapters/hubrise/src/main.rs:54` constructs its own `PgRestaurantRepository`, and `connect.rs:314-325`'s `await_restaurant_projection` polls `restaurants.by_id` **40× at 250 ms** before `create_catalog`, because that command's `RestaurantNotFound` guard reads the projection. This is also a FIFTH reader class — not a resolver, not `CommandDeps`, not a PM hook port, not gateway middleware, but a repository **the bin constructs for itself** — which is why no formula caught it. Post-split as mapped, HubRise onboarding stalls **silently, as a ten-second timeout on a poll that can never resolve**. **Adjacent violation, recorded with it**: that poll also breaches [ADR-20260810-231300](../adr/ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md) — the projector KNOWS it folded `RestaurantRegistered` before the clock does, so this is state-change PROPAGATION and must be pushed; as written it is a poll with no declared degraded mode, no observability contract and no detected path back, i.e. none of the three conditions that make a fallback legitimate. **Recorded alongside, for [#513](https://github.com/TheCaptainCompany/captain-food/issues/513) rather than for this row**: with `RUN_MAILBOX_WORKERS` on, the same bin spawns standalone mailbox workers for `RestaurantAccount`/`Restaurant`/`Catalog` (`main.rs:43-52`), so the adapter pod ALSO hosts the `CommandDeps` reads of STO-7(ii)/STO-8 — the app↔database CONNECT model cannot assume one app class per bin | **(a) Delete the wait; let the durable path do the retrying** — `create_catalog`'s own `RestaurantNotFound` guard is the real gate, and the adapter enqueues on `inbound_messages`, so the mailbox ALREADY provides the retry this loop hand-rolls in-process. Needs no new grant and no new channel; it is the only option that leaves ADP-1 standing exactly as decided, and it deletes a poll rather than dressing one up. **(b) Make the visibility a PUSH** — the adapter learns of projection visibility from a signal instead of polling (ADR-20260810-231300's primary transport); correct in principle, but it invents a channel from a read database's projector to an adapter, which is coupling ADP-1 removes. **(c) Grant the adapter CONNECT + SELECT on `read_common`** — zero design work, matches today's code; it is a **DECISION REVERSAL of a founder directive**, so it cannot be taken by the team and must never be quietly emitted by #513's grant emitter as a fait accompli | ⚠️ **OPEN / AMBER — founder-owned, because whichever way it goes, ADP-1 is the row being answered.** Team lean, recorded as a lean and not a decision: **(a)**, with (b) as the shape if a signal is wanted later — the retry the poll hand-rolls already exists durably, so (a) both honours ADP-1 and removes an undeclared poll, and it is the only option needing no new grant. **What must NOT happen in the meantime**: #513 emitting the CONNECT that makes (c) true by default — an isolation directive reversed by a grant emitter nobody read is exactly the failure this register exists to prevent. Until decided: OPEN comments on the `Restaurant` declaration (`projection_tables.yaml`) and on `read_common` (`databases.yaml`) |

---

## 33. Repository crates and the dissolution of `infrastructure` — PROP-20260811-173223 (product-owner direction, 2026-08-11) · **closes ISO-1 and ISO-2**

Design record: [PROP-20260811-173223](PROP-20260811-173223-repository-crates-and-the-infrastructure-split.md),
[#497](https://github.com/TheCaptainCompany/captain-food/issues/497). The third product-owner message
of that day and the third face of one idea — §31 decides *which units exist*, §32 *what shares a
recovery posture and a database role*, §33 *what a unit may link*.

✅ **REP-2 … REP-5 closed** on their recommendations, team-owned. The cost is honest: ~28 net-new
crates on a workspace near 90, mitigated by each being small and independently rebuildable. **REP-5**
restates the ranking so it is not lost: **the crate graph is load-bearing for the stated threat
model**, because a `GRANT` is invisible to the compiler and a dependency edge is not.

**The blocker nobody had named, recorded as REP-4**: `DomainEvent` is ONE enum over all 8 scopes,
defined in the facade and named by `EventStore` and the projector `Envelope` — so a per-boundary
repository crate that traffics in it re-imports everything, and until REP-4 lands the split delivers
module hygiene only. It is **not** an event-versioning question: storage is already
`(event_type TEXT, payload jsonb)`, so no stored contract moves.

**The one phrase that needed refining, with the code arguing the refinement** — *"the write
repositories generally inherit from the read repositories"* is right on the **log** (an actor cannot
decide without loading its own stream, so `EventStore: EventStreamReader` is a supertrait) and
**wrong on the read model**, because there are TWO read contracts: a **query** port (narrowed,
GraphQL-shaped, where `by_id` returns `None` for a CHECKED_OUT cart) and a **row-state** port
(unfiltered `load(id)`). The projection write repo inherits the row-state one; supertraiting it onto
the query port is over-privilege **and** a correctness bug.

**REP-1 stays as a confirm-or-redirect, not a blocker** — it modifies a shape the product owner
named, under *"just keep me informed"*.

| # | Decision | Options & the trade-off | Recommendation / status |
|---|---|---|---|
| **REP-1** | **The "inherit" refinement.** Recorded as a **confirm-or-redirect**, not a blocker, because it modifies a shape the product owner named — under *"just keep me informed"* ([ADR-20260810-221840](../adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md)) | **(a) Two read contracts; the projection write repo supertraits the ROW-STATE port only** — "write inherits read" stays literally true and the inherited method is exactly the one the direction describes (*"projectors … to know the current state of the rows to update them"*), while the 5-method GraphQL query surface stays out of the projector's reach. **(b) One supertrait `Write: Read` over the query port** — matches the phrase literally, and hands every projector the whole query surface *and* the OPEN narrowing that would silently stop CHECKED_OUT carts folding. **(c) Composition (write holds a read)** — buys nothing over the module-level codec sharing that already exists (`cart.rs:10,44,52`) and still exposes the narrowed port at runtime | ✅ **Recommended: (a).** The rule in one sentence: *a read model has a QUERY port (narrowed, GraphQL-shaped) and a ROW-STATE port (unnarrowed `load(id)`); the projection write repository supertraits the ROW-STATE port and nothing else; no crate holds both the query adapter and the write repository.* On the **log**, `EventStore: EventStreamReader` — supertrait, unqualified, and the reader half is what the projector, `bam` and the deletion engine need without `append` |

---

## 34. The API tier — cutting `server` out of the eight subgraphs (PROP-20260811-090000 §4.1–§4.4, PROP-20260811-150242 §5.1.9)

**Origin**: product-owner directive, 2026-08-11 — *"Remove the damn server crate it's currently the
purpose of what we are doing"*, and the measurement that triggered it: each of the 8 `graphql-*`
subgraph bins **declared 3 workspace crates and linked 44** (14x), against 1.5x for the 7
`gateway-*` bins, with 25 of the 44 reachable only through `server`. A catalog pod linked the Stripe
integration and the SSR renderer.

✅ **ALL THREE ROWS CLOSED on their recommendations**, all TEAM-OWNED — recorded here rather than in
a commit because each either reverses written text or changes a shape the generated schema already
promises. **API-1** (a): a cross-boundary FK navigation field is resolved by projector-side
composition, with (c) for `Restaurant` specifically if the column duplication proves too heavy.
**API-2** (a): the five permanently-empty cross-scope nav fields are **deleted** — a breaking SDL
change whose free window closes at the [#358](https://github.com/TheCaptainCompany/captain-food/issues/358)
cutover; deleting them makes the api graph acyclic, which is what lets per-scope API crates exist at
all. **API-3** (a): introspection gets an explicit home — a **hard precondition of slice A3**, because
the day per-scope roots land, introspection silently narrows and nothing fails.

**Sequencing, so this section is not read as a blocker**: slice **A1** — extract `api_runtime` +
`api_graph` and drop `server` from the eight manifests — is gated on **none** of these rows. It is a
pure crate move whose acceptance test is a byte-identical
`specs/generated/schema.generated.graphql`. A2 needs API-2; A3 needs API-1 and API-3.

⚠️ **The gate hole this run found is the durable part**: `api-nested-cross-scope` forbids cross-scope
nested api types and reported 0 errors, while the generated SDL contained **ten** such edges —
because the rule walks `$ref`s in the spec and the emitter *derives* the fields from FKs and
`navRoles:`.

---

## 35. The founder answer sheet of 2026-08-12 — ✅ ALL ROWS CLOSED ON ANSWERS

Record: [ADR-20260812-214021](../adr/ADR-20260812-214021-the-founder-answer-sheet-of-2026-08-12.md),
with a ten-lens `Consulted:` block. Six pre-existing rows closed in place with it (§27 Q7 · §28 Q1/Q2 ·
§31 BND-6/BND-7 · §32 JRN-1's founder-owed leg).

**The headline is not one of the answers — it is what they add up to: the critical path is
INVERTED.** *"I'm waiting for a working version before paying OVH"* turns **provision → deploy →
walk** into **walk → provision → deploy**, and the one leg the team could not supply was the exit
condition: *"a working version"* had **no acceptance criterion**, which made it a spend gate with no
exit.

✅ **INV-1 — closed in full.** The acceptance criterion was **answered by the founder on 2026-08-13**,
replacing the team's proposal with his own six clauses — customer created → payment authorised →
order created → accepted → delivered → captured, walked on the local all-databases stack with
authentication deliberately bypassed from the inside
([ADR-20260813-191111](../adr/ADR-20260813-191111-the-acceptance-criterion-six-clauses-walked-with-the-front-door-unlocked-from-inside.md)).
So the spend gate has an exit. **And the path was a MERGE, not a build** — `cutover-local-rehearsal` /
[PR #486](https://github.com/TheCaptainCompany/captain-food/pull/486) already carried the runbook, the
k3s CNPG overlay and the smoke overrides. Re-sequenced again by §45 **SEQ-1**.

✅ **CUT-1 — (B), the rule.** The cutover gets a **rule** rather than a list: *IN = only what the empty
log or a traffic pause makes cheaper*. That admits the storage split and excludes the pooler, the API
tier and the runtime decomposition — and it **immediately withdrew STO-4's sequencing**.

✅ **DB-HA — (A), three instances — recorded, NOT incurred.** With `podAntiAffinityType: required` on
a hostname topology, `instances: 3` on one node leaves two pods `Pending` forever. A is the EUR 67.80
trio; its +EUR 41.20 is unpayable until the EUR 26.60 base is, and **the 60 Gi of PVC it implies is
unpriced anywhere in the repo**. ⚠️ The runbook cited for the sizing detail **does not exist**.

✅ **SIR-1 (= Q-L2) — all NO, and the closure is on ATTESTATION, NOT INSPECTION** (legal lens; **not
clearance**). The two neutralisations owed before any re-sync are live in the tree. ⚠️ **The Art. 21
blocker survives forward-looking** — `RestaurantListingOptedOut` folds into **nothing**.

✅ **Q-L3 — no.** Load-bearing in two directions: it supports the empty-log window that JRN-1 and
CUT-1 both spend, and it dates the trigger the legal brief keys on.

✅ **KEY-1 — delete it now**, as instructed. ⚠️ The key's identity appears nowhere in the repo and
this record does not invent one.

⚠️ **Q-L1 is the one row still open**, and it is a legal precondition rather than a backlog item.

| # | Decision | Options & the trade-off | Answer / status |
|---|---|---|---|
| **Q-L1** ⚠️ **PARTIALLY CLOSED 2026-08-12 — three fields + a mediator STILL FOUNDER-OWED** | **The publishable identity block** (mentions légales / privacy notice) for the application, not the landing page | Remark, verbatim: *"Use the same info from join.captain.food"* — no fields supplied | ⚠️ **Partially resolves.** **Published today** (fetched 2026-08-12, `join.captain.food/mentions-legales` + `/confidentialite`): éditeur **and** controller = *association Caring Hope Foundation*, loi 1901, déclarée à Tours, RNA **W372020229**; rights contact **miam@captain.food**; host block **GitHub Pages / GitHub, Inc.**; pilot lawful basis = consent; retention ≤ 24 months; CNIL route named. **STILL FOUNDER-OWED, because the pages do not carry it**: a **postal address** (the page says *"Siège social : Tours (Centre-Val de Loire), France"* — a city, not the siège social as filed) · **a publishable phone** (absent from both pages) · **a named directeur de la publication + its statutory title** (the page says *"le·la représentant·e légal·e de l'association"* — a description, not a name). **Legal's instruction is VERIFY, DO NOT COPY, and two of its reasons are checkable**: the **host block is wrong for the app** (landing = GitHub Pages; the app host becomes OVH/CNPG, so copying publishes a false hosting declaration), and **no consumer mediator is named anywhere on either page** — a launch blocker in its own right, and not something the landing page can be mined for. Tracked as [#515](https://github.com/TheCaptainCompany/captain-food/issues/515) (the three missing fields + the mediator), so the owed items have an issue and not only a register row |

---

## 36. Supabase Auth for V0 — ✅ CLOSED 2026-08-13 ON A FOUNDER ANSWER

✅ **IDP-1 closed as (A)** — retain Supabase, with a **dated reversal condition**. Verbatim: *"For the
auth/identify we will use Supabase because it's free and easier"*. **All ten lenses recommended not
self-hosting now**, so the answer and the advice agreed. The wrapper stays identity-only — no business
data in the identity provider, which §46 **IDENT-1** later made an explicit ruling.

---

## 37. Recorded intent must execute itself — ✅ CLOSED 2026-08-13 ON A FOUNDER DIRECTIVE

Founder directive, verbatim: *"These things have been already said in the past I'm repeating
myself"*. Record:
[ADR-20260813-233418](../adr/ADR-20260813-233418-recorded-intent-must-execute-itself-the-anti-repeat-mechanisms.md).

✅ **AR-1 and AR-2 both CLOSED**, both trimmed to their **light** form. **(1) The unrealized-directive
sweep** is a standing `architect`-run step, **not** a validator rule — ~30 proposals carry an
un-maintained `_(filled at completion)_` while already shipped, so the header signal is mostly false
positives and an offline gate cannot see the live-PR state that separates "dropped" from "in flight".
**(2) A recorded behavioural guarantee carries its enforcing `rules.yaml` entry and test** — an
`Enforced by:` field on the ADR template, a cheap existence check, and the review lens. The
compiler-first and auto-classify-and-block forms were judged and **rejected as over-engineering**.

---

## 38. Capture-on-delivered review carry-forwards — PR #545 five-lens review (2026-08-14)

Four of the five carry-forwards from the [PR #545](https://github.com/TheCaptainCompany/captain-food/pull/545)
five-lens review are team-recorded in their proper homes. **One row is founder-owed and is
deliberately kept open**: it was added *after* the 2026-08-14 delegated list and commits Captain to
absorbing real money, so it is outside that delegation's explicit scope.

**The forward-trap the `dba` lens caught**, recorded here because it has no issue to attach to yet:
the decided per-service-type posture's unbuilt **at-table advance-capture arm**, dropped on top of
#545's authorize-first design, would let a `PaymentCaptured` on a still-`PENDING` payment drive
`PENDING→CAPTURED`, swallow the following `PaymentAuthorized`, and never fire `PlaceOrderProcess` —
**money captured, order never materialized**. When that arm lands, `PlaceOrderProcess` MUST also
materialize on `PaymentCaptured`-from-`PENDING`, pinned by a test.

**The `business` lens reframed same-day-only scheduling as a SOLVENCY constraint, not capacity**: a
~6-day-out order meets a ~7-day authorization expiry → `AUTHORIZATION_EXPIRED` on a fulfilled order,
so multi-day scheduling MUST ship [#175](https://github.com/TheCaptainCompany/captain-food/issues/175)
re-authorization first. The `legal` lens's seven counsel questions and two forward notes are in
[BRIEF-20260814-capture-on-delivered-counsel-packet](../legal/BRIEF-20260814-capture-on-delivered-counsel-packet.md);
**no lens output is legal clearance**.

| # | Decision | Options & the trade-off | Recommendation / status |
|---|---|---|---|
| **LOSS-1** 🟠 **FOUNDER-OWED — open 2026-08-14** | **Permanent-capture-failure loss allocation + operator runbook.** When a post-delivery capture ultimately fails (authorization dead, card permanently declined) on a **fulfilled** order, the food COGS and rider payout are already sunk — an **unbounded per-incident loss**. Under Connect separate charges & transfers the mechanical split is *restaurant eats COGS / Captain eats rider payout*. There is **no recorded write-off threshold, retry SLA, or comp-vs-pursue owner**; the current plan pages an operator, which is a fine V0 stopgap **only if a runbook exists**. Sets platform financial liability and the supply-side promise, so it is founder-owed | **(A) Captain absorbs** up to a recorded per-incident write-off threshold and makes the restaurant whole; above the threshold, escalate to recovery. Pros: protects the supply side (restaurant made whole), simplest consumer story. Cons: platform liability needs a funded reserve. **(B) Loss falls where the transfer would have** — restaurant eats COGS, Captain eats rider payout (the mechanical split). Pros: no cross-subsidy, matches money movement. Cons: the restaurant bears a loss for a card/consumer failure it did not cause — corrosive to the supply side Captain is trying to win. **(C) Pursue the consumer** for the fulfilled order (recovery posture). Pros: the consumer received the food. Cons: consumer recovery in France is legally/operationally heavy (legal packet CAP-7) and likely uneconomic per incident. |✅ **recommend bounded (A) for V0** — absorb up to a recorded threshold + make the restaurant whole, with a documented retry SLA (N attempts over M days before declaring permanent) and a **named operator owner** for the comp-vs-pursue call; escalate to (C) only above the threshold and only after counsel confirms the recovery framing (CAP-7). **Not dispatchable until answered — the paging stopgap is acceptable ONLY once the runbook (threshold, SLA, owner) exists.** **⚠️ ARCHITECT VERDICT 2026-08-14 — KEPT OPEN / FOUNDER-FLAGGED, deliberately NOT closed under the 2026-08-14 delegation.** LOSS-1 was added to this page **after** the decision list the founder pasted back with *"You don't need me for that … Go ahead team!!"*, so it is **not within the explicit scope** of that delegation. Three reasons it stays founder-owned despite the general delegation spirit: **(1)** it commits Captain to **absorbing real money** and standing up a **funded reserve** — a platform financial-liability policy, a different class from the product-shape choices (STRIX / D8–D11) the delegation covered; **(2)** over-reaching on a money-absorption policy is the **worse error** — the register's own rule is to default to OPEN in genuine doubt, and there is genuine doubt here; **(3)** it has a **legal leg** (CAP-7 recovery framing) that no lens output can clear (ADR-20260812-143619). The team's recommendation — bounded (A), absorb-to-threshold + make the restaurant whole + documented retry SLA + named operator owner — **stands as the recommendation awaiting founder sign-off**; the operator runbook may be drafted, but the write-off threshold, the reserve and the absorb-vs-pursue policy need the founder |

---

## 39. Per-instance authorization — a cross-tenant IDOR on BOTH sides, 83 of 118 operations — [#178 "Write-side per-instance authorization"](https://github.com/TheCaptainCompany/captain-food/issues/178) + [#618 "Read surfaces missing `ReadScope` — the read half of the write-path authorization gap (#178)"](https://github.com/TheCaptainCompany/captain-food/issues/618) / PROP-20260726-171500 (architect run, 2026-08-14; **SCOPE CORRECTED 2026-08-17**)

Design record: [PROP-20260726-171500](PROP-20260726-171500-write-side-per-instance-authorization.md)
(D1–D4 unanswered since 2026-07-26 and never surfaced in this register until 2026-08-14).
Obligation map: [BRIEF-20260816-idor-obligation-map](../legal/BRIEF-20260816-idor-obligation-map.md)
(**not legal advice, not clearance**).

⚠️ **This row was WRONG ABOUT ITS OWN SUBJECT until 2026-08-17, and the correction is the record.**
It described a cross-tenant **write** IDOR on the order lifecycle. The real surface is **83 of 118
operations on both sides**: 76 of 86 mutations with no proven domain binding (**37 bindable** /
**39 unbindable**, where *no payload field corresponds to the caller at all*, so removing ids from
payloads does nothing), plus **7 unscoped read surfaces**
([#618](https://github.com/TheCaptainCompany/captain-food/issues/618)), two of which return other
tenants' rows **when called with no arguments**. `approveRefund`/`denyRefund` consult no identity
anywhere. The *"cheaper because claims"* premise is corrected too: the **write** side resolves
identity by a database lookup in the mailbox worker, **only for `CUSTOMER`** — *"we already have the
identity at the handler"* is **false today** for every other role, and `external_tokens` is a flat
shared list with no per-partner identity.

The rationale for spending a session on the correction, from `legal-specialist`: **a risk record that
describes a smaller defect than the real one is weaker evidence than no record**, because it invites
the question of how well the team knows its own system. The obligation map's two findings **survive
the code fix** — free-text **Art. 9(1)** special-category prose needing an ordinary-case 9(2) basis,
and **blast-radius unboundability** forcing worst-case notification.

**Two founder rulings of 2026-08-18 land on this section** (§49): `approveRefund` is **not** narrowed
to `[ADMIN]` — the restaurant approves by default and the admin intervenes by exception — so the
cheap fix is off the table and the hole **must** close by **binding**, which moves the write-side
seam onto the critical path. And staff sign-in now has a mechanism, which is what makes binding
implementable for a non-CUSTOMER role at all.

| # | Decision | Options & the trade-off | Recommendation / status |
|---|---|---|---|
| **IDOR-1** 🟠 **TEAM-DECIDABLE / FOUNDER-INFORMED — open 2026-08-14; SCOPE CORRECTED 2026-08-17; DEADLINE RE-DEFINED 2026-08-17 ON A FOUNDER ANSWER (§45 IDOR-DEADLINE) — now the EARLIEST OF: a second restaurant credential outside the team INCLUDING demos and pilots · a rider credential to a non-team person · the first real customer order** | **Is closing the cross-tenant IDOR (#178 write + [#618](https://github.com/TheCaptainCompany/captain-food/issues/618) read) a V0-BLOCKER now, or a deadlined fast-follow — and what is the deadline?** *(2026-08-17: the question below was written against the write half alone; it now covers **83 of 118 operations across both sides** — see the scope-correction block at the top of §39. The **only** thing the correction changes in this row is what the answer will be binding on; nothing about the option space, the recommendation or the deadline is reopened by a fact correction.)* The fix itself is a straightforward correctness implementation (`WriteScope` binding the token's verified `restaurant_id`/`rider_id` to the target aggregate, envelope-carried, checked before journaling); there is no genuine option space in *whether* to fix a cross-tenant IDOR, only in *when*. Sequencing sits against the acceptance keystone, which by design walks auth-off from the inside (ADR-20260813-191111 §3/§6) | **(A) V0-blocker NOW / dispatch ahead of acceptance.** Pros: closes the worst write exposure first. Cons: it harms nobody **today** — Q-L3 empty-log window, single tenant, no real multi-restaurant tokens issued — so blocking the keystone on it inverts the value stack for zero present risk. **(B) Deadlined fast-follow — hard V0-LAUNCH blocker, sequenced after the acceptance keystone but BEFORE the first-real-order gate** ([#533](https://github.com/TheCaptainCompany/captain-food/issues/533)) and the auth walk ([#529](https://github.com/TheCaptainCompany/captain-food/issues/529)/[#532](https://github.com/TheCaptainCompany/captain-food/issues/532)). Pros: matches present risk (nil while single-tenant) yet cannot slip past the moment a **second** real restaurant token exists — accept→capture means one restaurant could trigger capture on money that is not theirs, the oversell/marketplace-trust catastrophe. Cons: the window stays open through acceptance (acceptable: acceptance issues no real multi-tenant tokens). **(C) Accepted V0 posture, no deadline** — **rejected**: V0 in Tours IS a multi-restaurant marketplace, so "before multi-restaurant traffic" = "before launch"; an undeadlined IDOR on the money path is not a posture, it is a launch defect waiting for a second tenant. |✅ **Recommended: (B).** `Priority` = **Urgent** (tier-1 security/correctness) in the backlog, but the lane is **AMBER** until this row records the deadline (then the WriteScope implementation is GREEN — touches `crates/**`, reverses nothing, final-vision = a compiler-first `WriteScope` witness). **Team-decidable + founder-informed** (a security-correctness sequencing call, not money-liability/legal); flagged to the founder in the 2026-08-14 run report because it gates whether the platform can safely run two restaurants. **Also records**: PROP-20260726-171500 needs the claims-not-projection refresh; D1–D4 are team-decidable now. **Age flag**: the underlying option space (#178 D1–D4) has been open **since 2026-07-26** with no register row — surfaced here for the first time. **Recorded 2026-08-17, NOT enacted — the trigger may bind one step too late**: two lenses independently argued the deadline should be the **earliest of** (i) a second restaurant credential outside the team *including a demo or pilot*, (ii) a rider credential outside the team, (iii) the first real customer order, because an IDOR needs **two principals** and the second credential exists at **onboarding**, not at the first order. ~~Moving a founder-facing deadline is a decision, not a correction, so it stays as written and this is a flag awaiting an answer.~~ ✅ **ANSWERED 2026-08-17 — ENACTED AS PROPOSED** (§45 **IDOR-DEADLINE**): the deadline is the **earliest of** (i) a second restaurant credential outside the team *including demos and pilots*, (ii) a rider credential to a non-team person, (iii) the first real customer order. **Verdict (B) and the AMBER lane status are unchanged**; only the date's definition moved, and it moved **earlier**. **The published-deadline condition binds it**: met, or publicly re-dated with a reason *before* it passes. **Residue**: §45 **IDOR-DEADLINE-GAP** — the three triggers are all team acts, and self-service CUSTOMER signup is not one of them |

---

## 40. Capability-allowlist coverage — extend the manifest gate to security-sensitive dependencies (founder insight, 2026-08-14)

Founder insight, verbatim: *"In .net the reference for the assembly of SQL controllable and we can
decide that only repositories related to SQL are referring it no one else. Can we do the same?
Otherwise we can put a unit test that will browse the source code to check that."*

**The pattern was already realized, and the founder named it after the fact.** The .NET
assembly-reference control maps exactly onto the Cargo **crate dependency graph**: a crate can spell
`sqlx::query!` only if its manifest declares `sqlx`, so restricting *which crates depend on `sqlx`*
is a compiler-adjacent control — stronger than a source-text scan, because it checks the structured
edge rather than a string. Cargo has **no** native `InternalsVisibleTo`, and `cargo-deny` `[bans]`
cannot express a per-crate grant of a workspace-wide dep, so the enforcement is a
**manifest-scanning unit test** — exactly the founder's own fallback. That test exists:
`capability_dependencies_are_allowlisted` walks every `Cargo.toml`, fails a non-allowlisted crate
that grants `sqlx` or `reqwest`, and **fails bidirectionally on a stale excuse**. Two sibling
manifest gates predate it, including `domain_and_application_never_depend_on_the_telemetry_sdk`.

**What is open is coverage**: two more capabilities are not yet in the allowlist.

| # | Decision | Options & the trade-off | Recommendation / status |
|---|---|---|---|
| **ENF-1** 🟢 **TEAM-DECIDABLE — open 2026-08-14** | **Extend `capability_dependencies_are_allowlisted` to cover `jsonwebtoken` and `aes-gcm` (and name any other security-sensitive capability that should be crate-gated)?** The mechanism is proven and the change is one allowlist entry + one capability name in the scan loop per capability. The only real wrinkle is the **final home of identity verification**: §33 ADP-1's leg-2 dissent (GraphQL lens) wants a future **identity bin** owning `auth_sessions` + `/auth/session`·`/refresh`·`/logout`, which would move `jsonwebtoken`/`aes-gcm` out of `server`/`infrastructure`. A gate written as "server only / infrastructure only" now must be updated in the same change that stands up an identity bin — the gate is bidirectional, so it will *fail loudly* and force the update rather than drift silently, which is the desired behaviour | **(a) Add both capabilities now, allowlisted to their present single holders** (`jsonwebtoken`→`server`, `aes-gcm`→`infrastructure`), each with its WHY. Pros: closes two uncontrolled security capabilities with a proven, zero-risk mechanism; the bidirectional check makes the future identity-bin move safe (it cannot land without updating the grant). Cons: one more line to move when the identity bin lands — a feature of the gate, not a cost. **(b) Add `jsonwebtoken` only** — crypto-at-rest is arguably lower blast radius than token verification. Pros: smallest. Cons: leaves the secret-decryption capability ungated for no principled reason once you accept the mechanism. **(c) Do nothing / rely on review** — reproduces exactly the side-door the `sqlx` gate exists to remove, for the two most security-sensitive deps in the tree. |✅ **Recommended: (a).** GREEN once decided — touches only `tools/codegen-rs/src/tests.rs`, reverses no recorded decision, adds no spec surface. Kept as an open row (not self-closed) because it is a **new enforcement-coverage decision** the founder explicitly invited (*"it should be used for another purpose somewhere"*) and it brushes the contested identity-bin home; tracking-issue text drafted in the 2026-08-14 architect run report |

---

## 41. Collection captures at READY, not at pickup — refinement of §1.2 (founder directive, 2026-08-14)

Founder directive, verbatim: *"for the pickup order the payment captured must happen when the order
is prepared"*. Record:
[ADR-20260814-141350](../adr/ADR-20260814-141350-collection-captures-at-ready-not-at-pickup.md), with
a per-lens `Consulted:` block.

✅ **CAP-READY — DECIDED.** A COLLECTION order captures at `OrderMarkedReady`, not at
`OrderDelivered`: **READY is collection's last controlled moment**, so capture-at-ready protects
against cook-then-no-show and is symmetric with capture-on-delivered for delivery. Empty log →
additive, no migration. **Business: HOLDS. Legal: DEFENSIBLE lawful prepayment, not a blocker** —
but it sharpens the disclosure and VAT tax-point questions, which is the open row below.

| # | Decision | Verdict | Status |
|---|---|---|---|
| **CAP-READY-LEGAL** 🟠 **COUNSEL-GATED (open) — sharpened 2026-08-14** | Capturing a COLLECTION order at READY takes payment **before possession transfers** (collection). Lawful prepayment, but it sharpens two already-open counsel questions for collection specifically: **CAP-3** (L221-5 disclosure must state the charge occurs at READY, before you collect) and **CAP-5** (VAT tax-point — fait générateur / exigibilité now decoupled from the physical handover for collection). Recorded as a CAP-3/CAP-5 collection addendum in [BRIEF-20260814-capture-on-delivered-counsel-packet](../legal/BRIEF-20260814-capture-on-delivered-counsel-packet.md). **No lens output is legal clearance** (ADR-20260812-143619) | — no verdict yet; counsel-gated | 🟠 **Build constraints on the unbuilt receipt engine ([#174](https://github.com/TheCaptainCompany/captain-food/issues/174)) and the checkout disclosure copy — NOT a blocker to the CAP-READY capture-trigger decision.** Neither clears without a French avocat |

---

## 42. A process manager is a write-side component and never reads the read side (founder directive, 2026-08-15)

Record: [ADR-20260815-030206](../adr/ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md).
Founder directive, verbatim: *"Process managers should never use the read side to work it's a write side
component"* / *"if the process manager ask the actors what it needs instead of risky projections the code
will be simpler and secured in terms of hydratation data"* / *"The actors can be queryable ... the process
managers will directly ask to the source of truth ... We just have to put in place the grpc transport"*.

**The rule is DECIDED; what stays open is how to ENFORCE it, how to make it cheap, and whether the second
reading of "ask the actor" (a query message over a transport) is ever built.** The ADR records the two
carve-outs (operator-authored referentials are not the read side; set-shaped reads have no actor to ask),
the adopted reading (fold the aggregate's own stream in-process — already how `place_order.rs:47` and
`delivery_dispatch.rs:126` work), and the honest accounting: **ONE of three read databases loses its
`captain_write` CONNECT** (STO-9 closes; STO-7/STO-8 are untouched because their readers are aggregate
command handlers, not PMs). Its STO-9 consequence is annotated on that row above as option **(e)**.

| # | Decision | Options & the trade-off | Recommendation / status |
|---|---|---|---|
| **PMW-2** 🟠 **AMBER — open 2026-08-15** | **Cross-aggregate activation residency, and the staleness fence it needs.** The founder's *"actors are kept in memory for a small amount of time to avoid reloading the stream uselessly"* is the design intent; **the code does not do it for the loads this rule creates.** `crates/infrastructure/src/mailbox/activation.rs:237-240` routes any `stream_name != self.scoped` straight past the cache, and says so in its header. Lifting that scoped-only restriction needs a fence that generalises, and `guard_freshness_in_tx` (`activation.rs:127-148`) does not — it compares ONE held version against `MAX(version)` for ONE stream. Two further findings make this a build item, not a given: **Payment activations never engage at all** (`surrogate_actor_id` keys the lane on a UUIDv5 of `"Payment:<intentId>"`, `actor_client/src/enqueue.rs:478`, while the stream is `Payment-pi_xxx`, `domain/src/payment.rs:26`, so `scoped` never matches — the one aggregate the settlement query needs), and **Catalog makes residency actively worse**: `put_locked` (`actor_runtime/src/activation.rs:142-181`) inserts THEN evicts LRU, so a large Catalog fill evicts every resident Order/Cart/Payment first (and, above the bound, itself) — a HubRise import burst at peak makes every subsequent order delivery pay a cold refold | **(a) Key residency on the STREAM the handler asks for** (fixing the Payment mismatch) **+ a multi-stream fence** — hold `(stream, served_version)` per held stream and re-assert all of them in the completion transaction. Pros: generalises the existing mechanism rather than inventing one; `(stream_name, served_version)` is sufficient — stream version is monotonic under append, and the one non-monotonic mover (GDPR stream deletion) moves it DOWN and still trips equality, so **no `lane.ownership_version` is needed** (dba, retracting his own earlier formulation: the lane read is a wasted round trip). Cons: the fence's cost scales with held streams; and it **cannot close for legs with external effects** (see PMW-3). **(b) Give Catalog its own answer and leave the rest** — a snapshot at the last full-replace event, or content-hash no-op suppression in `import_catalog` (`commands.rs:3155` appends unconditionally today). Pros: removes the one pathological actor; sizing says the money actors are tiny — Order/Payment/Cart at Tours V0 peak ≈ **under 5 MB against a 64 MB bound**, over-provisioned by two orders of magnitude. Cons: does not by itself make cross-aggregate loads resident. **(c) Do nothing** — the rule still holds, folds just always hit Postgres. Pros: correct today, zero risk. Cons: leaves the founder's stated efficiency intent unrealised, and the money path pays a stream load per decision | ⏳ **OPEN.** **Lean: (b) then (a)** — Catalog first (it is a live peak hazard whether or not this rule lands), residency generalisation second. **Owed regardless of which way this goes**: activation hit-ratio / bytes / eviction counters in `specs/observability.yaml` — there are NONE today, so the eviction storm above is invisible, and "is residency helping?" is currently unanswerable from telemetry |
| **PMW-3** 🔴 **OPEN — NOT adopted; nothing authorises building it** | **Actor queries as a mailbox/transport message — the founder's *"the actors can be queryable ... we just have to put in place the grpc transport"*.** This is the SECOND reading of "ask the actor", and it is a different mechanism from the adopted one (an in-process stream fold). Three objections stand, plus one absence: **(i) FENCING** — the lease fence is built from a message's `message_id`/`position`; a query has NEITHER, so there is nowhere to put the guard, and an unfenced read served by a lease holder can be served by a lane whose lease has already moved. **(ii) HEAD-OF-LINE** — a query queued behind commands on the settlement lane puts a Stripe capture behind whatever the actor is doing. **(iii) NO ACTOR DIRECTORY** — lanes are claimed by a **lease race**, not assigned, so "which process holds `Order-123`?" has no answer today; routing a query to a live activation is a **grain-directory** problem, not a transport problem, and gRPC solves the transport half only. **(iv)** The founder's own *"I don't think we should involve inbound messages table for queries to actors"* rules out the one addressing mechanism that exists | **(a) Do not build it** — the adopted in-process fold answers every read this rule creates, with no transport, no lease and no fencing question. Pros: closes STO-9 with zero new infrastructure. Cons: does not realise the founder's queryable-actor vision, and gives up residency-served reads that a live activation could serve. **(b) Build it, with dba's MINIMUM if it proceeds**: the reply carries **`(stream_name, served_version)`** and the caller **re-asserts that version inside its own fenced completion transaction**. Pros: makes a stale answer detectable rather than silent. Cons: **it cannot close for any leg with an external effect** — `complete_fenced` runs `handler.prepare()` BEFORE `pool.begin()` (`actor_runtime/src/completion.rs:69`) and `pm_delivery.rs:61-89` runs the whole `place_order` handler, **Stripe intent creation included**, inside `prepare`; so the order is read → irreversible money movement → open transaction → re-assert → abort, which turns a silent wrong capture into a loud one plus a stuck `RECEIVED` row. Plus a grain directory has to be designed first (iii) | ⏳ **OPEN, and explicitly NOT adopted by [ADR-20260815-030206](../adr/ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md).** **Lean: (a) for now.** **One compiler-first item is recordable and cheap TODAY, independent of this row**: a validator rule **refusing an actor-sourced `read:` step in any leg that also contains a `call:` step** — both node kinds already exist in the step DSL, so the "read-then-external-effect" shape becomes unspellable rather than reviewed. It rides PMW-1's rule. **2026-08-15 — the transport remains PARKED**: [PROP-20260815-142349](PROP-20260815-142349-actor-answers-block-and-the-ask-step.md) (Approved) consumes only this row's two buildable items — the dba minimum `(stream_name, served_version)` on every reply envelope, recorded as PM decision evidence, and the compiler-first `ask:`+`call:` refusal rule (V3) — and does not reopen or even name the transport; introducing a transport key is itself the future gate (D6) |

✅ **PMW-1 CLOSED 2026-08-15** as (a) plus additive §8 grammar, by
[PROP-20260815-142349 "Actor `answers:` + the PM `ask:`/`branch:` decision grammar"](PROP-20260815-142349-actor-answers-block-and-the-ask-step.md).

---

## 43. Opening hours and stock are checked SERVER-SIDE on place order, and a big catalog snapshots every 100 events (founder directive, 2026-08-15)

Record: [ADR-20260815-032807](../adr/ADR-20260815-032807-opening-hours-and-stock-are-checked-server-side-and-a-big-catalog-snapshots-every-100-events.md).
Founder directive, verbatim: *"In the place order process manager we should be careful about the opening
hours of the restaurant it must be checked / We can also check the stock of the items on the catalog /
These kind of checks will be also done on the screens but it must be also done on the server side / If a
catalog is too big > 100 events we will use snapshots to avoid reloading of the events from the stream we
should snapshot every 100 events to avoid too long actor loading delay <5sec"*.

**All three parts were verified against `main` and all three are real gaps.** The principle behind the
first two is one sentence and outlives V0: **a client-side check is a UX affordance, the server is the
guarantee.** RSO-1 and RSO-2 are **team-decidable** — the founder directed the outcome, only the
mechanism is open. SNAP-1's *policy* is the founder's (100 events, < 5 s) but *where a snapshot lives
and how it meets upcasting and GDPR erasure* is a genuine option space. **BUS-1 is not from the
directive** — it surfaced while verifying the peak-time window, and it is the founder's own no-polling
principle already violated in shipped code.

**The rows below carry four rounds of amendment, all made before any code was dispatched.** What
each round changed, in current terms rather than as an append log:

- **`young` + `vernon`** disproved RSO-2's implicit premise: a checkout re-check **cannot deliver what
  it appears to deliver**, because **nothing in the tree decrements stock when an order is placed**.
  Young's words are the record: *"it narrows the window and creates the appearance of a guarantee that
  the write model cannot deliver."* RSO-2 was **narrowed, not cancelled**, and the oversell half moved
  to **STK-1** — an *arbiter*, not a read. Three new rows came out of the same pass: **CHK-1**,
  **CAT-1**, **FEN-1**.
- **The framing correction that governs all of them**: `place_order` is a **command handler**, *not* a
  PM leg, so the restaurant fold, the cart fold and the catalog read on the checkout path are **not**
  governed by [ADR-20260815-030206](../adr/ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md).
- **`evans`** found a blocker in RSO-1 as recorded: **it would have introduced a new way to take live
  restaurants offline, shipped as a safety fix.** `opening_hours` is a `Vec` updated through
  `replaced_vec`, and the read side does `unwrap_or_default()` on a JSONB parse failure — so `[]` means
  **three indistinguishable things**: never declared, cleared, or unparseable. A boolean
  `f(opening_hours, timezone, now)` maps all three to **closed forever**. **The verdict is therefore
  three-valued, and what the guard does with the third value is an explicit recorded decision, not a
  default.** The row's claim that `RestaurantState` holds *"no opening hours"* was **false**; it came
  from a previous executor's report relayed unverified, which is why the correction is traceable here.
- **RSO-1's three blocking sub-questions are ANSWERED and the row is DISPATCHABLE.** Three of the
  answers say the row's own recorded text was wrong: both new scalars belong in
  `specs/common/scalars.yaml` (as recorded it **could not pass `make validate`**); the renderer computes
  nothing to replace, so RSO-1 **implements** the row for the first time; and the real emitter trap is
  **silent unreachability, not deletion, and it produces no compile error**. RSO-1 is an **emitter
  change** — the read-side call site is GENERATED and **has no clock** — and it forces a net-new
  `chrono-tz` dependency whose DST behaviour must be tested or the dependency is decoration. On the
  guard's third value the answer is **accept**, on a reasoning that **replaces** `evans`'s: the
  Sirene/Google-seeded population never reaches the guard (`RestaurantNotActive` rejects first), so the
  branch governs **deliberately activated** restaurants — **100% of which are `HOURS_UNDECLARED`,
  because no screen can set hours**. The decisive argument is **which failure announces itself**:
  accept produces a complaint, refuse produces silence, and a zero-order graph is indistinguishable
  from *"Tours has no demand"*. Three further rows were opened out of RSO-1's scope: **DSC-1**,
  **PAN-1**, **HRS-1**.

| # | Decision | Options & the trade-off | Recommendation / status |
|---|---|---|---|
| **RSO-1** 🟢 **TEAM-DECIDABLE — directed, open 2026-08-15** | **Where is *"is this restaurant open right now?"* derived, and what happens at the boundary?** The concept **does not exist anywhere today**: `orderable` is `ACTIVE_PARTNER + status ACTIVE + acceptance ≠ PAUSED` (`specs/network/api.yaml:21`) with **no hours term**, and the PlaceOrder guard chain (`specs/ordering/processmanager.yaml:40-49`) has **no closed-hours guard** — its whole list is `RestaurantPaused`, `CannotOrderTestRestaurant`, `DeliveryAddressRequired`, `OutsideDeliveryArea`, `PriceUnresolvable`, `PriceMismatch`. `RestaurantMarkedClosed` (`projection_tables.yaml:216`) is **permanent** closure → INACTIVE, not "closed tonight". The raw material exists: `opening_hours` + `timezone` columns (`projection_tables.yaml:207-228`), both on the api type (`network/api.yaml:38,42`). **Live consequence: a kitchen that shut at 22:00 renders `orderable: true` at 22:40 and the server accepts the order**. **⚠️ AMENDED 2026-08-15 — two findings change the row, both verified independently by `young` and `vernon`.** **(i) The guard computes the fact and throws it away** (young). The checkout snapshot frozen onto the event (`CheckoutSnapshot`, `commands.rs:2526-2541`) carries `restaurant_id` but **no record that the restaurant was ACTIVE, not PAUSED, and open** at the moment of acceptance — so a restaurant disputing an order accepted at 22:40 is asking a question **the log cannot answer**. Under RSO-1 the verdict must be **recorded on the event, not merely checked**; that is now in scope. **(ii) `isOpen` is a pure FUNCTION, not state** (vernon + ux): it is `f(opening_hours, timezone, now)` — nobody *knows* it, everybody *computes* it, and no event ever announces it. The modelling answer is therefore a ~~**`domain-common`**~~ **hand-written `crates/domain/src/` function over a value object with the clock injected** (sub-question (ii), settled 2026-08-15 — `domain-common` is generated AND upstream of `OpeningHoursSlot`, so it is not merely inconvenient but non-compiling), called identically by the storefront badge and the checkout guard — the only construction in which the two cannot disagree. This **replaces** any reading of (a) or (c) below as a projection column or as aggregate state (note `RestaurantState` already holds `timezone` — `crates/domain/src/restaurant.rs:79` — ~~but no opening hours~~ **and the opening hours too, at `restaurant.rs:83` — the "no opening hours" clause was FALSE, relayed from a previous executor's report and corrected 2026-08-15 by `evans`**; the hours being aggregate state does not make the verdict aggregate state, which is the point that survives). **⚠️ AMENDED AGAIN 2026-08-15 — `evans`, in mob briefing, before any code: one BLOCKER that is a precondition on the whole row, plus five corrections to its scope.** **THE BLOCKER — a boolean verdict takes live restaurants offline.** `opening_hours` is updated through `replaced_vec` (`restaurant.rs:95`), whose doc says *"an omitted array and an explicitly-empty one arrive identically … a non-empty array replaces, an empty one means 'not provided'"*, and the read side does `unwrap_or_default()` on a JSONB parse failure (`crates/server/src/graphql/generated/types.rs:1095`). So `[]` is **three indistinguishable facts**: **hours never declared** (the state of every Sirene/Google-seeded prospect), **hours cleared**, **hours unparseable**. A pure `f(hours, tz, now) -> bool` maps all three to **closed forever** — and since `orderable` reads no hours today, **RSO-1 as recorded would introduce a NEW way to take live restaurants offline, shipped as a safety fix.** `timezone` is nullable too (`restaurant.rs:79`, `specs/network/api.yaml:42`), with an undocumented account-level fallback — same shape, same defect. **Corrected in the RSO-1 spec phase (2026-08-15): that account-level fallback is PROSE ONLY — it has no materialized source (`View_RestaurantAccount` was deleted), and the stale note claiming it on `specs/database/tables/projection_tables.yaml` is fixed in the same diff; a NULL timezone — or one that does not parse while hours are declared — evaluates to `HOURS_UNDECLARED`, never to "closed".** **Correction 1 — `RestaurantClosed` is the WRONG NAME** (dissent from the definition of done as recorded): it collides with `RestaurantMarkedClosed` (`specs/network/events.yaml:358`), which is **permanent** closure → INACTIVE. Neither a reader nor a `grep` separates "closed tonight" from "closed for good", on the money path. **Correction 2 — `isOpen` is a UI term colonising the domain**: `specs/screens/restaurant_frontoffice.yaml:304` lists it among presentation fields *"not on the domain Restaurant/api type yet"*; it reads as **state**, and the thing is a **verdict at an instant**. The domain term already exists and **the ACL dropped it**: HubRise exposes `cutoff_time` (`specs/integrations/hubrise.md:21`) and it appears in **no mapping row, no scalar, and nowhere in `crates/**`** (verified) — which is precisely the term this row's "one minute before closing" sub-question needs. **Correction 3 — do NOT fold hours into `orderable`** (dissent from the recorded definition of done): every other `Restaurant` property is an event fold carried alongside `updatedAt` (`specs/network/api.yaml:44`), and a **time-varying** boolean has no meaningful `updatedAt` — it would read "3 days ago" for a value computed 4 ms ago that is wrong again in 20 minutes, and anything caching or ETag-ing on `updatedAt` would serve a stale `orderable`. **Correction 4 — PLACEMENT BLOCKER: `crates/domains/common/` is GENERATED** (`src/lib.rs:1`, `src/entities.rs:1`, `Cargo.toml:1` all say *"do not edit by hand"*), so the "pure function in `domain-common`" as recorded **cannot land there**. ~~**Correction 5 — the verdict is ALREADY computed a second time, in the renderer**: `restaurant_frontoffice.yaml:323-325`'s `opening_hours_row` takes `schedule: "{{ restaurant.openingHours }}"` with `open`/`closed`/`opens_at` labels. If RSO-1 sits beside it instead of replacing it, *"the two cannot disagree"* is claimed and not delivered~~ — **PREMISE FALSE (2026-08-15): the screen DECLARES the row, the renderer IMPLEMENTS nothing.** `crates/web/src/renderer.rs:346-349` folds `OpeningHoursRow` into the `InfoRow` arm and reads `label`/`value`, which that node does not carry (`crates/web/src/generated/screens.rs:423`) — it emits an empty div. There is no second computation to replace; RSO-1 builds the first one. Correction detailed in the definition of done | **(a) A projected `is_open` column**, refreshed by the projector. Pros: one place, cheap to read, `orderable` folds it in trivially. Cons: **it is a function of the CLOCK, not of events** — no event fires at 22:00, so the column is wrong between refreshes and needs a timer to maintain, which is a projection that decays. **(b) Computed at READ time** from `opening_hours` + `timezone`, in the restaurant's own tz. Pros: never stale by construction; no new storage; the tz column and its account-level fallback already exist. Cons: computed twice (api resolver + PlaceOrder guard) unless the derivation is shared code — and if it is not shared, screen and server can disagree, which is the same defect one layer up. **(c) On the aggregate**, folded into `RestaurantState`. Pros: the PM asks the actor, which is exactly ADR-20260815-030206's shape. Cons: same clock problem as (a) — an aggregate's fold is over events, and "it is now 22:40" is not one. **Boundary sub-question, independent of a/b/c**: an order placed **one minute before closing** — accept (the kitchen agreed to those hours), or refuse inside a lead-time margin? **⚠️ Amended 2026-08-15 (`evans`): the margin must NOT be derived from `preparation_time_minutes`** (`specs/network/api.yaml:43`) — that scalar means *ETA duration*, and reusing it as a *deadline* violates "one name = one dedicated scalar" semantically even though the types would compile. The term that answers this sub-question is HubRise's **`cutoff_time`**, which the ACL never mapped. **THREE NEW SUB-QUESTIONS, all of which must be answered BEFORE code** (`evans`, mob briefing): **(i) what does the guard do on `HOURS_UNDECLARED`?** Accept or refuse — an **explicit recorded decision, never a default**, because the default falls out of whichever way the function is written. *`evans`'s lean, recorded as a lean and not as the decision*: **accept** — refusing a paid order because seed data is missing is the worse failure; the sibling of *"a paid order nobody is told about"* is *"a restaurant nobody can order from"*, and the storefront badge should then render **nothing** rather than "Fermé". **✅ ANSWERED 2026-08-15 — the guard ACCEPTS on `HOURS_UNDECLARED`; `OUTSIDE_HOURS` is the ONLY refusing verdict. The outcome agrees with the lean and REPLACES its reasoning, which does not hold**: evans cited *"every Sirene/Google-seeded prospect"*, and those **never reach this guard** — `RestaurantRegistered` births a restaurant DRAFT (`crates/domain/src/restaurant.rs:14,197`) and `place_order` rejects `RestaurantNotActive` **before** any hours term (`crates/application/src/commands.rs:2398`), so the cold population is already refused by a better-named guard. **The branch actually governs restaurants a human DELIBERATELY ACTIVATED** — signed, slugged, live — and **100% of those are `HOURS_UNDECLARED`, because no screen can set hours**: `specs/screens/restaurant_backoffice.yaml:484` says so verbatim (*"a weekly slot editor is a repeatable per-day range control the SDUI component set does not have"*), and `specs/stories.yaml:128-140` (`ManageLocations`) has no hours step. **A restaurant onboarded through the product cannot leave `HOURS_UNDECLARED` by any means the product offers.** Every creation path writes `opening_hours: vec![]` — `crates/infrastructure/src/integrations/sirene.rs:216` (*"unknown from SIRENE"*) and `crates/adapters/hubrise/src/connect.rs:461` (*"wire shape unconfirmed"*), the latter an **ACL gap worth its own line**: `specs/integrations/hubrise.md:48` MAPS `Location opening_hours → Restaurant.openingHours` and the adapter does not implement it. Google ingestion does not exist at all (`crates/infrastructure/src/integrations/google.rs` is ownership-proof + link-probe stand-ins only, and creates no restaurant). **Production is 1 of 1**: `tools/smoke/prod-smoke.sh:310-315` registers the smoke restaurant with a `timezone` and **no `openingHours`**, so **under "refuse" the L4 smoke order placement fails and the acceptance gate breaks**. **The decisive argument, in these terms: which failure ANNOUNCES itself.** Accept produces a complaint we can act on. Refuse produces **silence** — and a zero-order graph is indistinguishable from *"Tours has no demand"*, corrupting the exact signal V0 exists to measure. **(ii) WHERE does the shared function live**, given `crates/domains/common/` is generated? **✅ ANSWERED 2026-08-15 by `vernon` + architect — (i) `crates/domain/src/` beside `restaurant.rs`.** Both call sites already reach it: `crates/server/Cargo.toml:41` and `crates/application/Cargo.toml:17` each declare `domain = { path = "../domain" }`, and `crates/server/src/graphql/cart_read.rs:19` already imports hand-written domain code from the server crate — the "one artifact imported by both call sites" requirement is met with **no new dependency edge**. **The recorded con is STRUCK**: ~~cons: the generated per-scope crates cannot reach it, so a future per-scope caller is stranded~~ — such a caller would be **illegitimate**, because `tools/codegen-rs/src/emit/domain_scopes.rs:241` emits into every scope crate *"Types only — aggregates/handlers/folds stay in their layer crates"*. Being unreachable from a scope crate is the declared design, not a cost. **Option (ii) — a hand-written carve-out inside `crates/domains/common/` — is STRUCK as "DOES NOT COMPILE", not "needs an emitter rule", and may not be reopened on cost grounds**: `OpeningHoursSlot` is generated into `crates/domains/network/src/entities.rs:13`, and `crates/domains/network/Cargo.toml` declares `domain-common = { path = "../common" }` — the kernel is **upstream** of the very type the function takes, so naming it from `domains/common/` is a **dependency cycle**, which no emitter rule can dissolve. **Correction 4's emitter claim was wrong in BOTH directions and is corrected here**: a hand-written FILE under an already-declared scope **survives** regeneration (`tools/codegen-rs/src/main.rs:302-307` writes the generated files one by one and deletes no extras; only a whole scope directory that stops being declared is swept, `main.rs:280-292`). What IS clobbered is **`src/lib.rs`** (emitted, `domain_scopes.rs:262`) and **`Cargo.toml`** (written whole, `main.rs:298`). **So the real trap is silent UNREACHABILITY, not deletion, and it generalises beyond this row**: regeneration erases the `mod` declaration while leaving the file on disk — no compile error, no diff in the file's own content, nothing a test would catch. In a generated crate the fragile artifact is the module **index**, never the module. The requirement either way is non-negotiable: **ONE artifact imported by both call sites** — two implementations that agree today is a *convention*, and a convention is the worst kind of context-map edge. **(iii) what carries the verdict on the api type**, given correction 3 rules out `orderable`? Recommended: a self-describing **`serviceWindow: { state, opensAt, closesAt, evaluatedAt }`**, with `acceptingOrdersNow = orderable && serviceWindow.state == OPEN` composed at the edge if a boolean is still wanted — the storefront already carries the `restaurant.opens_at` key (`specs/screens/restaurant_frontoffice.translations.yaml:67`), so *"Ouvre à 11:30"* comes free. **✅ ANSWERED 2026-08-15 — `serviceWindow` is a FIELD on `Restaurant`, not a separate query**, with `closesAt` renamed **`lastOrderAt`** and a fourth member **`validUntil`**; the full shape, its non-nullability, the clock seam, the `chrono-tz` dependency it forces, the peak analysis and the tests it owes are in the definition of done opposite. **The answer also carries a BLOCKING correction to amendment (1) and disproves the premise of correction 5** — both recorded there, because both change what an executor would otherwise build | ⏳ **OPEN.** **Lean: (b)**, with the derivation as **one shared pure function** consumed by both the api resolver and the guard — the clock argument kills (a) and (c) as primary sources, and a shared function is what stops screen and server disagreeing. **Definition of done**: a derived `isOpen`; a new `errors.yaml#/RestaurantClosed` whose typed context carries **the next opening slot** (a UI that says "closed — opens tomorrow 11:30" is a different product from one that says "closed"); a guard step beside `RestaurantPaused`; and **`orderable` re-derived to include hours**. Plus its `rules.yaml` entry and behaviour test (ADR-0032). **Added 2026-08-15**: the shape is a **pure function in `domain-common`, clock injected, one call site each in the api resolver and the checkout guard** — not a column and not aggregate state; and the **verdict is carried onto the frozen `CheckoutSnapshot`** (the restaurant was ACTIVE / not PAUSED / open, with the hours window it was judged against), so the log can answer the dispute later. A test that asserts the guard *rejects* is not sufficient — one must assert the accepted event *carries* the verdict. **⚠️ DEFINITION OF DONE AMENDED 2026-08-15 (`evans`, mob briefing, before any code) — the version above is superseded on five points, and the row is NOT dispatchable until sub-questions (i)–(iii) opposite are answered.** **(1) The function returns a THREE-VALUED verdict**, not a boolean: **`OPEN` / `OUTSIDE_HOURS` / `HOURS_UNDECLARED`**, with a matching `ServiceWindowVerdict` scalar in ~~`specs/network/scalars.yaml`~~ — **CORRECTED 2026-08-15: `specs/common/scalars.yaml`, and this is a BLOCKING correction, not a preference.** Amendment (6) below puts the verdict on `CheckoutSnapshot` (`specs/common/entities.yaml:167`), and kernel purity (`tools/codegen-rs/src/validate/scopes.rs:358`, rule `scope-kernel-purity`) makes a `common/` → `network/` reference a **hard validator error**, so the row as recorded could not pass `make validate`. `CutoffTime` lands in `specs/common/scalars.yaml` for the same reason; the precedent is `TimeZone` at `specs/common/scalars.yaml:188`. This is the blocker's whole answer: the three meanings of `[]` must survive into the type instead of being collapsed at the boundary. **(2) The error is named `OutsideServiceHours`, not `RestaurantClosed`** — it parallels the guard already in the chain, `OutsideDeliveryArea` (`specs/ordering/processmanager.yaml:47`): spatial boundary, temporal boundary, one sentence shape, and no collision with permanent closure. Its typed context still carries **the next opening slot**. **(3) The function is named for SERVICE HOURS / CUTOFF, not for doors** — e.g. `serving_at(hours, cutoff, tz, at) -> ServiceWindowVerdict`; **`cutoff_time` gets mapped in `specs/integrations/hubrise.md`** with its own scalar, and closing is **`min(slot.to, cutoff)`**. Do **not** derive any margin from `preparation_time_minutes`. **(4) `orderable` is NOT re-derived to include hours** — instead the api type gains `serviceWindow` (sub-question iii). Related, and cheap to fold in: the `orderable` formula exists as **prose in three places** — `specs/network/api.yaml:21`, `specs/network/scalars.yaml:126`, `crates/server/src/graphql/generated/types.rs:1076` — **none of them a `$ref`**; RSO-1 owes a `specs/network/rules.yaml` entry anyway under ADR-0032, so **make that entry the single statement** the other three cite. ~~**(5) RSO-1 must REPLACE the renderer's own computation**, `opening_hours_row` (`specs/screens/restaurant_frontoffice.yaml:323-325`), not ship beside it~~ — **PREMISE FALSE, corrected 2026-08-15: the renderer computes NOTHING, so there is nothing to replace.** `crates/web/src/renderer.rs:346-349` collapses `ComponentKind::OpeningHoursRow` into the **`InfoRow` arm**, which reads `label` and `value` — **neither of which exists on that node**: the emitted node carries `schedule`, `labels.open`, `labels.closed`, `labels.opens_at` (`crates/web/src/generated/screens.rs:423`). The component therefore renders an **empty div** today. **RSO-1 IMPLEMENTS this row for the first time**; an executor sent to "replace the existing computation" would hunt for code that is not there. The underlying requirement survives intact — one artifact, no second computation — but it is a *build*, not a *replacement*. **(6) The `CheckoutSnapshot` verdict records the WINDOW and the INPUTS, not a boolean** (`young`, refined by `evans`): `{ verdict, windowFrom, windowTo, timezone, evaluatedAt }` on `specs/common/entities.yaml#/CheckoutSnapshot` (`:167-205`). A stored `wasOpen: true` is **unfalsifiable** six months later; a stored window is **evidence**. **✅ AMENDED A FOURTH TIME 2026-08-15 — the three blocking sub-questions are ANSWERED by their owners; the row is DISPATCHABLE, and it is bigger than it looked.** **(i) The guard ACCEPTS on `HOURS_UNDECLARED`; `OUTSIDE_HOURS` is the ONLY verdict that throws `OutsideServiceHours`.** Reasoning opposite (the cold population never reaches the guard; the branch governs deliberately-activated restaurants, 100% of which are `HOURS_UNDECLARED` because no screen can set hours; and refusal breaks the L4 smoke gate). **Money correction to the "22:40" framing that opens this row**: capture is MANUAL (`crates/adapters/stripe/src/outbound.rs:245`, authorize-then-capture), so at 22:40 the card is **held, not charged** — the cost of accepting is not a wrong charge, it is that **nothing releases the hold**, because the acceptance-timeout auto-cancel is declared and **unbuilt** (`crates/application/src/generated/process_managers.rs:915`) and nobody is notified at all (`docs/STATUS.md:794-796`, gap **G8** — *"nobody is told about a paid order"*; note STATUS also carries an unrelated GDPR-lettered G8 at `:1447`, so cite the line). **Therefore: building the acceptance timeout removes most of the accept-branch's cost without refusing anyone — same effort, strictly more value**, and it is the better next chunk than hardening the refusal. **Badge — a DISSENT from "render nothing"**: render nothing in the CUSTOMER's slot (the ETA carries the decision), but the missing-hours prompt **MUST render in the restaurant's own backoffice, with the consequence attached** — the only person who can fix it is the one reading it, and a prompt shown only to people who cannot act is not a prompt. **Revisit conditions, each a SEPARATE future decision**: hours become settable **and** an activation precondition — at which point `HOURS_UNDECLARED` on an ACTIVE restaurant should become **unrepresentable** rather than refused (compiler-first, ADR-20260803-234035) — plus the two metrics named under `HRS-1`, which do not exist today. **Consumer-information exposure, FLAGGED in the RSO-1 spec phase (2026-08-15) — a flag for the legal queue, not a clearance**: accepting on `HOURS_UNDECLARED` means a consumer can commit to a purchase with NO pre-contractual indication of the time limit of performance (Code de la consommation L111-1 / L221-5 territory — the delivery/execution window is pre-contractual information for distance selling); the acceptance stands on the recorded reasoning above, and this sentence exists so the exposure is a named, dated fact rather than an implication. **(ii) The function lives in `crates/domain/src/` beside `restaurant.rs`** — settled, with the `crates/domains/common/` option struck as non-compiling and correction 4's emitter claim corrected; detail opposite. **(iii) `serviceWindow` is a FIELD on `Restaurant`, and the definition of done is now:** **(a)** `serviceWindow { verdict, opensAt, lastOrderAt, evaluatedAt, validUntil }` — **`closesAt` is renamed `lastOrderAt`**, because with closing at `min(slot.to, cutoff)` a field called `closesAt` gets rendered as *"open until"* and is **wrong by the cutoff margin, on the money path**. **(b)** **`validUntil` is NEW and non-negotiable**: `min(next clock transition, evaluatedAt + horizon)`, the horizon a scope-owned `configuration.yaml` key. `lastOrderAt` is null under both `OUTSIDE_HOURS` and `HOURS_UNDECLARED`, and a nullable expiry reads to every cache as *"cache forever"* — a restaurant that declares its hours at 18:50 on a Friday would keep rendering a blank badge in every warm cache **at the hour it matters**. **(c)** The field itself and `verdict` / `evaluatedAt` / `validUntil` are **NON-NULL**, justified because the function is **TOTAL**: a nullable field resurrects the exact ambiguity the three-valued verdict exists to kill. **(d)** **The clock is read ONCE PER REQUEST, not per row**, so every card in a list agrees. Follow the existing precedent instead of inventing a `Clock` port: `crates/application/src/sms_guard.rs:26` records that there is **no `Clock` port in this workspace** and that *"`now` is a parameter"*. Inject the instant at the three seams that already inject the request correlation id — `crates/server/src/graphql/routes.rs:158,272` and `crates/server/src/web_ssr.rs:52`. **(e)** **`chrono-tz` is a NET-NEW workspace dependency** (zero occurrences in any `Cargo.toml` or `.rs` file today). Without a compiled-in IANA database the DST boundary is wrong for one hour on the last Sunday of October — a **Saturday night**, i.e. peak. **A DST behaviour test is mandatory or the dependency is decoration.** **(f)** **Peak: no N+1 is possible** — `Restaurant` is a `SimpleObject` (`crates/server/src/graphql/generated/types.rs:737`), there are **zero** `#[ComplexObject]` impls in `crates/server/src/graphql/`, `opening_hours` is already parsed for every row regardless of selection (`types.rs:1095`), and the list clamps to 200 (`crates/application/src/queries.rs:47,50`). **(g)** **TWO `specs/network/rules.yaml` entries, not one**: the verdict rule, AND **`OrderableExcludesServiceHours`** — without the second, the next agent "fixes" `orderable` to fold in hours and correction 3 is **silently reversed**. **(h)** **Minimum five behaviour tests**, including an `opening_hours: []` regression test (the accept branch) and the DST test. **(i)** One-line clearance for `evans`: **`OPEN` already exists as a `CartStatus` value** (`specs/ordering/scalars.yaml:72`) — no schema collision, and a ubiquitous-language cost the lens accepts, since the two live in different types and neither is ever read as the other. **(iv) NEW, and it changes who can do the work: RSO-1 is an EMITTER change, not a spec-only one.** The read-side call site is **GENERATED** — `crates/server/src/graphql/generated/types.rs:1070`, `impl From<RestaurantRow> for Restaurant`, takes **only the row and has no clock**, so the current construction cannot compute a time-varying field at all. The resolution is **two hardcoded string literals in `tools/codegen-rs/src/emit/server_graphql.rs`** (the conversion literal at `:293`, the `restaurants` resolver body at `:654`) threading a request instant. **Do not dispatch RSO-1 as a `specs/**` change.** **Explicitly OUT of RSO-1's scope, opened as their own rows**: `DSC-1` (declared discovery filters silently dropped), `PAN-1` (the latent panic the implementation must not duplicate), `HRS-1` (the third meaning of `[]` and the observability contract the accept branch owes) |
| **RSO-2** 🟢 **TEAM-DECIDABLE — directed, open 2026-08-15** | **Re-validate each line's ORDERABILITY at checkout.** This is a **recorded TODO that was never done**, verbatim at `crates/application/src/commands.rs:2450-2452`: *"OfferUnavailable / InsufficientStock / InvalidOptionSelection — re-validating each line's ORDERABILITY at checkout ... pending"*. `require_orderable_line` (`commands.rs:791-812`) runs on **`add_cart_line` only** (`:918`, `:950`) plus the quantity path (`:1007`) — **never at checkout**. Checkout's only protection is fail-closed *pricing*: a line that LEFT the catalog rejects with `PriceUnresolvable`; a line still in the catalog but flipped `UNAVAILABLE`, or with stock now zero, **prices fine and is accepted**. That window is the peak window — a cart open twenty minutes on a Friday at 19:30 while the restaurant 86s a dish. **⚠️ AMENDED 2026-08-15 — SCOPE NARROWED, still directed, still worth doing.** `young` and `vernon`, asked independently, both showed the re-check **cannot deliver what it appears to deliver**. Young, verbatim: *"it narrows the window and creates the appearance of a guarantee that the write model cannot deliver."* **The disproof**: `OfferStockUpdated` is emitted by **`UpdateOfferStock` and the inbound HubRise inventory sync ONLY** (`specs/catalog/actors.yaml:76-77` and `:87-89`; `crates/application/src/commands.rs:3091-3123`) — **nothing decrements stock when an order is placed.** A checkout re-check is therefore a race **with no writer at all**: two customers each read quantity 1, both are accepted, and the count never moves. Reading it *fresher* — projection, stream fold or snapshot — changes nothing, because there is nothing fresher to read. **So, in these words, so nobody later cites RSO-2 as the oversell fix: RSO-2 does NOT close oversell under concurrency.** **What it DOES buy, and nothing catches today**: the **single-customer** case — an item that left the catalog or was flipped `UNAVAILABLE` between add-to-cart and pay — since `require_orderable_line` (`commands.rs:791-812`) runs only on `add_cart_line` (`:919`, `:948`), `require_stock_covers` only on the quantity path (`:1007`), and `commands.rs:2450-2453` is an open TODO on the checkout path. The oversell half moves to **STK-1** below | **The posture question is the real one, and the ux lens has already answered it: fail-CLOSED on the money, fail-open on add-to-cart.** Refusing a payment for a dish that just sold out is a recoverable disappointment; taking the money for it is the failure mode CLAUDE.md names as the worst there is. **(a) Call `require_orderable_line` per line at checkout** — reuses the existing guard and its four error codes verbatim. Pros: zero new concepts; the errors are already catalogued and already have messages. Cons: **N catalog reads at the hottest moment** unless it rides the `CatalogSnapshot` seam (`crates/application/src/pricing.rs:129`) that `price_cart` already uses to make it N→1. **(b) Fold orderability into the repricing pass** so one walk of the snapshot answers price AND orderability. Pros: strictly one read, and the two checks cannot drift apart. Cons: conflates two concerns in one function; a pricing failure and an availability failure need **different** error codes and different UI | ⏳ **OPEN.** **Lean: (a) reading through the `CatalogSnapshot` seam** — keeps price and orderability as separate, separately-testable guarantees while paying one read, and (b)'s coupling buys nothing the seam does not already give. **Definition of done includes SPLITTING A TEST**: `specs/tests.yaml#/TestCartAddLineIsRejectedWhenOfferNotOrderable` declares `thrown:` as an **any-of over `OfferNotFound` / `OfferUnavailable` / `InsufficientStock`** while its `when:` uses `offerId: "off-missing"` — **it passes on `OfferNotFound` alone, so `require_stock_covers` (`commands.rs:816-834`) could be deleted with a green suite.** Three tests, one per code, each with a `when:` that can only produce its own — then the checkout re-check's own tests on top. **Added 2026-08-15 — a scope FENCE the executor must honour**: the `rules.yaml` text, the test names and the PR body must all describe this as a **staleness guard**, never as a stock guarantee; a rule worded *"an order is never accepted for an out-of-stock offer"* would be **untrue the moment two customers arrive together** and would be a false pin on a real gap. Wording that holds: *"an order is not accepted for a line whose offer has left the catalog or become unavailable since it was added."* Oversell → **STK-1** |
| **SNAP-1** 🟠 **AMBER — policy directed, design open 2026-08-15** | **Aggregate snapshots: every 100 events, actor load < 5 s — where does a snapshot LIVE, and how does it meet upcasting and erasure?** The thresholds are the founder's and are adopted verbatim. **There is no event-sourcing snapshot mechanism in the tree today** (verified: no table, no event, no load path). **One false friend**: `CatalogSnapshot` (`crates/application/src/pricing.rs:129`) is a read-side *pricing* helper, unrelated to rehydration. Catalog is the right first target and the team named it before the directive arrived (**PMW-2** option (b)): `CatalogImported` carries the **whole menu** (`specs/catalog/events.yaml:217-249`), `import_catalog` appends **unconditionally** with no content-hash suppression (`commands.rs:3155`), and residency makes it worse — `put_locked` (`actor_runtime/src/activation.rs:142-181`) **inserts then evicts LRU**, so a large Catalog fill evicts every resident Order/Cart/Payment first | **Where it lives — (a) its own table** keyed `(stream_name, version)`. Pros: the log stays pure; a snapshot is droppable at will, which is what disposability requires. Cons: a second store to write, back up and erase. **(b) A `Snapshot` event on the stream itself.** Pros: no new storage, replays for free. Cons: **it makes the snapshot part of the immutable history — precisely what a snapshot must NOT be**; and it changes what `CatalogImported`-shaped replay means. **Upcasting**: Young's rule decides it — snapshots are **disposable and rebuildable, never authoritative** — so a snapshot carries the code version that produced it and a mismatch means *throw it away and refold*, **never** *upcast it*. **GDPR**: a snapshot is a **second copy** of the data the events carried; erasure ([ADR-20260731-160000](../adr/ADR-20260731-160000-order-erasure-tombstone-then-stream-deletion.md)) that deletes the stream and leaves the snapshot **has erased nothing** — deletion must be in the same transaction and *enforced*, not remembered. **Threshold scope**: per aggregate TYPE or global? An Order terminates in tens of events and would never benefit; a global rule adds a write to every aggregate for nothing | ⏳ **OPEN.** **Lean: (a) own table, per-aggregate-type threshold declared in the DSL** — the erasure and disposability arguments both cut against (b), and a spec-owned threshold makes it a decision like `binding:` rather than a constant. **AMBER** because the shape touches the event store, the erasure path and the DSL. **Owed WITH the mechanism, not after it**: activation hit-ratio / bytes / eviction counters in `specs/observability.yaml` — **there are none today**, so the founder's **< 5 s** budget is currently unmeasurable, and a budget nobody can measure is not a budget |
| **BUS-1** 🔴 **OPEN — already broken in shipped code, on the money path** | **`operationStatusChanged` is a declared product subscription served by a process-local bus, so the client polls — and the poll is the PRIMARY transport.** Declared at `specs/common/api.yaml:234-243` (open to every role path, ownership-scoped), served by the monolith at `crates/server/src/graphql/schema.rs:219` over a `tokio::broadcast` whose `OperationUpdate` payload (`crates/actor_client/src/status_bus.rs:20-38`) **carries no serde**. Post-split the subgraph bins build **fresh empty buses** (`crates/server/src/bin_support.rs:11-13,39-41`, whose own header admits completions *"reach this process's POLL reads but not its push subscribers"*) and the gateway **refuses the WS handshake outright** — `501 NOT_IMPLEMENTED`, *"use POST; poll reads are authoritative"* (`crates/gateway_runtime/src/lib.rs:311-319`). So the client polls **30 × 1 s** (`crates/web/src/actions.rs:15,26`). **Under [ADR-20260810-231300](../adr/ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md) it fails all three conditions**: primary rather than declared-degraded, **not observably degraded** (no `*_push_down_total{reason}` contract exists for it), and **no detected path back**. Two precision notes: the poll's justifying comment says *"command handling is a single in-process journal write"* — it does **not** literally name `command_journal`, but its premise died when `PM_MAILBOX_DELIVERY` flipped and handling moved to a **different process** ([ADR-20260812-000000](../adr/ADR-20260812-000000-the-pm-mailbox-flip-rides-the-journal-retirement.md)), so the 30 s ceiling was sized against a latency profile that no longer exists; and `operationStatusChanged` appears in `specs/screens/` **once**, as a **comment on the CUSTOMER checkout action** (`specs/screens/restaurant_frontoffice.yaml:464`), so **the person eating this is the customer staring at a spinner after paying** | **(a) Make `OperationUpdate` a wire contract and fan out over Postgres `LISTEN/NOTIFY`** — the mechanism `mailbox_wake.rs` already runs, canary and all. Pros: reuses a push path that exists, is proven, and already has the positive-liveness detection ADR-20260810-231300 demands; the `#385` cross-process fan-out follow-up is already recorded as owed. Cons: needs serde on `OperationUpdate` (it is one of the six unshaped reply types listed in [ADR-20260815-030206](../adr/ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md) Correction §5) **and** WS proxying in the gateway, which is a second piece of work. **(b) Declare the poll a degraded mode properly** — a `*_push_down_total{reason}` contract, a visible "degraded" posture, a detected path back. Pros: cheap; makes the current state honest instead of silent. Cons: it is the *fallback*, and shipping only the fallback is what the ADR calls *"a poll with an excuse"*. **(c) Leave it.** Cons: the customer's post-payment spinner is the **worst-consequence surface in the product** and the poll gives up after 30 s with "still processing" | ⏳ **OPEN.** **Lean: (a), with (b) landing FIRST and independently** — (b) is hours of work and makes the current degradation visible today, which is the precondition ADR-20260810-231300 actually names; (a) is the fix. **File this loudly**: it is the founder's own principle already violated in production code, on the checkout path, and it was found incidentally rather than by any gate — **no rule or test fails on it today** |
| **STK-1** 🟠 **AMBER — opened 2026-08-15, split out of RSO-2** | **Closing oversell needs an ARBITER, not a read.** RSO-2 narrows the window; it cannot close it, because **no writer decrements stock on order placement** (proof on the RSO-2 row). A guarantee needs a mechanism that **claims units atomically and releases them on payment failure or timeout** — reservation-shaped. **The precedent is already in the tree and the reasoning is already written**: `specs/database/tables/reservations.yaml:1-22` defines write-side reservation tables as a category of their own and says a projection cannot arbitrate because *"two owners claiming the same storefront address in the same second would both pass a read-model lookup and only diverge once the projector caught up — by which point both were told 'yes'"*. Substitute *"the last portion of the plat du jour"* and the argument is unchanged. **The shape differs**: stock wants a conditional `UPDATE ... WHERE remaining >= qty` plus `CHECK (remaining >= 0)` rather than a `UNIQUE` — but it is the **same table category** (domain-owned write state, not a projection, not a read model, not a journal). **What makes this a proposal and not a patch — the ORDERING problem** (vernon): `complete_fenced` runs `handler.prepare()` **before** `pool.begin()` (`crates/actor_runtime/src/completion.rs:69,71`), and the Stripe intent is created inside `prepare`. A reservation taken in the fenced transaction therefore happens **after the money hold** — exactly backwards. The reservation must be taken **in `prepare`, in its own transaction**, which makes **release** a compensating action the PM owns: release-on-decline and release-on-timeout become `PaymentProcessRow` state (`crates/application/src/generated/pm_state.rs:15`) and new PM legs | **(a) A `stock_reservations` write-side table + conditional decrement in `prepare`, released by PM legs.** Pros: the only option that actually closes oversell; reuses a category the DSL already defines and justifies; the DB arbitrates once, atomically, and cannot be raced. Cons: a real build — a new table, a decrement/release pair, two new PM legs, a timeout sweep, and stock becomes **write state with an owner** rather than a catalog attribute (which is the honest modelling truth, but it is a change). **(b) Model stock decrement as a domain event on the Catalog stream** and let the aggregate's fold arbitrate. Pros: no new table; replays. Cons: **one aggregate per transaction** — the Catalog and the Payment/Order streams are different aggregates, so the fold-then-append gives freshness, not atomicity (see **CHK-1**); and it puts every order's write on the Catalog stream, which is the actor PMW-2 already names as the residency hazard. **(c) ACCEPT-AND-COMPENSATE — the honest business alternative, not a cop-out.** For food, the restaurant phoning the customer to swap the dish is a **legitimate answer**, arguably better than refusing a payment, and it may be the right V0 posture for Tours. Pros: zero build; matches how independents already operate; keeps the money path simple at peak. Cons: it is only honest if it is **designed** — the restaurant must be *told* (accept/substitute/refund affordance) and the customer must be *reachable*; undesigned, it is just oversell with a story | ⏳ **OPEN — AMBER, and deliberately NOT pre-decided.** Blocked by nothing; **sequenced against the money path** — (a) cannot be specified before the `prepare`-vs-transaction ordering above has an owner, and it touches `specs/payments/**` (new PM legs + `PaymentProcessRow` state), `specs/database/tables/**` (a new reservation table) and `specs/catalog/**` (who owns stock). **(c) must be costed honestly against (a) before either is chosen**, and the choice is the founder's kind of question, not a lens's: it trades a build against a service posture. Whichever way it goes, **RSO-2 ships first and independently** — it is strictly better than today under either outcome |
| **CHK-1** 🔴 **OPEN — a shipped comment states a guarantee the code does not make** | **`crates/application/src/commands.rs:2392` calls the restaurant fold *"authoritative, race-free"*. It is false, and the falsehood has already been quoted into a record.** `place_order` folds `Restaurant-{id}` (`commands.rs:2393-2394`) and then appends to the **Payment** stream (`create_if_absent` on `domain::payment::stream(...)`, `commands.rs:2559`), **never passing a restaurant `expected_version`** to `EventStore::append` (`crates/application/src/ports.rs:54-60` — note the port method is `append`, there is no `save`). One aggregate per transaction: the restaurant can go PAUSED one millisecond after the fold and the checkout still completes. **The fold buys FRESHNESS, not ATOMICITY.** **The consequence to record plainly, because it reframes a decision already taken**: **fold-vs-projection is a LATENCY and COST decision, not a correctness class.** Swapping a projection read for a stream fold shrinks the stale window from projector-lag (seconds) to fold-to-append (microseconds) and changes nothing about which races are possible. [ADR-20260815-030206](../adr/ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md) already states the equivalent for settlement (*"This does NOT make settlement transactional, and must never ship described as 'race-free'"*) — the same sentence is owed here, and the comment it quotes at its line 388-390 is the one that is wrong | **(a) Correct the comment in place** to name the guard/advisory distinction inline — *"folded from ITS stream for freshness; this is an ADVISORY guard, not a fence: no restaurant `expected_version` is asserted at append, so a PAUSE racing this fold is not excluded."* Pros: one comment, zero behaviour change, kills the phrase before it is quoted again. Cons: none — but it is a `crates/**` edit and is therefore **FLAGGED here, NOT made** in this docs-only change. **(b) Make it actually a fence** — a cross-aggregate version assertion. Cons: that is PMW-2's multi-stream fence and it **cannot close for a leg with an external effect** (the Stripe hold happens in `prepare`, before the transaction) — the same ordering wall STK-1 hits. **(c) Leave it.** Cons: it has *already* been cited as evidence of compliance; a false comfort on the money path propagates into records | ⏳ **OPEN. Recommendation: (a), as a one-line comment fix riding whichever PR next touches the checkout path** — it is not worth its own dispatch, but it must not be forgotten, because the phrase is load-bearing in at least one ADR's argument. **(b) is STK-1/PMW-2's problem, not this row's.** The reframing sentence — *fold-vs-projection is latency and cost, not a correctness class* — is the durable output of this row and should be quoted wherever the fold-vs-projection choice is argued again |
| **CAT-1** 🟠 **AMBER — opened 2026-08-15; an event-shape addition and a genuine option space** | **`RestaurantState` holds no catalog id, so `restaurantId → catalogId` is answered by a set query with a tiebreak no aggregate ever decided.** The only path in the tree is `crates/infrastructure/src/persistence/catalog.rs:27-31`: `SELECT ... FROM catalog WHERE restaurant_id = $1 ORDER BY created_at DESC LIMIT 1`. Two things are wrong with it, and the second is worse: it is a **set query over a population no aggregate owns**, and its **newest-wins tiebreak is an infrastructure accident** — nobody decided it. Two concurrent `CatalogCreated` for one restaurant and **the write side silently changes which menu prices the order**. Verified: `RestaurantState` (`crates/domain/src/restaurant.rs:29-80`) carries `status`, `order_acceptance`, `slug`, `timezone`, `address` … and **no catalog reference of any kind** | **(a) The restaurant APPOINTS its live catalog** (young) — a `CatalogActivated`-shaped fact on the **Restaurant** stream, after which `restaurantId → catalogId` is answered by **the fold `place_order` already performs** at `commands.rs:2393`. Pros: **it dissolves the problem rather than fixing it** — no set query, no tiebreak, no second read on the checkout path, and "which menu is live?" becomes a decision with an author and a timestamp instead of a `LIMIT 1`. It also makes menu switchover (seasonal card, lunch vs dinner) expressible, which the current shape cannot represent at all. Cons: a new event on an existing stream + a backfill story for restaurants that already have a catalog, and it makes catalog activation an explicit act someone must perform. **(b) A `UNIQUE (restaurant_id)` on `catalog`** to make the ambiguity unrepresentable. Pros: cheap; compiler-first in spirit. Cons: it **forecloses multiple catalogs per restaurant** — a product decision smuggled in as a constraint — and still leaves the id discovered by query rather than by fold. **(c) Leave it and document the tiebreak.** Cons: documenting an accident does not make it a decision | ⏳ **OPEN — AMBER.** **Lean: (a)**, on the final-vision-first rule (ADR-20260808-235113): (b) is the cheap intermediate and (a) is the clean shape, and (a) is the one that also removes a read from the hottest path. AMBER because it adds an **event shape** to the Restaurant aggregate (`specs/network/events.yaml` + `actors.yaml` + a `rules.yaml` entry + behaviour test, ADR-0032) and needs a **backfill** decision for existing catalogs. **Sequenced with RSO-2**, which will walk the catalog at checkout: doing RSO-2 first and CAT-1 after means the re-check is written against `by_restaurant` and rewritten later — worth one line of thought before dispatching either |
| **FEN-1** 🔴 **OPEN — a fence on the money path that a client can simply omit** | **`expectedTotal` is OPTIONAL.** `crates/application/src/commands.rs:2460` is `if let Some(expected) = &cmd.expected_total`, and its own comment calls it *"a CONFIRMATION only"*. A client that omits the field is **charged the server-repriced amount with no confirmation against what the screen displayed** — the `PriceMismatch` guard simply never runs. Young, verbatim: *"on the money path an optional fence is not a fence."* The check is otherwise correct (equality against the recomputed total, fail-closed pricing above it) — the defect is purely that it is skippable, and it is skippable by the **least trustworthy** caller: anything that is not the first-party client | **(a) Make it REQUIRED** in `specs/ordering/commands.yaml#/PlaceOrder`. Pros: the mistake becomes unspellable (ADR-20260803-234035 — the type system, not a check); every caller is forced to state what it showed the customer. Cons: it is a **contract change** on a shipped mutation — any existing caller omitting it breaks, so it needs the callers enumerated first. **(b) Keep it optional in the schema, REJECT when absent** at the handler with a typed error. Pros: no schema break; the behaviour is identical to (a) from the customer's side. Cons: it is (a) expressed one layer too late — the compiler stays silent and a new caller learns at runtime. **(c) Leave it optional.** Cons: a fence with a documented bypass is a comment, not a guarantee | ⏳ **OPEN. Lean: (a)**, per compiler-first and final-vision-first, **preceded by a caller enumeration** — the web client, the SDUI checkout action (`specs/screens/restaurant_frontoffice.yaml`) and any test fixture that omits it. If enumeration shows a caller that genuinely cannot compute the total, that is a finding about that caller, not a reason to keep the fence optional. **Cheap and worth doing with RSO-1/RSO-2**, since all three harden the same handler — but it is a `specs/**` + `crates/**` change and gets its own row so it is not silently folded into an unrelated PR |
| **BSY-1** 🟠 **AMBER — opened 2026-08-15 by `evans`; NOT in RSO-1's scope** | **`BUSY` is a word in the ubiquitous language that changes nothing.** `specs/network/scalars.yaml:154-156` declares `OrderAcceptanceMode` as `NORMAL \| BUSY \| PAUSED`; a restaurant can set it (`specs/network/commands.yaml:273`, `SetOrderAcceptanceMode`) and the value round-trips (`crates/infrastructure/src/persistence/enum_sql.rs:48`, `crates/domains/network/src/scalars.rs:122`) — and then **nothing reads it**. `orderable` tests only `≠ PAUSED` (`specs/network/api.yaml:21`, `specs/network/scalars.yaml:126`, `crates/server/src/graphql/generated/types.rs:1076-1078`); **no guard** in the PlaceOrder chain (`specs/ordering/processmanager.yaml:40-49`) mentions it; **no screen** renders it; **no rule** in any `rules.yaml` names it. Verified: outside the declaration, the setter and the enum plumbing, its only appearances in the whole tree are three test lines (`specs/tests.yaml:105,848-868`) that assert the mode was set — i.e. the tests prove the *word* is storable, not that it *means* anything. **A term the model has not learned is worse than an absent one**: the restaurant flips to BUSY believing it has told the platform something, and the platform has heard nothing — the affordance renders and does nothing, which CLAUDE.md's domain lens names as worse than no control | **The domain answer, and the reason this is not just dead-code cleanup: BUSY should mean a LONGER ETA, and the ETA is the product.** A kitchen at capacity is not closed and is not open-as-usual — it is *slower*, which is exactly the axis the customer decides on before ordering. **(a) BUSY multiplies/offsets the quoted ETA** — pros: it makes the term mean the thing the restaurant intends by pressing it, on the one number that drives conversion; it needs no new state, only a term in the ETA derivation; cons: the ETA derivation must first *have* a single owner to add a term to, and `preparationTimeMinutes` (`specs/network/api.yaml:43`) is the only input today. **(b) BUSY suppresses the storefront from discovery ranking** without blocking direct orders — pros: reduces inbound at peak, which is the operational intent; cons: invisible to the restaurant, and it silently costs them money in the hour they most need it. **(c) BUSY = PAUSED with softer copy** — pros: trivial; cons: it deletes a distinction the restaurant deliberately reached for, and PAUSED already exists. **(d) Delete BUSY from the enum** — pros: honest, removes the phantom control; cons: it is a **shipped enum value** that may already be stored, so removal is a migration, and it throws away the one signal a restaurant volunteers about its own load | ⏳ **OPEN — AMBER. Lean: (a)**, on CLAUDE.md's own lens (*"the ETA is the product"*), with (d) as the honest fallback if (a) is not built — a term that means nothing should not stay in the language. **Explicitly OUT of RSO-1's scope**, and recorded here so RSO-1 is not quietly widened to carry it: RSO-1 is about the **temporal service window**, BSY-1 is about **capacity**, and conflating "we are shut" with "we are swamped" is the exact modelling error that produced this row. **AMBER** because (a) touches the ETA derivation and (d) is an enum migration on a shipped value; either way it needs `specs/network/rules.yaml` + a behaviour test (ADR-0032), since today **no test would fail if `BUSY` were deleted from the semantics entirely** |
| **DSC-1** 🟠 **AMBER — opened 2026-08-15 while answering RSO-1; explicitly OUT of RSO-1's scope** | **Seven of the nine discovery filter args are declared, shipped, and silently dropped** — the input type declares **eleven** args in total, of which `limit` and `offset` are pagination rather than filters, and of the remaining **nine** only `search` and `orderableOnly` reach the read side. `RestaurantsQueryInput` declares `tags`, `serviceType`, `openNow`, `city`, `priceRange` (`specs/network/api.yaml:75-79`) **plus `list` and `listingStatus`** (`:80-81`) — all eleven args are emitted onto the input type (`crates/server/src/graphql/generated/inputs.rs:1254-1276`) — and the resolver then builds `RestaurantFilter { search, orderable_only, limit, offset }` and nothing else (`crates/server/src/graphql/generated/query.rs:250`), because `RestaurantFilter` **has only those four fields** (`crates/application/src/queries.rs:36-44`). **A client filters and gets unfiltered results, with no error.** That is CLAUDE.md's own *"a control that renders but does nothing is worse than no control"*, and it is **already public**. **`listingStatus` fails on BOTH sides at once**: the resolver drops it here, and the write side never reads `listing_status` either — [#573 "PlaceOrder never reads `listing_status` — the server accepts orders for restaurants the read side declares not orderable"](https://github.com/TheCaptainCompany/captain-food/issues/573) — so the partnership funnel is neither filterable on the read side nor enforced on the money path. **The spec contradicts the code's own admission**: `specs/network/api.yaml:70-72` promises *"All args are optional filters resolved by the read side (Restaurant); the query returns only matching restaurants"*, while `crates/application/src/queries.rs:34` states *"V0 applies a subset (the rest are accepted and ignored until the read model backs them)"* — the api.yaml prose is the false one, and no gate compares them. Under versionless evolution each arg must be **implemented or deprecated**, never silently deleted | **(a) Implement all seven in the read model** — pros: the schema stops lying; every declared affordance works. Cons: `openNow` is not a column and depends on RSO-1's verdict function, so it cannot ship before it. **(b) Implement the six that are pure predicates** (`tags`, `serviceType`, `city`, `priceRange`, `list`, `listingStatus`) **and sequence `openNow` behind RSO-1** — pros: removes six of seven silent drops immediately, with no dependency; cons: leaves one lying arg, which needs saying out loud in the api.yaml description rather than left implicit. **(c) Deprecate and remove the unbacked args** — pros: honest and cheap; cons: it is a **breaking schema change on a shipped public query**, and it throws away declared product intent (`openNow` is exactly the filter a hungry customer at 22:40 wants). **Genuine option space inside `openNow` specifically, and the reason this is AMBER**: the naive fix filters in Rust **AFTER** `LIMIT` (`crates/infrastructure/src/persistence/restaurant.rs:91-101` pushes `ORDER BY` then `LIMIT`/`OFFSET`, and the rows are decoded after), so a post-filter returns **short pages** and **breaks pagination at 19:30** — precisely peak. Pushing it into SQL instead means the service window must be expressible as a predicate over projected columns, which is a different design from RSO-1's in-process function | ⏳ **OPEN — AMBER. Lean: (b)**, with `openNow` sequenced behind RSO-1 and its pagination question answered before it is built, never as a post-`LIMIT` filter. **Tracking issue**: [#574 "Discovery filters are declared and shipped but silently dropped by the `restaurants` resolver — seven of nine"](https://github.com/TheCaptainCompany/captain-food/issues/574) (link added 2026-08-15; the recording session had no GitHub API access and could only name the title). **AMBER**: it touches `specs/network/api.yaml`, needs a `rules.yaml` entry plus behaviour tests per ADR-0032, and (c) would be a shipped-schema migration. **Verified against `main` line by line during the RSO-1 answer pass**, not relayed |
| **PAN-1** 🟢 **TEAM-DECIDABLE — opened 2026-08-15 while answering RSO-1** | **A latent panic on the public discovery list.** `crates/server/src/graphql/generated/types.rs:1093` is `address: serde_json::from_value(row.address).expect("Restaurant.address: invalid jsonb")`, while the very next jsonb column one line down uses `unwrap_or_default()` (`:1095`). The two policies sit **adjacent in the same generated `From<RestaurantRow>` impl**, so one bad `address` jsonb row takes down **every** request that lists restaurants — the public storefront's first screen — rather than dropping one card. It is generated, so it is an emitter question, not a hand-fix: the literal lives in `tools/codegen-rs/src/emit/server_graphql.rs:293` | **(a) Make the emitter total for jsonb columns** — a non-null column that fails to parse yields the type's default and an emitted `tracing::warn!` naming the row, never a panic. Pros: one policy for every generated conversion, and the failure becomes a signal instead of an outage; cons: a silently defaulted `address` is a wrong answer rendered confidently, which is the defect class `HRS-1` opens. **(b) Drop the row** — the conversion becomes fallible and an unparseable row is omitted from the list with a counter. Pros: never renders a wrong address; cons: it changes a `From` into a `TryFrom` across every generated read type, a much larger emitter change. **(c) Leave it** — cons: a `.expect` on the public read path is an availability bug waiting for one malformed projection write | ⏳ **OPEN. Lean: (a) with the warn counter**, and (b) recorded as the final-vision shape if the read path ever needs to distinguish *absent* from *corrupt*. **Immediate obligation on RSO-1 regardless of how this row settles**: the `serviceWindow` implementation touches this exact impl and **must not add a second `.expect`** — a panic introduced by a safety feature is the worst possible provenance |
| **HRS-1** 🟠 **AMBER — opened 2026-08-15 while answering RSO-1; the accept branch's owed instrumentation** | **The third meaning of `[]` is a BUG that nothing counts, and the accept decision is invisible without a contract.** RSO-1's three-valued verdict correctly separates *hours never declared* from *outside hours* — but it still folds the **third** meaning, **hours present and UNPARSEABLE**, into `HOURS_UNDECLARED`, because `crates/server/src/graphql/generated/types.rs:1095` swallows the parse failure with `unwrap_or_default()`. **Unlike the other two, this one is a DEFECT, not a state**: a restaurant that declared its hours and is being treated as if it never did looks identical, in every log and every metric, to one that never filled the form. Separately, `specs/observability.yaml` has **no service-window contract at all** — so the accept branch, which is the branch that deliberately lets an order through, produces **no signal whatsoever**, and RSO-1's own revisit condition (*"revisit when the metrics say so"*) is **permanently unmeetable by construction** | **(a) Count the parse failure separately** — a distinct verdict or a distinct counter dimension, so *corrupt* is never reported as *undeclared*. Pros: the defect becomes findable; cons: a fourth verdict widens the type every caller must match, so the counter dimension is likely the right level. **(b) An observability contract `service_window_verdict_total{verdict}`** in `specs/observability.yaml`, incremented at the single shared evaluation point. Pros: one declaration answers both *"how often does accept fire?"* and *"is anyone stuck on undeclared hours?"*; cons: it is operational telemetry, so per ADR-20260811-014129 it belongs on OTLP and does **not** replace the persona-activity **business metric** the feature owes as a projection. **(c) Do neither** — cons: accept is unobservable, the revisit condition is undischargeable, and the corrupt case is undiagnosable | ⏳ **OPEN — AMBER. Lean: (a) + (b) together**, landed **with** RSO-1 rather than after it, because a decision whose revisit condition depends on a metric that does not exist is not a decision with a revisit condition. **AMBER**: it adds a `specs/observability.yaml` contract, and (a) touches the verdict's shape. **Explicitly OUT of RSO-1's scope as a build item**, recorded here so it is neither silently folded in nor silently forgotten |

---

## 44. How the mob's fan-out is priced (founder question, 2026-08-16) — ✅ DECIDED 2026-08-16

Records: [ADR-20260816-020752](../adr/ADR-20260816-020752-the-loops-context-budget-a-dispatch-card-snapshot-semantics-and-phase-commits.md)
(the question) ·
**[ADR-20260816-134352 "The mob's checkpoint goes to declared concerns, and review is priced by reversibility"](../adr/ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md)**
(the ruling) · **[ADR-20260817-105845](../adr/ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)**
(the amendment).

Founder question, verbatim: *"Do you have recommendations to optimise tokens consumption?"* Founder
ruling, verbatim: *"Go for the Recommendation: (b)+(c), with holub's verification condition."*

✅ **MOB-COST-1 — DECIDED as (b)+(c).** Six of the seven answers were technique and are the team's;
this row was the one that **amends a founder directive**, so it was treated as a decision reversal
whatever the diff size. The outcome: **the CHECKPOINT goes only to lenses that DECLARED a concern at
briefing** (any lens may opt back in), and the chunk's **reversibility class** sizes the briefing
roster — full mob for money movement, stored event shapes, legal surfaces and anything Tours-facing;
2–3 lenses for reversible refactors, generated artifacts and doc sweeps.

**Measured baseline, honestly**: ~2.5M tokens for one merged work item, of which the mob fan-out was
~1.2M per chunk — and **no per-item instrument exists**, so that figure is a reconstruction, not a
reading. The dispatch card cuts the per-lens cost ~10x whichever way the row went, so the decision
was about **detection policy, not the bill**.

✅ **MOB-COST-1a — CLOSED 2026-08-17, and the automatic reversion was STRUCK.** The rule that a MISS
reverts the class was struck on n=2, where **neither miss was a roster-width miss** — the committed
claim-time card for the first said *"Briefing roster: WHOLE ROSTER"*, so the wrong arithmetic was in
front of every lens, and the second was rejected by `vernon` as his own depth miss. The shared cause
was **a coordinator-authored derived number consumed as established fact with nothing verifying it**,
and the replacement rule is the one that earned it: **a dispatch card may not state a derived number
without naming its antecedents, and any bare number it does state is marked `UNVERIFIED input`.**
The named residue: a genuine roster-width miss now has no automatic consequence and returns to the
founder.

---

## 45. The founder answer sheet of 2026-08-17 — ✅ ALL SIX ROWS ANSWERED

Six rows went to the founder as one decision queue and came back answered the same day. Records:
**[ADR-20260817-105844 "The walk goes first, on ONE database, and production stays suspended on purpose"](../adr/ADR-20260817-105844-the-walk-goes-first-on-one-database-and-production-stays-suspended.md)**
(PROD-1 + SEQ-1) ·
**[ADR-20260817-105845 "A dispatch card may not state a derived number without its antecedents"](../adr/ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)**
(MOB-ANTECEDENT, amending §44).

**Two of the six went against the team's own recommendation** (PROD-1 and REV-1). They are recorded
with the reasoning that supports what he chose; the counter-arguments live in the rows' option
columns, and neither is softened into a half-answer.

✅ **PROD-1 — production STAYS SUSPENDED, as a deliberate recorded state.** The point is that the
state is now **decided, not merely persisting**. ⚠️ The defect underneath it is not the 503: the
nightly `prod-smoke` had been **RED for 19 consecutive scheduled runs** (last green 2026-07-29), of
which the billing suspension explains only 13, with **no record treating it as a broken gate**.

✅ **SEQ-1 — the walk goes FIRST, on ONE database.** The acceptance criterion is unchanged as what
*certifies* (local, eleven databases, full enforcement) and simply stops **gating** the first
end-to-end reading. This resolves the 2026-08-13 ↔ 2026-08-14 contradiction in favour of the
2026-08-13 sequence, and does **not** overturn final-vision-first, because the final step **cannot be
built**: the split band is blocked on STO-7, STO-8 and STO-9 independently, with STO-10 parked and
RDR-1 open upstream of the grant emitter.

✅ **MOB-ANTECEDENT** — see §44. ✅ **STO-10-PARK** — parked until the walk lands, **reported blocked**,
with the [#513](https://github.com/TheCaptainCompany/captain-food/issues/513) CONNECT prohibition
intact. ✅ **IDOR-DEADLINE** — the deadline becomes the **EARLIEST OF** a second restaurant credential
outside the team *including demos and pilots* · a rider credential to a non-team person · the first
real customer order; strictly tighter than the wording it replaced.

✅ **REV-1 — `claude-review` comes OUT of required checks**, recorded as a knowingly-given-up
mechanical guarantee whose compensating control is the mandatory independent reviewer pass.
⚠️ **NOT EXECUTED**: a 403 from the session's agent proxy on the ruleset write path — an egress
block, not a GitHub denial — so it remains an open **action** (not a decision) on
[#593](https://github.com/TheCaptainCompany/captain-food/issues/593).

| # | Decision | Options & the trade-off | Recommendation / status |
|---|---|---|---|
| **IDOR-DEADLINE-GAP** 🟠 **OPEN — raised 2026-08-17 by the executor landing IDOR-DEADLINE; the trigger set omits the one credential nobody issues** | **The three triggers are all things the TEAM does. For `CUSTOMER`, nobody issues anything** — `requestPhoneVerification`/`verifyPhone` are `roles: [PUBLIC, CUSTOMER]` and a first verified phone CREATES the Customer, so a stranger self-issues a CUSTOMER credential the moment the surface answers (verified 2026-08-17, §39). And **two CUSTOMER-reachable reads take a caller-supplied id with no ownership check** — `orderConversation` and `reclamation` — which are exactly the two unbounded free-text stores the Art. 9(1) finding is about. So *"production restored with signup open"* is an event that issues credentials outside the team, reaches other customers' complaint text and message threads, and **trips none of (i), (ii) or (iii)** | Not an option space yet — a completeness question about a deadline just published. The candidate fourth trigger is *production restored while self-service signup is open*, which is the same event §39's replacement control already names (*"the gate must be SIGNUP, not onboarding"*) | 🟠 **OPEN, deliberately NOT enacted by the executor.** **It is not a contradiction of the answer**: the new trigger set is strictly TIGHTER than the *"before the first real order"* wording it replaces, and the gap it leaves existed identically under the old one — so landing IDOR-DEADLINE improved the record and creating this row is the honest residue, where stopping would have left the weaker deadline standing. **It is closed by circumstance today and only by circumstance**: PROD-1 keeps production down, so no surface answers. **The ask**: confirm the fourth trigger, or record why signup-open is covered by (iii). Circumstance changes without a record; a deadline should not depend on one |

---

## 46. The founder rulings of the night of 2026-08-17/18 — ✅ THREE RULINGS, ALL ANSWERED IN ONE SITTING

Three rulings came back in one sitting, after the whole roster of thirteen lenses was consulted.
Records:
**[ADR-20260818-004646 "No business identifier lives in the identity provider"](../adr/ADR-20260818-004646-no-business-identifier-lives-in-the-identity-provider.md)**
(IDENT-1) ·
**[ADR-20260818-004647 "Database-level security lands at the CloudNativePG cutover, on the empty database; and the settlement read comes back into scope"](../adr/ADR-20260818-004647-database-level-security-lands-at-the-cutover-and-the-settlement-read-returns-to-scope.md)**
(RLS-SEQ, and §32 **STO-9** back in scope). Both carry a per-lens `Consulted:` block
(ADR-20260812-143619).

✅ **IDENT-1 — no business info is stored inside the identity provider, and it is V0.** A token
carries the auth subject; the `sub` → domain-id mapping is resolved from our own Postgres. Asked V0
or post-first-order, the founder answered **"v0"**, so it sequences **before** the write-side
enforcement seam. It **reverses the read-scope half of §22's identity-bridge row / CARD-11**, it is a
**MIGRATION** (tokens in the wild carry `captain_food.customer_id` today) whose phase order is
recorded before it lands, and its price is stated rather than softened: `read_scope` stops being pure
and the enforcement slice's *zero I/O at peak* claim dies with it. ⚠️ **Premise corrected 2026-08-18**
(the ruling unchanged): only **one** business identifier was stored, and **three of the four roles had
no authentication path at all** — which raised STAFF-AUTH, answered in §49.

✅ **RLS-SEQ — database-level security lands at the CloudNativePG cutover, on the EMPTY database,
starting at `OrderConversation` and NOT at `OrderTracking`.** Three of the four drafted tables fail
for measured reasons, the sharpest being that a policy on `OrderTracking` turns the pre-capture
settlement read into a **silent** `HookOutcome::Skip` — RLS **filters** rows, it does not raise:
food delivered, money never collected, reported green. What is emitted was settled separately by
[ADR-20260818-171500](../adr/ADR-20260818-171500-mode-gates-the-whole-per-table-subtractive-surface-including-the-owners-write-policy.md)
(`mode:` gates the whole per-table subtractive surface), and the measured design is
[PROP-20260818-010343](PROP-20260818-010343-database-level-security-the-measured-design.md),
[#638](https://github.com/TheCaptainCompany/captain-food/issues/638).

✅ **AUTHZ-LOCUS** — PROP-20260726-171500 §D1 is closed **against the proposal's own recommendation**,
recorded here because a proposal's recommendation may not be overturned silently. ✅ **AUTHZ-GRAMMAR**
— the `authorization:` block is **declined as new grammar**; the corrected design is
[#636](https://github.com/TheCaptainCompany/captain-food/issues/636): finish the `requires:` emitter,
with completeness keyed on `actors.yaml receives[]`.

⚠️ **The three externally-authored ADRs are HELD, not deposited.** `ADR-20260817-232744`,
`ADR-20260817-232745` and `ADR-20260817-232746` (authored outside the team, 2026-08-17) are held
until corrected and are **not** in `docs/adr/`. What of theirs survives is carried, corrected, by
AUTHZ-LOCUS and AUTHZ-GRAMMAR above, by
[#635](https://github.com/TheCaptainCompany/captain-food/issues/635) and
[#636](https://github.com/TheCaptainCompany/captain-food/issues/636), and by ADR-20260818-004647.

---

## 47. Strix autonomous pentest — a first gated, sandboxed, defensive run — PROP-20260814-000240 (founder interest, 2026-08-14)

> **Numbering note.** This section was published as a second "§37" from 2026-08-14 to 2026-08-18,
> colliding with §37 "Recorded intent must execute itself". It is **§47** as of 2026-08-18; no
> record outside this file cited it by number.

Design record: [PROP-20260814-000240](PROP-20260814-000240-strix-security-audit.md), tracking issue
[#548 "Evaluate Strix for a gated pre-launch DAST pass against our own endpoints (authorized defensive)"](https://github.com/TheCaptainCompany/captain-food/issues/548).
**Authorized defensive** testing of our own pre-launch product. **Verdict: GO-NARROWLY.**

✅ **STRIX-1 and STRIX-2 both ADOPTED (A), founder-delegated 2026-08-14** (*"You don't need me for
that … Go ahead team!!"*). The proposal is `Approved` with its three `Concerns` checked. A single
**gated, sandboxed, bounded** black-box DAST pass on a **dev** target, framed as a defensive pentest
evidence pack for counsel — **never a PCI/RGPD certificate** — with hard time and token caps on the
shared proxy.

**The value argument is measured, not asserted**: our strongest controls are compiler- and
validator-enforced, so the residual risk **migrated to exactly the surface static gates cannot
reach** — runtime authz composition (cross-tenant/cross-role IDOR), request cost (**zero
`depth`/`complexity` limiter anywhere in `crates/server/src/graphql`**), the `X-Forwarded-Host`
ingress precondition the code documents but cannot enforce, SSRF on adapter outbound, and error-path
secret leakage. **White-box Rust scanning is noise; skip it.** The durable half: **every confirmed
finding becomes a permanent deterministic CI test** — the agent is a discovery instrument, not a
standing gate.

⚠️ **Dispatch is GATED, not open.** STRIX-1's GO authorizes *building the containment harness and
running under the plan*; **it does not license a raw scan**, and it ranks **below the acceptance
keystone**. **The higher-leverage durable half the founder flagged is separable and does not depend
on the Strix run**: the GraphQL request cost/depth limiter with its cost-limit CI test, a matching
`specs/observability.yaml` request-cost contract (none exists), and a permanent authz-matrix CI
suite.

---

## 48. Reader-set derivation carry-forwards — [#564](https://github.com/TheCaptainCompany/captain-food/issues/564) nine-lens mob checkpoint (2026-08-15)

> **Numbering note.** This section was published as a second "§42" from 2026-08-15 to 2026-08-18,
> colliding with §42 "A process manager is a write-side component". It is **§48** as of 2026-08-18;
> `docs/STATUS.md` was updated in the same change.

The mob checkpoint of PR [#566](https://github.com/TheCaptainCompany/captain-food/pull/566) stopped
nothing, and surfaced **one genuine option space that a test is currently foreclosing**, plus build
constraints for PR2 that must not evaporate with the session (the anti-repeat discipline of §37).
**PR1 (the grammar + its gates) and PR2 (the derivation that consumes it) are deliberately split**:
the declaration is provably incomplete today, so a derivation built on it now would be honest about
the wrong set, and a money-path runtime narrowing does not belong under a doc-comment diff.

**Carried undeclared reads — a NON-EXHAUSTIVE list; PR2's independent derivation is what closes it.**
The branch originally enumerated exactly two; the independent third look found a third it never
named. **(1)** `ReclamationProcess`/`ReclamationResolved` reads `OrderTracking` with no `read:` step.
**(2)** `PlaceOrderProcess`/`PaymentAuthorized` loads `Payment-<intentId>` for the frozen
`CheckoutSnapshot`. **(3)** `DeliveryDispatchProcess`/`OrderMarkedReady` folds the Order aggregate's
own stream to read `OrderPlaced.mode`, because `OrderTracking` does not carry `mode`. **(3) is also
the grammar counterexample RDR-1 wants**: `mode` exists on **no** projection table, so this read is
*inexpressible* under the borrowed-projection-shape rule — stronger option-B evidence than the
`balance_cents` hole. Practical grant risk today is nil, hence carried rather than blocking.

| # | Decision | Options & the trade-off | Recommendation / status |
|---|---|---|---|
| **RDR-1** 🟡 **TEAM-OWNED — open 2026-08-15** | **What may an `EVENT_STREAM` `read:` step's `model:` point at?** Today it borrows a PROJECTION TABLE's shape while the leg folds the aggregate and never reads that table — `model:` is documented as "borrowed SHAPE only". That is not merely the status quo: it is **GATED**. `either_source_is_legal_on_a_projection_shaped_model` (`tools/codegen-rs/src/tests.rs`, cited by name because line numbers do not survive) asserts both sources are legal on a projection-shaped model *and its doc comment says so on purpose* — "a future tightening that demanded a stream-shaped model for `EVENT_STREAM` would break every hand-written leg, and this is the test that would stop it". A design commitment with a gate on it is a register row, not a rustdoc | **(A) Keep the borrowed projection shape** (status quo + its gate). Pros: no churn; the projection table genuinely IS the field set the fold produces for these legs; one vocabulary for every `read:` step. Cons: the spec says a leg reads a table it never reads, which is the exact conflation #564 exists to remove — solved one level up (`source:`) and left standing one level down (`model:`). **(B) An `EVENT_STREAM` step's `model:` `$ref`s the AGGREGATE** (`actors.yaml#/Cart`), the entity the fold actually produces. Pros: this is the **final-vision** shape under [ADR-20260808-235113](../adr/ADR-20260808-235113-final-vision-first-no-intermediate-steps.md) — the declaration stops naming a thing it does not touch, and a derivation can then ignore `model:` entirely for stream steps rather than filtering it out. Cons: an aggregate has no column list, so every downstream consumer of `read.model` columns (`where:` checking, the C4 sequence emitter, the generated hook signatures) needs a second resolution path; breaks all four committed stream steps at once. **(C) A third `model:` form per source** — refused on sight as (A) and (B) at the same cost. | ✅ **recommend deciding this BEFORE PR2's emitter, not after**: PR2 derives reader sets from `source:` and will encode an assumption about `model:` either way, and reversing it afterwards means re-deriving grants that will by then be in generated manifests. **Unguarded hole to close whichever way it goes**: nothing checks that a borrowed projection shape is INHABITABLE by the fold, so a guard could reference a projector-COMPUTED column (a SUM, a denormalized join) on an `EVENT_STREAM` step and validate green — `CustomerCreditBalance.balance_cents` is exactly such a column and is now borrowed by a committed step |

---

## 49. The founder rulings of 2026-08-18 — ✅ THREE RULINGS PLUS A CLEARED QUEUE

Records:
**[ADR-20260818-094500 "Staff sign-in has a mechanism; refund approval stays with the restaurant; the executor refuses a stale base"](../adr/ADR-20260818-094500-staff-auth-mechanism-and-refund-approval-stays-with-the-restaurant.md)**
(rulings A, B, C — eleven lenses replied, and the `Consulted:` block records what each caught) ·
**[ADR-20260818-101500 "The restaurant signs in by email link, and #638 freezes at chunk 1"](../adr/ADR-20260818-101500-the-restaurant-signs-in-by-email-link-and-638-freezes-at-chunk-1.md)**
(the cleared queue).

✅ **STAFF-AUTH — ANSWERED for two of the three roles.** Ruling A, verbatim: *"For the rider the
mobile app will ask the phone number handled by Supabase with OVH sms is required because it's their
tool for working. For the restaurant, they have an app but they will not download it yet they will
start with the web."* The **rider** signs in by phone, Supabase-handled, OVH SMS as the sender —
required for V0 because the phone is the rider's working tool. The **restaurant** starts on the web,
and its mechanism was settled the same day as an **email link, not a phone OTP**: the deciding
argument is that `SMS_MAX_SENDS_PER_DAY_GLOBAL` is platform-wide and is described in its own
declaration as the only ceiling on the bill, so putting the restaurant on that bucket would make a
restaurant-side surge and a **rider lockout at Friday peak** the same number. It does **not** license
cloning `verifyPhone`, which is register-or-identify and creates the Customer; staff sign-in is
**identify-only against a pre-provisioned roster**. Restaurant onboarding is named open by the
founder.

✅ **Ruling B — `approveRefund` is NOT narrowed to `[ADMIN]`.** Verbatim: *"this is an exception where
the admin makes an intervention. The approval of the refund must be done by the restaurant by
default."* `roles: [RESTAURANT, ADMIN]` stands and the live widget stays on the restaurant back
office. **The consequence is not neutral, and it is why this is a ruling and not a comment**: the
cheap fix for the write-side hole is off the table, so the hole **must** close by **binding** — an
identity actually bound to the restaurant that owns the order — which moves the write-side
authorization seam from *beside* the critical path to *on* it. Lands on §39.

✅ **Ruling C — the executor refuses a base it was not given.** Landed in `.claude/agents/executor.md`
in the same change: a run whose base commit is not the one its dispatch card names is refused rather
than rebased silently.

✅ **#638 freezes at chunk 1** (merged, PR [#644](https://github.com/TheCaptainCompany/captain-food/pull/644));
chunk 2 is **not dispatched**. The reason is **ordering, not correctness**: a second authorization
layer under a first that does not exist defends nothing, every restaurant caller is
`Identity::Unbound` today, and row security **structurally cannot** close the refund hole, because
`approveRefund` is a participant check against folded state. This is the founder exercising the
override he reserved in
[ADR-20260810-215503](../adr/ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md),
recorded so a concurrent session cannot read the frozen chunk as available work.

⚠️ **One role is still unruled.**

| # | Decision | Options & the trade-off | Recommendation / status |
|---|---|---|---|
| **STAFF-AUTH-AM** 🟠 **OPEN — FOUNDER-OWNED; raised 2026-08-18 as the residue of ruling A** | **How does an ACCOUNT MANAGER sign in?** Ruling A named the rider (phone/SMS) and the restaurant (web, then email link). **Account managers were not mentioned and are not ruled** ([ADR-20260818-094500](../adr/ADR-20260818-094500-staff-auth-mechanism-and-refund-approval-stays-with-the-restaurant.md) says so in terms), so one of the four roles still has **no authentication path at all**. | **(a)** Email link, same mechanism as the restaurant — one staff sign-in path, identify-only against a pre-provisioned roster, no second surface to build or attack. **(b)** Phone/SMS like the rider — consistent with a field role, but puts a second population on the `SMS_MAX_SENDS_PER_DAY_GLOBAL` bucket the restaurant was deliberately kept off. **(c)** No account-manager sign-in in V0 — account managers work through an ADMIN acting as, under the explicit logged act-as path of §3. | 🟠 **OPEN. Lean: (a)**, and it is cheap because it is the restaurant's mechanism with a different roster. The cost of leaving it open is present tense but bounded: it blocks standing up an account manager for a pilot, and it is the one remaining role for which **binding** (ruling B's consequence on §39) has nothing to bind to. **(c) is the honest V0 answer if account managers are not a V0 persona at all** — which is itself the question. |

---

## Maintenance

The `architect` reconciles this file on each run: new proposals add rows, answered decisions collapse
to their outcome and the record that holds them, and a decision open for many runs gets flagged in
the report with its age. **A decision nobody is making is the most expensive thing in the backlog,
and it will never surface on its own.**

**The shape is a rule, not a preference**
([ADR-20260818-193000](../adr/ADR-20260818-193000-the-register-is-a-queue-and-a-closed-row-collapses-to-its-record.md)):

- **The queue table at the top is the product of this page.** A row is added there the moment it is
  opened anywhere below, and removed only when it is answered.
- **A closed row collapses to its outcome plus the record that holds the reasoning.** The argument
  belongs in the ADR and the proposal; the history belongs in git. Do not append a "previously"
  block, and do not append a new reconciliation header — amend in place.
- **Section numbers are anchors other records cite.** Never reuse one, and never renumber a section
  without grepping `docs/**` and `.claude/**` for `DECISIONS §NN` in the same change. Uniqueness is
  enforced by `register_section_numbers_are_unique` in `tools/codegen-rs`; a duplicate fails
  `make validate`.
