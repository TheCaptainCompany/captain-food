# ADR-20260809-212810 — VerifyPhone claim-stamp posture: stamp → rotate → park, cookie is the transport

## Status

Accepted (realized by [PR #438 "feat(#437): verifyPhone stamps captain_customer_id before token issue; customer bearer transport"](https://github.com/TheCaptainCompany/captain-food/pull/438),
for [#437 "verifyPhone stamps captain_customer_id before token issue; customer bearer token rides the session (#429 blocking precondition)"](https://github.com/TheCaptainCompany/captain-food/issues/437),
epic [#429 "Production with test data: a test customer places a real order against a test restaurant, paid with Stripe test mode"](https://github.com/TheCaptainCompany/captain-food/issues/429)).

## Context

Since [PR #434 "feat(#433): ReadScope resolves from JWT claims for ALL roles"](https://github.com/TheCaptainCompany/captain-food/pull/434),
`ReadScope` is a pure function of verified `app_metadata` claims and no server path wrote them:
every CUSTOMER-guarded read refused the very customer who just paid. The claim must be inside the
token the client ends up holding — which fixes an ordering, a failure posture, and a transport.

## Decision

### 1. Ordering — stamp, THEN rotate, THEN park

`verify_phone`: verify the OTP → resolve the Customer (`by_phone`; an existing customer wins, else
the command's id) → **stamp** `captain_customer_id` + `captain_role` via the Supabase admin API →
**rotate** the session (`refresh_session`) so the new access JWT carries the claim → **park** ONLY
the rotated token (keyed by the actor's `cause_id`) for `/auth/session` pickup. Park-then-stamp is
the bug this ADR exists to make unspellable in review: a parked pre-stamp token yields a signed-in
customer whose tracking screen refuses them.

### 2. Failure posture (verbatim, from the mob review)

Verification stands — the OTP is a consumed external fact; an unstamped token is NEVER parked;
recovery = fresh OTP, idempotent re-stamp; `claim_conflict` is a defect, never retried. On any
stamp failure the handler skips rotation AND parking, logs, and returns Ok — the customer is
identified-but-cookieless, exactly the posture of a failed `/auth/session` pickup. Failures emit
`claims.stamp` (OTel ERROR) + `customer_claim_stamp_failed_total{reason}` at the ACL boundary
(reasons bounded: `not_configured | claim_conflict | provider_error`).

### 3. Idempotency decision (pure `stamp_decision`, decision-before-any-write)

| Provider `app_metadata` state | Decision |
|---|---|
| different `captain_customer_id` | `ClaimConflict` — refuse, never overwrite |
| same id AND `captain_role` == `"CUSTOMER"` exactly | No-op (redelivery-idempotent) |
| same id, role missing **or any other value** | PUT both keys (repair — role hardcoded CUSTOMER, id already equal, safe) |
| no id | PUT both keys |

The role-exactness row is the checkpoint-(b) fix: an any-role no-op would rotate and park a
wrong-role token. Both keys always travel in one PUT (GoTrue merges `app_metadata` shallowly);
`stamp_put_body` hardcodes `CUSTOMER`, so a wrong-role stamp is unspellable.

### 4. Cookie is the transport — bearer plumbing rejected as a dead control

The httpOnly `captain_auth` cookie of
[PROP-20260724-150500](../proposals/PROP-20260724-150500-client-auth-token-wiring.md)
([#112 "Client auth-token wiring: JWT storage, Authorization on HTTP+WS, sign_out, auth cookie for SSR 302"](https://github.com/TheCaptainCompany/captain-food/issues/112))
is the shipped and confirmed design: JS can never read the token, the browser attaches it to
same-origin fetch and the WS upgrade on its own, and the server reads it as the `Authorization`
fallback. Client-side `Bearer` plumbing (GraphQL POST header + WS `connection_init` token) was
briefed for this change and REJECTED with code evidence: no browser code path can ever hold the
token, so the control would render and do nothing. `WsClient::connect(Some(_))` remains for
header-incapable-but-token-holding clients (e.g. desktop), never the web storefront.

**Recorded consequence (inherent to #112, not decided here): the cookie is host-only, so sign-in
is per host — a storefront (`{slug}.captain.food`) sign-in does not sign the customer into the
marketplace host, and vice versa.**

### 5. Reversal note on PROP-20260724-225804

[PROP-20260724-225804](../proposals/PROP-20260724-225804-supabase-identity-adapter.md) recorded
"Admin-key (service_role) flows: not needed — using service_role would over-privilege the
adapter." That concern is **superseded** for exactly one operation: `SUPABASE_SECRET_KEY` is now
spec-declared (`specs/` configuration + `identity.stamp_customer_claim`) for the single admin
`app_metadata` write above — presence-gated (absence never fails boot, only fails the stamp
closed), 5s-bounded per call against the 30s mailbox lease — as is the rotation POST, since it too
runs inside the VerifyPhone delivery (worst case GET + PUT + refresh = 15s < 30s). The user-facing
OTP-verify call itself remains unbounded (reqwest client default) — a PRE-EXISTING pattern
predating this change and outside this PR's scope, noted here rather than silently inherited. The
anon-key posture of every other identity operation is unchanged.

## Alternatives considered

- Park-then-stamp (the pre-#437 accidental shape) — rejected: the parked token predates the claim.
- Client bearer transport — rejected as a dead control (§4).
- Retry `claim_conflict` — rejected: a conflicting stamped id is a defect to investigate, not a
  transient; retrying could overwrite an identity.

## Consequences

- The first paid session holds a claim-bearing cookie; `ReadScope::Customer` resolves without any
  DB bridge, at peak included.
- Compliance: the Art. 30 processor note for the Supabase-held `captain_customer_id` claim
  (lawful basis, erasure reach, counsel question on auth logs/backups) lives with the #194 erasure
  obligations in
  [PROP-20260726-170000](../proposals/PROP-20260726-170000-event-log-integrity-evolution-and-erasure.md)
  — updated in the same change as this ADR.
- Known gap (pre-existing, found by the observability lens on this work): the
  customer-identification contract's `otp.verify` span is implemented nowhere — tracked separately.
