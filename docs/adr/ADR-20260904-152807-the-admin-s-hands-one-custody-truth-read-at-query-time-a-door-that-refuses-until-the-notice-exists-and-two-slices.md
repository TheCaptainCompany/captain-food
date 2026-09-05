# ADR-20260904-152807 — The admin's hands: one custody truth read at query time, a door that refuses until the notice exists, and two slices

<!-- Filename: docs/adr/ADR-20260904-152807-the-admin-s-hands-one-custody-truth-read-at-query-time-a-door-that-refuses-until-the-notice-exists-and-two-slices.md -->

## Status

Accepted — a **team decision by consent** under
[TEAM-DECIDES-OPTION-SPACES](../decisions/TEAM-DECIDES-OPTION-SPACES.yaml): the whole roster was
briefed before any code (full mob — the admin's act is the Art. 11(5) human decision, a legal
surface, and the release flip of step 4), thirteen lenses answered, and the one split (fold the held
job into the roster, or read it at query time) is resolved by the safer option on a legal surface.
Realizes **step 4-iii** of
[ADR-20260904-081527](ADR-20260904-081527-rider-standing-is-a-grant-on-the-identity-row-the-doors-are-human-only-and-step-4-lands-in-three-slices.md)
§9–§12 and splits it into **slices A and B** (PROP §11 row 4 rewritten). It also **records
the release gate as a mechanism**: a declared configuration key that refuses the restrict door until
the SMS notice exists, with a decision row and a codegen test — "no production `RestrictRider`
before #874" was prose. The founder reads this record.

**Relates**: [ADR-20260904-014136](ADR-20260904-014136-rider-restriction-ships-now-with-the-smallest-closed-set-of-grounds-and-counsel-can-only-add.md)
§3 (no performance ground — and no per-rider count on the admin surface), §6(i) (the human is the
envelope `user_id`), §6(iv) (no automated signal suggests a restriction),
[ADR-20260904-124600](ADR-20260904-124600-the-restricted-rider-is-told-on-the-client-leg-first-keyed-on-the-server-s-own-reason-and-the-page-get-leg-rides-with-the-socket.md)
(4-ii: the rider's notice, `$reload`, the two-checkpoint transient),
[ADR-20260810-231300](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)
(monitoring keeps a poll), [ADR-20260817-105844](ADR-20260817-105844-the-walk-goes-first-on-one-database-and-production-stays-suspended.md)
(production suspended; the `View_DeliveryJob` → table conversion is the cutover slice's, #883),
[ADR-20260817-105845](ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)
(the threshold default is `UNVERIFIED input`), [PUBLISH-PRECONDITIONS](../decisions/PUBLISH-PRECONDITIONS.yaml)
(the row shape reused for the restriction door's preconditions),
[#874 "Rider restriction: the SMS notice owed before the first production restriction"](https://github.com/TheCaptainCompany/captain-food/issues/874),
[#877](https://github.com/TheCaptainCompany/captain-food/issues/877) (the bam fold's grain),
[#883](https://github.com/TheCaptainCompany/captain-food/issues/883) (`View_DeliveryJob` → table),
[#654](https://github.com/TheCaptainCompany/captain-food/pull/654) (touches `observability.yaml` —
a merge fence for slice B), [#868](https://github.com/TheCaptainCompany/captain-food/issues/868)
(`rider_topbar`, untouched).

## Enforced by

Written by slice A with their tests: the roster fold (register → row; restrict → `RESTRICTED` +
ground; reinstate → `ACTIVE`; a from-zero replay byte-identical; a legacy `SUSPENDED` row keeps
`standing = ACTIVE`); `riders` / `rider` FORBIDDEN on every non-ADMIN path including RIDER; the
validator rule extending screen-binding checks to the sheets a screen opens (§6); the codegen test
that while [RIDER-RESTRICTION-PRECONDITIONS](../decisions/RIDER-RESTRICTION-PRECONDITIONS.yaml) is
`open`, `RUN_RIDER_RESTRICTION_DOOR`'s production value is `"false"`; the write-door test that the
key OFF yields a typed refusal on `restrictRider` while a restricted rider is STILL refused by the
guard (the key never touches the read side); the render tests of §5–§6. Slice B: the gauge's
empty-population / over-threshold / reinstated-to-zero / replay-identical tests and the test that the
contract's threshold equals the key's default.

## Context

4-i shipped the doors and 4-ii the rider's notice; nothing lets an ops person restrict a rider from a
screen, and the only appender of `RiderRestricted` is a GraphQL client. The briefing corrected four
card facts: the 3-ii watch worker lives at `crates/infrastructure/src/integrations/delivery_handback_watch.rs`
(not `workers/`); the nested navigation-selection gap is closed on `main` since 4-ii; option (b)
below contradicts a landed table rule (`RiderRestriction` refuses `phone`); `PageOffset` is a
network-scoped scalar. The one split: dba, business and graphql-architect want the held job FOLDED
into the roster (`View_DeliveryJob` is a view over the log that no index can serve, so a per-row read
on the list and a 30-second gauge sweep are the wrong cost at peak); vernon, young, architect, holub
and farley refuse a second fold of the custody lifecycle (two truths for "does this rider hold food"
under two checkpoints is the divergence the 4-ii journal recorded, on a screen that drives a legal
act).

## Decision

1. **Option (a): a new projection table `RiderRoster`** — `rider_id` pk, `display_name`, `phone`,
   `status` (availability), `standing`, `ground`, `decided_at`, `effective_at`, `reinstated_at`;
   fed by the five rider events only; own `ProjectorGroup` checkpoint `"RiderRoster"` from 0 (never
   a prefix under an advanced checkpoint); **never `auth_ref`**; no CHECK constraint
   (`DbFaultPolicy::Skip` semantics, the 4-i reasoning); indexes `(display_name, rider_id)` for the
   page order and a partial index on `standing = 'RESTRICTED'`. `Rider` stays `internal: true` with
   its one reader class; option (b) — widening `RiderRestriction` with the profile — is refused: it
   reverses that table's landed rule and makes a table named for one concept carry another.
2. **One custody truth, read at query time — never a folded column.** The DETAIL reads
   `held_by_rider(rider_id)` (the 4-ii port, one narrowed row) — the legal act is decided on the
   fresh read; the LIST joins the page's riders to the held statuses in ONE set-based query
   (`rider_id = ANY($1)`), never N per-row reads; the gauge (§8) drives from
   `RiderRestriction WHERE standing = 'RESTRICTED'` (a tiny set) joined to the view. The cost of a
   view over the log is accepted with its recorded owner: #883's table conversion makes all three
   index-only; the PR body carries the `EXPLAIN`. What this refuses: `held_delivery_job_id` /
   `held_status` columns folded from delivery events into the roster — a second fold of the lifecycle
   `View_DeliveryJob` already derives, under a group over two aggregates' streams, that drifts on a
   rebuild and answers differently from the port.
3. **Two slices, one step, dispatched back to back.** **A — the roster and the hands**: the table,
   migration, group, pin and chain entry; `PageOffset` promoted to `specs/common/scalars.yaml` (a
   delivery → network `$ref` for a paging scalar is the wrong DAG edge); `riders(limit, offset)` and
   `rider(riderId)` `roles: [ADMIN]`; story steps `ViewRiders` / `ViewRider` under
   `ManageRiderStanding`; the two screens, the sheet, the reinstate control (§5–§6); the door key,
   the decision row and its codegen test (§7); the `RIDER_REQUESTED` procedure page (§6); the
   validator rule for sheet bindings. **B — the dead-man and the measure** (§8), dispatched the moment
   A merges. Why two: #875 hit the ceiling on one large diff; the gauge is monitoring with a threshold
   nobody has measured, and the legal surface must not wait on it.
4. **The API.** `riders` returns `[RiderRosterEntry!]!` **ordered by the contract, not by an
   argument**: riders holding a job first, then `RESTRICTED`, then `ACTIVE`, each by `display_name`
   — the SDUI has no list sort, so the query's description declares the order and no `orderBy` exists
   in V0. `rider(riderId)` is nullable. `RiderRosterEntry { riderId, displayName, phone, status,
   standing }` non-null (the write path justifies them); `ground`, `decidedAt`, `effectiveAt`,
   `reinstatedAt` nullable; `heldDelivery: DeliveryJob` nullable — the existing type, never a
   narrower one; the client selection under `heldDelivery` takes stage, `foodLocation` and
   `pickupAddress` and **does not select `restaurant { displayName }`** (a delivery → network nav
   hop the D8 check cannot see; the restaurant's name on the roster is a declared gap and the D8
   check's extension to nav edges is a follow-up rule). `rider` also carries
   `restrictionDoorOpen: Boolean!` filled at the resolver from configuration (§7), the
   `contestContact` shape. Both operations are intra-scope; `OPERATION_SCOPES` derives them; only
   `/admin/graphql` composes them.
5. **The screens, in `specs/screens/system.yaml`** (strings under a `roster.*` block in
   `system.translations.yaml` — never `rider.restrict.*`, one letter from the rider's
   `rider.restricted.*`). **`riders`** (`/system/riders`, a triage, not a directory): per row the
   name, the standing badge, the held-job stage badge with the pickup address (only when held),
   `item_action` to the detail; phone and availability stay on the detail. **Badges are one `badge`
   per enum value with a fixed variant and a per-row `visible_when`** — never `variant_when`, which
   the renderer does not consume at HEAD; never a raw enum token on a French screen. Two columns,
   two vocabularies: *Disponibilité* (Hors ligne / Disponible / En course / **Suspendu (ancien
   statut)** for the legacy value, a warning badge, never hidden and never rendered as *Restreint*)
   and *Accès* (*Actif* / *Restreint*). **`rider_detail`** (`/system/riders/:riderId`, resolver
   `rider.byId`): the four facts above the fold in this order — the held job now with its stage and
   `foodLocation`; standing + ground + `effectiveAt` (`format_datetime`); the phone **with a
   `phone_call` action** (SUPPORT-CONTACT decided the rider's route to support, never ops calling a
   rider; the register is silent, ux and business call it legitimate, legal notes purpose limitation
   — declared once in `system.yaml` `actions`); availability. **No per-rider count of any kind on the
   detail** (handbacks, declines, rating — the requalification exhibit ADR-014136 §3 refused, even
   uncounted by any chip). "Restreindre l'accès" opens the sheet, gated on `standing == 'ACTIVE'`
   AND the door open; "Lever la restriction" is a direct Tell gated on `standing == 'RESTRICTED'`,
   `$reload` after; no sheet, no ground, no free text on reinstatement.
6. **The sheet `restrict_rider_sheet`** (on the detail route; it reads `rider.*`): a header the admin
   reads before any chip — the rider's name and, when a job is held, *"Tient une commande —
   récupérée. La restriction ne la lui retire pas."*; the consequence line *"Ne recevra plus de
   courses ; garde l'accès pour terminer ou rendre la course en cours."*; four chips
   `single_select`, values the closed scalar, **no chip preselected, no catch-all chip, no free
   text, no `disabled_when`** (an empty ground is the mutation's typed rejection surfaced by
   `inline_error`); labels naming the FACT with the rider notice's own nouns — *À la demande du
   rider* / *Justificatif expiré* / *Identité non concordante* / *Compte compromis*; the line
   *"Effectif : maintenant"*; the notice line *"Le rider est informé dans l'application à sa
   prochaine ouverture."* — **no SMS is claimed until #874**; under the chips, one sentence for
   `RIDER_REQUESTED`: *"Conservez le message du rider (procédure)."* pointing at
   `docs/legal/rider-requested-restriction-procedure.md` (one page: what counts as the rider's
   message, filed keyed on `rider_id` + the event's `decidedAt`, who files, where; retention is the
   Art. 11 log's — counsel question 6); the confirm **"Restreindre l'accès maintenant"** (it
   promises only what happens, never *Confirmer*); `inline_error`; `on_success: [close_sheet,
   navigate "$reload"]`. The legal lens's preview of the rider-facing sentence for the selected chip
   cannot be built (no form-field reactivity, #872): the chips name the same fact as the rider's
   strings, and whether a preview-and-confirm is sufficient evidence of the human decision is
   counsel question 8. The one-tick window after `$reload` in which the detail may re-render
   *"Restreindre"* is accepted and declared: a second submit yields `RiderAlreadyRestricted` in
   `inline_error`, never a second event (a test). The validator rule **`screen-sheet-binding-unknown`**
   extends the binding walk to the sheets a screen opens — today a `{{ rider.riderld }}` typo in a
   sheet passes `make validate` and would dispatch the Art. 11 act with an empty id.
7. **The door refuses until the notice exists — a mechanism, not prose.** A declared key
   `RUN_RIDER_RESTRICTION_DOOR` (`specs/delivery/configuration.yaml`, bool, `default: false`,
   `deploy.production: "false"`, the `ROUTE_REPLACEMENT_BIRTH_THROUGH_LANE` precedent) consumed at
   the WRITE door only: `restrictRider` OFF returns a typed, supervisable refusal
   (`RiderRestrictionDoorClosed`); `reinstateRider` is never gated; **the key never touches the read
   guard** — a restricted rider is refused with the key OFF (the named mutant, a test). The screen
   hides *"Restreindre l'accès"* when `rider.restrictionDoorOpen` is false (a live control bound to a
   closed door is the control that does nothing). A decision row
   [RIDER-RESTRICTION-PRECONDITIONS](../decisions/RIDER-RESTRICTION-PRECONDITIONS.yaml) (open) names
   the two preconditions — #874 merged, and an alert route named for the gauge — and the key; a
   codegen test refuses a production value other than `"false"` while the row is open (the
   `RUN_SIRENE_WORKER` lesson: prose said STOPPED while the deploy value said true). Flipping the
   default is a separate recorded decision that closes the row.
8. **Slice B — the dead-man and the measure.** Gauge `rider_restricted_holding_job_age_seconds`
   as a **section of the `rider-restriction` contract** (same feature, same question); emitted in
   `delivery_handback_watch_tick` after the handback gauge and before the heartbeat, which the
   contract names explicitly as this gauge's liveness proof; anchored on **`now − effective_at`**
   through `RiderRestriction` (not the internal `Rider`) joined to the held statuses — "how long
   since the restriction has this rider still held food" (a job accepted after `effective_at`
   overstates, the safe direction); 0 every sweep; no `rider_id` label — an INFO event
   `rider.restricted.holding_job` (`rider_id`, `delivery_job_id`, age) per stranded row, joined by
   aggregate ids (no `correlation_id` on a timer tick, a recorded divergence from the #748 shape);
   threshold from `RIDER_RESTRICTED_CUSTODY_MAX_AGE_SECONDS` whose default is **`UNVERIFIED input`**
   — `DELIVERY_OFFER_MAX_TTL_SECONDS` bounds an offer and nothing is offered here, the ETA is
   per-job; the gauge's non-zero is the action trigger and the threshold a debounce; a test that the
   contract's threshold equals the key's default (the #870 300-vs-900 lesson). Severity PAGE-class;
   the alert route repeats the recorded known gap, and "route named" joins #874 in §7's row. The bam
   measure lands on the riderId grain now as **`heldJobAtDecision`** (V0 `effectiveAt == decidedAt`
   is its precondition, recorded; #877 re-declares the grain) — spec-only until the bam runtime
   exists (#484), said so in the hand-back. No "restriction decided" signal is owed: `command.receive`,
   `event.store.append` and the fold already witness it; the ground stays off OTLP.
9. **The system surface is not host-routed today, and no admin door exists — a third precondition,
   not this slice's to build.** `system.captain.food` renders a static line
   (`crates/server/src/hosts.rs` `HostRoute::System => text(...)`), `Surface` has no `System`
   variant (`crates/web/src/handwritten.rs`), and `specs/screens/system.yaml` declares no
   `requires_auth` or `unauthenticated:` door — the mailbox supervision screen has the same status,
   reachable by no browser. So slice A ships the two screens **dark**: declared, rendered and asserted
   natively (`every_screen_of_every_surface_renders` walks four surfaces and will not see them — the
   dedicated system render tests are the only cover), the walk exercising the door through GraphQL as
   ADMIN. Reaching them from a browser needs the System host to serve the SDUI shell AND an admin
   sign-in door; the door is the magic-link door step 6 builds for restaurant staff (PROP §5), so
   **"an ADMIN can reach `/system/riders` with a session" joins the preconditions row of §7** and is
   owed by step 6 (the routing rides with it). Beck's finding, at the briefing; a card defect
   (the card assumed reachability).
10. **Records and packet.** SPEC-LOG sentences per slice; PROP §11 row 4 rewritten as A/B; the counsel
   packet of ADR-081527 gains **Q6** (retention period and acceptable form of the `RIDER_REQUESTED`
   message), **Q7** (does the rider have a right to know WHICH human decided — Art. 15 GDPR
   recipients vs the admin's own data), **Q8** (is the admin's preview + confirm sufficient evidence
   of a human decision, or is a logged acknowledgement needed), **Q6-bis** (acceptable form,
   location and retention for the filed `RIDER_REQUESTED` message where it may carry Art. 9 data —
   health or family reasons routinely accompany "please restrict me") and **Q9** (must a REFUSED
   restriction attempt — a closed-door REJECTED mailbox row holding `riderId` + `ground` — be
   retained, and for how long, given it is not an Art. 11 decision); the return-leg pay exposure stays
   named, unpriced (rider pay has no record). Card defects banked: the worker path; the nav-depth gap
   listed as open; (b) presented as live; `PageOffset`'s scope.

## Alternatives considered

- **Fold `held_delivery_job_id` / `held_status` / `held_since` into the roster** (dba, business,
  graphql-architect) — refused (§2); the cost argument is right and its owner is #883.
- **Extend `RiderRestriction`** — refused (§1), a landed-rule reversal.
- **One slice** — refused (§3).
- **`variant_when` badges** — refused (§5): unconsumed by the renderer at HEAD.
- **Phone as text only** (business) — refused for the detail (§5): on a phone at 19:40 a number as
  text is the anti-affordance; the register is silent on ops calling a rider.
- **A per-rider handback or decline count on the detail** — refused (§5), ADR-014136 §3.
- **Suspension as the gate** ("production is suspended anyway") — refused (§7): a fact, not an
  invariant; the resume is one sync.

## Consulted (ADR-20260812-143619 — one line per lens)

Briefing before any code; **no lens output is legal advice or clearance**.

- **vernon** — the handler never reads the delivery job (the four facts are the human's read-side
  precondition, not an aggregate invariant); the door is a Tell; the held job joined at read; the
  gauge read-side only.
- **young** — (a); a folded held column is a second fold of the custody lifecycle; `heldJobAtDecision`
  with its V0 precondition; the gauge's staleness may only delay an alarm; join `RiderRestriction`,
  never `Rider`.
- **evans** — *Disponibilité* / *Accès*; *Suspendu (ancien statut)*; *Justificatif expiré*;
  `RiderRoster`; `ViewRiders`; the `roster.*` key block.
- **dba** — the cost of a view over the log on the list and the 30-second sweep (its owner #883);
  the indexes; no CHECK; the migration footprint; the rollback.
- **graphql-architect** — the shapes; promote `PageOffset`; drop the cross-scope nav selection;
  `OPERATION_SCOPES` derived; sheets are not walked by the binding gate; a second submit → typed error.
- **beck** — consulted for the card (the first failing tests, the mutants, the expected-red list).
- **ux-designer** — the list as a triage, held-first; badges per value (`variant_when` dormant);
  the four facts in order; `phone_call` legitimate; the sheet's header and confirm label; no
  `disabled_when`; the `$reload` window declared.
- **observability-agent** — a section, not a contract; same tick, heartbeat named; the
  `effective_at` anchor; `UNVERIFIED input`; PAGE-class; the threshold-equals-default test; no new
  decided signal.
- **legal-specialist** — what the sheet shows and must not show; no preselection; the notice line
  without an SMS claim; the procedure page; the key AND the row AND the test; Q6–Q8.
- **business-specialist** — the stage prices the restriction; restricted and food-holding rows
  first; PAGE at 19:40; the measure's question; return-leg pay.
- **farley** — the key at the write door, never the read guard; own group rollback-safe; the
  set-based list join; the same tick with an EXPLAIN; the walk through the screen's operations.
- **holub** — two PRs, one step; the held job on the detail; pagination controls and severity
  debate as inventory; card size as the controllable lever.
- **architect** — the three card corrections; (a) final-vision-first; join at read; two slices; no
  open PR on `system.yaml`; #654 as slice B's merge fence; nothing outranks 4-iii; the gate as a
  mechanism.

**Addendum (step 5, 2026-09-05):** the counsel packet's Q6–Q8 gains a ninth question, posed by
[ADR-20260905-065415](ADR-20260905-065415-the-restriction-fact-terminates-the-rider-s-socket-a-connection-local-standing-read-inside-the-guard-and-one-writer-to-the-transport.md)
§11 — *does abrupt loss of the working channel before the statement of reasons is displayed
constitute the decision taking effect without its accompanying statement under Art. 11(3)?* — not
answered here, and no lens output above is legal advice or clearance for it either.
