# ADR-20260812-214021 — The founder answer sheet of 2026-08-12: the flip is taken, the registry is destroyed, and nothing is paid for until a working version can be seen

- **Status**: Accepted (founder answers, 2026-08-12)
- **Date**: 2026-08-12
- **Governed by**: [ADR-20260812-143619](ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md)
  (every founder message goes to the whole team; a record created from a founder directive carries a
  `Consulted:` block)
- **Closes register rows**: [DECISIONS](../proposals/DECISIONS.md) §27 Q7 · §28 Q1, Q2 · §31 BND-6,
  BND-7 · §32 JRN-1 (the one founder-owed leg) · and the new §35 rows INV-1, CUT-1, DB-HA, SIR-1,
  Q-L1, Q-L3, KEY-1
- **Relates**: [ADR-20260807-114122](ADR-20260807-114122-mks-starts-at-one-node.md) (the €26.60 entry
  rung and the €67.80 trio) · [ADR-20260807-002705](ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md)
  (CloudNativePG on MKS) · [ADR-20260809-050000](ADR-20260809-050000-morning-brief-eight-decisions.md)
  (#429's target is the production deployment) · `ADR-20260812-000000` *the PM-mailbox flip rides the
  journal retirement* — the flip this sheet confirms; it lands with
  [PR #500](https://github.com/TheCaptainCompany/captain-food/pull/500) and is **not on `main` yet**,
  so it is named rather than linked · [ADR-20260808-235113](ADR-20260808-235113-final-vision-first-no-intermediate-steps.md)
  (final vision first) · [ADR-20260810-215503](ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md)
  (the team ranks)

## Consulted (ADR-20260812-143619)

All ten lenses answered on the sheet before it was recorded. One line each; *"nothing in my lens"*
is a complete answer and none of these was that.

- **architect** — the re-ranking under the binding value method:
  [#429 "Production with test data: a test customer places a real order against a test restaurant, paid with Stripe test mode"](https://github.com/TheCaptainCompany/captain-food/issues/429)
  is re-pointed off OVH onto local k3s **without re-scoping**; the
  [#494 "Storage boundaries and least-privilege database users"](https://github.com/TheCaptainCompany/captain-food/issues/494)
  storage chain drops below it because it is downstream of a payment decision it cannot unblock.
- **holub** — CUT-1 = B is a *rule*, not a list: "IN = only what the empty log or a traffic pause
  makes cheaper" is a test any later candidate can be run through, which is why only the storage
  split ticks.
- **dba** — **withdraws STO-4's sequencing** (see §5.1): the pooler is a precondition of the
  **bin-fleet flip**, not of the storage split; with the monolith deployed, eleven databases × one pod
  is ~55 backends of 220.
- **farley** — the working version is a **merge, not a build** (`origin/cutover-local-rehearsal`
  already carries it); and **local is demo, never evidence** — the rehearsal strips the backup
  configuration, so restore stays untested until a cluster exists.
- **beck** — the acceptance criterion must be executable or it is not a criterion: smoke L1→L4 plus a
  recorded browser walk, both of which fail loudly.
- **graphql-architect** — the L4 assertion the founder's own release gate rests on (`CAPTURED`) needs
  a webhook ingress that no local overlay provides.
- **business-specialist** — DB-HA = A is a recorded decision, **not an incurred cost**; the +€41.20
  is unpayable until the €26.60 base is, and the PVC leg is unpriced anywhere in the repo.
- **legal-specialist** — SIR-1's all-NO closes the retroactive risk **on attestation, not
  inspection**; the Art. 21 blocker survives forward-looking; Q-L1 resolves only partly, and *verify,
  do not copy*.
- **ux-designer** — BND-6 = B only works if the label carries it: "ready in ~25 min" is honest,
  "arrivée estimée" over a kitchen time is the shipped defect it must not repeat.
- **observability** — a spend gate needs a signal: the L4 `CAPTURED` assertion and the browser walk
  are the only two things that distinguish "deployed" from "working", and neither is emitted today.

**Nothing in this record is legal advice or clearance** (ADR-20260812-143619): the legal lens's
grades are preserved as grades, and agreement between lenses does not upgrade a hedged finding to a
settled one.

## The answers, verbatim as received

- **JRN-1 = A** — take the `PM_MAILBOX_DELIVERY` flip now in
  [PR #500 "#242 Runtime D: retire command_journal"](https://github.com/TheCaptainCompany/captain-food/pull/500),
  under the empty-log window, with the **L4 smoke as the release gate before traffic is routed**.
- **OVH** — *"I'm waiting for a working version before paying OVH"*.
- **CUT-1 = B** — the rule: **IN = only what the empty log or a traffic pause makes cheaper**. Ticked
  explicitly IN: **the storage split (per-boundary + per-adapter databases)**. **Not** ticked: the
  pooler, the API-tier split, the runtime decomposition.
- **DB-HA = A** — three instances, inside the cutover.
- **SIR-1 / Q-L2** — the database no longer exists · no registry rows · no `Restaurant-*` streams ·
  **no outreach was ever sent** · **no refusal was ever recorded** · instruction: *delete and record
  the destruction*.
- **Q-L1** — no fields supplied; remark: *"Use the same info from join.captain.food"*.
- **Q-L3 = no** — there is no real phone-verified end user.
- **BND-6 = B** — the kitchen time, labelled **"ready"**.
- **BND-7 = A** — estimate only, no remedy.
- **Q1 = A** — authenticated, server-side only.
- **Q2 = A** — yes in principle, after the DPIA.
- **Q7 = A** — not now.
- **KEY-1** — delete the stray key now.

## Decision 1 — JRN-1: the flip is taken, and the release gate is L4

The one leg JRN-1 reserved to the founder — *"flipping `PM_MAILBOX_DELIVERY` to true is a money-path
posture change … it needs a staging smoke and a one-line ADR"* — is answered **A**: take it now,
inside the empty-log window, with the **L4 smoke as the release gate before traffic is routed**.

That is not the staging smoke the row asked for, and the difference is the decision: a staging smoke
of the *gated* form has nothing to smoke against on an empty log (no PM deliveries exist to observe),
while L4 on the deployed cluster smokes the *ungated* form with a real Stripe test PaymentIntent. The
gate moves from *before the merge* to *before traffic*, which is the externally-forced-sequence clause
of [ADR-20260808-235113](ADR-20260808-235113-final-vision-first-no-intermediate-steps.md) rather than
a waiver of gate-then-stabilize.

Two verified consequences worth stating, because they change work already recorded:

1. **JRN-1 option (a) becomes obsolete rather than pending.** Its interim grant — `SELECT` on
   `command_journal` for the platform graph, with "removal is a named line in the default-flip ADR's
   checklist" — is not owed at all if [PR #500](https://github.com/TheCaptainCompany/captain-food/pull/500)
   merges: the PR **drops the table** and **deletes the
   posture row** (`specs/database/tables/referential.yaml`, `RuntimePosture` is left declared and
   empty). Verified on `origin/242-retire-command-journal`. The matrix requirement reduces to
   `CONNECT captain-write` + `SELECT` on `inbound_messages` and `mailbox_partitions`.
2. **The observability half is largely inside the same PR.** #500's branch already rewrites
   `specs/observability.yaml`'s `command-acceptance` contract to one door (`dispatch_outcome`:
   `enqueued | duplicate_skipped`, `spawned` removed) and **deletes the `CommandChannel` and
   `CommandJournalStatus` scalars** outright, so the closed set is not left wrong. What a follow-up
   still owes is narrower than the queue page stated — see §6.

## Decision 2 — the inversion: no infrastructure spend until a working version can be seen

*"I'm waiting for a working version before paying OVH"* inverts the critical path. Until now the
sequence was **provision → deploy → walk**; it is now **walk → provision → deploy**. This is recorded
as a decision in its own right because it re-points work that was already ranked (§5.3) and because it
creates a gate whose exit condition does not exist yet.

**The one FOUNDER-OWED leg: *"a working version"* carries no acceptance criterion.** A spend gate
without one is a gate with no exit — it can be argued satisfied and argued unsatisfied on the same
evidence. The team's proposed criterion, offered so it can be confirmed or replaced:

> **Smoke L1→L4 green on local k3s, plus a recorded browser walk**: an order placed, paid,
> the restaurant told, tracking moving **without a reload**, and the order completing.

Both halves are needed and neither substitutes for the other: the smoke proves the API and money path
and is *structurally incapable* of seeing an unmounted page (it never opens a browser), while the walk
proves the customer-visible half and cannot assert a Stripe capture.

**The path is a MERGE, not a build** (farley; verified against the tree):

- `origin/cutover-local-rehearsal` / draft
  [PR #486](https://github.com/TheCaptainCompany/captain-food/pull/486) already carries
  `docs/runbooks/cutover-local-rehearsal.md` (224 lines), the local-rehearsal CNPG overlay
  (`deploy/platform/local-rehearsal/`, which swaps `captain-db-retain` for k3s `local-path` and drops
  the OVH `backup:`/`managed:` blocks), the generated monolith overlay
  (`deploy/generated/monolith/{namespace,server,ingress,kustomization}.yaml`) and the smoke's
  `SMOKE_SCHEME` / `SMOKE_PUBLIC_BASE` / `SMOKE_ADMIN_BASE` overrides.
- Its recorded result: **L1 and L2 pass against a fresh, empty database**, with the full **45/45**
  migration chain applied to `20260810113000`.
- On `main`, `tools/smoke/prod-smoke.sh` hardcodes `https://api.${SMOKE_BASE_DOMAIN}` /
  `https://${slug}.${SMOKE_BASE_DOMAIN}` with `SMOKE_BASE_DOMAIN` defaulting to `captain.food` and
  **no scheme or base override at all** (`:41,48-49`) — i.e. an unroutable base URL with production
  down, and no way to point it at a port-forwarded stack. That is the concrete sense in which the
  working version is a merge away rather than a build away.

**Two remaining gaps, both outside the merge:**

- **`SUPABASE_SECRET_KEY` as its own repository secret** — hard-stops L3 (role-JWT minting), and L4
  is downstream of L3. #486 already switches `.github/workflows/prod-smoke.yml` from
  `secrets.RENDER_API_KEY` to `secrets.SUPABASE_SECRET_KEY`, which is the correct shape: the old
  lookup read the key **off the Render service**, making the smoke structurally unable to verify
  anything that is not Render. ⚠️ **Not verifiable from this session** (no repository-settings
  access): STATUS records a GitHub Actions secret of that name as existing on 2026-08-09
  (`render-config-sync` run 31335187939). It is a **founder/coordinator confirmation**, not
  necessarily a founder action.
- **A webhook ingress for L4's `CAPTURED` assertion.** The local overlay applies the Ingress object
  and nothing serves it (no ingress-nginx, no cert-manager, no external IP), so Stripe cannot reach
  the deployment to deliver the event the assertion waits for.

**Rejected fallbacks, with the reason recorded so they are not re-proposed:**

- **A cheap interim host (Render + Supabase)** — a recorded crash-test rejection; re-adopting it is a
  **decision reversal**, not a shortcut, and it is a second deploy path to maintain at the exact
  moment the first one is being proved.
- **A recorded-walkthrough-only artifact** — it would satisfy the letter of the gate and prove
  nothing about the pipeline, which is what the gate is for.

**Local is demo, never evidence** (farley). The rehearsal overlay removes `barmanObjectStore`, so WAL
archiving, base backups and the restore drill are **entirely untested**, and at `instances: 1` that
is the only recovery path. The runbook says so itself. Therefore: **the restore drill is the first
post-provisioning act**, not a follow-up — and no claim about recovery may cite the rehearsal.

## Decision 3 — CUT-1 = B: the cutover admits only what the empty log makes cheaper

Chosen: **the rule**, not a list. *IN = only what the empty log or a traffic pause makes cheaper.*
Ticked explicitly IN: **the storage split — per-boundary plus per-adapter databases**
([#494](https://github.com/TheCaptainCompany/captain-food/issues/494) + ADP-1's six adapter
databases, eleven total). Not ticked, and therefore **out of the cutover**: the pooler, the API-tier
split (§34), and the runtime decomposition into the 57-bin fleet.

The rule is preferable to a list because it decides the *next* candidate without another sitting: a
change is IN if doing it later means a data migration or a paused checkout, and OUT if it is the same
work at any time. The storage split passes (eleven databases created empty vs eleven created by
migration under load); the pooler fails it (a deployment-topology change, identical cost whenever it
happens); the runtime decomposition fails it (crate and manifest work, unaffected by log contents).

## Decision 4 — DB-HA = A: three instances, inside the cutover

Chosen: **`instances: 3`**, landed as part of the cutover rather than as a later climb.
`deploy/platform/cnpg/cluster.yaml` is `instances: 1` today with the 3-instance quorum-synchronous
ladder fully written and unreferenced in `deploy/platform/cnpg/ha/`.

**Three money facts, stated plainly because the answer is a decision and not a payment:**

1. **DB-HA = A is recorded, not incurred.** The +€41.20/month it implies is unpayable until the
   €26.60/month base is, and Decision 2 says the base is not paid until a working version is seen.
   Recording it now is correct and costs nothing; treating it as spend authorised would contradict the
   same sheet.
2. **Three instances imply three nodes, and that is inside his answer.** `cluster.yaml` sets
   `enablePodAntiAffinity: true` with `podAntiAffinityType: required` and
   `topologyKey: kubernetes.io/hostname`. On one node, `instances: 3` leaves **two pods `Pending`
   forever** — the cluster never reaches a healthy phase and quorum-synchronous replication would
   block every write. So DB-HA = A is the **€67.80/month** trio shape (3 × d2-8 + LB S,
   ADR-20260807-114122's own figure), not €26.60 + a config flag.
3. **The volume leg is unpriced.** The cluster requests a **20 Gi** PVC on a `Retain`
   `cinder.csi.openstack.org` / `type: high-speed` class; at three instances that is **60 Gi**, and
   **no per-GB price for it appears anywhere in the repo**. ADR-20260807-114122 cites
   `docs/runbooks/mks-bootstrap.md §2` for the sizing detail and
   **that file does not exist** — `docs/runbooks/` is absent from `main` altogether (it appears only
   on the #486 branch, and only as the rehearsal runbook). The trio figure is therefore a
   compute-plus-LB figure with a storage line missing, and the gap is a citation to a document that
   was never written.

## Decision 5 — SIR-1 / Q-L2: delete, and record the destruction

The founder's answers are all NO — the database no longer exists, no registry rows, no `Restaurant-*`
streams, **no outreach ever sent, no refusal ever recorded** — and the instruction is *delete and
record the destruction*.

**This closes the retroactive risk on ATTESTATION, not on INSPECTION**, and the record must say which
(legal lens; not clearance). Nothing can now be inspected: the store that would evidence absence is
the store that is gone. Four things make the attestation evidential rather than a sentence, and they
are the follow-up actions of this ADR:

1. **State how and when the rows ceased** — which store held `external_sirene_restaurants`, and the
   date/mechanism of its disappearance (project deletion, not row deletion).
2. **Make the absence inspectable while it still is** — capture the current project list as the
   contemporaneous artifact; after the next provisioning it is unobtainable.
3. **State whether any backup or PITR window survives** the deleted store, because a surviving
   window means the rows have not ceased, they are merely offline.
4. **Name who attested and on what date** — an unattributed attestation is a claim with no author.

**Two neutralisations are owed BEFORE any re-sync, and both are live in the tree today:**

- **`.github/workflows/sirene-sync.yml`** is paused only by a commented-out `cron`;
  **`workflow_dispatch` is deliberately kept** ("pausing the schedule is not the same as removing the
  capability") and the job writes `external_sirene_restaurants` from `secrets.DATABASE_URL`. Pointing
  that secret at a new store and dispatching the workflow re-creates ~200k real INSEE rows, in one
  click, with every gate green.
- **The `DATABASE_URL` repository secret** is the credential that did the writing. Per SIR-1 it must
  be **revoked and the revocation logged** — that log line is part of the attestation in item 1, not
  a separate hygiene task.

**The Art. 21 blocker SURVIVES, forward-looking**
([#505](https://github.com/TheCaptainCompany/captain-food/issues/505)). Verified:
`RestaurantListingOptedOut` folds into **nothing** —
`crates/application/src/generated/projectors.rs:59` is `DomainEvent::RestaurantListingOptedOut(_) =>
state`, i.e. the objection register has no read-side effect. Unpausing the registry sync stays blocked
by that, independently of the destruction: the retroactive question is closed, the prospective one is
not.

## Decision 6 — Q-L1: partially resolved by the landing page, partially still owed

The remark *"Use the same info from join.captain.food"* resolves the identity block and does not
resolve the rest. Fetched and read on 2026-08-12:

**Published today** (`join.captain.food/mentions-legales` and `/confidentialite`): éditeur and
controller = *association Caring Hope Foundation*, loi 1901, déclarée à Tours, **RNA W372020229**;
rights contact **miam@captain.food**; host block = **GitHub Pages / GitHub, Inc.**; lawful basis for
the pilot form = consent; retention = pilot duration, max 24 months; CNIL complaint route named.

**Still FOUNDER-OWED**, because the pages do not carry it:

- **A postal address.** The mentions légales say *"Siège social : Tours (Centre-Val de Loire),
  France"* — a city, not the siège social as filed.
- **A publishable phone number.** None appears on either page.
- **A named directeur de la publication with its statutory title.** The page says *"le·la
  représentant·e légal·e de l'association"* — a description, not a name.

**Legal's instruction is *verify, do not copy*** — and two of its reasons are checkable rather than
cautionary. (i) The **host block is wrong for the app**: the landing page is on GitHub Pages, while
the application host becomes OVH/CNPG, so copying that block would publish a false hosting
declaration. (ii) **No consumer mediator is named anywhere on either page** — a launch blocker in its
own right for a French B2C service, and not something the landing page can be mined for because it
does not have it either.

## Decision 7 — Q-L3 = no: there is no real phone-verified end user

Recorded as the fact it is: **no real phone-verified end user exists**. This is load-bearing in two
directions and should be read as evidence with a shelf life. It supports the empty-log window that
Decisions 1 and 3 both rely on, and it dates the trigger the legal briefs already fix — the **first
real customer order**, which is simultaneously the Art. 35 DPIA deadline, the Art. 17 erasure trigger
and the médiation de la consommation registration deadline. The answer is true today and stops being
true at the first walk that is not the team's own.

## Decision 8 — BND-6 = B and BND-7 = A: the ETA is a labelled kitchen estimate, with no remedy

- **BND-6 = B** — show the **prep-time-only** estimate, **labelled as "ready"**, not as arrival. It
  is buildable today from `preparationTimeMinutes` and is currently rendered on **zero** customer
  screens. The label is the whole decision, not a detail (ux lens): the shipped defect D13 found is an
  `eta_bar` labelled *"Estimated arrival" / "Arrivée estimée"* bound to the kitchen **ready** time and
  rendering during `OUT_FOR_DELIVERY`. Option B implemented with that label is option C in disguise.
- **BND-7 = A** — the displayed estimate is an **estimate, not a promise**: no goodwill credit, no
  refund, no commercial consequence. The frozen value exists as an internal promised-vs-actual quality
  signal only, and is never presented as a commitment.

Together they unblock D13.5: the read-side composition can be built, and the freeze onto `OrderPlaced`
carries no remedy semantics. The freeze remains a payload change on an already-emitted event, so it
is still a **migration** needing its upcasting story — cheap while the log is empty, and that is the
same window Decisions 1 and 3 spend.

## Decision 9 — Q1 = A, Q2 = A, Q7 = A: behaviour tracking is server-side, restaurant-facing, and ours

- **Q1 = A** — **authenticated, server-side only**: no new client identifier, and no analytics read of
  the existing `X-SESSION-ID` cookie. **Graded, because A is not a blanket exemption** (legal lens):
  it plausibly removes the **Art. 82 / cookie-banner** obligation, since no terminal-equipment access
  happens for an analytics purpose — and it removes **neither** the **Art. 13 transparency** duty
  **nor** the need for a **lawful basis** for the processing itself. The privacy page's own claim
  (*"La mesure d'audience éventuelle est réalisée de façon anonyme et sans cookie"*) is consistent
  with A and is not a substitute for either. What A costs is recorded and accepted: the pre-cart
  funnel is unattributable, so browse-to-cart conversion is not computable; C (a dedicated analytics
  device id + banner) remains the recorded upgrade path.
- **Q2 = A** — **yes in principle, built after the DPIA**: the restaurant sees its own storefront's
  behaviour data. Deciding it now is what makes the taxonomy designable for it. The tail is recorded
  rather than waved through: it makes the restaurant a controller or joint controller of that
  processing, needing a controller/processor arrangement that does not exist today.
- **Q7 = A** — **not now** to a hosted product-analytics SDK. This **converges with MET-Q7**, already
  answered *"no hosted analytics SDK — ours, server-side"* on 2026-08-11; §27's Q7 row was still open
  and is now closed by the same answer reached twice, one day apart, from two directions.

Sequencing is unchanged and is the part that must not be lost: **the mechanism plus zero live events,
then the DPIA, then the first three events**. Validator rule R10 keeps that order executable — the
emitter produces nothing while no `docs/legal/DPIA-*.md` exists — and instrumentation before the DPIA
is processing that should not have started.

## Decision 10 — KEY-1: delete the stray key now

Accepted as instructed: **delete it now.** ⚠️ **The key's identity is not recorded anywhere in the
repo**, and this record does not invent one. The row is carried in §35 with the identification owed by
whoever raised it; the adjacent revocation that *is* identified is SIR-1's `DATABASE_URL` (Decision 5),
which is a different credential and does not discharge this one.

## What the mob changed in the reading — recorded as corrections, not silent edits

### 5.1 STO-4's sequencing is WITHDRAWN by the DBA lens (substance unchanged)

STO-4 recorded the pooler as *"a prerequisite, not a follow-up"* **to the storage split**. That
sequencing is withdrawn: its arithmetic (~37 bins × 5 = 185, projector pools doubling to 80, ~235
against `max_connections: "220"`) is entirely a **57-pod runtime-split** figure, and the runtime split
is **not in the cutover** (CUT-1 = B). With the monolith deployed and
`DATABASE_POOL_MAX_CONNECTIONS` defaulting to **5**, eleven databases × one pod is **~55 backends of
220** — comfortable. The pooler is re-targeted as a **blocking precondition of the bin-fleet flip**,
and the monolith's per-database pool should be **capped** so eleven pools cannot silently become the
next connection storm. Session mode, not transaction mode, is unchanged and non-negotiable
(transaction mode accepts `LISTEN` and delivers nothing, which breaks the push-driven mailbox).

### 5.2 PROP-20260809-021351's gap table was STALE and is corrected in place

It is a living document, so the table is corrected rather than annotated. Re-verified item by item:

- **G5 (checkout inert / data-less SSR shell) — FIXED** by
  [#420 "Customer delivery reassurance"](https://github.com/TheCaptainCompany/captain-food/issues/420)
  + [#451 "Storefront checkout: price the cart at read"](https://github.com/TheCaptainCompany/captain-food/issues/451).
  The hardcoded
  `restaurant_name: ""` / `cart_line_count: 0` / `formatted_total: ""` have **zero occurrences** in
  `crates/web/src/router.rs`, the `if !screen.sdui { return; }` guard above the crate's only
  `mount_to_body` is gone (`renderer.rs:645-646` records its removal), and the shell now carries the
  cart and the publishable key under named tests.
- **G6 (confirmation stuck on the not-found hero) — FIXED** by #420. `TrackingState::new(order_id)`
  survives only in tests and in the comments recording the fix; production SSR builds
  `TrackingState::from_resolved`.
- **G7 (the live update never arrives) — FIXED**, and the queue page was wrong to list it as
  remaining. Both named defects are gone: the subscription now accepts **this order's
  `DeliveryJob-` stream** as well as `Order-<id>` (`subscription.rs:172-184`) and dedupes on the
  row's **`updated_at`** rather than `status` (`:147-154`), with the reason written in place — a
  status-keyed dedupe *"SWALLOWED exactly the movement the tracking screen exists to show"*. The
  browser half exists too (`handwritten.rs::mount_tracking`, pull-then-push over the
  graphql-transport-ws client, re-pull on every reconnect). What is **unproven** is end-to-end in a
  browser against a deployment — which is exactly what Decision 2's recorded walk is for.
- **C1 — HALF fixed, and the half that remains moved house.** The **total** is fixed: the cart
  projector is now a pure money-free fold and the read side prices lines fresh via
  `application::pricing::price_cart` (ADR-20260810-112836 / PROP-20260810-231500 Option B, #451). The
  `TODO(runtime)` cart accessors the row cited **no longer exist**. The **competitor comparison is
  still never computed** — it is now `uber_comparison: None` in the read seam
  (`crates/server/src/graphql/cart_read.rs:187`, *"a policy read this seam does not perform yet
  (#463 …)"*), with the Order-side `uber_*` columns still `TODO(runtime)`
  (`projectors/order_tracking.rs:95`). So the row's verdict must not be marked simply "fixed".
- **G7b (no staff browser sign-in) — STILL REAL**: no `/auth/callback` route in `crates/**` (the only
  callback is HubRise OAuth) and no sign-in screen in `specs/screens/**`.
- **G8 (nobody is told about a paid order) — STILL REAL, and it is the domain lens's named worst
  failure mode.** `crates/application/src/ports.rs` is 140 lines declaring exactly four traits —
  `EventStore`, `GoogleOwnershipVerifier`, `GbpOrderLinkProbe`, `RestaurantRepository` — and **zero
  notification anything**.
- **C2 (fee breakdown hard-zeroed) — STILL REAL**: `crates/application/src/pricing.rs:105-113` builds
  the breakdown with `delivery`, `service_fee`, `restaurant_contribution`, `rider_payout` and
  `captain_net` all at zero.

This is the stale-sentence-on-an-unchanged-line class CLAUDE.md warns about: every one of G5/G6/G7/C1
was true when written, the lines they cite still exist in some form, and only re-reading the code
distinguishes a fixed defect from a live one.

### 5.3 The re-ranking, and the method clause that justifies it

Under [ADR-20260810-215503](ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md) the
team ranks, and [docs/BACKLOG.md](../BACKLOG.md)'s method binds the ranking:

- **[#429 "Production with test data"](https://github.com/TheCaptainCompany/captain-food/issues/429)
  is re-pointed off OVH onto local k3s WITHOUT re-scoping.**
  [ADR-20260809-050000](ADR-20260809-050000-morning-brief-eight-decisions.md) fixed its target as
  *"the real production deployment, exercised end to end by test customers ordering from test
  restaurants and paying through Stripe test mode"*. The inversion changes the **host** the work is
  proved on, not the target. **The caveat is farley's own and is not smoothed over**: under "local is
  demo, never evidence", a local walk satisfies **Decision 2's spend gate** and does **not** close
  #429, whose target is the production deployment and which closes on the provisioned cluster.
- **The #494 storage chain drops below it.** Method clause: *value-first — foundations first, then
  features in value-stream order* — and a foundation that cannot be applied is not first. #494's
  eleven-database split lands **at** the cutover (CUT-1 = B ticks it IN), and the cutover is now
  downstream of a **payment decision it cannot itself unblock**. Ranking it above the thing that
  unblocks payment would be ranking work by size rather than by value.
- **Nothing was re-ranked to make it dispatchable**, and no `Priority` bucket was moved to legitimise
  a recommendation. This ADR reverses a previously stated order (#494 above #429), so it also gets a
  line in [`docs/STATUS.md`](../STATUS.md).

## Consequences

### Positive

- The spend gate has a **proposed executable criterion** instead of a phrase, and the criterion fails
  loudly (smoke exit code + a recorded walk).
- The cutover has a **rule** rather than a list, so the next candidate needs no new sitting.
- Six open register rows close on answers rather than on decay, and one (§27 Q7) closes on the same
  answer reached twice independently.
- The stale gap table stops mis-directing whoever picks up the demo epic: two of the three "blocking"
  customer-path rows were fixed weeks ago and one (G8) is untouched and worse than the others.

### Negative

- **DB-HA = A is recorded against a budget that cannot pay it**, and the recorded trio price is
  incomplete (no storage line, and the runbook it cites does not exist). The next person reading
  €67.80 will still be reading an under-estimate.
- **The attestation posture is inherently weaker than inspection** for SIR-1, and the window to
  capture even contemporaneous evidence closes at the next provisioning.
- **Q-L1 remains blocking for launch** on three fields plus a consumer mediator, and the landing page
  cannot be mined for any of them.
- The L4 release gate for Decision 1 depends on a **webhook ingress that does not exist locally**, so
  "the flip is gated" is a promise until that is built.

### Follow-up actions

1. **Confirm or replace the acceptance criterion** in Decision 2 (founder). Until then the spend gate
   has no exit condition.
2. **Merge the working-version path** ([PR #486](https://github.com/TheCaptainCompany/captain-food/pull/486))
   and confirm `SUPABASE_SECRET_KEY` exists as its own repository secret; then build the webhook
   ingress L4 needs.
3. **Execute the SIR-1 destruction record** — the four evidential items in Decision 5, plus revoking
   and logging `DATABASE_URL` and neutralising `sirene-sync.yml`'s `workflow_dispatch`.
4. **Identify and delete the KEY-1 key**, and record which key it was.
5. **Re-target the pooler** onto the bin-fleet flip and cap the monolith's per-database pool (§5.1).
6. **The restore drill is the first post-provisioning act** — and it currently verifies **one**
   database (`-d app` / `dbname=app`), which eleven databases makes a partial drill.
