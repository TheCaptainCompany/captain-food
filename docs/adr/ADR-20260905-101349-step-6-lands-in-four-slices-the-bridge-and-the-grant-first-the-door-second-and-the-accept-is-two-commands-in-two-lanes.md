# ADR-20260905-101349 — Step 6 lands in four slices: the bridge and the grant first, the door second, and the accept is two commands in two lanes

<!-- Filename: docs/adr/ADR-20260905-101349-step-6-lands-in-four-slices-the-bridge-and-the-grant-first-the-door-second-and-the-accept-is-two-commands-in-two-lanes.md -->

## Status

Accepted — a **team decision by consent** under
[TEAM-DECIDES-OPTION-SPACES](../decisions/TEAM-DECIDES-OPTION-SPACES.yaml): the whole roster was briefed before any
code (full mob — an identity surface, stored event shapes, Tours-facing: `HOLD: human`), thirteen lenses answered,
all consented to the slicing and its order, and the splits inside it are resolved below with the rule that carried
each. Realizes **step 6** of
[PROP-20260831-180622](../proposals/PROP-20260831-180622-staff-authentication-the-roster-the-invitation-and-the-door.md)
§5, §6, §8, §9, §11 row 6 and settles its open questions §13 Q1 (FORK 1) and Q2 (`MemberAuthority`), in the order the
founder fixed on 2026-09-04 (3, 4, 5, 6, 7). The founder reads this record; the external items in §11 are his.

**Relates**: [ADR-20260830-213135](ADR-20260830-213135-the-staff-auth-answers-captain-binds-the-first-person-and-the-owner-invites-the-rest.md)
(Captain binds the first person; the owner invites the rest), [ADR-20260818-101500](ADR-20260818-101500-the-restaurant-signs-in-by-email-link-and-638-freezes-at-chunk-1.md)
(email link), [ADR-20260818-004646](ADR-20260818-004646-no-business-identifier-lives-in-the-identity-provider.md)
(a role-only claim; the binding is re-derived per request), [ADR-20260831-220559](ADR-20260831-220559-the-person-is-a-principalkind-not-an-eighth-usertype.md)
(`PrincipalKind::MEMBER`), [ADR-20260829-230418](ADR-20260829-230418-aggregates-own-the-facts-isolation-first.md) /
[AGGREGATES-OWN-THE-FACTS](../decisions/AGGREGATES-OWN-THE-FACTS.yaml) (decided, plan open: no new development stages
an append onto a foreign stream), [ADR-20260904-014136](ADR-20260904-014136-rider-restriction-ships-now-with-the-smallest-closed-set-of-grounds-and-counsel-can-only-add.md)
(the closed-ground precedent), [ADR-20260904-081527](ADR-20260904-081527-rider-standing-is-a-grant-on-the-identity-row-the-doors-are-human-only-and-step-4-lands-in-three-slices.md)
§11 (the slicing precedent), [ADR-20260904-152807](ADR-20260904-152807-the-admin-s-hands-one-custody-truth-read-at-query-time-a-door-that-refuses-until-the-notice-exists-and-two-slices.md)
§9 (the System surface is not host-routed), [ADR-20260905-065415-the-restriction-fact-terminates-the-rider-s-socket-a-connection-local-standing-read-inside-the-guard-and-one-writer-to-the-transport](ADR-20260905-065415-the-restriction-fact-terminates-the-rider-s-socket-a-connection-local-standing-read-inside-the-guard-and-one-writer-to-the-transport.md) (step 5),
[RIDER-RESTRICTION-PRECONDITIONS](../decisions/RIDER-RESTRICTION-PRECONDITIONS.yaml) (open — precondition (1)),
[SUPPORT-CONTACT](../decisions/SUPPORT-CONTACT.yaml), [REVOKED-COLLEAGUE-NOTICE](../decisions/REVOKED-COLLEAGUE-NOTICE.yaml)
(open, counsel), [ADR-20260817-105844](ADR-20260817-105844-the-walk-goes-first-on-one-database-and-production-stays-suspended.md)
(production suspended; the walk on one database).

## Context

Steps 3, 4 and 5 of part C are merged and every one of them shipped dark: no admin can sign in (the System host renders
a static line), the restrict door is pinned off in production, the rider population is zero. Step 6 is the staff
roster and the door — the first slice a person can walk end to end. What is already on main (architect, verified):
both magic-link legs in `specs/services.yaml` (`send_email_magic_link`, `verify_email_token` returning the PROVEN
email and the provider-session trio, "the staff magic-link login rides it"), implemented in
`crates/infrastructure/src/integrations/supabase_auth.rs` with no staff caller; R1 `graphql_role` and
`Surface::role_for` (#854); `SUPPORT_CONTACT` (2c-i); the identify-only PUBLIC pair pattern (2c-ii); the
`ScopeMembership` store (grant narrow, revoke BROAD — every member of one role on one scope); 2a's
`auth_subject_reservations` keyed `(principal_kind, auth_subject)` with no release; `MemberId` and `PrincipalKind::MEMBER`.
Genuinely new: both aggregates, a third stamper, the `Member` bridge table, five screens, and GraphQL depth/complexity
limits (zero hits repo-wide). Two facts that reshape the PROP's text: the runtime delivers commands over a
`StagingEventStore` whose staged appends flush inside the mailbox's completion transaction with no stream-name guard
(vernon: `crates/infrastructure/src/mailbox/handler.rs`, `flush.rs`), so a handler that appends to two aggregates is a
foreign-stream append under the fence, not "two appends" — the shape the open AGGREGATES-OWN-THE-FACTS plan forbids
for new development; and a stamped subject with no binding row is `Identity::Unbound` → `ActingRole(Public)`
(architect: `crates/server/src/auth.rs`), so a MEMBER stamp over an empty table fails closed.

## Decision

1. **Four slices, in this order, WIP one, each `HOLD: human`, never two of them concurrent on `auth.rs` /
   `supabase_auth.rs`.** **6-i — the bridge and the grant**; **6-ii — the door** (with the public-graph limits);
   **6-iv — roster and invitation**; **6-iii — System host routing**, last and only once an admin can sign in, shipping `requires_auth: true` + `unauthenticated:` + a System refusal screen or staying dark (ux: an un-darkened board with 401'd reads shows a lane holding a paid order as "none"; architect:
   `requires_auth` bounces to `unauthenticated:`, which the System surface lacks until a door exists, and SSR runs as
   the anonymous PUBLIC principal, so un-darkening System earlier renders the admin shell to any browser — a rendered
   control that does nothing). 6-i is a hard dependency of 6-ii **for verification, not compilation** (architect,
   beck, holub): without a `Member` row the door has one observable outcome — the refusal — and the enumeration-oracle
   test has no positive arm. 6-i is inventory until 6-ii lands; it is bounded (holub): one dispatch commitment,
   6-ii starts when 6-i merges, and 6-i ships only what 6-ii's happy path consumes.
2. **FORK 1 → Option A, with the accept as two commands in two lanes.** young and evans for A (a targeted revoke by
   `membership_id` — the existing broad revoke arm would strip a whole roster on one revoke and a replay would
   restore it, a rebuild becoming an authorization event; and under C "membership" names a person who never joined,
   on what §12 calls an employment record); vernon for C because A's accept as written appends two streams from one
   lane while the isolation plan is open. Reconciled: A's stream identity stands, and 6-iv's **accept is
   `AcceptRestaurantInvitation` on the invitation stream followed by `GrantRestaurantAccess` (`basis: MEMBER_INVITATION`)
   on the membership stream, sequenced by the accepting member's client — acceptance-first, PENDING — never one handler
   staging two streams and never a process manager** ("the human is the process manager" stands for V0). FORK 2
   unchanged: the membership fold is a check, not a lock. 6-i itself is one aggregate, one stream, one lane.

   **Amendment, round 2 of the 6-iv card (2026-09-05, consent, Consulted block below).** §2's leg 2 as
   first landed was `GrantRestaurantAccess` widened to accept an ADMIN-authored grant OR an
   invitation-carried one on the same command, distinguished by which optional fields were present.
   That is exactly the "illegal states stay spellable" shape ADR-20260803-234035 rules out -- a caller
   could send `basis: MEMBER_INVITATION` with no invitation proof, or an ADMIN-shaped payload through
   the public door. **Split by privilege, not by field**: `GrantRestaurantAccess` reverts to
   ADMIN-only with every field required (the 6-i shape, unchanged); a new PUBLIC command
   `GrantRestaurantAccessByInvitation { invitationId, token }` is leg 2 of the accept, proof is the
   SAME token `AcceptRestaurantInvitation` already verifies (no new legal actor, no new proof
   instrument), and `membershipId`/`memberId` are DERIVED -- UUIDv5 of `invitationId` under a
   command-local namespace constant, never caller-supplied -- so one invitation can never mint two
   memberships: the invariant is structural, not a runtime check. The two-lane, client-sequenced
   accept (leg 1 on the invitation stream, leg 2 on the membership stream, never one handler staging
   both, never a process manager) is UNCHANGED by this split; only leg 2's command shape and
   authorization surface change.
3. **Vocabulary (evans).** `MemberAuthority = { MANAGER, OPERATOR }` — `ADMINISTRATOR` refused (one stem for two
   populations on an authorization surface; `restaurant_manager` already means the person who runs one shop); two
   values, additive-only, no `*AuthorityChanged` (a change is revoke + grant). `AccessEvidence` is renamed
   **`AccessBasis`** (only `GOOGLE_BUSINESS_PROFILE` is evidence we hold); the closed set of four is DECLARED
   (`CAPTAIN_ONBOARDING | GOOGLE_BUSINESS_PROFILE | OWNER_DECLARATION | MEMBER_INVITATION`) and 6-i's command ACCEPTS
   only `CAPTAIN_ONBOARDING`. Identifiers drop "staff": `requestMemberSignInLink` / `confirmMemberSignIn` (rider
   precedent). `ScopeMembership.member_id` acquires a second meaning the day a MEMBER row lands; 6-i carries the
   disambiguating column note and names §6.5's semantics fix as its owner. No provider term (`sub`, `app_metadata`,
   `accessToken`) in any command or event payload.
4. **The reservation is 2a's table, not a new one (dba).** `auth_subject_reservations` is already keyed
   `(principal_kind, auth_subject)` and its own note names this case; a second table would arbitrate one invariant
   twice. Cost: one `BoundPrincipal` variant, zero DDL. Revocation never releases it (§6.4) — a lifetime identifier
   binding, written down as such with its future path (pseudonymise / crypto-shred, never delete) (legal).
5. **The `Member` bridge**: its own `ProjectorGroup "Member"` born at 0, `auth_subject UNIQUE`, no other index,
   `database: read_common`; rebuild = checkpoint reset, never TRUNCATE — a rule in its `rules:` that holds only while
   `Member` carries no grant-shaped column (dba). The staff arms ride `ScopeMembership`, whose recipe is the opposite
   (DELETE + reset + full replay, all read databases): the slice times a from-zero replay and puts the number in the
   runbook with its antecedent. Predicates over these tables are GRANT tests only; the rebuild recipes land as
   executable tests, never prose (young, beck).
6. **Two gates, each with a `decisionRow` (farley).** `RUN_MEMBER_ACCESS_GRANT` on 6-i's grant path — "nothing
   reachable because no UI exists" is intent, not a gate, and the first hand-provisioned grant about a Tours human is
   the irreversible moment that starts every legal clock (legal). `RUN_MEMBER_SIGN_IN_DOOR` on 6-ii. Both default
   false, production `"false"`; each flip a separate recorded decision. Code ships dark first; the writer key flips
   after (an unknown `eventType` hard-errors in the event store while projectors skip it — a rollback after the first
   appended fact is no longer clean).
7. **The third stamper is role-only**: `stamp_member_put_body()` → `{"role": "MEMBER"}`, hardcoded, selected at
   compile time (#437), **no `member_id`** (graphql-architect, farley: the binding re-derives per request from the
   `Member` row; a stamped id is a cache nobody can invalidate and makes rollback a provider write that can fail).
8. **The ADMIN beneficiary set is not closed by the door.** `Identity::Admin` carries no binding — its scope IS the
   role — so an email-only ADMIN stamp would turn "type an address" into platform staff. 6-ii stamps MEMBER only; ADMIN
   stays hand-provisioned (graphql-architect's C, architect); a `CAPTAIN_ONBOARDING` grant with a platform scope
   resolved at the seam is recorded as the final vision. Consequence, recorded not hidden: **6-ii + 6-iii do not
   discharge RIDER-RESTRICTION-PRECONDITIONS (1)**; that row is amended to say what does.
9. **The public-graph limits ride 6-ii and are not a `/public` patch.** `limit_depth` / `limit_complexity` apply on
   EVERY role's schema build (graphql-architect: the staff host is authenticated, not trusted), which means a per-role
   schema map or a per-request extension keyed on the path role — today ONE `CaptainSchema` serves `/{role}/graphql`
   (beck: the card chooses; the executor must not discover it). Values are **codegen-derived, never guessed**: the
   client documents are generated (`crates/web/src/graphql.rs` `query_document` over `ResolverKey::selection()`), so
   codegen emits the max depth/complexity per role as constants plus a test that reds when a fragment pushes past the
   configured limit; until that exists every value is `UNVERIFIED input`. Instruments (observability):
   `graphql_request_rejected_total{role, reason}` PLUS observed depth/complexity histograms and a `graphql_limit_max`
   gauge — zero rejections must not read like "limits not installed". Depth limits do not bound send abuse: the
   link-request door reuses the `SmsSendAuthorizer` wall shape for email.
10. **The door (6-ii)**: the PUBLIC pair as MUTATIONS (Q7 — the `verifyPhone` precedent; a door absent from the SDL does
    not exist for a client); the confirmation panel says the same thing whether or not the address is on a roster, and
    the `Member` bridge is consulted ZERO times on the request leg — the enumeration oracle is a TEST (body, status,
    timing class), never a metric, and no `on_roster` label may exist at request time because computing it creates the
    oracle (observability, beck). A `member-sign-in` observability contract lands in the SAME PR: spans
    `member.signin.link_request` / `member.signin.confirm` (`business.result`: linked | not_linked | token_invalid |
    token_expired | lookup_failed) / `claims.stamp`; counters for link requested, confirmed, stamp failed, refused,
    and a gate-liveness gauge; never an email, token or messageId as a label; `correlation_id` breaks across the mail
    hop as it does across the SMS hook — join hashed-email + window. The refusal screen (§8.5) keeps no nav, no counts,
    no phone; it gains a legal/privacy link (legal: LCEN 6-III + Art. 13) and its wording is counsel-reviewable copy.
11. **What 6-i must record before the first real grant (legal, grade (a)/(b), VERIFY-FIRST — not clearance)**:
    `RevokeRestaurantAccess.ground` is a **closed enum, smallest set, no free text, no performance ground** (the
    ADR-014136 precedent — free text puts "fired for theft" in an immutable log); §6.5's "no engine to hang on" is
    corrected (a `deletion:` engine exists, gated off; Member's block is owed on the same clock as Customer's); the
    Art. 14 notice to the person Captain provisions from the owner's say-so is due at obtaining, not at sign-in; an
    Art. 30 entry "staff access management"; the magic-link email is a processing with Supabase as processor.
    **External, the founder's, named not decided**: the Supabase DPA and region; Art. 26 joint controllership with
    restaurants; `support@captain.food` as the de facto rights channel and its response clock; SPF/DKIM/DMARC on
    `captain.food` (email deliverability is unproven and admin-gated — farley); REVOKED-COLLEAGUE-NOTICE stays open.
12. **Business metric (observability, ADR-20260811-014129)**: 6-i lands the fold `restaurants_with_a_bound_member`
    (a distinct-identity denominator, inexpressible as a counter) on the activity that owns the roster; a
    `time_to_first_sign_in` measure would need a `MemberSignedIn` fact — a stored-shape option for 6-ii's checkpoint,
    not settled here. No roster metric may ever read as a seat count (business: per-user pricing would puncture the
    0%-commission wedge); the pass tablet is likely a shared device credential — never make per-person invitation
    required to work the pass.
13. **Honesty about reach (holub, farley).** Production is suspended by decision; nothing here reaches a person in Tours
    until that decision is re-taken. The honest claim for 6-ii is "the first slice a person can WALK end to end on one
    database" — and the walk takes its token from the admin `generate_link` path (hermetic), with email delivery proven
    in a separate hand-dispatched drill to a founder-held address. The release question is: can a Tours restaurateur,
    on a phone, go from email to a roster-bound session with no Captain touch beyond the grant — and in how many
    seconds? Peak lockout named (business): the access cookie is one hour; an operator who 401s at 19:40 and waits on a
    fifteen-minute email link is the paid-order-nobody-is-told-about failure in an auth costume. ux verified it: the
    access cookie's Max-Age is 3600 and `/auth/refresh` exists with NO caller in `crates/web`. **A silent refresh retry
    in the data layer before any `unauthenticated:` bounce, plus `?next=` return-to-screen, is a named dependency of
    flipping `RUN_MEMBER_SIGN_IN_DOOR`** (a longer TTL would reverse ADR-20260810-194548's deliberate ~1 h token). The
    parked provider session must be claimable by the LINK-OPENING tab (a mail webview is not the requesting tab); the
    refusal screen prints the verified address (whom it refuses); "step 0" — how a hand-provisioned restaurateur learns
    the URL — is a named GAP(journey).

## Alternatives considered

- **6-iii first** (un-darken the System host to give 4-iii-A a walk): refuted twice (§1). **6-ii before 6-i**: refuted
  — a door whose only product is a refusal pointing at a founder-read mailbox (business), untestable positively (beck).
- **FORK 1 Option C**: the honest runner-up (§3); rejected for the language and the broad-revoke reasons in §2, with
  vernon's isolation objection to A answered by the two-lane accept rather than by a route gate (C3 machinery, not
  step 6's).
- **A new `member_subject_reservations` table** (PROP §6.4 "copy of 2a"): rejected — one invariant, one table (§4).
- **`ADMINISTRATOR | OPERATOR`**: rejected for the stem collision (§3). **A stamped `member_id`**: rejected (§7).
- **Limits on `/public` only**: rejected (§9). **No gate on 6-i**: rejected — the first grant is the irreversible act.

## Consequences

Positive: the person a Tours restaurateur signs in as exists in the model before the door does; one reservation table;
one emitted guard unchanged; the isolation plan is honoured without a route gate; the limits arrive with the entry point
they bound. Negative: 6-i is inventory for one dispatch; the accept costs the client two commands; ADMIN remains
hand-provisioned and the restriction door's precondition (1) is not discharged by this step — said plainly.

## Follow-up actions

- Amend RIDER-RESTRICTION-PRECONDITIONS (1) to name what discharges it (an admin door), same change as this record.
- PROP §6.4 (one reservation table), §6.5 (`deletion:` wording), §2.1 (`MemberAuthority` values, `AccessBasis`), §13
  Q1/Q2 closed — rewritten in place under the LIVING doctrine, same change.
- The founder's queue gains the external items of §11 with a recommendation each.
- Cards: 6-i now; 6-ii on 6-i's merge; 6-iv; 6-iii last.

## Consulted (ADR-20260812-143619 — one line per lens)

- **architect** — consent; 6-i before 6-ii is a verification dependency, not compilation (a MEMBER stamp over an empty table fails closed); 6-iii first refuted twice; 6-ii + 6-iii do not discharge RIDER-RESTRICTION-PRECONDITIONS (1) if the door stamps MEMBER only — decide at briefing; limits on every role's schema; slices never concurrent on `auth.rs`.
- **holub** — consent, bounded: 6-i is inventory (one dispatch, WIP 1, only what 6-ii consumes — drop the two unproducible basis values from ACCEPTED input); 6-iii first is inventory for another epic; "first slice a human in Tours can use" is false while production is suspended — say "walkable end to end".
- **business-specialist** — consent; 6-ii before 6-i refuted on adoption; shared device credentials likely at the pass — never require per-person invitation, never a seat count; the one-hour cookie at 19:40 is the lockout to design for; activation is unmeasurable today.
- **vernon** — consent on the order; objects to A as written (a two-stream handler is a foreign-stream append under the open isolation plan), favours C; answered by the two-lane accept (§2); catches any handler staging a second stream and `verify_email_token`/reservation placed in `handle` rather than `prepare`.
- **young** — consent; A (targeted delete by `membership_id`; the broad revoke arm makes a rebuild an authorization event); declare four basis values, implement one; `MANAGER|OPERATOR`-class two values, no `*AuthorityChanged`; the reservation table must have a backup, `Member` need not; rebuild recipes as executable tests.
- **evans** — consent; A on language; `MemberAuthority = MANAGER | OPERATOR`, refuse `ADMINISTRATOR`; `AccessEvidence` → `AccessBasis`, declared four / accepted one; `requestMemberSignInLink` / `confirmMemberSignIn`; `ScopeMembership.member_id`'s two meanings need an owner; no provider term in payloads.
- **graphql-architect** — consent; limits on every role schema at `Schema::build`, values codegen-derived from the generated client documents; role-only stamper; ADMIN option space A/B/C — C now, B as final vision; Q7 mutation; email send-abuse wall reuses the SMS authorizer shape.
- **beck** — consent; per-role limits are unbuildable as scoped today (one `CaptainSchema`) — the card chooses the mechanism; first failing tests: the reservation refutations and the no-release-after-revoke, the rebuild PAIR (prove TRUNCATE wrong), the byte-identical oracle with the bridge consulted zero times, an over-deep document refused AND the deepest real query passing; six mutants named.
- **farley** — consent; two keys with `decisionRow`s; code dark first, writer key after; no staging and no email record exist — hermetic `generate_link` in the walk, delivery in a drill; role-only stamp so rollback is a Postgres revoke; deliverability is external/admin-gated.
- **dba** — consent; objects to a new reservation table — reuse 2a's; `Member` own group from 0, `auth_subject UNIQUE`, no new index, `read_common`; the `ScopeMembership` opposite-recipe coupling — time the from-zero replay and record it.
- **observability-agent** — consent; the sign-in contract lands with 6-ii, 6-i's fold with 6-i; OTLP-only for the anonymous door; never an address/token label; refuse any `on_roster` label; limits need headroom histograms and a max gauge; the correlation break across the mail hop.
- **legal-specialist** — consent; the closed revoke ground; §6.5's false ground corrected; the lifetime binding written; Art. 14 and Art. 30 before the first grant; the refusal screen's legal link; four external items for the founder; not clearance.
- **ux-designer** — consent with two conditions: the 19:40 lockout is real (`/auth/refresh` exists, Max-Age 3600, and NO caller in `crates/web`) — one silent refresh retry before any `unauthenticated:` bounce plus `?next=` is a named dependency of flipping the door key (a longer TTL would reverse ADR-20260810-194548); the parked session must be claimable by the link-opening tab; print the verified address on the refusal screen; 6-iii ships `requires_auth: true` + `unauthenticated:` + a System refusal screen or stays dark (an un-darkened board with 401'd reads shows a lane holding a paid order as "none"); step 0 (how a provisioned restaurateur learns the URL) is a named GAP(journey).

## Consulted, round 2 amendment (2026-09-05, ADR-20260812-143619 — one line per lens)

- **reviewer** — consent; the widened single-command shape let a caller spell `basis: MEMBER_INVITATION` with no proof or an ADMIN-shaped payload through the public door — split by privilege closes both, and derived ids make the one-membership-per-invitation invariant a compile/derivation fact, not a runtime check to remember.
- **vernon** — consent; leg 2 stays a single-aggregate command on the membership stream either way, so the split changes authorization surface, not stream topology; the derived `membershipId` is still minted by the handler, never client-supplied, matching ADR-0034.
- **young** — consent; two commands read cleanly on the write side; the derived-id namespace constant needs its own test asserting determinism (landed as a unit test on the fold), not just an integration happy path.
- **evans** — consent; `GrantRestaurantAccessByInvitation` names the ubiquitous-language act precisely (grant, by invitation) rather than overloading the ADMIN grant with a mode flag.
- **legal-specialist** — consent; no new proof instrument or legal actor is introduced — the token is the same one `AcceptRestaurantInvitation` already verifies — so Art. 14 analysis in RESTAURANT-INVITATION-PRECONDITIONS is unaffected by the split itself.
- **ux-designer** — consent; the two-lane client sequencing the invitee experiences is unchanged; the split is invisible below the `/invitation` screen.
- **beck** — consent; the STOP-then-split shape is exactly what a mutant on "ADMIN payload through the public path" would have caught had round 1 shipped the widened command — recorded as the round-2 finding it is.
- **graphql-architect** — consent; `GrantRestaurantAccessByInvitation` is PUBLIC-schema-only, `GrantRestaurantAccess` stays ADMIN-schema-only — the role-as-path ACL now separates them at the SDL level, not just at runtime.
- **observability-agent** — consent; no new span/label shape needed — leg 2's business.result values are unchanged by which command carries them.
- **dba** — consent; no new table; the derived id is computed in the command handler, not stored redundantly anywhere.
- **business-specialist** — consent; no seat-count or billing semantics touched by the split.
