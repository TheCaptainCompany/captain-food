# ADR-20260905-223957 — The ADMIN door in two slices: a `PlatformMembership` binds the ADMIN seam, then the System host is routed

<!-- Filename: docs/adr/ADR-20260905-223957-the-admin-door-in-two-slices-a-platform-membership-binds-the-admin-seam-then-the-system-host-is-routed.md -->

## Status

Accepted — a **team decision by consent** under
[TEAM-DECIDES-OPTION-SPACES](../decisions/TEAM-DECIDES-OPTION-SPACES.yaml): the whole roster was briefed before any
code (full mob — the identity seam, a new stored fact, the platform's own people: `HOLD: human`), thirteen lenses
answered, one split (vernon: generalise the membership aggregate vs evans/young/dba/business: platform standing is its
own relationship) resolved by the rule that a split takes the option that reverses no decided record. Refines
[ADR-20260905-101349](ADR-20260905-101349-step-6-lands-in-four-slices-the-bridge-and-the-grant-first-the-door-second-and-the-accept-is-two-commands-in-two-lanes.md)
§1 and §8: the DESTINATION those sections recorded ("a `CAPTAIN_ONBOARDING` grant with a platform scope resolved at
the seam") is kept; the SLICING they left open is decided here. Realizes the missing precondition of step 6 of
[PROP-20260831-180622](../proposals/PROP-20260831-180622-staff-authentication-the-roster-the-invitation-and-the-door.md)
and discharges precondition (1) of
[RIDER-RESTRICTION-PRECONDITIONS](../decisions/RIDER-RESTRICTION-PRECONDITIONS.yaml) once both slices land.
Register rows: [ADMIN-DOOR-PRECONDITIONS](../decisions/ADMIN-DOOR-PRECONDITIONS.yaml) (open, team),
[PLATFORM-STANDING-VOCABULARY](../decisions/PLATFORM-STANDING-VOCABULARY.yaml) (decided by this ADR).
Reversal check: [PRINCIPALS-MEMBER](../decisions/PRINCIPALS-MEMBER.yaml) says "`ScopeType` is untouched" — this
ADR keeps it untouched (that is why option A won); `RLS-SEQ` finding (3) ruled a `ScopeType` widening unapproved —
respected. No record is reversed.

## Context

6-ii stamps MEMBER only and ADMIN stays hand-provisioned (ADR-20260905-101349 §8). Today
`crates/server/src/auth.rs:~308` mints `Identity::Admin { sub }` from the token ROLE alone — no binding row, no seam
— and `ReadScope::Admin` is `ScopePredicate::All` over 77 ADMIN operations (graphql-architect): a role claim is the
only thing between an anonymous token and cross-tenant read. 6-iii (System host routing) is pinned "last and only
once an admin can sign in" (§1), so step 6 cannot close without deciding how an ADMIN is BOUND. The architect named
this the next chunk on 2026-09-05 (6-iii blocked; step 7 depends on a complete 6; the value method binds).

## Decision

1. **Platform standing is its OWN relationship — a `PlatformMembership` aggregate, its own stream, its own event; `ScopeType` is NOT widened, `RestaurantAccessGranted` is NOT reused.** (evans: `ScopeType` names "one protected instance" and there is one platform and no platform id; `PrincipalKind` says ADMIN "can never be a member of anything" — both kernel sentences would become false, and PRINCIPALS-MEMBER's "untouched" would be reversed; young: `RestaurantAccessGranted.scopeId` is `$ref RestaurantId`, so a platform grant on it is permanently false history — a new event type is owed EITHER way; dba: RLS-SEQ already ruled the widening unapproved, and the expensive mistake is a prefix on the existing `ScopeMembership` projector group, whose checkpoint rewind is a platform-wide authorization denial for the drain.) Vernon's "generalise, don't fork" is honoured on MECHANISM, not vocabulary (architect): the seam reader, the bridge recipe and the projector-group-from-0 recipe are SHARED code paths (`resolve_member_scope`, the `Member` bridge shape), and the fold has the same two invariants (exists-once, revoked-at-most-once) — one invariant per aggregate, never two spellings of one lane. The word stays **ADMIN** (evans: no synonym; never "staff" in an identifier); the grant records an operational ACT — `CAPTAIN_ONBOARDING` — never a corporate status (business: Captain is a SAS today, the SCIC conversion is deferred to M18; ASSOCIE/COOPERATEUR/MANDATAIRE/SALARIE are forbidden values).
2. **The seam binds ADMIN.** `RequestRole::Admin` yields `Identity::Unbound` unless the seam resolves a live
   `PlatformMembership` grant for the token's subject — `Identity::Admin` becomes unspellable without a row
   (compiler-first, ADR-20260803-234035). `ReadScope::Admin` is KEPT; only its PRODUCER changes (graphql-architect:
   a new variant re-churns generated code for an identical predicate). The seam's read is a projection GRANT test
   (`EXISTS(grant row)`), Tell-side, never an Ask into the lane (vernon, PMW-3). The bridge `PlatformMember`
   (`auth_subject UNIQUE`, own `ProjectorGroup` born at 0, `read_common`, rebuild = checkpoint reset never TRUNCATE —
   dba) is the `Member` recipe copied byte-for-byte; the platform projector group is its OWN group from 0 (dba).
3. **The first admin is a recorded ACT through the mailbox — never a row, never SQL, never a data migration.**
   (vernon, young, farley, beck, legal, observability — unanimous.) An idempotent one-shot step DISPATCHES the
   ordinary `GrantPlatformAccess` command (basis `CAPTAIN_ONBOARDING`), the subject read from a secret never the repo
   (a Tours human's identifier in git is immutable and unerasable — farley); running it twice appends one fact
   (beck); it runs inside an OTLP-wired process so the irreversible act has telemetry (observability); it leaves the
   Art. 5(2) accountability artifact — who, when, basis, authority (legal). `GrantPlatformAccess` is `roles: [ADMIN]`
   in steady state; NO public "claim the first admin" path ever exists (graphql-architect: the install-wizard hole).
   Revocation of platform access is deferred until a second admin exists (holub) — recorded here as a follow-up,
   not a gap hidden.
4. **Two slices, WIP one, never concurrent on `auth.rs`/`supabase_auth.rs`:**
   - **6-v — the platform grant and the ADMIN seam binding**: `PlatformMembership` (grant only), the `PlatformMember`
     bridge, the seam change (§2), the one-shot bootstrap (§3), behind `RUN_PLATFORM_ACCESS_GRANT` (default false,
     production `"false"`, `decisionRow:` → ADMIN-DOOR-PRECONDITIONS). No third stamper and no new door: the ADMIN
     role claim stays hand-provisioned at the identity provider (§8 of the parent ADR stands); what changes is that
     the claim alone no longer suffices (holub's shortest path: the seam refuses an unbound ADMIN + one recorded
     grant act + 6-iii's refusal screen). Class **HOLD: human**, full-mob checkpoint (identity seam, a new stored
     fact, Captain's own people).
   - **6-iii — System host routing**: `HostRoute::System` serves the SDUI shell; every System screen `requires_auth:
     true` + `unauthenticated:` → a System sign-in/refusal screen of its OWN (ux: never reuse the restaurant surface's
     — its `not_linked` copy lies about the population); the anonymous browser gets the sign-in shell and NEVER the
     board (ux: a board with 401'd reads shows a lane holding a paid order as "none"); a bound-but-not-ADMIN principal
     gets its own refusal screen with no nav and no "request access" control (a control that does nothing). Class
     **HOLD: human**, narrow roster (Tours-facing surface, no stored shape). Discharges RIDER-RESTRICTION-PRECONDITIONS
     (1) together with 6-v.
5. **Observability**: a NEW `admin-sign-in` contract (observability: never widen `member-sign-in` — populations differ
   by orders of magnitude and pre-aggregation cannot be decomposed) — `admin.identity.resolve` span inside
   `auth.read_scope`, `result ∈ {resolved, not_found, lookup_failed}`, `business.correlation_id`; two dead-man gauges
   at BOTH composition roots (`platform_access_grant_enforcing`; the door gauge arrives with 6-iii); NO `email`, `sub`,
   `admin_id` or `on_roster` label anywhere — the platform population is tiny, so any label is near-identifying. The
   `auth.scope_membership.business.scope_type` bounded population is NOT touched (no PLATFORM value exists). Flip
   evidence is named on the row. farley's gate: a codegen test that every `RUN_*` key is `declare_flag`'d
   unconditionally at both roots with the same default — the parity has drifted twice on comments alone — lands with
   6-v. **AMENDED 2026-09-06 by [ADR-20260906-113444](ADR-20260906-113444-every-run-key-declares-runkind-door-or-worker-and-the-parity-gate-filters-on-it.md)**:
   the gate's population is the DECLARED DOORS (`runKind: door`), never every `RUN_*` key — a worker may carry a
   `decisionRow:` and stays out of the parity population; the proxy `decision_row.is_some()` is gone.
6. **Legal posture (advice recorded, never clearance)**: a Captain admin is provisioned from the person themself —
   **Art. 13 at collection**, not the restaurant colleague's Art. 14; a SECOND Art. 30 entry (Captain as SOLE
   controller over its own worker, incl. admin action logs and their retention — grade (b), verify), never a widening
   of "staff access management" (Art. 26 territory); the labour posture (employee / associé / mandataire) sits
   upstream of the lawful basis and is counsel's + the founder's; no entity capacity is recorded and no Art. 30
   register artifact exists in the repo — the founder's, external. Revoke ground, when it arrives, is a closed enum.
7. **The tests that fail first (beck)**: `an_admin_token_with_no_platform_grant_is_unbound` (red today — `auth.rs`
   returns `ReadScope::Admin`; the `graphql_acl.rs` prose "ADMIN carries no domain claim BY DESIGN" is deleted in the
   same diff); `the_bootstrap_replays_from_domain_events_alone` (bootstrap → TRUNCATE the projection → replay → the
   admin still resolves — a seeded row cannot pass it); `running_it_twice_appends_one_fact`; the grant bridge
   consulted ZERO times on any request leg that does not carry the admin's own verified subject; one walk suite
   (router → mailbox worker → projector → `/admin/graphql` `riders` admitted; revoke deferred). Mutants: the role-only
   mint restored; the bootstrap appending twice; a resolution keyed on email.

## Alternatives considered

- **B — generalise `RestaurantMembership` with `ScopeType::PLATFORM`** (vernon): one aggregate, one lane, one revoke;
  rejected because it reverses PRINCIPALS-MEMBER, widens a stored column for a population of 1–3, buys no code reuse
  once the event forks (young), and the mechanism reuse it wanted is delivered anyway (§1).
- **A public "claim the first admin if none exists" bootstrap**: rejected (graphql-architect) — reachable forever from
  the open path, its predicate racing a lagging projection.
- **A seeded projection row / env-provisioned admin / committed data migration**: rejected unanimously — vanishes at
  the next checkpoint reset or restore drill (the moment it locks every admin out), or puts a real person in git.
- **6-iii before the binding**: refuted twice in the parent ADR; a routed System host with role-trust ADMIN is the
  cross-tenant read behind one claim.

## Consequences

Positive: `Identity::Admin` is unspellable without a recorded grant; step 6 can close; RIDER-RESTRICTION-PRECONDITIONS
(1) has a discharge path; the platform relationship has its own honest vocabulary and its own register entry.
Negative: a second grant lane and a second bridge; revocation of platform access is a follow-up; the founder holds
the external items (controller entity and Art. 30 register, labour posture, the first admin's identity as a secret).
Holub's waste warning stands on the record: (1) is one of SEVEN open rider-restriction preconditions — discharging it
is starting, not finishing; seven consecutive dark PRs have reached nobody in Tours while production is suspended
(ADR-20260817-105844). One deliverable per card: the four ceiling hits (#875, #885, #899, #901) tracked card size.

## Follow-up actions

- Issues: "#639 part C step 6-v: the platform grant and the ADMIN seam binding" (Urgent — role-trust identity is
  tier-1 security; above 6-iii, which it blocks); "#639 part C step 6-iii: System host routing behind auth"; the GREEN
  lane [#904](https://github.com/TheCaptainCompany/captain-food/issues/904) (silent refresh + `?next=`).
- Rows: ADMIN-DOOR-PRECONDITIONS (open, team) — the flip conditions of `RUN_PLATFORM_ACCESS_GRANT` and, later, the
  System routing; PLATFORM-STANDING-VOCABULARY (decided by this ADR).
- Revocation of platform access: a follow-up once a second admin exists (holub), recorded on the row.
- PROP-20260831-180622 §11 row 6 gains 6-v; the living proposal's §5 FORK 3 is not rewritten (the door lives where it
  did; this ADR adds the binding, not a door).

## Consulted (ADR-20260812-143619 — one line per lens)

- **architect** — named the chunk (6-iii blocked on its own condition; step 7 depends on a complete 6); option A first; two slices; 6-v `Urgent`; mechanism shared, vocabulary forked.
- **vernon** — generalise, don't fork; one invariant per aggregate; two spellings never; seam read is a projection grant test, never an Ask (PMW-3). CATCH: a seeded bootstrap row. Split resolved against his option by the no-reversal rule; his mechanism-reuse point carried.
- **young** — the widening is additive on TEXT but reusing `RestaurantAccessGranted` is false history → a new event type; variant deployed before anything emits it; live drift `scope_membership.rs:196` passthrough to fix. CATCH: seeded row/env/INSERT bootstrap.
- **evans** — keep the word ADMIN; `ScopeType PLATFORM` breaks two kernel sentences and reverses PRINCIPALS-MEMBER; platform standing is its own relationship and word; `AccessBasis` is restaurant-worded. CATCH: PLATFORM on ScopeType or ADMIN in PrincipalKind without a register row; any ADMIN synonym.
- **graphql-architect** — 77 ADMIN operations behind a role claim; keep `ReadScope::Admin`, change its producer; the first grant is not a graph mutation; never a PUBLIC bootstrap; limits per role. CATCH: a platform-grant/first-admin mutation reachable by a non-ADMIN role.
- **legal-specialist** — Art. 13 not 14; a second Art. 30 entry (sole controller); labour posture upstream; no register artifact exists; closed revoke ground; Art. 5(2) artifact from the bootstrap. CATCH: reusing the restaurant notice/entry; a bootstrap without a stored attributable act. Never clearance.
- **farley** — bootstrap = idempotent one-shot dispatching the ordinary command, subject from a secret; never SQL, never a committed data migration; a codegen test for `RUN_*` parity at both roots; secrets via spec configuration. CATCH: a bootstrap writing a projection row or baking an identity into migrations.
- **dba** — RLS-SEQ already ruled the widening; zero DDL for the value; the expensive mistake is a prefix on the existing `ScopeMembership` group → own group from 0; bridge = `member` byte-for-byte; reset-never-TRUNCATE. CATCH: a prefix on the existing group or any checkpoint rewind.
- **observability-agent** — a new `admin-sign-in` contract; two dead-man gauges at both roots; no labels (tiny population); flip evidence named; shadow first. CATCH: a bootstrap outside an OTLP-wired process; the `scope_type` bounded population silently widened.
- **ux-designer** — a System sign-in screen of its own; two refusal screens; the shell never the board for an anonymous browser; `?next=` survives the 19:40 lockout. CATCH: a `requires_auth` that gates only data requirements and paints an empty roster.
- **beck** — the red-first tests (§7); TRUNCATE-and-replay proves the bootstrap; grant not email; one walk suite. CATCH: `Identity::Admin` constructible outside the seam; walk evidence without `DB_TESTS_REQUIRED=1`.
- **business-specialist** — Captain is a SAS today, population 1–3; the grant records an act, never a status. CATCH: any governance-status value in an identifier, basis or column.
- **holub** — the whole seam is not needed before 6-iii; minimum = refuse an unbound ADMIN + one recorded grant act + the refusal screen; defer the graph mutation's UI, roster/revoke, tooling; one deliverable per card; (1) is one of seven open preconditions. CATCH: a platform aggregate before a second admin exists — answered in §1: the smallest honest RECORDING of the act needs a stream, and the alternatives reverse a decided row or store false history.
