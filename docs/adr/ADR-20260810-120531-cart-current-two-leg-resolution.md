# ADR-20260810-120531 — `cart.current` resolves in two legs (claim, then session); OPEN-only, LIVE reprice

- **Status**: accepted (product-owner facts relayed in-session, 2026-08-10; architect verified all three against the tree)
- **Context**: [#451 "Storefront checkout: price the cart at read"](https://github.com/TheCaptainCompany/captain-food/issues/451), PROP-20260810-231500, corrects part of ADR-20260810-112836's Phase-1 API posture

## Product-owner facts (verbatim, the recorded approval for this spec correction)

1. "Carts are built ANONYMOUSLY, keyed by session id (cookie on web / stored in the native app),
   BEFORE any customer identity exists; on identification a PROCESS MANAGER associates the cart
   to the customer_id via the shared session id." (CartBindingProcess — verified in
   `specs/processmanager.yaml` and the `CartBoundToCustomer` fold.)
2. "Once a payment INTENT is created, the cart is LOCKED to prevent modification during
   authorization (the order doesn't exist yet)" — the cart is saved at intent to create the order
   on authorization.
3. Phase 1 of #451 had declared "guests get NO by-id cart read; the guest mini-cart is a recorded
   gap" — WRONG: the session-keyed cart IS the guest path, not a gap, and gating every cart read
   to CUSTOMER+ made the storefront `/cart` screen (roles `[PUBLIC, CUSTOMER]`) bind to an
   unreachable resolver for anonymous visitors.

## Decision

`current` (specs/ordering/api.yaml) is `roles: [PUBLIC, CUSTOMER]` and resolves in TWO legs:

- **Leg 1 — claim**: a verified CUSTOMER claim resolves the claim-holder's most-recently-updated
  OPEN cart (`ReadScope::Customer`, the `myReclamations` pattern).
- **Leg 2 — session**: otherwise (anonymous, or the CartBindingProcess association not yet
  folded), a valid `X-SESSION-ID` resolves the session's most-recently-updated OPEN cart WHERE
  `customer_id IS NULL OR customer_id = <claim if present>`. The session id is an UNAUTHENTICATED
  correlator — scoping only, never identity; the NULL-or-claim filter keeps a cart already bound
  to someone else invisible to whoever replays the session id.

**OPEN-only, LIVE reprice on everything returnable**: `CartStatus` is `OPEN | CHECKED_OUT` today —
no LOCKED exists in the tree — so `current` returns OPEN carts only and every cart it returns is
priced fresh via the one `price_cart` authority. No locked-amount rendering ships in this slice.

## Named follow-ups

- [#465 "CartLocked lifecycle: lock the cart at payment intent"](https://github.com/TheCaptainCompany/captain-food/issues/465)
  — fact 2's lifecycle is NOT yet modeled (AMBER): a LOCKED cart must render the INTENDED amount
  (the lock is the freeze point, earlier than CheckoutSnapshot), never a fresh reprice. The
  CustomerIdentified merge-model gap is folded into this issue.
- [#466 "Validator rule: every screen binding's resolver must be reachable by the screen's roles"](https://github.com/TheCaptainCompany/captain-food/issues/466)
  — the gate hole that let Phase 1 ship an unreachable binding: no screen⊆resolver roles rule
  exists, so the validator was green while `/cart` was dead for PUBLIC.
