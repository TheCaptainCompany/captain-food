# ADR-20260829-092233 — `specs/PRODUCT_SPEC_WEB_CLIENT.md` is retired: the DSL is the one authority

**Status**: Accepted · **Date**: 2026-08-29 ·
**Decider**: the **FOUNDER / Tech CEO**, verbatim below ·
**Register row**: [docs/decisions/PRODUCT-SPEC-WEB-CLIENT-RETIRED.yaml](../decisions/PRODUCT-SPEC-WEB-CLIENT-RETIRED.yaml) ·
**Session**: https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

## Status

Accepted — realized in the same change (a docs/specs-lane commit to `main`).

## The directive, verbatim

> *"retire the product spec file"*

Given 2026-08-29, in the same founder message as the option-1 choice on
[#755 "Local dev: accept `{slug}.localhost` as tenant space so storefronts are viewable without /etc/hosts"](https://github.com/TheCaptainCompany/captain-food/issues/755),
after the 13-lens consult below; the team's recommendation (a) — retire, with a migration of the
unique content — was accepted.

## Context — the grounds

`specs/PRODUCT_SPEC_WEB_CLIENT.md` was the pre-DSL prose product spec for the customer web client.
It had become an **unverified prose spec**: nothing executable consumes it (no emitter, no
validator rule, no test reads it), while the YAML DSL it predates grew executable authorities for
almost everything it says. Where the two disagreed, the file was wrong:

- **§5 asserted Next.js / Tailwind / TypeScript** against ADR-0034/0035 (full-stack Rust, Leptos →
  WASM SDUI).
- **§3.4 / §3.5 diverged from the commands DSL and the capture posture**: its `placeOrder` shape
  (`customerInfo`, `deliveryMode`, no `orderId`/`paymentMethodId`) does not match
  `commands.yaml#/PlaceOrder` (`customerContact`, `serviceType`, client-generated `orderId`,
  required `paymentMethodId`), and its "on successful payment the backend writes `PaymentCaptured`"
  contradicts authorize-then-capture — funds are HELD at confirmation and captured at handover
  (ADR-20260808-195315 §1.2).

A prose page that nothing checks and that re-teaches retired decisions is a drift generator, not a
spec. Retirement follows the `ARCHITECTURE_OVERVIEW.md` precedent (ADR-0036 extracted its one
uncovered fact and the file was removed).

## The decision

1. **The file is deleted.** The YAML DSL (+ its ADRs and integration docs) is the one authority for
   what the customer web client promises.
2. **Unique content migrated first** (the migration map):
   - **§3.0 identity semantics** → `specs/integrations/supabase.md`: a new **§5 "Checkout gating
     (account required)"** (browse/cart anonymous; checkout requires a verified phone — structural
     in `placeOrder`'s `roles: [CUSTOMER]`; why `OrderPlaced.customerId` is REQUIRED, #144), and
     the **§6 passkey stance** sharpened (passkeys stay provider concerns, NOT domain events — same
     as OTP/sessions; the business case is SMS-per-OTP cost, costed in PROP-20260724-233605 /
     `sms_guard.rs`, mitigated by passkey re-auth with OTP fallback; single-origin RP-ID topology
     stays in ADR-0036). `specs/ordering/events.yaml` (`OrderPlaced.customerId`) now cites
     supabase.md §5 instead of the retired file. Already covered elsewhere, so NOT duplicated:
     register-or-identify on first verification (`specs/customer/commands.yaml` `VerifyPhone`,
     supabase.md §2), RP ID = bare `captain.food` / ≤5 origins (ADR-0036).
   - **§4 NFRs** → the numbers were already declared as the screens' executable `performance:` /
     `accessibility:` contracts (`target_lighthouse_mobile: 90`, `SSR_first`, TTFB 500 / FCP 1500 /
     LCP 2500 ms, WCAG AA — `captain_frontoffice.yaml`, `restaurant_frontoffice.yaml`); the one
     unique figure, the founding **"main flows in ~2 s on 4G, mobile-first"** budget, moved into
     `docs/frontend/renderer-architecture.md` §3, which cited "§4" and now names the numbers and
     the contracts. The file's "basic a11y" line was deliberately NOT migrated (legal lens: it
     would understate EAA/RGAA if treated as authoritative; the WCAG-AA contract stands).
   - **§3.7 restaurant-onboarding entry point** → a **declared screen gap** on `partner_landing`
     in `specs/screens/captain_frontoffice.yaml` (chosen over a GitHub issue because the entry
     point itself already EXISTS in that screen — its apply CTA targets
     `https://restos.captain.food/onboarding` — and screens are the spec surface where a promise
     with no backing surface is declared as a gap, never left looking live): the destination form
     has no `restaurant_backoffice.yaml` screen and no owner self-signup story activity, though
     the API admits owner self-signup (`registerRestaurantAccount`).
   - **localStorage-cart reversal (ll. 99-100)** — nothing to migrate: the file already delegated
     to `database.md`, which holds the Cart authority (server-side aggregate, ephemeral
     `Cart-<id>` streams, guest `session_id`, `CartBoundToCustomer`).
   - **vernon's two boundary facts** — nothing to migrate: the server-side Cart aggregate with
     guest identity and customerId binding is `CartBindingProcess` +
     `rules.yaml#/GuestCartsBoundOnIdentification` + `CartBoundToCustomer`; "Apple Pay adds no
     event" is the `PlaceOrder.paymentMethodId` description in `specs/ordering/commands.yaml`
     ("the wallet behind it … does not change the domain").
3. **Inbound references repointed** (live surfaces only): `CLAUDE.md` §Specifications index entry
   removed (covered by the register row — removing an indexed entry is a decision, per the
   residency rule); `crates/adapters/stripe/src/outbound.rs` doc comment →
   `specs/payments/api.yaml` `paymentStatus` (ADR-20260720-015500);
   `specs/ordering/events.yaml` → supabase.md §5; `docs/frontend/renderer-architecture.md` §3 →
   the screens' `performance:` contracts. **Historical records keep their citations verbatim**
   (SPEC-LOG rows, `PATH-ADDRESSED-STOREFRONT.yaml`, ADR-20260829-082615, proposals, journals).

## Alternatives considered

- **(b) Keep the file, subordinate it** (a banner declaring the DSL wins on conflict) — evans's
  preference; rejected: a subordinate prose spec still gets read and quoted, and nothing enforces
  the banner.
- **(c) Keep and ratchet it** (a gate diffing its claims against the DSL) — holub's preference;
  rejected: the gate would be a hand-maintained parser over free prose, permanent cost for a file
  with three unique sections.

## Consequences

Positive: one authority; three drift sites gone (§5 stack, §3.4 command shape, §3.5 capture); the
unique content now lives where its consumers already look, and the onboarding promise is an
explicit, renderer-surfaced gap instead of prose. Negative: the file was an approachable narrative
overview for newcomers — `specs/generated/documentation.generated.md` is the replacement narrative
surface. Follow-up: none; the gap note carries the onboarding surface forward.

## Consulted

13-lens consult, in-session 2026-08-29 (one line per lens):

- **architect** — retire; the residual is prose-only, every executable claim already has a DSL home.
- **evans** — subordinate preferred; retirement acceptable provided the outcome is ONE authority
  per fact, which the migration map delivers.
- **holub** — ratchet preferred; no objection to retirement.
- **ux** — provided the section map; named the three unique sections (§3.0, §4, §3.7) to migrate.
- **beck** — nothing executable consumes the file, so no new gate is needed by its removal.
- **farley** — no pipeline consumer; four prose references to repoint, listed in the dispatch.
- **graphql** — two API drifts confirmed (§3.4 placeOrder shape, §3.2 restaurant query framing).
- **business** — two economics notes unique (SMS-per-OTP cost rationale, passkey mitigation).
- **dba** — nothing unique; one history note (the localStorage-cart reversal already delegates to
  database.md).
- **young** — the file is derived narrative, not source; deleting narrative loses no fold.
- **vernon** — two boundary facts to keep IF not already in YAML (they are — see the map).
- **legal** — nothing binding in the file; its "basic a11y" line would understate EAA/RGAA if
  treated as authoritative, so it must not be migrated as an authority.
- **observability** — nothing in my lens.
