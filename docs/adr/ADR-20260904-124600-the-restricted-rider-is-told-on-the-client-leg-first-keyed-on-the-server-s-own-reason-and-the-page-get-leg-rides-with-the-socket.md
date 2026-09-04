# ADR-20260904-124600 — The restricted rider is told on the client leg first, keyed on the server's own reason, and the page-GET leg rides with the socket

<!-- Filename: docs/adr/ADR-20260904-124600-the-restricted-rider-is-told-on-the-client-leg-first-keyed-on-the-server-s-own-reason-and-the-page-get-leg-rides-with-the-socket.md -->

## Status

Accepted — a **team decision by consent** under
[TEAM-DECIDES-OPTION-SPACES](../decisions/TEAM-DECIDES-OPTION-SPACES.yaml): the whole roster was
briefed before any code (full mob — a legal surface, `HOLD: human`), thirteen lenses answered, and
the one split (build the page-GET leg now or with the socket) is resolved by the safer option on a
legal surface. Realizes **step 4-ii** of
[ADR-20260904-081527](ADR-20260904-081527-rider-standing-is-a-grant-on-the-identity-row-the-doors-are-human-only-and-step-4-lands-in-three-slices.md)
§11 and **amends it in part** (§3 below): the document-GET bounce moves to step 5 beside the socket
re-resolution, and a mutation-denial leg is added. The words and the strings are §7 of that record,
unchanged. The founder reads this record; the lower-tier trip row
[LOWER-TIER-TRIP](../decisions/LOWER-TIER-TRIP.yaml) is queued to him separately.

**Relates**: [ADR-20260904-014136](ADR-20260904-014136-rider-restriction-ships-now-with-the-smallest-closed-set-of-grounds-and-counsel-can-only-add.md)
§6(ii) (ground, both dates, contact, how to contest — a screen showing the ground with no contact is
the failure), [SUPPORT-CONTACT](../decisions/SUPPORT-CONTACT.yaml) (no voice leg; the address is
configuration), [ADR-20260904-015903](ADR-20260904-015903-the-custody-doors-are-a-new-fact-a-rider-hands-a-job-back-with-the-food-s-whereabouts-and-the-read-models-fold-it.md)
§7 (the rider holding food must see the pickup address), [ADR-20260830-234532](ADR-20260830-234532-the-second-sitting-publish-france-wide-revocation-is-immediate-and-the-objection-chain-was-decided-22-days-ago.md)
Answer 1 (the socket resolved once — step 5), #872 (the SDUI gaps this record designs around),
#858 (no deadline is printed until it makes one true), #874 (the SMS notice, not this slice),
#717 (no restaurant phone on `DeliveryJob` — declared on the card, not fixed).

## Enforced by

Written by 4-ii with their tests: validator rules `screen-restricted-binds-uncarved-op` (a screen
that is a `restricted:` target, or declares `while_restricted: true`, binds only operations carrying
`whileRestricted:` for its role and never mounts `rider_topbar`; ERROR) and
`screen-restricted-route-unknown` (the target declares `while_restricted: true` and carries no
`restricted:` of its own); the server test that a standing refusal and a role refusal carry
DIFFERENT `extensions` (the reason is present only on the former); the native render test that the
notice shows both dates and the contact for each of the four grounds and the catch-all and never a
raw ISO instant; the renderer test that a `FORBIDDEN` without the reason does not navigate.

## Context

4-i shipped the fact, the standing and the doors; a restricted rider today gets a bare `FORBIDDEN`
with no ground and no contact — the ADR-014136 §6(ii) failure and, for a rider holding food, the
custody problem 3-ii exists to close. The briefing put the bounce's location to the roster:
server-side at document GET, client-side on a refused read, or both. Facts the roster corrected on
the card: **no machine-readable `RESTRICTED` signal exists** — `StandingGuard` and `RoleGuard` both
emit `code: FORBIDDEN` and only the message differs, and the web transport flattens the errors array
to a string; the SDUI resolver alias root for a key `standing.mine` is `standing.*`, so the card's
`myStanding.heldDelivery` paths are unspellable (a card defect, banked: a lower-tier executor would
have copied it and the held-job card would never render, failing closed); `!= null` IS in the
predicate grammar; the page GET makes zero Postgres reads today and SSR reads run as PUBLIC; no
date filter exists in the renderer (`format_currency` is the only one); `copy_to_clipboard` is a
declared action key with no listener; `$reload` is the declared same-page navigate token; the
renderer's 2c-ii 401-leg decision is inline in `spawn_local` and has never been seen red.

## Decision

1. **The client leg is the mechanism now, keyed on the server's own reason — never on `FORBIDDEN`
   alone.** `StandingGuard` keeps `code: FORBIDDEN` (ADR-081527 §4 unchanged) and adds the additive
   extension `reason: RIDER_RESTRICTED`; the one string constant lives in a crate both `server` and
   `web` depend on, asserted by the server test and the renderer test (a hand-copied string is the
   test that lies). The `web` transport parses `extensions` into a typed value instead of
   stringifying. A `FORBIDDEN` with no reason — an order the rider does not hold — never bounces.
   The alternative discriminator, re-reading `myStanding` on every `FORBIDDEN`, was refused: it
   taxes the refused path with a read and the 99.9 % ACTIVE riders would pay it on every page if made
   a data requirement; the error-carried reason costs zero reads and is joinable to the server's
   fact.
2. **The bounce fires on reads AND on a refused mutation.** `restricted: { type: navigate, route:
   "/restricted" }` is a per-screen key on `jobs` and `job_detail` (the `unauthenticated:` twin),
   emitted as `restricted_route` beside `unauthenticated_route` by the web emitter — never a
   hand-typed string. The decision "which outcome bounces where" is ONE pure function a native test
   hits, covering the hydrate loop's refused read and `interact.rs`'s refused Tell — a rider mid-job
   on `/jobs/:orderId` reads `delivery.byOrder` successfully (carved), the page paints, and the next
   Tell is refused: that refusal navigates to `/restricted`, never a toast. The 2c-ii 401 leg moves
   into the same function and is seen red for the first time.
3. **The document-GET leg is the final form and rides with step 5 — amending ADR-081527 §11.** The
   page GET would resolve the rider with the seam's one `SELECT rider_id, standing`; that is the
   same question the socket must answer per push (ADR-20260830-234532), so it is built once, as one
   function with three callers (GraphQL request, page GET, socket), in step 5. Its outage posture is
   recorded now: `LookupFailed` renders the shell and lets the GraphQL path refuse — never a 302 to
   a page saying *"Votre accès est restreint"* on a database blip, which would be a false legal
   statement at peak. Nothing in this slice makes a page GET depend on Postgres.
4. **The `/restricted` screen.** Id `restricted`, route `/restricted`, `roles: [RIDER]`,
   `requires_auth`, `unauthenticated: /sign-in`, `while_restricted: true`, NO `rider_topbar`,
   resolver key **`standing.mine: { query: myStanding }`** (every path `standing.*`). The notice's
   title, footer and contact are UNCONDITIONAL; the ground row and both date rows are gated on
   `standing.restriction != null`; the transient row on `standing.restriction == null` reads
   **"Détails de la restriction pas encore disponibles."** (key `rider.restricted.details_pending` —
   a loading state named as one; never *"en attente"*, which says the decision is undetermined).
   The five ground labels (`rider.restricted.ground.{rider_requested, eligibility_document_lapsed,
   identity_mismatch, account_compromise, unrecognised}`) are bound EXPLICITLY, one per value —
   never a key built from the value, so a fifth ground counsel adds fails a test instead of
   rendering blank. Both dates print even when equal ("Décidé le" / "Effectif depuis" — the equal
   values ARE the V0 statement, counsel question 3's subject; never collapsed to one line). A
   **`format_datetime` filter** (Europe/Paris, `fr`) lands in the renderer beside `format_currency`;
   the event and the read model keep the UTC instant. The contact is plain selectable text — no
   copy button (a declared action with no listener is the control that does nothing, on a legal
   surface) — and the address is bound ONCE from `SUPPORT_CONTACT` configuration through the 2c
   refusal-screen precedent; no translation string hard-codes it. No deadline, no verdict word, no
   capacity named for the mailbox (counsel question 2).
5. **The held-job card and its sheet.** `visible_when: "standing.heldDelivery != null"`; the card
   carries the restaurant's name and the pickup address (ADR-015903 §7 — the address is the biggest
   element; the restaurant's phone is #717, declared as a gap on the card); the one control opens a
   **second sheet `rider_restricted_handback_sheet`** bound to `standing.heldDelivery.status` /
   `.id` — no screen-level alias grammar exists and inventing one is a second naming scheme the
   validator cannot see; both custody chips at PICKED_UP or later, the `NOT_COLLECTED` literal from
   ASSIGNED, `inline_error`; `on_success: [close_sheet, navigate "$reload"]` re-executes the
   screen's reads (`heldDelivery` is a view over the log — correct on the next read; only
   `standing`/`restriction` can lag). The control is gated on `standing.heldDelivery.foodLocation
   == null`; once set, the after-state text renders instead — no control is ever live on a job the
   rider no longer holds. `myStanding.heldDelivery` gains the `held_by_rider(rider_id)` port
   (#879's item) in this slice: the screen makes it a per-paint read, and the current `for_rider`
   walks the rider's whole history plus the PENDING pool over a view of the log.
6. **Observability**: nothing new is emitted. The client leg is declared **RESERVED** in the
   `rider-restriction` contract (the `web` crate carries no OTel — the `sdui_degraded_render`
   convention); the server-visible half is the denial counter, the span and the reason. The
   page-GET leg's joinable *"told"* event (`rider.restricted.bounced`, `rider_id` nested, the #748
   shape) is recorded here for step 5.
7. **Records**: one SPEC-LOG sentence (what a restricted rider is now promised: the fact, both
   dates, a human review route); PROP §8.6's mockup loses its topbar chip; #858 gets one link
   ("copy landed, deadline still unprinted"); the counsel packet's question 5 fixture is the landed
   strings verbatim; the return-leg pay exposure (a per-drop rider restricted mid-route earns
   nothing for the ride back) is named in the PR body as an exposure, not a blocker at zero riders.
8. **Concurrency fence**: #868 edits `rider_topbar` in `specs/screens/rider.yaml` — not dispatched
   alongside 4-ii. The mailbox fence is untouched by this slice.

## Alternatives considered

- **Server-side 302 at document GET now** (architect: buildable, one more caller of the existing
  resolver) — deferred to step 5 (§3): two readers of one lagging read model at different ticks can
  make the door and the screen disagree (young), it adds a Postgres dependency to every rider page
  GET with an undecided outage posture (farley), and it does not cover the mid-job rider anyway.
- **A distinct `extensions.code: RESTRICTED`** (architect, evans) — an additive `reason` beside the
  unchanged `FORBIDDEN` keeps ADR-081527 §4 true and breaks no client; the discriminator is equally
  machine-readable.
- **Re-read `myStanding` on every `FORBIDDEN`** (ux, farley) — refused (§1).
- **Reuse `rider_handback_sheet` through an alias** — refused (§5).
- **"Motif : en attente"** — refused (§4); **"Motif : chargement…"** (legal) — the same loading fact,
  evans owns the French.
- **A copy-the-address button** — refused (§4).

## Consulted (ADR-20260812-143619 — one line per lens)

Briefing before any code; **no lens output is legal advice or clearance**.

- **vernon** — one aggregate: the handler never reads `Rider.standing`; the handback stays a Tell,
  no Tell-then-Ask; (c) keyed on a distinct signal.
- **young** — (c) with (b) now, single reader; additive `reason` with a tolerant reader; the
  transient spans three checkpoints; `on_success` fires at acceptance — declare, never paper.
- **evans** — restreint / rétabli / Motif; the transient's sentence; `standing.mine`; bind the five
  labels explicitly; `while_restricted` ↔ `whileRestricted`, one word.
- **dba** — storage does not distinguish the options; `held_by_rider` in or before 4-ii;
  `heldDelivery` is a view (no lag), `standing` is checkpointed (can lag).
- **graphql-architect** — (c) with (b) non-negotiable; `reason: RIDER_RESTRICTED` + a typed
  transport; `standing.*` roots; a second sheet; the two validator rules' shapes.
- **beck** — the discriminator as one shared constant; the bounce decision as a pure fn seen red;
  the first failing tests by name; the four lower-tier mutants.
- **ux-designer** — the spellable screen; `$reload`; the after-state from `foodLocation`; no copy
  button; `format_datetime` as a renderer gap; the mockup lines.
- **observability-agent** — the denial counter is the atom for "refused", not "told"; the client
  leg RESERVED; the reason extension makes the bounce falsifiable; the (a) event for step 5.
- **legal-specialist** — the strings fit to land; bind the address once from configuration; both
  labels stand when equal; never *"en attente"*; no copy button; what the screen must not say.
- **business-specialist** — both custody chips; the restaurant phone (#717) declared; return-leg
  pay named; the `foodLocation` split as the metric that says whether the screen works.
- **farley** — (b) now, (a) recorded with its outage posture; the native render test through the
  router as the legal-surface proof; `restricted_route` emitted, never hand-typed; deploy ≠ release.
- **holub** — the shortest slice is (b); the copy button and (a) are inventory; the trip of
  ADR-013450 §5 fired on #875 and owes the founder a row.
- **architect** — (c) is the register's own decision; no `RESTRICTED` signal is landed; (b)'s
  mid-job hole; a second sheet; #868 as a concurrency fence; 4-ii is next.
