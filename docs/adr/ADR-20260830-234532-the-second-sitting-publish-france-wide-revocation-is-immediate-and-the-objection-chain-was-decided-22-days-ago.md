# ADR-20260830-234532 — The second sitting: publish France-wide, revocation is immediate, and the finding that outranks both — the objection chain was decided 22 days ago and never built

**Status**: Accepted · **Date**: 2026-08-30 ·
**Decider**: the **FOUNDER / Tech CEO**, answering three of the six residues
[ADR-20260830-213135](ADR-20260830-213135-the-staff-auth-answers-captain-binds-the-first-person-and-the-owner-invites-the-rest.md)
named the same evening. Five lenses were consulted before this record was composed
(`Consulted:` below, ADR-20260812-143619) ·
**Closes**: residue 2 (publish scope) and residue 4 (token TTL) of that ADR ·
**Settles the SHAPE only** of residue 1 (the support contact) — the string is still owed ·
**Relates**:
[ADR-20260830-191457](ADR-20260830-191457-a-role-guard-takes-a-witness-and-an-unbound-caller-is-recorded-as-public.md) ·
[ADR-20260818-004646](ADR-20260818-004646-no-business-identifier-lives-in-the-identity-provider.md) ·
[ADR-20260728-011344](ADR-20260728-011344-slug-lifecycle-and-sirene-inbound-events.md)
(nulled slugs on `NON_PARTNER` rows — the reason a crawled listing has no page) ·
[PROP-20260808-142532 §21 D3/D4](../proposals/DECISIONS.md) + 
[BRIEF-20260808-listing-opt-out-objections](../legal/BRIEF-20260808-listing-opt-out-objections.md)
(**the decision this record finds unbuilt**) ·
**Issues**: [#639](https://github.com/TheCaptainCompany/captain-food/issues/639) ·
[#800](https://github.com/TheCaptainCompany/captain-food/issues/800)

---

## The finding that outranks the three answers

The founder was asked how wide to publish. Answering it surfaced something larger, and it belongs
first because it is true **today**, at Touraine scale, with the crawl paused, and it was **already
decided**:

> **The objection chain was approved on 2026-08-08 and has been unimplemented for 22 days.**

`PROP-20260808-142532` D3/D4 are `Approved` — "fold to a hidden listing status", and the orthogonal
`delisted` boolean. At HEAD, verified line by line:

| Claim | Evidence |
|---|---|
| The opt-out fold is a literal no-op | `crates/application/src/generated/projectors.rs:59` — `DomainEvent::RestaurantListingOptedOut(_) => state,` |
| The D4 boolean was never built | `grep -rn 'delisted' specs/ crates/` → 10 hits, **all** `RemoveRestaurant`/`RestaurantRemoved` prose |
| An opted-out restaurant stays a prospect | `ProspectionPipeline.fedBy` (`specs/database/tables/projection_tables.yaml:297-303`) does not list the opt-out event |
| Non-partner rows are listed publicly by default | `crates/infrastructure/src/persistence/restaurant.rs:83` filters `listing_status` **only** when `orderable_only == Some(true)`; the `restaurants` query carries no guard and declares no `roles:` |
| The only door to the objection cannot open | `opt_out_restaurant_listing` requires `ownership.verify(..)`, and the only wiring is `FailClosedGoogleOwnershipVerifier` |

So the exposure the legal brief graded on 2026-08-08 — *"the entire lawfulness of the
`ProspectionPipeline` rests on objections actually working"* — is unchanged 22 days later. **The
register's usual pathology is a decision nobody is making. This is its mirror: a decision made,
approved, briefed, and not executed.** The founder's answer does not create it; it multiplies its
blast radius roughly thirtyfold.

## Answer 1 — rider revocation: immediately, on the next request

Chosen over a 5-minute and a 1-hour window. The write path re-derives the binding from our Postgres
on every authorized request, so an outstanding token's remaining life stops mattering. This is the
option `vernon`'s rule in the previous ADR already implied — *no irreversible act is authorized by
the read index; the write path re-derives from the stream* — and `legal-specialist` notes it is the
rare answer that **removes** an obligation: a non-zero TTL would have made every misconduct
revocation an Art. 33 breach analysis, because a suspended rider retains the customer's name,
address and phone for the length of the window.

**Three things "next request" does not cover, and the slice owes all three.** Naming them here so
the design does not inherit the phrase as if it were a guarantee:

1. **The socket does not re-resolve.** `authorize_and_resolve_scope` resolves the caller's
   `ReadScope` **once**, and the WS leg calls it from the `connection_init` closure — its own
   doc-comment says ONCE (`crates/server/src/graphql/routes.rs:210-211`). A rider suspended at 19:40
   with an open subscription keeps `ReadScope::Rider` until they disconnect. Revocation must
   terminate the socket, not merely deny the next query.
2. **Per-request *identity* resolution is not per-request *authorization*.** Resolving `sub →
   riderId` from the `Rider` table gives a current identity; the revocation fact must be a term in
   the derivation, or the same cache has been rebuilt one layer down.
3. **A rider makes no request while standing still.** `ux-designer`: the founder-visible number is
   not "how long does a token survive" but *"how long can a suspended rider stand on the pavement
   believing they are still working"* — which is push-shaped, not TTL-shaped (ADR-20260810-231300).

`dba` corrected one clause of the recommendation as it was put: *"we already resolve read scope per
request"* is true of the seam and false of the cost. `resolve_customer_scope` does I/O for exactly one
arm — CUSTOMER under `CustomerIdentitySource::Postgres`; every other role falls to a pure claims
function with **zero I/O**. So this is a **new** round trip on the rider path, not a marginal addition
to an existing one. It is nonetheless genuinely marginal, but for the right reason: `rider.auth_ref`
is `NOT NULL UNIQUE`, so it is one unique-btree probe against a table of tens of rows that stays
resident in `shared_buffers` — the cost is the round trip, not the work. **The shape that does not
hold, and that nobody should build**: re-deriving by folding the `Rider-{id}` stream per read request
— unbounded per request, and it puts the read path on `domain_events`, which CLAUDE.md forbids
outright. The previous ADR's *"the write path re-derives from the stream"* is correct **for the write
path**, where the mailbox worker loads the aggregate once per command anyway; copying that sentence to
the read path costs three orders of magnitude.

### And the slice's real deliverable is the handback, not the lock screen

`ux-designer` found the operational defect that "immediately" creates. `DeclineDelivery`,
`ReportDeliveryIssue` and `ResolveDeliveryIssue` are declared in `specs/delivery/commands.yaml` and
`grep -rn` across `specs/` returns **zero** matches for their operation names: three commands, no
door. The rider surface is two screens. So an immediately-revoked rider holding a customer's paid,
cooked food has **no way to hand it back**, while the restaurant's board still shows `EN_ROUTE` and
the customer's tracking still counts down an ETA that will never arrive.

That is the platform's worst failure mode — *a paid order nobody is told about* — arriving through
the security feature. **Revocation of ACCESS and release of CUSTODY of the food are two different
transitions, and the security one must not execute the product one.** "Immediately, next request"
applies to `acceptDelivery`, `confirmPickup`, the job list and the online toggle; it must not apply
to the one mutation that gets the food out of a suspended rider's hands.

## Answer 2 — crawl AND publish France-wide

Chosen over *crawl wide, publish Tours only*, which `business-specialist` and `legal-specialist` had
reached independently and which was presented as the recommendation. It is consistent with what the
founder has said throughout (*"we will crawl all the restaurant and food truck from France based in
the Insee api"*). **It is his decision and it is recorded as made, not hedged.**

What it changes is size, not permission. What follows is what it obliges.

### It is not executable as stated: there is nothing to publish to

`business-specialist` and `ux-designer` reached this independently. Per-listing public pages **do
not exist by recorded decision** — ADR-20260728-011344 D5 nulled slugs on `NON_PARTNER` rows, and
`specs/network/api.yaml:137-140` resolves `restaurant` **by slug**, so no query can serve a slugless
listing. The marketplace declares three routes (`/`, `/search`, `/partner`) and no restaurant detail
route at all.

**Every one of the legal preconditions needs a page to live on** — the Art. 14 notice "reachable
from every seeded listing", the objection route, the provenance statement, the claim door. There is
no such page. A nationally published crawled listing is currently a card in a grid with no
destination.

### Two facts the founder did not have when he answered

Recorded because they were not in the option text, and because they change what the decision means
rather than whether it was right:

1. **National publish publishes natural persons' home addresses.**
   `crates/infrastructure/src/integrations/sirene.rs:111-113` composes a sole trader's display name
   as `"{prénom} {nom}"`, and the address comes from `adresseEtablissement`, which for
   micro-entrepreneurs is frequently the domicile. So this is not 200k businesses; it is tens of
   thousands of *entrepreneurs individuels* — name plus home address — on a public unauthenticated
   query (*Manni* C-398/15; GDPR Art. 4(1)).
2. **We fetch people who asked the State not to be published.**
   `grep -rni 'diffusion|statutDiffusion|nonDiffusible' crates/ specs/` → **zero hits**.
   `restauration_query` filters commune, administrative state and NAF only, and
   `crates/sirene_ingest/src/wire.rs` deserializes no diffusion field, so the status cannot reach
   the ACL even if INSEE sends it. At Touraine scale that is a possible bad row; nationally it is
   statistically certain.

### The publish switch stays gated, and the gate is counsel's

`legal-specialist` grades the change from one city to national as **structurally** different on five
counts, four of which carry a clock: the Art. 35 DPIA stops being arguable and may trigger Art. 36
prior consultation (CNIL, 8 weeks, extendable by 6); Art. 5(1)(d) accuracy must be met by a
mechanism rather than by hand, and DECISIONS §7 D4 records that the SIRENE worker has **no
`UpdateRestaurant` at all**, so closures are swallowed; the Art. 12(3) one-month clock couples the
support contact to the publish switch; and Art. 37(1)(b) DPO designation becomes arguable-to-likely.

**One sequencing fact is sharper than all of them**: Art. 14(3)(a) runs the notice clock from
**obtaining**, not from display. The notice is therefore already owed for the rows we hold. And the
Art. 6(1)(f) balancing test, the Art. 30 record and the diffusion filter attach to the **crawl**,
not to the publish — so restarting the crawl is itself a regulated act with its own owed artifacts,
and un-pausing before they exist buys nothing and starts clocks.

> **No crawled listing is shown publicly until non-diffusible units are excluded at fetch rather
> than at display, a dated legitimate-interest balancing test and an Art. 35 DPIA covering national
> publication and prospect scoring exist, an Art. 14 notice and a SIREN/SIRET-keyed objection route
> requiring no Google proof are reachable from every listing and answered within one month by a
> named staffed channel, and an automated path can correct or withdraw a listing when INSEE says the
> establishment changed or closed — because at 200k rows every one of those is impossible to do by
> hand afterwards.**

That sentence is a precondition list, not a clearance. **No lens output is legal advice**, and
sufficiency is counsel's to confirm.

### Proof gates the GRANT, never the REFUSAL

`ux-designer`'s rule, and the one design correction this record makes binding:
`OptOutRestaurantListing` has `googleOwnershipProof` in `required`
(`specs/network/commands.yaml:356`). Claiming a listing acquires rights over a business identity and
must be proven; asking us to stop publishing acquires nothing and costs the asker something.
Requiring proof there inverts the risk — a wrongful removal costs us one listing we never had
permission to publish, a wrongful retention is an Art. 21 breach with a one-month clock. And the
15–30% of French independents who cannot reach a verified Google Business Profile are **exactly the
population the gate excludes**. The proof must become nullable with an evidence discriminator; that
is a stored-event shape change and therefore `HOLD: human`.

`specs/stories.yaml:151-158` compounds it by making `OptOut` a **step inside the `ClaimListing`
activity**. "Get me off your site" is not a step of "claim my listing" — it is a different person
with the opposite outcome, and at national scale it is plausibly the higher-volume activity. It owes
its own backbone entry.

### The shape that delivers the answer without the failure mode

`business-specialist`'s Option A, recorded as the recommendation the proposal will carry, because it
gives the founder the national surface he asked for rather than arguing him out of it. The
discriminator already exists: `DeliveryPartnerRegistration` is keyed on `(partner, cityId, channel)`
with an `APPROVED` review state, so **a city with no APPROVED registration is a city with no
fulfilment, as a recorded fact rather than a toggle**. Derive a per-city coverage verdict the way
`ServiceWindow` is already derived, and give the card three states instead of two — `ORDERABLE`,
`LISTED_COVERED_NOT_PARTNER` (the only state carrying a claim CTA, because it is the only one where
claiming leads somewhere), and `LISTED_NO_COVERAGE` (a waitlist, which converts the failure mode
into ranked next-city demand measured rather than guessed).

**The load-bearing half is the default filter, not the copy.** `orderableOnly` is opt-in —
*"non-partner cards show by default"* — and the marketplace binds `restaurants.all` with no
arguments at all. Publishing wide without changing that default ships the failure mode on day one
with no further decision by anyone: page 1 of a Tours delivery app becomes 24 arbitrary
newest-crawled listings nobody can order from, ordered `created_at DESC LIMIT 24`.

**And the claim funnel runs off the CRAWL, not off the PUBLISH.** `RecordProspectContact` needs a
database row, never a public URL. So crawling wide already buys the next-city option value, the
outreach funnel and the "200k French restaurants referenced from open data" slide. Publishing wide
adds exactly one incremental path — organic self-discovery — which needs an SEO build that does not
exist and that ADR-20260728-011344 deliberately removed the slug reservation for.

## The number in the question was wrong, and it was wrong in a way that mattered

The question put to the founder carried *"roughly 200k rows"*, marked `UNVERIFIED input` under
ADR-20260817-105845. `dba` re-derived it from repo antecedents and found the marking earned its
keep: **200k is neither population.**

| Population | Rows | What it sizes |
|---|---:|---|
| **Publish surface** (registered listings) | **~250–300k** | opt-out volume, support load, Art. 14 notices |
| **Mirror** (`external_sirene_restaurants`) | **~1.0M** | storage, WAL, drain time, backup |

Antecedents, named as the rule requires: ADR-20260728-143000 measured production at **339,077 rows /
655 MB** across 37 of 101 departments (1.93 kB/row), and projected *"~2 GB for this one table"*
full-France — 2 GB ÷ 1.93 kB ≈ **1.04M**. An independent route agrees: departments 01–37 are ≈35% of
French economic activity, so 339k ÷ 0.35 ≈ 970k. The 200k figure came from
`specs/database/tables/integration_staging.yaml:29-34`, describing a **partial-coverage re-translation
batch**, and had been carried into the residue as if it were the publish population.

**And the 3.5× gap between the two is itself a finding.** `restauration_query` wraps its predicates in
INSEE's `periode(...)`, which matches **any** period rather than the current one. Without
`dateFin:null` inside it, the sweep returns every établissement that was *ever* active under a
restauration NAF — long-closed restaurants and businesses that have since changed activity — and the
mirror declares NO ROW RETENTION, so they accumulate forever. One API call against department 37
comparing `header.total` with and without `dateFin:null` settles it. **Before the national crawl, not
after: deleting 700k rows later is a migration; not fetching them is a string.**

## Three things `dba` would gate the crawl on, each with a failure Touraine cannot produce

1. **The `periode(dateFin:null …)` check** — above. It changes every number here by ~3×.
2. **A sweep ledger and an absence circuit breaker.** `ABSENCE_GRACE_DAYS = 21` is documented as
   "≈3 missed weekly runs", which holds only if each department is swept weekly. A France sweep needs
   ~4h against a 75-minute budget, so the rotation is multi-week and **a row that misses ONE sweep
   already satisfies the absence predicate**. Worse, `sweep.rs:141-146` increments `covered` even when
   a page-level failure breaks the page loop, and `reconcile_absent` derives `swept_at` from
   `max(last_seen_at)` **of the very rows it is judging** — so a department that failed at page 3 of
   10 is indistinguishable from one fully swept. Named scenario: one INSEE 5xx on page 3 of
   department 75 mass-closes ~15,000 Paris prospects four weeks later, visible only after the fact.
   The budget case is already handled at `sweep.rs:121-123`; the failure case is not.
3. **The suppression list as a `domain_events` fact keyed on SIREN**, behind an
   `AdmittedEtablissement` witness whose only constructor runs both import filters — so there is no
   expressible path from an `Etablissement` to a domain fact that skips either.

### Why the naive suppression shape fails, and the fourth way is the one to fear

A flag on the mirror row fails four ways. **(i)** Both upsert statements in `staging.rs` keep a row's
status only while `payload_hash` is unchanged, so the day INSEE records an address correction the
hash moves, `status` returns to `PENDING`, the payload is written **back**, and the removal is
silently reverted — by an ordinary weekly sweep, with no re-crawl of a deleted row required.
**(ii)** It cannot express SIREN-level: an objection from an *entrepreneur individuel* is about the
person, and therefore about SIRETs **that do not exist yet**. **(iii)** `adapter_sirene` is declared
`recovery: refetch` — a database that is *by declaration* reconstructible from INSEE, which does not
know about the suppression. **A restore drill would republish every removed person and report
green.** **(iv)** The domain cannot read it: cross-scope access is via projections, never joins.

That third one is the split worth stating plainly: **an irreplaceable data-subject right exercise had
been designed into the rebuildable box.** The drill's assertion set must additionally require that
the suppression projection is non-empty after replay — one assertion, and it is the whole difference
between a drill that proves the legal posture and one that passes by refetching.

**This does not reverse §21 D4.** The orthogonal `delisted` boolean is the *aggregate-side* half and
stands exactly as decided on 2026-08-08; this is the *registry-side* half, which D4 was not asked
about. Both are needed and they compose.

## Two scale facts for the record

**A national re-translation is ~1000× a peak evening's entire write volume** — ≈4.5 GB of WAL across
the staging upsert, `inbound_messages`, mailbox delivery and the reconcile updates, against ~5 MB for
a Friday peak. And ADR-20260728-143000 D5 decided *"the backfill IS the normal paced sweep"*, which is
correct and cheap at Touraine but means that at national scale **adding one field to
`wire.rs::Etablissement` changes the digest of every record in France and schedules the whole
thing** — an innocuous serde change, invisible in review, with no gate. `dba`'s instrument is a
fixture test pinning `payload_hash` to a constant whose failure message says exactly that.

**And the crawl shares the `Restaurant` actor lane with live partners.** Every SIRENE fact stages as
`actor_type: "Restaurant"` against 5 partitions, so at national volume ~200k messages queue ahead of
every partition and a Tours restaurant marking an item out of stock at 19:40 waits behind the
registry backlog. This is `vernon`'s head-of-line argument from the previous ADR, applied where the
volume is five orders of magnitude larger. The `Prospect` actor already exists and is deployed.
Separately, the drain runs **inside the deployed `server` process on the GraphQL pool**, loops with no
budget and no deadline, and is guarded by a per-process `AtomicBool` rather than the lease-and-fencing
the projection and mailbox workers take — while the generated `worker-sirene-sync` CronJob bin already
exists, making the move config-level rather than a build.

## Answer 3 — the support contact: a functional role address on a domain we control

"Name a channel, not a delay" survives at national scale, and more strongly: a delay is a promise
made per message, a channel is a promise made per route, and any printed delay breaks in week one
and is then visible on every listing page in the country. The copy must render it as a **role, not a
person** — a personal address on a statutory identification element makes a natural person the
published contact for a company obligation and cannot be handed over without reprinting every
screen.

**The string is NOT decided.** The local part, who reads it at Friday 19:00–21:30, and six of the
seven confirmations remain owed. `SUPPORT_CONTACT` is a required configuration key with no default,
so this blocks the refusal screen booting — it is carried as a declared dependency, never as a copy
TODO.

**The highest-leverage finding**: build the self-serve removal form and the support address **stops
being the GDPR intake by default**. A removal that is one tap and takes effect immediately never
becomes a message, never starts a one-month clock, and never lands in the peak-hour queue. Two
populations, two routes; conflating them is the defect.

**One question the seven omit**: they are all about the restaurateur. After answer 1 the worst-case
reader of that string is a **suspended rider**, on a bike, in the dark, holding a customer's food at
20:15 on a Friday. That is not an email use case. Does the route have a voice leg at peak? Without
one, the handback screen's "Appeler le support" button is bound to nothing.

## Sequencing — the objection chain goes first, and part C's proposal runs beside it

Under `docs/BACKLOG.md` §"How value is defined", both are tier 1 (data retention/compliance), so
they rank on **risk retired × what it unblocks**. The objection chain wins on three counts: its
decision is already taken and its lane is GREEN and dispatchable today, while part C cannot be
*implemented* at all this run because its next artifact is a proposal; answer 2 multiplies the
population it protects; and part C's own claim door depends on the crawl restarting, which must not
happen before objections work. They touch disjoint files — `specs/network/**` + `crates/**` versus
`docs/proposals/**` — so both run.

**The crawl stays paused.** The founder answered *how wide*, not *whether we are ready*. #800's two
guards stand, and the objection chain is now a third precondition of un-pausing.

## Consulted

- **legal-specialist** — the five structurally-different obligations and their clocks; Art. 14(3)(a)
  running from obtaining rather than display; the crawl-versus-publish split; the sole-trader
  name-plus-domicile finding; the eight-item counsel packet. Grades its own outputs (a)/(b)/(c) and
  states that none of it is advice or clearance.
- **business-specialist** — that "publish France-wide" names no mechanism today; that the claim
  funnel runs off the crawl; the claim→refuse→cold-email-three-times sequence; Option A and the
  coverage verdict; that the TTL is now a session-ergonomics number and should be set long; that the
  closed reason vocabulary excluding performance grounds leaves no lever for peak decline behaviour,
  which is an economic problem, not a sanction one.
- **ux-designer** — the handback, and that it is the slice's real deliverable; the three commands
  with no door; *proof gates the grant, never the refusal*; that there is no listing page for any
  precondition to live on; the removal journey and its closed reason set; the five things part C's
  invitation journey must get right.
- **dba** — corrected the publish population and separated it from the mirror population, with
  antecedents; the `periode(...)` defect inflating the mirror ~3×; the absence-debounce collapse and
  the page-failure blindness that mass-closes a department; the four ways the naive suppression shape
  fails, including the restore drill that republishes and reports green; the `AdmittedEtablissement`
  witness; the WAL arithmetic and the payload-hash trigger; the shared `Restaurant` lane; and the
  correction below on re-derivation cost.
- **architect** — the 22-day finding and its five lines of evidence; the register consequences
  below; the seven-step dependency chain behind "immediately, next request"; the ten issues; and six
  things to stop.

## Register consequences

`STAFF-AUTH` was amended twice on 2026-08-30 while still a legacy key, which
`docs/decisions/_legacy.yaml` makes a same-change migration trigger. That is repaired here, and the
gate that did not notice is filed. New rows: **`PUBLISH-SCOPE`** (created and closed in this change
— an undeclared decision cannot be reversed by the `reconsiders` protocol, and publishing 200k
listings is precisely the decision someone will want to challenge), **`RIDER-REVOCATION-TTL`**
(created and closed), **`SUPPORT-CONTACT`** (open, founder), **`PUBLISH-PRECONDITIONS`** (open,
counsel), **`PRINCIPALS-MEMBER`** (open, team — CLAUDE.md question 2, a migration, because
`UserType` is stored on every `domain_events` row), **`REVOKED-COLLEAGUE-NOTICE`** (open, counsel).

The four mechanical obligations get **no decision rows**: they were decided on 2026-08-08 and filing
them again would re-ask an answered question (ADR-20260828-120500). They are issues.
