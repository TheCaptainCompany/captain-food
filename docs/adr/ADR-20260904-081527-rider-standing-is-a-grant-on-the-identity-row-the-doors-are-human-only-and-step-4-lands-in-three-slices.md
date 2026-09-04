# ADR-20260904-081527 — Rider standing is a GRANT on the identity row, the doors are human-only, and step 4 lands in three slices

<!-- Filename: docs/adr/ADR-20260904-081527-rider-standing-is-a-grant-on-the-identity-row-the-doors-are-human-only-and-step-4-lands-in-three-slices.md -->

## Status

Accepted — a **team decision by consent** under
[TEAM-DECIDES-OPTION-SPACES](../decisions/TEAM-DECIDES-OPTION-SPACES.yaml)
([ADR-20260904-013834](ADR-20260904-013834-the-team-decides-option-spaces-and-spec-diffs-external-legal-and-admin-gated-actions-stay-with-the-founder.md)):
the whole roster was briefed before any code (ADR-20260809-013142, full mob — `HOLD: human`, a
stored event shape and a legal surface), thirteen lenses answered, the two genuine splits are
resolved below by taking the option that keeps the seam a pure fold, and the founder reads this
record. Realizes **step 4** of
[PROP-20260831-180622](../proposals/PROP-20260831-180622-staff-authentication-the-roster-the-invitation-and-the-door.md)
§11 (row 4 rewritten as **4-i / 4-ii / 4-iii** in this change) under the founder's rulings of
2026-09-04: [ADR-20260904-014136](ADR-20260904-014136-rider-restriction-ships-now-with-the-smallest-closed-set-of-grounds-and-counsel-can-only-add.md)
(the four grounds, additive-only, the Art. 11 duties) and form question 5 (*"keep the approved
order: 3, 4, 5, 6, 7"*). Nothing here reopens either.

**Relates**: [RIDER-REVOCATION-TTL](../decisions/RIDER-REVOCATION-TTL.yaml) (decided: a restriction
bites on the next request; *"the revocation fact must be a term in the derivation"*),
[ADR-20260830-234532](ADR-20260830-234532-the-second-sitting-publish-france-wide-revocation-is-immediate-and-the-objection-chain-was-decided-22-days-ago.md)
Answer 1, [ADR-20260810-194548](ADR-20260810-194548-six-decision-answer-sheet-claim-staleness-closed.md)
(the origin decision: riders get revocation with a reason code, a log and human review),
[ADR-20260904-015903](ADR-20260904-015903-the-custody-doors-are-a-new-fact-a-rider-hands-a-job-back-with-the-food-s-whereabouts-and-the-read-models-fold-it.md)
§6 (the `whileRestricted` key — **amended in part by §4 below**) and §10 (the one-arm fence
carve-out — **amended by §8 below**), [ADR-20260810-231300](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)
(why a clock term in a grant predicate is refused), [ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md)
(why the standing rides in the type and `SUSPENDED` leaves the door's scalar),
[ADR-20260808-235113](ADR-20260808-235113-final-vision-first-no-intermediate-steps.md) (the
future `effectiveAt` is designed here and shipped later, not shimmed), the review path
[#858 "Rider restriction: the Art. 11(3)–(4) review path"](https://github.com/TheCaptainCompany/captain-food/issues/858).

## Enforced by

Written by 4-i with their tests (ADR-0032): `rules.yaml#/RiderStandingIsAGrant` (the seam admits a
RIDER-role operation iff the identity row's `standing` is `ACTIVE`, or the operation declares
`whileRestricted: [RIDER]`; a missing row is `Unbound`, never a grant), `RiderRestrictionLifecycle`
(restrict only an unrestricted rider, reinstate only a restricted one, a second ground needs a
reinstatement first — the Art. 11 log is never overwritten), `RestrictionTakesEffectWhenDecided`
(V0: `effectiveAt == decidedAt`, both server-set, never in the past; the future form is §5),
`RestrictRiderIsAHumanAct` (the door is `roles: [ADMIN]` with `requires: acting: { ADMIN: any }`
and no process manager may `send` it — validator rule `pm-sends-human-only-command`),
`RiderAvailabilityNeverSpellsRestriction` (`ChangeRiderStatus` cannot name `SUSPENDED`, and a
restricted rider cannot go `AVAILABLE`). Plus the validator rules of §4 and the codegen tests of §6.

## Context

Step 4 makes a restriction bite on the next request without a token round-trip. The seam
(`resolve_rider_scope`, #849) resolves `auth_ref -> rider_id` from the `Rider` identity row on
every request; RIDER-REVOCATION-TTL decided the restriction must be a **term in that derivation**.
The briefing card put five questions to the roster: (1) the shape of the term — a nullable
`restricted_since` column, a `ScopeMembership`-style revoke, or a per-request stream fold;
(2) the exact carve-out set; (3) the 4-i / 4-ii split; (4) `decidedAt` versus `effectiveAt` and a
future date; (5) the outage posture. Two facts on the card were wrong at HEAD and are corrected
here: the `Rider` read model has **five** columns (`rider_id`, `auth_ref`, `display_name`, `phone`,
`status`), not two, so `status = 'SUSPENDED'` already sits in the seam's row unread; and the rider
record holds a **phone**, no email, so the notice channel is SMS. Three lenses reported the
pre-squash branch tip `df451998` as `main`; `origin/main` was `2162c6f1` as the card said.

Two genuine splits came back. **(a)** A future `effectiveAt`: six lenses (young, dba,
graphql-architect, farley, beck, holub) refuse a clock term in the grant predicate; the business
lens wants it for `ELIGIBILITY_DOCUMENT_LAPSED` (restrict at 19:40, effective after the shift, so
the vigilance clock is honoured without the #862 cost); the legal lens permits it per ground for
`ELIGIBILITY_DOCUMENT_LAPSED` / `RIDER_REQUESTED` and refuses it for the two protective grounds,
adding that the V0 fallback *"loses nothing legal at zero riders"*. **(b)** Whether the
`RiderRegistered` projector arm may write `standing`: dba says never (a from-zero replay re-grants
every restricted rider for the drain — the recipe PROP §6.4 chose to protect availability inverts
into an authorization event); young accepts it as a bounded read-side window. Both are resolved
below by the option that keeps the seam a pure, clock-free, replay-neutral fold.

## Decision

1. **The term is `Rider.standing`, grant-shaped, NOT NULL, a dedicated scalar `RiderStanding
   { ACTIVE, RESTRICTED }`** — never a nullable `restricted_since` (`IS NULL ⇒ allowed` is
   `NOT EXISTS(restriction)` wearing a column, the shape PROP §6.4 forbids by name; a lost row, a
   partial replay and a reinstatement would be one NULL), never a `ScopeMembership` revoke (it is
   Order-scoped and would over-deny the held job), never a per-request stream fold (forbidden by
   CLAUDE.md and PMW-3). Named `standing`, not `status`: `RiderStatus` is availability, the rider's
   own fact; standing is the platform's; a restricted rider may legitimately be `ON_DELIVERY`
   holding food. The seam's one `SELECT` returns `(rider_id, standing)` — one read, never two — and
   still never reads `status`. **`ReadScope::Rider` becomes the struct variant `Rider { id,
   standing }`** so a guard that ignores standing does not compile (compiler-first); `Identity` and
   the `derived:` injection keep matching `Rider { id, .. }` — a restricted rider on the carve-out
   still resolves to a rider scope, because `handBackDelivery` derives its `riderId` from it.
   The `Rider` table rule that today reads *"the resolver reads `auth_ref -> rider_id` and NOTHING
   else … a read model is not an authorization oracle"* is **rewritten** in 4-i to the split it now
   has to state: the row is a term in the read-side derivation and a GRANT test only; the write
   side keeps its own authority by folding `Rider-{id}`; the row is never the arbiter of an append.
2. **The fold writes `standing` only from the restriction facts, and the creation arm never
   touches it.** As LANDED (round 2 item 8 — dba, farley — corrects this sentence's own mechanism
   claim, the guarantee stands): the upsert's `DO UPDATE SET` does NOT omit `standing` — it is
   included like every other mutable column, because the SAME statement also carries
   `RiderRestricted`/`RiderReinstated`'s real writes, and SQL-level omission would silently drop
   those too. The never-write guarantee instead lives in the `RiderCompute::standing` hook itself
   (`crates/application/src/projectors/rider.rs`): on a REPLAYED creation (checkpoint-reset
   rebuild-in-place over an existing row — never `TRUNCATE`) it returns the PRIOR row's value
   unchanged rather than a literal `ACTIVE`, so `row.standing` computed for a `RiderRegistered` fold
   is never anything but "whatever was already there, or `ACTIVE` for a genuinely fresh row" — the
   `created_at` precedent's INTENT (never let the creating arm move it), generalised to a column two
   OTHER events are the sole live write authority for. Pinned by
   `RiderCompute::standing`'s own unit test (`crates/application/src/projectors/rider.rs`, the
   replay-in-place mutant, M5 on the 4-i card) and the DB-gated
   `a_restricted_fact_flips_standing_and_a_reinstated_fact_flips_it_back` /
   `a_rider_predating_the_restriction_group_still_gets_backfilled_by_its_own_replay` suites. End
   state is identical to a three-literal `derive:` map, and a from-zero replay by checkpoint reset —
   still the recipe, never `TRUNCATE` — **re-grants nobody** for the length of the drain. The fold keys on the FACT,
   never on the ground's value or on `status` (ADR-20260904-014136 §5): a legacy
   `RiderStatusChanged { SUSPENDED }` alone leaves `standing = ACTIVE`. Migration: `ALTER TABLE
   rider ADD COLUMN standing TEXT NOT NULL DEFAULT 'ACTIVE'`, metadata-only, **no checkpoint
   rewind** (a NEW event type with zero stored occurrences — the 3-ii rewind precedent was for a
   column over events that already existed), **no CHECK constraint** (a constraint fault on this
   path is skipped by `DbFaultPolicy::Skip` and the rider silently stays granted), no covering
   index. The attribution — ground, `decidedAt`, `effectiveAt`, reinstatement — lives in a **new
   read model `RiderRestriction`** (keyed on `riderId`, no `auth_ref`, no phone), the source of
   `myStanding` (§4) and of 4-iii's admin surface; the identity row carries the grant and nothing
   the notice needs.
3. **The read-only catch-all decode variant serves TWO paths and is unspellable on three.** With
   strict decoding an unknown stored ground fails the whole `Rider-{id}` load (blocking
   `ReinstateRider`) **and** — verified in the briefing, the part ADR-014136 §5 left open — the
   projector logs the fold fault, **skips the event and advances the checkpoint**, so the stale
   grant stands. So `RiderRestrictionGround` gains a variant `UNRECOGNISED` with `#[serde(other)]`
   on the domain enum and in `EnumText::from_text`, and the `bam` projector's decode tolerates it
   too. It is unspellable **at the command door** (the GraphQL enum is the closed four; the
   handler additionally rejects it as a belt), **on the wire as a value** (the output field
   `ground` is nullable; `UNRECOGNISED` renders `null`, and the notice renders *"Motif non reconnu"*
   with the contact, never a blank), and **on OTLP** (the ground is attribution for counsel and the
   notice, never a span attribute or a metric label). The raw text stays in the immutable
   `domain_events.payload` for counsel.
4. **The carve-out grammar, exact, and the set — amending ADR-20260904-015903 §6 by one query.**
   `whileRestricted: [ROLE]` joins the closed key set for queries and mutations, a SUBSET of
   `roles:`; the SDL gains `@whileRestricted(roles: [UserType!]!)` beside `@auth`; the guard is
   `RoleGuard.and(StandingGuard)` — two orthogonal questions, `RoleGuard` untouched — emitted on
   EVERY role-guarded operation with an empty carve set when the key is absent (fail-closed by
   absence lives in the emitter, never in the author's memory). `StandingGuard` reads `ReadScope`
   only, never a claim (a claim has no standing). Validator rules, each with a red-on-mutant test:
   `api-while-restricted-not-subset` (every value in `roles:`; `roles:` omitted is an ERROR —
   nothing to carve out of an open operation), `api-while-restricted-no-standing-source` (a closed
   set of standing-bearing roles, today `{RIDER}`), `api-while-restricted-mutation-derives-actor`
   (a carved MUTATION must declare `derived: { riderId: rider }`, or a restricted rider acts as
   another under the carve-out). **The set is `{ myStanding, delivery, reportDeliveryIssue,
   handBackDelivery }`.** `myDeliveries` is NOT in it: it returns the unassigned PENDING pool to any
   rider, which under `ACCOUNT_COMPROMISE` hands customer addresses to whoever holds the session,
   and a rider told *"vous ne recevrez plus de courses"* must see no offers. What §8.6 needs
   instead is one additive query **`myStanding`** `roles: [RIDER]`, `whileRestricted: [RIDER]`,
   returning `{ standing, restriction { ground, decidedAt, effectiveAt }, heldDelivery }` — the
   held job keyed on `ReadScope::Rider.id` (never the JWT `sub`, #869) so the screen bootstraps
   with no `orderId` in hand. `operationStatus` / `operationStatusChanged` carry no `roles:`, hence
   no `RoleGuard`, hence no standing guard: **operations with `roles:` omitted are unaffected by
   restriction**, stated as a rule, ownership scoping still applying — which is what lets the
   restricted rider's one Tell confirm instead of ending in silence. `changeRiderStatus`,
   `acceptDelivery`, `declineDelivery`, `confirmPickup`, `completeDelivery`: out.
5. **`decidedAt` and `effectiveAt` are both server-set and equal in V0; the future form is designed
   here and shipped later.** `RestrictRider`'s input carries `riderId` and `ground` only — an
   admin-typed `decidedAt` is a backdating vector inside the Art. 11 log, and the SDUI has no
   date-time input to pick an `effectiveAt` honestly. The handler stamps both fields with the same
   server instant; the payload keeps both (the legal shape, ADR-014136 §6(ii)); the seam reads
   `standing` with no arithmetic and no clock. **Final design, recorded now** (final-vision-first;
   staging is externally forced by counsel questions 3–4 below and the missing component): a
   future `effectiveAt` is **permitted for `ELIGIBILITY_DOCUMENT_LAPSED` and `RIDER_REQUESTED`**
   (a document's expiry, the rider's chosen date, the notice-period shape if a French préavis
   duty applies) and **refused forever for `IDENTITY_MISMATCH` and `ACCOUNT_COMPROMISE`** (a
   deferred protective measure is self-contradictory and, on a later breach analysis, an exhibit
   against the platform); it is realised as **a scheduled fact and a due-row worker that appends
   `RiderRestricted` at the instant and pushes** — never a `now() >= effective_at` term in the
   grant predicate, because a projection whose answer flips with no event recorded is not a fold
   and nothing pushes the socket (ADR-20260810-231300's tiebreaker). `RestrictionNeverRetroactive`
   holds in both forms. The business lens's cost is accepted for V0: at 19:40 the founder's tool for
   a lapsed document is to wait for the shift's end, and a forgotten restriction is the vigilance
   exposure the ground exists to close — the scheduled form is the remedy and it is designed.
6. **A human decides — three layers, and `SUSPENDED` leaves the door.** (i) `restrictRider` /
   `reinstateRider` are `roles: [ADMIN]`, no `derived:` (the target is payload, the actor is the
   envelope); (ii) the `receives:` entries carry `requires: acting: { ADMIN: any }` with no
   `EXTERNAL` key, and a new validator rule `pm-sends-human-only-command` makes a `processmanager.yaml`
   that `sends` such a command a `make validate` ERROR — today a PM `send` of `RestrictRider`
   validates clean, which is the mutant; (iii) a companion validator rule
   `pm-emits-human-only-event` (round 3, #639 part C step 4-i) makes a `processmanager.yaml`
   `receives[].emits:` of `RiderRestricted`/`RiderReinstated` a `make validate` ERROR too — no PM
   inbox may declare producing the RESULT of a human decision directly, bypassing the door that
   `pm-sends-human-only-command` guards. `ChangeRiderStatus.status` gets its own scalar **`RiderAvailabilityTarget
   { OFFLINE, AVAILABLE, ON_DELIVERY }`** — one name, one scalar; the event keeps `RiderStatus` —
   so `SUSPENDED` is a GraphQL validation error at the door, not a handler check; the four
   `→ SUSPENDED` entry edges leave the lifecycle, `SUSPENDED → OFFLINE` stays as the exit for
   legacy rows (fold-side reading uses the payload's target, so no stored row breaks), and the
   scalar's comment is rewritten to *LEGACY — retired by `RiderRestricted`, parse-only*. The
   aggregate's belt (vernon): a restricted rider's `ChangeRiderStatus → AVAILABLE` is rejected
   `RiderAccessRestricted` — one aggregate, no cross-aggregate read.
7. **One word: restriction.** The act denies every operation except the custody doors and the
   held-job read; the account survives, the binding stays — that is a *restriction* in Directive
   (EU) 2024/2831's own gradation, and *"suspendu"* over a fact called `RiderRestricted` is the
   spec/copy split on a legal surface. So: `RiderStanding`, `RiderRestrictionGround` (**renaming
   the proposal's `RevocationGround` before anything is stored** — "revocation" already means a
   partner's availability and a `ScopeMembership` grant; ADR-014136's Context keeps its verbatim
   citation), `RestrictRider` / `ReinstateRider` as the acts; rider copy **"Votre accès est
   restreint."** / **"Votre accès est rétabli."**; admin chips **"Restreindre l'accès"** /
   **"Lever la restriction"** — never *"réintégrer"*, the labour-law remedy after a nullified
   dismissal, exactly the subordination vocabulary ADR-014136 §3 refuses to store. The four
   comment/rule sites that still say *suspend* / *revoc* for this act are rewritten in 4-i and the
   executor greps both terms across `specs/**` before the turn ends. The four `fr` ground strings —
   the Art. 11 reasons text, each naming the observed FACT, the exit and the contact, no verdict —
   are **proposed copy, counsel-reviewable, not clearance**, landed by 4-ii with the screen:
   `RIDER_REQUESTED` — *"À votre demande. Vous avez demandé la restriction de votre accès. Pour le
   rétablir, écrivez à support@captain.food."*; `ELIGIBILITY_DOCUMENT_LAPSED` — *"Justificatif
   expiré. Un document obligatoire pour livrer n'est plus valide. Transmettez un justificatif en
   cours de validité à support@captain.food pour rétablir l'accès."*; `IDENTITY_MISMATCH` —
   *"Identité non concordante. L'identité connectée ne correspond pas à la personne vérifiée à
   l'inscription. Contactez support@captain.food pour clarifier la situation."*;
   `ACCOUNT_COMPROMISE` — *"Sécurité du compte. Nous avons des raisons de penser que votre compte a
   été utilisé par un tiers ; l'accès est restreint pour vous protéger. Contactez
   support@captain.food pour le rétablir."*; shared footer *"Vous pouvez contester cette décision
   et demander son réexamen par une personne : support@captain.food."* — **no response deadline is
   printed until #858 makes one true**; the catch-all gets *"Motif non reconnu — contactez
   support@captain.food."*. Labels: *Motif*, *Décidé le*, *Effectif depuis* — the screen shows
   **both dates** (PROP §8.6 showed one; ADR-014136 §6(ii) requires both). The contact names a
   mailbox; Art. 11(2) names a person — the capacity behind the mailbox is counsel question 2 and
   an exposure, not a blocker at zero riders.
8. **The fence admits one additive arm per new `receives:` entry, E0004-forced — amending
   ADR-20260904-015903 §10's "exactly one".** 4-i adds `RiderInbox::RestrictRider` and
   `RiderInbox::ReinstateRider` in `crates/infrastructure/src/inbox.rs`, each a hand-written arm
   calling an application handler, no routing, fencing or catch-all machinery touched, additive
   only (`git diff origin/main -- crates/infrastructure/src/inbox.rs | grep -c '^-[^-]'` is `0`).
   Antecedent, restated honestly: #780 closed 2026-08-30; the last commits on the fenced path are
   `db39f94b` (#846), `97df9577` (#852 — already a **two**-arm precedent) and `6cf74887` (#870);
   no open issue carries `status/in-progress` except #639; no open PR touches a fenced path. **The
   fence globs are now named in one place** — `crates/infrastructure/src/mailbox/**`,
   `crates/infrastructure/src/inbox.rs`, `crates/application/src/process_managers/**`,
   `crates/actor_runtime/**`, `tools/codegen-rs/src/emit/actor_inbox.rs`,
   `tools/codegen-rs/src/emit/pm_orchestrators.rs`, `specs/*/processmanager.yaml` — so the
   executor's self-check can be run as written. The rule replaces per-arm re-carving; the fence
   otherwise stands.
9. **Observability, split by what emits it.** In **4-i**: the `rider-identity` contract's
   `rider.identity.resolve` span gains the attribute `business.standing` (`ACTIVE | RESTRICTED`,
   on `result=resolved` only — an attribute on the wide event, never a label on the histogram, so
   *"why was THIS rider denied at 19:40"* is answerable per request); a new **`rider-restriction`**
   contract with `rider_restricted_denied_total{operation}` (arrival-shaped, a counter; `operation`
   bounded by the closed `api-operation-key` set; no `rider_id` label — the rider goes in the INFO
   trace event beside it, joinable by `correlation_id`), emitted by `StandingGuard` at the gateway
   boundary — the first guard-level emitter, today a `FORBIDDEN` emits nothing — and
   `rider_standing_lag_positions` (the `scope_membership_lag_positions` mirror: while the `Rider`
   projector lags, a restricted rider is still GRANTED, and *"immediately"* is a measured claim
   only with this gauge); the contract states that an already-open WS subscription NEVER fires the
   counter (resolved once at `connection_init` — step 5's problem, documented not fixed). Declared
   only in the PR that emits each (`obs-metric-no-emitter`). In **4-iii**: the dead-man that
   matters — 3-ii's `delivery_handed_back_unreassigned_age_seconds` measures a job handed BACK and
   not re-offered and is silent for the restricted rider who has NOT handed back — a gauge
   `rider_restricted_holding_job_age_seconds` over `View_DeliveryJob ⋈ Rider(standing=RESTRICTED)`,
   emitted every sweep with 0, on a declared configuration key whose default the card marks
   `UNVERIFIED input`; no command-payload attribute is claimed on `command.validate` (the 3-ii
   lesson). The ground stays off OTLP (§3).
10. **A business metric for the new admin activity, in the cheapest replayable form.**
    `restrictRider` / `reinstateRider` need story steps (`op-uncovered-by-story` is an ERROR), so
    4-i declares the admin activity **`ManageRiderStanding`** and, binding under
    ADR-20260811-014129, its fold **`RiderRestriction`** — question: *how many riders are
    restricted right now, on which fact, for how long, and how many came back?* — grain entity
    keyed on the restriction fact (a rider can be restricted twice), closed by `RiderReinstated`;
    measures ground, `decidedAt`, `effectiveAt`, reinstatement; metrics: count by ground (a closed
    scalar plus the catch-all as its own bucket — a declared bounded population) and the
    restricted-to-reinstated duration (never-reinstated rows excluded; *"still restricted"* is the
    gauge's job). **Never `groupBy riderId`** — a per-rider restriction rate is the
    performance-and-behaviour ground ADR-014136 §3 refused, and `HandBackIsNeverALever` already
    fences that direction. 4-iii adds the measure `heldJobAtEffectiveAt` (the #862 exposure in
    units). `ViewMyStanding` joins the rider's `Deliver` activity (the outcome is still the food's
    custody).
11. **Three slices, all `HOLD: human`, all on the lower executor tier, dispatched as one train.**
    **4-i — the fact, the standing and the doors**: scalars (`RiderStanding`,
    `RiderRestrictionGround` + catch-all, `RiderAvailabilityTarget`), `RestrictRider` /
    `ReinstateRider`, `RiderRestricted` / `RiderReinstated`, errors, the `receives:` entries with
    `requires: acting`, the lifecycle edit, rules and behaviour tests, the two arms, the `Rider`
    standing column and migration with the creation-arm discipline, the `RiderRestriction` read
    model and migration, the catch-all decode, `ReadScope::Rider { id, standing }` and the seam,
    the `whileRestricted` grammar + directive + `StandingGuard` + the three validator rules +
    `pm-sends-human-only-command`, the api doors `restrictRider` / `reinstateRider` `[ADMIN]` and
    `myStanding` `[RIDER]`, the carve-out on the three existing operations, the story steps and
    the `ManageRiderStanding` activity with its fold, the 4-i observability, the table-rule and
    comment rewrites, SPEC-LOG. **4-ii — the restricted rider is told**: the `/restricted` screen
    (`roles: [RIDER]`, no topbar) with a server-side `restricted: { navigate }` bounce key on the
    rider screens at document GET and on a client `RESTRICTED` read error, the notice (ground `fr`,
    both dates, contact, how to contest), the held-job card whose one control opens the existing
    handback sheet **on that screen** (its variables from `myStanding.heldDelivery`, screen
    resolver data — no second screen, no chaining, the copy promises only the handback), the
    validator rule `screen-restricted-binds-uncarved-op` (a bounce target may bind only carved
    operations for its role), the renderer bounce test, the strings of §7, PROP §8.6 rewritten.
    **Amended 2026-09-04 by [ADR-20260904-124600](ADR-20260904-124600-the-restricted-rider-is-told-on-the-client-leg-first-keyed-on-the-server-s-own-reason-and-the-page-get-leg-rides-with-the-socket.md) (team consent, the 4-ii briefing)**: the
    document-GET bounce moves to step 5 beside the socket re-resolution (one resolver, three
    callers; `LookupFailed` renders the shell, never a 302); the client leg is keyed on an additive
    `extensions.reason: RIDER_RESTRICTED` (no `RESTRICTED` read error existed) and fires on a refused
    MUTATION too; the sheet is a second sheet bound to `standing.heldDelivery.*` (the alias root —
    `myStanding.*` above is unspellable).
    **4-iii — the admin's hands**: a roster read model (`display_name`, `phone`, `standing`,
    ground, dates, the held job and its stage — *never* `auth_ref`; `Rider` stays `internal: true`
    with its one reader class), `riders` / `rider(riderId)` `[ADMIN]`, the `riders` list with a
    standing badge and the `rider_detail` route (sheets read screen resolver data, never
    `item.*`) with the restrict sheet (four chips, no free text, *"Effectif : maintenant"* as
    text) and *"Lever la restriction"*, the held-job measure on the fold, the 4-iii dead-man, and
    the WS gap of ADR-20260830-234532 recorded as accepted at this flip. **Deploy order** inside
    the train: 4-i is the dark deploy by construction — no toggle: a toggle whose OFF makes the
    guard ignore standing is the first mutant shipped as a feature, and *"immediately"* with a
    TTL of infinity would be a second decision; what makes it dark is that the only ADMIN door has
    no screen and production is suspended (ADR-20260817-105844) with a rider population of zero.
    **4-ii lands before 4-iii** (a restricted rider must never see a bare `FORBIDDEN`), and
    **no `RestrictRider` runs on a production rider before 4-ii merges** — should production
    resume first, the door goes behind a refuse-at-door activation before the resume. Rollback of
    a wrong restriction is `ReinstateRider`, a new fact, never a row edit; the legacy `SUSPENDED`
    rows are never turned into `RiderRestricted` with a fabricated ground. **A separate, BINARY
    rollback** (deploying an older build, round 3 item 5, farley): after any binary rollback, reset
    the `Rider` checkpoint on roll-forward — a `RiderRestricted` appended in the new-binary window
    and skipped by the old, single-group arrangement's checkpoint is otherwise a standing GRANT,
    silently, for exactly the rider it was meant to deny.
12. **The durable notice is owed before the first production restriction, as its own issue.** The
    screen is pull-shaped: a rider who does not open the app is not *"provided"* a statement on the
    effective day, and step 5 cuts the socket. The rider record holds a phone and the OVHcloud SMS
    hook already exists (OTP), so the notice is an SMS sent by a comms consumer of
    `RiderRestricted` — sending, never deciding, so ADR-014136 §6(i) is not engaged; the sender is
    a process manager and therefore fenced, filed with a `deferred:` block. Gating condition
    *"before any production `RestrictRider`"*, linked from #858. For `RIDER_REQUESTED` the SMS is
    also the evidence loop: a rider told *"à votre demande"* who did not ask has the contest route
    in hand, and the ops procedure *"retain the rider's message"* is the proof artifact the event
    cannot carry (free text refused).

## Counsel packet (for the hour with an avocat, under PUBLISH-PRECONDITIONS — not advice)

1. Art. 11(3) Directive 2024/2831: is a screen shown on next app open *"provided at the latest on
   the day the decision takes effect"*, or is a pushed SMS at `effectiveAt` required?
2. Art. 11(2): does a functional mailbox satisfy *"contact person"*, and what capacity should the
   copy name given the cooperative's statutes?
3. Is there a French-law notice / préavis or stated-reasons duty in force today, before
   transposition, for a platform restricting an independent rider (ordonnance 2022-492 platform-work
   provisions; Code de commerce L.442-1 II rupture brutale) — graded (c); if yes, the future
   `effectiveAt` of §5 becomes mandatory for the non-protective grounds.
4. Per-ground future `effectiveAt` (permit for LAPSED / REQUESTED, refuse for the protective two):
   confirm or amend.
5. Review the four `fr` strings of §7 as the Art. 11 statement text; confirm no ground reads as a
   verdict.

## Consequences

- A restriction bites on the next request with one grant-shaped read, no clock, no second query;
  a from-zero rebuild re-grants nobody; an unknown ground can neither block a reinstatement nor
  leave a stale grant. The costs accepted: no mid-shift deferred restriction in V0 (§5), the
  projector-lag window now measured rather than absent (§9), and an already-open socket keeping
  its scope until step 5.
- `myDeliveries` keying on the JWT `sub` (#869) is not fixed here; `myStanding.heldDelivery` keys
  on `ReadScope::Rider.id` so 4-ii does not depend on it. PROP §7.1's 7b (re-derive per pushed
  payload) is assigned to **step 5** with 7a.
- **Card defects banked** (ADR-20260817-105845 attribution): the two-column `Rider` claim and
  *"email/SMS"* were card defects; the `df451998` head reported by three lenses is an invited-lens
  depth miss (a stale local branch read as `main`); none is roster width.

## Alternatives considered

- **Nullable `restricted_since`** — refused (§1); the mutant *"fold forgot the row on reinstate"*
  and *"row lost"* would be indistinguishable from *"reinstated"*.
- **A `ScopeMembership` revoke** — refused; Order-scoped, DELETE+replay rebuild fails every rider
  closed for the drain, and a revoke cannot carve out the held job.
- **A second lifecycle machine on `Rider`** — refused (vernon); the grammar admits one `status:`
  per aggregate, and folding restriction into availability is what `SUSPENDED` did wrong.
- **A future `effectiveAt` as `now() >= effective_at` in the seam** — refused (§5); designed as a
  scheduled fact instead.
- **A `RIDER_RESTRICTION_ENFORCED` toggle** — refused (§11).
- **Carving `myDeliveries`** — refused (§4); `myStanding` instead.
- **A separate standing witness beside `ReadScope::Rider(RiderId)`** — smaller blast radius, but
  two values that must travel together are the thing the type system should make one; the struct
  variant's compile-error inventory is loud and listed on the card.
- **`RevocationGround` kept as the name** — refused (§7) before anything is stored.

## Consulted (ADR-20260812-143619 — one line per lens)

Briefing before any code; **no lens output is legal advice or clearance**.

- **vernon** — one aggregate per transaction (restrict never touches the held `DeliveryJob`);
  restriction is a fact in state, not a second machine; `RiderAlreadyRestricted` /
  `RiderNotRestricted` are the aggregate's true invariants; the `AVAILABLE`-while-restricted belt;
  three layers for the human-only door; one arm per `receives:` entry as the fence's rule.
- **young** — grant-shaped NOT NULL via literals; the catch-all makes the revoke *unskippable*,
  not merely loadable; remove the entry edges, keep the exit; a future `effectiveAt` is not a fold.
- **evans** — one word, *restriction*; `RiderStanding` never *status*; `RiderRestrictionGround`;
  *"lever la restriction"* never *"réintégrer"*; the four comment sites; the grep-shaped test.
- **dba** — the creation arm must not write `standing` (the re-grant window); `ADD COLUMN … DEFAULT
  'ACTIVE'`, no rewind, no CHECK, no covering index; the admin list is a different read model;
  `DbFaultPolicy::Skip` inverts on this path.
- **graphql-architect** — the exact grammar, directive, `GuardExt::and`, three validator rules;
  `myDeliveries` must not be carved; `myStanding`; the struct variant and its inventory; the
  catch-all must not null a list response.
- **beck** — the derived injection needs `ReadScope::Rider` intact; the first failing tests by
  file and name; the six mutants with their expected red; the DB-gated split and the incantation.
- **ux-designer** — the dedicated `/restricted` screen and the `restricted:` bounce (no negation
  grammar, no `rider.*` context); sheets read screen data, never `item.*`; no date input; the
  mockup lines; the one action must confirm.
- **observability-agent** — `business.standing` as an attribute; `rider_restricted_denied_total`;
  the lag gauge; the WS-never-fires note; the ground off OTLP; 3-ii's gauge is not this dead-man.
- **legal-specialist** — `decidedAt` server-set; `effectiveAt` per ground; the four strings and
  the footer; SMS is the only held channel and the notice is a blocker before the first production
  restriction; the counsel packet.
- **business-specialist** — the four facts the founder needs before a chip is tappable; the cost
  stack of a wrong restriction at `PICKED_UP`; a future `effectiveAt` for a lapsed document is a
  real use (accepted as the V0 cost, §5); the fold's question.
- **farley** — there is no production, say so; readers first inside one PR; no toggle; the
  release flip is the admin binding; the projection rule rewrite; the walk as a DB-gated test.
- **holub** — 4-i without a door is a fact only a test can append; the `fr` strings ride the first
  relevant slice; the dead-man that matters; the stale-branch litter; the tally.
- **architect** — five columns not two; five arms not three; `roles:`-omitted operations;
  `op-uncovered-by-story` is an ERROR; the fence globs recorded nowhere; the stale antecedent;
  ADR-20260810-194548 as the origin decision.
