# ADR-20260808-171056 — Register sweep: the team decides 30 open decisions by consent

## Status

Accepted by ensemble consent — **customer veto window OPEN** on every row below. Recorded under
ADR-20260808-144738 decision 3; second use of the mechanism (first: ADR-20260808-155656).

## Context

The customer asked the team to work the entire open-decision register so they decide only what
genuinely requires them. Five-lens sweep on 2026-08-08 (session
https://claude.ai/code/session_01AKgDqRbCcCxtUePWPRfxtp): the architect classified every open
row (re-measuring reality against the register); legal-specialist, business-specialist,
graphql-architect, ux-designer and dba then ran objection passes (silence = consent, objection =
named harm, cure folded where offered) plus evidence verdicts. Result: 30 rows team-decided
below; 9+1 rows go to the customer's brief
([BRIEF-20260808-customer-decisions.md](../proposals/BRIEF-20260808-customer-decisions.md));
7 register blocks were found stale/superseded (register overhaul mandated below).

## Decisions — by consent, veto window open

### Decided as recommended (no objection from any lens)

- §1 F (PROP-172000 D1): explicit `from:` input-source declaration — with the NAMING NOTE below.
- PROP-170000 D2: additive-only event evolution + `event_version` NOW (start-clean cutover makes
  it free; price rises at first production order) — **condition**: the validator gate must also
  prove generated-serde tolerance (`#[serde(default)]`-safe), not only the YAML diff.
- PROP-170000 D5, `version` + `id` legs: correct the spec to match the code (0/1-based; the
  mailbox owns append idempotency).
- PROP-171500 D2: derive scope ids where the role implies one scope — **timing note**
  (graphql): the input-field deletions are breaking-but-free only while no client ships; land
  before first deploy. The actor-side `requires.acting` stays the final check.
- PROP-171500 D3: sequence immediately after #144 lands (which it has NOT — register note fixed).
- PROP-172000 D3: rewrite the drifted product-spec §4–§5 to ADR-0034.
- PROP-172500 D1: postal-code delivery areas now, geocoding next (business: Tours river
  crossings make zones unusually truthful; fee LEVELS priced from rider economics upward).
- PROP-165000 D3: buyer-total-first rounding, residual cent to a stated leg, pinned by an
  odd-total test.
- PROP-165000 D4: per-zone delivery fee (business CONFIRM with the rider-economics condition).
- PROP-165500 D2: re-validate at checkout; decrement only Captain-managed offers (business:
  emphatic — POS double-decrement manufactures phantom oversell). Rejection copy names the item,
  one-tap remove-and-continue.
- PROP-165500 D3: per-service-type price override on `Offer` (business CONFIRM: the wedge is
  protected by the comparison surface, not by price control); resolve the `catalog(restaurantId)`
  ambiguity in the same change; watch the delta drift as a coaching signal.
- PROP-165500 D4: confirm the public audience for catalog images on #134 now.
- PROP-165500 D6: menu scheduling deferred until combos.
- PROP-164500 D4/D5: timed pause + exception days (eleven French public holidays).
- PROP-164500 D6/D7: same-day scheduling slots; address correction before `PREPARING` — D6
  sequenced behind §1 B (authorization life bounds the window).
- PROP-172000 D2: rejection-reason enum + optional note — **cures folded** (graphql): `OTHER`
  ships day one; the free-text note is declared in the erasure scope (restaurant-authored PII).
- §3 PROP-172500 D3: stall sweep emits `DeliveryAssignmentReleased` (dependency resolved).
- §4 PROP-120055 D3/D4/D5 as proposed — plus dba's caution made binding: dedupe must never
  share `storage_key` across rows (one subject's erasure must not touch another's bytes).
- PROP-133700: mark **Deferred (post-V0)**, keep #96. PROP-144500: **Deferred** until
  live-translation work starts. §9 PROP-004500 D4: presence-only `/config` readiness endpoint,
  post-cutover follow-up.
- §11 PROP-032306 D1 (ratify: direct integration) and D3 (acquisition field on `OrderPlaced`) —
  **cure folded** (graphql): the acquisition scalar is declared ONCE in `specs/common` so a
  future `ExternalOrderReceived` shares it.
- §19 D3 (static generated status page) — **cure folded**: the page renders its own generation
  timestamp and goes visibly stale (a frozen "all green" during the outage that killed its
  publisher is worse than no page). §19 D4: L2–L4 after cutover.

### Decided with the objecting lens's cure (customer-endorsed where noted)

- **PROP-164500 D3 — notification channel**: in-app + sound **AND SMS in the same V0 slice**
  (never "then"), escalation SMS at ~60–90 s unacknowledged, visible sound-armed/blocked state
  on `orders_queue`. Two lenses converged independently (ux: backgrounded-tab/autolock/gesture
  reality of a mobile-web back office; business: ~0.05 € SMS vs ~25 € refund + the customer's
  annual stream). **Customer-endorsed 2026-08-08.** The supervised pilot on #166 alone stays
  acceptable only while a human watches every order — condition stated.
- **PROP-172500 D5 — rider↔customer contact through the conversation**: plus masked/bridged
  call fallback and rider-side one-tap canned chips ("customer unreachable at the door" needs
  synchronous escalation; a keyboard on a bike violates the one-hand rule).
  **Customer-endorsed 2026-08-08.**
- **PROP-170000 D1 — skipped-events guard**: the gap is real and WIDER than the row stated
  (push-wake reads inside the race window; FOUR unguarded readers including the deletion
  engine — a skipped event there is a skipped GDPR erasure trigger; gap-detection is
  definitively dead post-ADR-20260731-160000 since erasure makes position gaps legitimate).
  Decided with dba's three adaptations binding: (1) `xact_id xid8` stamp + scans bounded at
  `pg_snapshot_xmin`, with an oldest-write-transaction-age alert; (2) the idle gate arms on the
  SAFE head, never `MAX(position)`; (3) one shared safe-head helper across all four readers.
- **PROP-170500 D4 — GraphiQL/Voyager**: keep, ADMIN-gated — **and self-hosted**; gating a
  no-CSP CDN bundle onto the authenticated admin origin moves a script-injection surface to the
  worst place. Self-host or drop Voyager.
- **PROP-170500 D5 — LISTEN/NOTIFY fan-out**: decided with reconcile-on-reconnect (NOTIFY has
  no delivery guarantee; a missed nudge is a silently stale tracking screen at peak) and the
  note that the gateway currently answers WS upgrades 501 — transport lands first.
- **PROP-170000 D5, `ce_events` leg — REVERSED to code-to-spec**: make the function sargable
  (`stream_name LIKE category || '-%'`); correcting the spec would enshrine a full-log seq scan.
- **PROP-171500 D1 — REWORDED before consent** (dba caught a contradiction with the APPROVED
  actor set): the dispatch-layer check is the fast-fail **pre-filter** (no mailbox row for an
  obviously-forbidden attempt, denial counter); the **authority is the actor's aggregate-state
  check** per PROP-20260728-135632's #235 correction. As originally worded the row would have
  re-opened a product-owner decision on the losing side.
- **§4 PROP-120055 D1 — RE-PREMISED**: object store = OVH Object Storage (EU), presigned S3
  URLs; Supabase Storage references are historical (ADR-20260731-061609, ADR-20260807-002705).
  Binding additions (dba): the `files` registry is IRREPLACEABLE state and lives with
  `captain-core`'s backup/PITR posture, never in replay-restorable views; the weekly restore
  drill gains a bucket↔registry orphan reconciliation (a PITR-stranded object is a GDPR leak
  that looks like compliance).
- **§4 PROP-185140 (authorization set) — decided WITH the graphql restatements**: intent stands
  end to end; scope predicates are EMITTED into every generated subgraph resolver's SQL
  (unscoped resolver unspellable); `ScopeMembership` is its own consumer-schema projector with
  one checkpoint and a declared cross-scope GRANT exception; the account-wide snapshot folds
  network events from the single log (event-carried); the guard mounts in each `graphql-{scope}`
  service and NEVER in the no-auth gateway; §3.3.4/§3.3.5 struck as moot. Two NEW rows enter
  the register instead of being improvised: the identity-bridge home (JWT claims for all roles
  vs common-schema bridge tables — must not invent a third mechanism beside `Actor.domain_id`)
  and §6.4 claim staleness, which STAYS OPEN as the one real policy question.
- **PROP-165500 D1 — allergen model** (legal evidence): 14-category Annex II enum + explicit
  "not declared" — with the binding modification that NOT_DECLARED **gates orderability** in the
  distance-selling UI (or, strict minimum, a specific functional contact means on the product
  sheet). "Required before publishing" is binding, not aspirational.
- **PROP-172500 D2 — proof-of-delivery photo** (legal evidence): legitimate-interest basis with
  a recorded LIA; **dispute hold** (files referenced by an open reclamation/chargeback suspend
  expiry — card windows outrun the 90-day window); rider UI guidance: package/door, never a
  person.
- **§4 PROP-120055 D2 — retention windows** (legal evidence): per-kind windows confirmed with
  the same dispute-hold rule, tombstone `uploaded_by` anonymized after a stated horizon, and
  the KYC note: under Connect, Captain stores ZERO KYC documents — the framework must not
  quietly acquire an AML retention regime.
- **§19 D2 — public metrics** (legal + business convergent): platform-wide aggregates ONLY; no
  per-restaurant/per-postcode/per-rider dimension ever without consent (sole-trader metrics are
  personal data; a partner's volume published is an adoption killer); prefer k ≥ 10 per cell
  when slicing ever starts.

### Escalated to the customer (added to the brief)

- **PROP-165500 D5 — merchandising order**: business-lens MODIFY (the recommendation never
  names WHO FUNDS the promo code, and on a 0%-commission platform that is the whole question);
  the reshape is a resequencing, and sequencing is the product owner's. Brief chapter 6.

### Named notes that ride the decisions

- `from:` naming collision (screens vs api.yaml scope-binding) — rename one before both DSLs
  ship the key (graphql).
- `SUSPENDED` rider status is deactivation machinery: written, communicated, appealable
  criteria required when it gains a write path (legal; Platform Work Directive file).
- NEW LAUNCH PRECONDITION surfaced by legal, on no register row until now: the mandatory
  **consumer mediator** (médiation de la consommation) registration.
- Business signals for every "revisit with production data" clause (funnel conversion, cohort
  repeat rates, rider decline/utilization, baskets, notification acknowledgement latency) have
  NO observability contracts — rides #400; a register row points at it.

## Consequences

- The register overhaul lands with this ADR: the 7 stale blocks move to their decided/superseded
  sections (retention sweep IMPLEMENTED; workers-run-where decided by the bins ADRs; PROP-201500
  superseded by ADR-20260808-144738; PROP-004500 already approved 2026-07-29; PROP-014500 and
  PROP-181926 superseded by the OVH/GitOps ADRs), counts and "last reconciled" are corrected,
  and the new rows above are added.
- Every decision here remains subject to the customer veto window and to plan-mode gates at
  realization — this ADR settles direction, not implementation.

## Refs

ADR-20260808-144738 · ADR-20260808-155656 · the five lens reports (session above) ·
[BRIEF-20260808-customer-decisions.md](../proposals/BRIEF-20260808-customer-decisions.md) ·
`docs/legal/BRIEF-20260808-listing-opt-out-objections.md`
