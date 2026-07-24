# PROP-20260724-225804 — The real Supabase Auth adapter (`IdentityService`)
- **Status**: Approved (product-owner: "Go! #117", 2026-07-24)
- **Date**: 2026-07-24
- **Tracking issue**: [#117 "Supabase Auth HTTP adapter (supabase-acl): the real IdentityService implementation"](https://github.com/TheCaptainCompany/captain-food/issues/117)
- **Realized by**: (pending)

## Why

The generated `IdentityService` port has ONE implementation — `FailClosedIdentityService` (every op
rejects). So no login works: [#112](https://github.com/TheCaptainCompany/captain-food/issues/112)'s
whole session-cookie machinery is built against a stub. This adapter makes it real: OTP verify/send +
session refresh against Supabase Auth, returning the provider session #112 parks.

## Decision — a project-agnostic Supabase REST adapter, env-gated, in `crates/infrastructure`

`SupabaseIdentityService` (in `integrations/supabase_auth.rs`, beside the fail-closed stub it
replaces) calls the Supabase Auth REST API:

| IdentityService op | Supabase call |
|---|---|
| `send_phone_otp` | `POST /auth/v1/otp { phone }` |
| `verify_phone_otp` | `POST /auth/v1/verify { type: "sms", phone, token }` → `{ access_token, refresh_token, expires_in, user.id }` |
| `send_email_magic_link` | `POST /auth/v1/otp { email }` |
| `verify_email_token` | `POST /auth/v1/verify { type: "email", email, token }` → same + the proven `email` |
| `refresh_session` | `POST /auth/v1/token?grant_type=refresh_token { refresh_token }` |

- **`authRef`** = the Supabase `user.id`. The verify responses' `access_token`/`refresh_token`/
  `expires_in` flow straight into the #112 output trio the handler parks (the plumbing already
  exists).
- **Typed rejections**: a 4xx from verify maps to the catalogued `InvalidVerificationCode` /
  `InvalidVerificationToken` / `VerificationCodeExpired` (the adapter raises them, same canonical
  codes as the stub); anything else → `Repository` (technical).
- **`from_env()`**: reads `SUPABASE_URL` + `SUPABASE_PUBLISHABLE_KEY` (the anon key the OTP flows use
  as `apikey`). Absent → `None`, and the composition root falls back to `FailClosedIdentityService`
  — same env-gate pattern as `StripePaymentGateway` (STRIPE_SECRET_KEY). No config ⇒ auth stays
  anonymous-only, never a half-configured surface.
- **Project-agnostic**: the adapter reads `SUPABASE_URL`, so WHICH Supabase project auth resolves
  against is pure config, not code.

## The project-pointing finding (surfaced, not decided here)

Verified 2026-07-24: `SUPABASE_URL`/`SUPABASE_JWKS_URL` currently point at **`zcshlzhiinwmpzujuiep`
(the captain-food DATA project)**, not **`ijjdhjcglcuguifqdoii` (the `captain-identity` auth
project)** the ADR-20260722-174500 split intends. This adapter works against EITHER (it's
project-agnostic), so:
- #117 is **verified against the current config** (data project) via the email magic-link path —
  proving the CODE independently of the pointing.
- **Repointing to `captain-identity`** is an env-only change (`SUPABASE_URL` + `_JWKS_URL` + keys)
  deferred to the [captain-identity](https://github.com/TheCaptainCompany/captain-identity) migration
  ([its #1](https://github.com/TheCaptainCompany/captain-identity/issues/1)); it needs the identity
  project's auth providers configured first (dashboard) and is a clean pre-launch cutover (no real
  users' tokens to invalidate yet).

## Verification (live, without OVH)

The **email magic-link path uses Supabase's native email** — no OVH dependency — so #117 is verifiable
end-to-end now: request magic link → verify token → session parked → `POST /auth/session` mints the
cookie → an authenticated `/customer/graphql` call succeeds with it. Phone OTP verify is the same code
path; it only lacks SMS DELIVERY until [#118](https://github.com/TheCaptainCompany/captain-food/issues/118)
(OVH hook). Unit tests mock the HTTP (a fake Supabase responder); `make rust` green.

## Considered alternatives

- **A separate `crates/adapters/supabase` crate** (like the partner adapters): heavier; the identity
  ACL has no inbound webhook surface of its own (ADR-0015), so it fits `infrastructure/integrations`
  beside the stub. Revisit when auth migrates to captain-identity (then it's that service's core).
- **Admin-key (service_role) flows**: not needed — OTP verify/send are anon-key operations; using
  service_role would over-privilege the adapter.
