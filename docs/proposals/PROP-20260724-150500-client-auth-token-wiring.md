# PROP-20260724-150500 — Client auth-token wiring: the httpOnly session cookie
- **Status**: Approved (product-owner standing directive, 2026-07-24: "Go on these subjects — you
  don't need me")
- **Date**: 2026-07-24
- **Tracking issue**: [#112 "Client auth-token wiring: JWT storage, Authorization on HTTP+WS, sign_out, auth cookie for SSR 302"](https://github.com/TheCaptainCompany/captain-food/issues/112)
- **Realized by**: (pending)

## Why

Identity stops at the server: `VerifyPhone` SUCCEEDs, the Customer exists — and the client stays
anonymous. The CUSTOMER role path is unreachable, the staff surfaces (restos./riders.) 401 by
design with no way to log in, the WS carries no token, `sign_out` is disabled, and
[#92](https://github.com/TheCaptainCompany/captain-food/issues/92)'s server-side `requires_auth`
302 waits on an auth cookie. The blocker is a genuine design gap: Supabase is WRAPPED behind our
GraphQL (ADR-0015/0047) so verification happens server-side, but mutations return ONLY the
acceptance envelope (ADR-20260720-015500) — the session token has no channel to the client.

## Decision — the BFF-minted httpOnly session cookie

**The token never touches client-side storage.** The flow:

1. **The identity service returns the provider session** (spec change, the
   [#50](https://github.com/TheCaptainCompany/captain-food/issues/50) precedent of adding `email`
   to `verify_email_token.output`): `identity.verify_phone_otp.output` and
   `verify_email_token.output` gain `accessToken` + `refreshToken` + `expiresIn` — Supabase's
   verify APIs already return them; today the adapter drops them on the floor.
2. **The VerifyPhone/VerifyEmail handler parks the session** in a new transport table
   `auth_sessions` (the `hubrise_connections` category: never event-sourced, never in api.yaml):
   `message_id` (pk, the acceptance handle), ciphertext session (AES-GCM under an env key),
   `session_id` (the anonymous X-SESSION-ID that journaled the command), short TTL (minutes),
   single-read. The tokens are NOT in the event log, NOT in the journal payload, NOT in GraphQL.
3. **`POST /auth/session { messageId }`** (a transport endpoint next to the adapters, not GraphQL):
   requires the caller's `X-SESSION-ID` to MATCH the row's — the same ownership rule as
   `operationStatus` — then answers `Set-Cookie: captain_auth=<access JWT>; HttpOnly; Secure;
   SameSite=Lax` (+ a refresh cookie scoped to `/auth`), deleting the row (single-read). Guessing a
   messageId yields nothing without the minting session.
4. **Every server-side consumer reads the cookie**: the existing `AuthContext` JWKS verification
   gains a cookie fallback beside the `Authorization` header — one verification path, two carriers.
   That single change lights up: authenticated GraphQL over HTTP (cookies ride same-origin fetch),
   the WS upgrade (browsers DO send cookies on the handshake — no `connection_init` token needed on
   the browser path), AND [#92](https://github.com/TheCaptainCompany/captain-food/issues/92)'s SSR
   302 + authenticated SSR data (the fallback handler sees the cookie).
5. **Refresh**: `POST /auth/refresh` rotates via the refresh cookie. **`sign_out`**:
   `POST /auth/logout` clears both cookies (the `auth`-kind action finally wires).
6. **Staff login**: the same machinery over the EXISTING email magic-link ops
   (`send_email_magic_link` → `verify_email_token`) — role authorization stays where ADR-0047 put
   it: the JWT's `captain_role` claim (Supabase app_metadata, set at onboarding), verified against
   the role path. No new domain surface.

## Why this over the alternatives

- **A GraphQL read returning tokens** (post-SUCCEEDED query): puts bearer tokens in GraphQL
  responses and localStorage — XSS-readable, cacheable, loggable; and adds an api.yaml surface for
  something that is transport, not domain.
- **Client-direct Supabase for the auth exchange**: the OTP is consumed by whoever verifies it —
  client-direct verification would bypass `VerifyPhone` (CustomerRegistered/Identified, cart
  binding) or force a double-verification dance. Wrapping stays (ADR-0015).
- httpOnly cookies cost CSRF care: mutations already carry the journaled `messageId` idempotency +
  `SameSite=Lax` + same-origin `fetch`; the `/auth/*` endpoints take no side effects beyond their
  own session.

## Scope of change

| Layer | Change |
|---|---|
| `specs/services.yaml` | verify outputs gain `accessToken`/`refreshToken`/`expiresIn` (nullable — a provider without sessions degrades) |
| `specs/database/tables/integration_connections.yaml` | + `auth_sessions` (message_id pk, ciphertext, session_id, expires_at) + migration + retention in `sweep_retention()` |
| `crates/infrastructure` (supabase adapter) | capture the session from the verify responses |
| `crates/application` | VerifyPhone/verify-email handlers park the session row (port + Pg store) |
| `crates/server` | `/auth/session`, `/auth/refresh`, `/auth/logout`; `AuthContext` cookie fallback; hosts/SSR pass the request principal (#92's 302 + authenticated SSR) |
| `crates/web` | after `verify_otp` SUCCEEDs, call `/auth/session` (same-origin, cookie lands), flip an `authenticated` client state (the `conditional`/`if_authenticated` branches + `requires_auth` guard read it); `sign_out` → `/auth/logout` |

## Verification

- Unit: session parking (encrypt/decrypt round-trip, single-read, TTL, session-ownership refusal);
  AuthContext cookie-vs-header parity (same JWT both carriers → same Principal).
- Server test: full flow with a stub identity service — verify → journal SUCCEEDED → `/auth/session`
  with right/wrong X-SESSION-ID → cookie set/refused; GraphQL CUSTOMER path 200 with cookie, 401
  without; logout clears.
- `make rust` green; `sweep_retention` covers `auth_sessions`.
