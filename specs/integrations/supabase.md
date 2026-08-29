# Supabase Auth integration

Captain.Food delegates **customer identity** to **Supabase Auth** (passwordless: phone SMS OTP + optional
email magic link). SMS is delivered by **Twilio**; in dev a **mock provider** stands in. We **wrap**
Supabase behind our GraphQL (ADR-0015): the client only ever talks to our API, never to Supabase
directly. All Supabase calls go through the **`supabase-acl`** adapter (c4-l3) so the Supabase SDK never
leaks into the aggregates — which is also what lets us swap the mock in dev.

## 1. Data exposed by Supabase Auth

- **User identity**: `user.id` (UUID), `phone`, `email`, and verified flags.
- **OTP / magic link**: phone OTP challenge + code; email magic-link token / session.
- **Session**: JWT access + refresh tokens (lifetime, expiry). **Out of domain** — never recorded.

## 2. Supabase Auth → Captain.Food domain mapping

| Supabase Auth | Domain | Note |
|---|---|---|
| `user.id` | `authRef` (`ExternalReference`) on `CustomerRegistered`/`CustomerIdentified`, indexed on `View_Customer.auth_ref` | the identity bridge |
| `user.phone` (E.164) | `PhoneNumber` on `CustomerRegistered` / `CustomerPhoneChanged` | composed from `dialingCode` + `nationalNumber` at our boundary |
| `user.email` | `email` via `CustomerEmailVerified` (verified-only) | not set on `UpdateCustomerInfo` |
| OTP verify result | `VerifyPhone` → `CustomerRegistered` \| `CustomerIdentified` | synchronous |
| magic-link verify result | `ConfirmEmailVerification` → `CustomerEmailVerified` | token verified server-side |
| session (JWT) | — | out of domain (GraphQL `@auth` path-role handles authz) |

## 3. Request / report split (command vs inbound event)

Auth verification is **orchestrated by us and rejectable**, so it is a **command** (CLAUDE.md rule), not
an inbound event:
- `RequestPhoneVerification` / `RequestEmailVerification` / `RequestPhoneChange` → call Supabase to send;
  **emit nothing**.
- `VerifyPhone` / `ConfirmEmailVerification` / `ConfirmPhoneChange` → verify **synchronously**, then emit
  the verified fact (or throw `InvalidVerificationCode` / `VerificationCodeExpired` /
  `InvalidVerificationToken` / `EmailAlreadyInUse` / `PhoneAlreadyInUse`).

There is **no Supabase webhook** in V0. The synchronous path has no "lost event" risk (nothing is in
flight; the user retries on failure); the crash-after-verify case is covered by **idempotency** (keyed by
phone/authRef + code/token) + the **outbox**. A webhook → **inbound event** is future hardening for
abandoned magic links / admin-side Supabase changes (model it then like Stripe's `PaymentCaptured`).

## 4. Phone format & localization

- The API receives `dialingCode` (the **`+33`** the picker emits — NOT ISO `FR`) + `nationalNumber`; the
  backend composes E.164 (`+33` + national, leading 0 stripped) for Supabase and stores the canonical
  `PhoneNumber`. The picker's list has **no API query today** — `phoneCountries` was deleted with
  [#305 "View_* read declarations: no spec says which surface reads which view"](https://github.com/TheCaptainCompany/captain-food/issues/305)
  as unreached by any screen and unimplemented; the `PhoneCountry` reference table remains, and the
  surface that needs the list declares a query when it is built.
- The first SMS is localized by `locale` (optional), **defaulted from the dialing code** (`+33` → fr-FR;
  shared codes like `+1` pick a primary). Authenticated sends use the **stored** locale
  (`ChangeLanguage` / `View_Customer.locale`) — no per-call language param.

## 5. Checkout gating (account required)

An account is required to place an order (Uber Eats model), and the account IS the verified phone:
browsing restaurants, viewing catalogs and building a cart need no identification, but **checkout
requires a verified phone** — `placeOrder` is `roles: [CUSTOMER]` (ordering `api.yaml`), and a
CUSTOMER identity exists only through `VerifyPhone` (§2), which registers a new number
(`CustomerRegistered`) or resolves a returning one (`CustomerIdentified`). An anonymous visitor is
prompted to verify before paying; their open guest carts are bound on identification
(`CartBindingProcess`, `rules.yaml#/GuestCartsBoundOnIdentification`). This gate is why
`OrderPlaced.customerId` is REQUIRED (#144): no path can produce an order without an identified
customer.

## 6. Gaps / decisions

- **Rate limiting / abuse**: handled by Supabase + cross-cutting `RateLimited`; not a domain event.
- **Sessions / passkeys / social login**: provider concerns; post-V0. When passkeys land (WebAuthn
  via Supabase Auth — a **beta** feature there), enrolment and sign-in stay provider concerns and
  are **NOT domain events** — the same stance as OTP sends and sessions. Their business case is SMS
  cost and friction: every OTP sign-in costs one SMS (per-message pricing — the costed figures are
  recorded in PROP-20260724-233605 and `crates/application/src/sms_guard.rs`), so a returning
  customer re-authenticating with a device passkey (Face ID / Touch ID) sends **no SMS**, with
  phone OTP as the fallback (new device, unsupported browser). The single-origin topology that
  makes one passkey work across every `{slug}.captain.food` storefront (RP ID = bare
  `captain.food`, small fixed origin set) is ADR-0036.
- **Email-verified webhook**: deferred (see §3).
- **Twilio templates**: per-locale OTP message templates configured in Supabase/Twilio; selected by the
  resolved locale.
