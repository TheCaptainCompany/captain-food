# ADR-20260906-023825 — The ADMIN sign-in door is the member door's shape, identify-only against the platform grant

<!-- Filename: docs/adr/ADR-20260906-023825-the-admin-sign-in-door-is-the-member-doors-shape-identify-only-against-the-platform-grant.md -->

## Status

Accepted — a **team decision by consent** ([TEAM-DECIDES-OPTION-SPACES](../decisions/TEAM-DECIDES-OPTION-SPACES.yaml)),
briefed 2026-09-06 (eight lenses, all option A: ux, beck, graphql-architect, observability, legal, farley, evans,
vernon). Refines [ADR-20260905-223957](ADR-20260905-223957-the-admin-door-in-two-slices-a-platform-membership-binds-the-admin-seam-then-the-system-host-is-routed.md)
§2/§4/§5/§6 — that ADR named the seam, the two slices, the `admin-sign-in` observability contract and the legal
posture, and was explicitly SILENT on the mechanism by which an ADMIN obtains a token in a browser at all (its
own §2 objection: "nothing lets an ADMIN obtain a token in a browser"). This record closes that gap, decided at
the 2026-09-06 briefing. Also cites: [ADR-20260905-101349](ADR-20260905-101349-step-6-lands-in-four-slices-the-bridge-and-the-grant-first-the-door-second-and-the-accept-is-two-commands-in-two-lanes.md)
§8/§10 (the member-door precedent this record transposes); [ADMIN-DOOR-PRECONDITIONS](../decisions/ADMIN-DOOR-PRECONDITIONS.yaml)
(open, team) items (6)/(7), which this record starts discharging; [RIDER-RESTRICTION-PRECONDITIONS](../decisions/RIDER-RESTRICTION-PRECONDITIONS.yaml)
item (1); [ADR-20260818-101500](ADR-20260818-101500-the-restaurant-signs-in-by-email-link-and-638-freezes-at-chunk-1.md)
(one stamper per role, hardcoded); [ADR-20260830-234532](ADR-20260830-234532-the-second-sitting-publish-france-wide-revocation-is-immediate-and-the-objection-chain-was-decided-22-days-ago.md)
(a claim carries `{ role }` only); [PLATFORM-STANDING-VOCABULARY](../decisions/PLATFORM-STANDING-VOCABULARY.yaml)
(decided — the word is ADMIN, the verb is GRANT); [PRINCIPALS-MEMBER](../decisions/PRINCIPALS-MEMBER.yaml) /
`RLS-SEQ` (untouched — `ScopeType`/`PrincipalKind` are not touched by this record).

## Context

6-v (PR #907, merged) made `Identity::Admin` unspellable without a live `PlatformMembership` grant — the seam
(`resolve_platform_scope`) refuses an unbound ADMIN token. But nothing in the repo lets a real person **obtain**
an ADMIN-role token in a browser at all: the identity provider stamps the ADMIN claim only through the existing
hand-provisioned Supabase console flow, and no product-owned door exists on `system.captain.food` for a person to
sign in through. Without this record, 6-iii (System host routing) would route a host with no door behind it —
exactly the state ADR-20260905-101349 §1 refused for the member surface ("6-iii ships `requires_auth: true` +
`unauthenticated:` + a refusal screen or stays dark"). ADMIN-DOOR-PRECONDITIONS items (6) and (7) name the System
sign-in/refusal screens and the `?next=`/silent-refresh dependency as open; this record is the mechanism decision
those items presuppose.

## Decision

1. **A PUBLIC pair, `requestAdminSignInLink` / `confirmAdminSignIn`, lands in `specs/common/` (kernel), beside
   `GrantPlatformAccess`** — the `requestMemberSignInLink`/`confirmMemberSignIn` shape (ADR-20260905-101349 §7-§10)
   transposed to the platform context. Identify-only against the `PlatformMember` bridge (the SAME read port 6-v
   already built, `application::queries::PlatformMemberRepository`): a verified email with no live grant stamps
   nothing and creates nothing. The stamped `{ role: ADMIN }` claim is a HINT the seam RE-DERIVES on every request
   — pinned exactly as 6-v's seam already behaves: **a stamped ADMIN claim with no grant row resolves
   `Identity::Unbound`**, never `Identity::Admin`. `roles: [PUBLIC]` EXPLICIT, `graphql_role: PUBLIC` on the
   System surface's own `sign_in` screen — never a public QUERY reflecting platform-staff state (no `doorOpen`
   flag, no viewer probe): the client-visible claim role is a RENDER HINT only, truth is the server refusal.
2. **The pair is addressed to its OWN actor type, `AdminSignIn`, `mailbox.partitions: 5` — never to
   `PlatformMembership`.** `PlatformMembership` is `partitions: 1` (population 1-3, human-paced): addressing an
   anonymous PUBLIC pair to it would let any stranger's sign-in attempt head-of-line block the single lane
   carrying `GrantPlatformAccess` and the one-shot bootstrap behind up to four IdP round trips (vernon,
   PMW/isolation precedent — the `RestaurantMembership` vs `RestaurantInvitation`/`AcceptRestaurantInvitation`
   split is the same reasoning one level down). `AdminSignIn` emits nothing and holds no stream: pure routing,
   chosen for lane hygiene, the `RestaurantMembership`'s `RequestMemberSignInLink`/`ConfirmMemberSignIn` receiver
   shape (own aggregate, `emits: []`).
3. **The refusal is `AdminAccessNotGranted` — never "linked" and never `MemberNotLinked` reused.** (evans:
   "linked" binds two pre-existing things — a login and a `Member` row; on the platform side there is nothing to
   link, the grant row's PK IS the aggregate identity, `specs/common/scalars.yaml#/PlatformMembershipId`.) The
   context's verb is GRANT throughout this bounded context (`GrantPlatformAccess`, `PlatformAccessGranted`,
   `PlatformAccessAlreadyGranted`, `PlatformAccessGrantDoorClosed`) — the refusal follows: `AdminAccessNotGranted`,
   never implying a pending link, never offering a self-service path, never revealing a lookup outcome beyond the
   typed refusal itself.
4. **Routing ships UNGATED; the door itself ships behind `RUN_ADMIN_SIGN_IN_DOOR`** (default false,
   `decisionRow: ADMIN-DOOR-PRECONDITIONS`) — the `RUN_MEMBER_SIGN_IN_DOOR` shape exactly: OFF refuses BOTH
   mutations with the typed `AdminSignInDoorClosed` BEFORE the identity provider is touched. Routing the host
   (`Surface::System`, `HostRoute::System` serving the SDUI shell) is a SEPARATE, always-on concern from the door
   key: an anonymous browser reaching `system.captain.food` always gets the sign-in shell, never the board, whether
   or not the door itself is flipped (ux: a dark door returning a typed translated refusal into `inline_error` IS a
   real control, not a fake-disabled one).
5. **No public QUERY reflects platform-staff state anywhere** (graphql: the install-wizard-hole precedent, one
   level down): no `doorOpen` flag, no viewer probe on `/public/graphql`, no "am I an admin" resolvable by an
   unbound caller. The board renders solely from data that RETURNED (a refused read renders nothing, never a
   degraded truth); SSR uses the SAME seam as the live path.
6. **Refusal screens carry NO "Se déconnecter" until [#94](https://github.com/TheCaptainCompany/captain-food/issues/94)
   lands** (ux STOP): the member door's `not_linked` screen offers a `navigate → /sign-in` fallback because
   sign-out wiring does not exist yet either, but on the System surface a caller who already holds an
   ADMIN-claimed-but-ungranted cookie would LOOP back through `/sign-in` and land on `no_access` again forever —
   the same cookie is still valid and still fails the same grant test. The `no_access` refusal names the support
   route and leaves the loop unbuilt rather than shipping a control that renders and does nothing (or worse, one
   that renders and does something wrong).

## Alternatives considered

- **B — reuse `GrantPlatformAccess`'s door as the sign-in mechanism** (an ADMIN "logs in" by having someone grant
  them access on the spot): rejected — sign-in and grant are different acts on different timelines (a grant is a
  standing decision made once by an existing admin; a sign-in happens every session), and conflating them would
  make every sign-in attempt an authorization event on the write-side lane vernon's finding (§2) already protects.
- **C — an unreviewed ad-hoc copy of the member door's screens, with no team review of the wording**: rejected —
  the refusal copy sits on a legal surface (Art. 13, no "not yet linked" implication) and a Tours-facing surface
  (HOLD: human); shipping unreviewed copy here is exactly the risk class this ADR's own briefing exists to catch.
- **Addressing the pair to `PlatformMembership` directly** (vernon's Q1c, rejected): head-of-line blocks the
  single-partition grant lane behind an anonymous caller's IdP round trip; the STOP condition on the dispatch
  card that executes this ADR names it explicitly.

## Consequences

Positive: an ADMIN can obtain a token in a browser at all, closing the gap ADR-20260905-223957 §2 named; the
seam's existing fail-closed behaviour (stamped claim, no grant → `Identity::Unbound`) gets its first REACHABLE
caller; `AdminAccessNotGranted` gives the refusal an honest, ungeneric name; ADMIN-DOOR-PRECONDITIONS items (6)
and (7) have a concrete mechanism to discharge against.

Negative: a fourth stamper (`stamp_admin_claim`) and a fourth hardcoded PUT body function
(`stamp_admin_put_body`); revocation of platform access remains undecided (a named follow-up, not resolved here
— vernon Q1b: no `RevokePlatformAccess`/`PlatformAccessRevoked` exists anywhere, so once stamped there is no
un-stamp mechanism beyond the seam's own per-request re-derivation over the projection row); the
`no_access` refusal screen has no exit control until #94 lands.

## Follow-up actions

- Revocation of platform access (vernon Q1b): once a second admin exists, a `RevokePlatformAccess` command /
  `PlatformAccessRevoked` event on the `PlatformMembership` stream is the honest shape — tracked as a follow-up
  issue for the coordinator to file, not built in this slice.
- `?next=` return-to-screen + the silent `/auth/refresh` retry (issue [#904](https://github.com/TheCaptainCompany/captain-food/issues/904))
  remain named preconditions of flipping `RUN_ADMIN_SIGN_IN_DOOR`, exactly as for the member door.
- `docs/decisions/ADMIN-DOOR-PRECONDITIONS.yaml` `note:` gains the landing evidence for items (6)/(7) in the
  same PR that lands this record's code.
- `docs/SPEC-LOG.md` gains one row in the same commit as the spec text (not this ADR's own commit).

## Consulted (ADR-20260812-143619 — one line per lens, briefed 2026-09-06)

- **ux-designer** — option A (the walked sequence sign_in → confirmation → sign_in_return → board | no_access
  exists; B dead-ends on `AuthSubjectHoldsAnotherRole`; C is unreviewed copy); a dark door returning a typed
  translated refusal is a real control, ship UNGATED; the server-answered seam decides board vs refusal, never
  the client-visible claim; STOP on any "Se déconnecter"/navigate-to-sign-in control on `no_access` (loops on
  System until #94).
- **beck** — option A (the only option with an existing test oracle, `member_sign_in_door.rs`'s door-closed
  fixture and enumeration-oracle pattern; B is unspellable against the existing seam; C deletes its own test);
  named the eight red-first tests and mutants m1-m7, including the revoked-grant/still-valid-cookie case (parent
  ADR §8's answer) and the gauge-at-both-roots proof.
- **graphql-architect** — option A, placement correction: `specs/common/api.yaml` beside `grantPlatformAccess`,
  same-scope `$ref`; `roles: [PUBLIC]` explicit; the request leg touches the identity port only, zero bridge
  calls; STOP on any public QUERY reflecting platform-staff state (the install-wizard hole in query clothing).
- **observability-agent** — the `admin-sign-in` contract GROWS (never `member-sign-in`, which stays untouched);
  request leg carries no `business.result` (the enumeration-oracle-in-telemetry risk at population 1-3); confirm
  span's result vocabulary widened to the mapper's true set; the {15,50}ms budget is one index probe and must be
  split from the door legs' own {400,1200}ms budget; defect counter `admin_claim_stamp_failed_total`; §20 ratchet
  (every metric needs its `pub const` + `metric::IDENT`).
- **legal-specialist** — option A keeps collection on a Captain-controlled surface (B would show restaurant-worded
  notice text to Captain's own worker — the Art. 13/14 confusion the parent ADR §6 forbids; C collects on the IdP
  page, unverifiable); copy must carry the Art. 13 elements as a named `gaps:` line (no real legal page exists
  yet); neutral refusal wording, no entity/capacity/employment words; the typed email's SECOND store (the mailbox
  payload row) needs its retention named before any flip.
- **farley** — option A (the only option whose verdict rides the existing pipeline); `system.captain.food`
  already routes in both deploy trees — STOP on any `deploy/generated/**` diff; no new secret (the request leg
  reuses `EMAIL_QUOTA_KEY_HMAC_SECRET`); `RUN_ADMIN_SIGN_IN_DOOR` needs a MANDATORY `decisionRow`; the bootstrap
  is an EXECUTED step of the environment recipe, not an optional key.
- **evans** — the rename `AdminAccessNotGranted` (blocking language finding — "linked" implies two pre-existing
  things that do not exist here); the verb throughout this context is GRANT; three terms at three layers (system
  = host/surface, platform = standing/context, ADMIN = role) — a `SystemMembership`/`PlatformAdmin`/
  `AdminMembership` anywhere would be a finding; kernel placement beside `GrantPlatformAccess` confirmed.
- **vernon** — the bridge read is a repository read inside the handler, not an Ask (PMW-3 does not bind); the
  hazard is a decision followed by an irreversible external effect (the stamp), made safe only because the seam
  re-derives per request — pinned explicitly in this record; `PlatformMembership` is WIDTH 1 and reused strictly
  as a read port, never as this pair's address — the pair gets its OWN width-5 actor type
  (`RestaurantMembership`/`RestaurantInvitation` split precedent); no revoke command exists anywhere yet
  (follow-up, not built here).

**CONSENSUS: 8/8, option A.**
