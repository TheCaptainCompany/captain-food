# ADR-20260810-015941 — Stripe publishable key delivery to /checkout: the decisions of the #440 chunk

**Status**: Accepted · **Date**: 2026-08-10 · **Decider**: the #440 mob (ADR-20260809-013142
protocol; checkpoints (a) and (b) each passed with three PROCEEDs), realizing
[#440 "Stripe publishable key: StripePublishableKeyTest scalar + payments configuration key, SSR-delivered to /checkout so the payment element can mount"](https://github.com/TheCaptainCompany/captain-food/issues/440),
the first unchecked item of
[#429 "Production with test data: a test customer places a real order against a test restaurant, paid with Stripe test mode"](https://github.com/TheCaptainCompany/captain-food/issues/429).
Product decision context: decision 5 of
[ADR-20260809-050000](ADR-20260809-050000-morning-brief-eight-decisions.md) (one deployment,
Stripe keys per order mode; the type-level mode witness is due before any LIVE key exists).

## Decisions

### 1. stripe.js ships in the CHECKOUT shell only, and only when a key exists

The `<script src="https://js.stripe.com/v3/">` tag is injected into the checkout document head
iff a mountable publishable key reached the shell — never on any other page, never key-less.
Stripe's own guidance prefers the tag on EVERY page (fraud signal collection); the mob chose
privacy: no customer browser talks to Stripe before the customer is on the pay step. The
Stripe-cookie consent line this implies is routed (legal lens, checkpoint (a) briefing) to
[#400 "Epic: reality-sensing infrastructure — agents closer to customers, mission metrics as contracts"](https://github.com/TheCaptainCompany/captain-food/issues/400),
whose GDPR-posture scope carries the consent surface. No CSP existed to update.

### 2. Deferred Elements posture is the FINAL VISION, not a stopgap

The element mounts with `elements({mode: 'payment', amount, currency})` (the cart's own total,
display-level), not with a `clientSecret`. Architect rationale (checkpoint (b), ratified):
**acceptance-first means no PaymentIntent can exist at the /checkout landing** — the intent is
created by the PlaceOrderProcess AFTER `PlaceOrder` is accepted. Creating an intent earlier just
to feed the element would invert the write path and orphan an intent for every abandoned
checkout. The confirm leg (a later #429 chunk) pins the real intent by `clientSecret`
(`ElementsConfig::ForIntent`).

### 3. The baked value arrives by ONE sanctioned extraction (Option A)

`STRIPE_PUBLISHABLE_KEY` is a non-secret and must ride the artifact as a literal per-profile
`deploy:` value (the `SUPABASE_PUBLISHABLE_KEY` precedent; the validator's
`config-nonsecret-from-secret` rule is an ERROR, and the product-owner directive behind it is
recorded in [ADR-20260729-020000](ADR-20260729-020000-configuration-rides-the-artifact-secrets-ride-ci.md)).
No `pk_test_` value exists anywhere in the repo, so the value is read ONCE from the
`STRIPE_PUBLISHABLE_KEY_TEST` repo secret via a branch-only, clearly-marked temporary workflow
step (its own commit, reverted as its own commit before merge — it must never reach `main`),
then baked. Farley's hygiene step: after the baked deploy is live, the `STRIPE_PUBLISHABLE_KEY`
env var is DELETED from the hosting service (env > baked precedence — a leftover dashboard value
would silently win), and the repo secret is retired for this key. Until then the missing baked
value is a NAMED deploy fact, not a silent hole.

### 4. Business activation constraint (recorded, binds future work — NOT built here)

**Intent, verbatim: the first real-restaurant activation must be mechanically impossible while
checkout serves `pk_test_`.** The `^pk_test_` scalar anchor makes go-live a spec change by
design; this constraint additionally binds the ACTIVATION side: whatever ships restaurant
activation for real (non-TEST) restaurants must carry an executable gate (validator rule, type
witness, or startup check — compiler-first, ADR-20260803-234035) refusing it while the delivered
publishable key is test-mode. Recorded here so the future activation chunk inherits it as a
requirement, not a memory.

### 5. Surface-bins disposition: config-needs-from-SSR-closure, on the #385 track

The split-topology surface bins (`crates/surface_runtime`) SSR checkout too, but their
scope-filtered generated Configs (ADR-20260807-183024 D5) do not carry the payments-scope key —
they pass `None` today (explicit, commented) and would serve the degraded checkout. Ruling: do
NOT re-home the key. The right shape is a GENERATOR slice on the
[#385 "Bin runtime wiring: business runtimes inside the 49 shells (mailbox hosting, projection filtering, subgraph slices, gateway composition, surface assets)"](https://github.com/TheCaptainCompany/captain-food/issues/385)
track: a bin's config key set derives from the CLOSURE of what its SSR surface needs, not only
from its own scopes. The coordinator files the follow-up issue; the monolith (what is deployed)
threads the key correctly.

## Consequences

- A dead payment control is unrepresentable: key absent/empty/malformed ⇒ `PublishableKey::parse`
  yields `None` ⇒ `payment_unavailable_state` + disabled pay button + no stripe.js tag; the same
  degrade is reused browser-side when stripe.js itself fails.
- The degraded render is OBSERVED: `checkout_degraded_render_total{reason=stripe_key_absent}` is
  emitted at the SSR render boundary and is the first declared metric in this repo whose emission
  is proved by a test through the production path
  (`crates/server/tests/checkout_degraded_metric.rs`); `stripe_js_load_failed`/`mount_threw` are
  reserved client-side reasons, unemitted until a browser beacon exists (no OTel in WASM).
- Go-live (issue-tracked on
  [#254 "Go-live Stripe switch: the secret key and the webhook secret must move to live mode TOGETHER"](https://github.com/TheCaptainCompany/captain-food/issues/254))
  now has THREE spec-anchored preconditions on this path: the live secret key scalar swap, the
  live publishable scalar + mode witness (decision 5 of ADR-20260809-050000), and decision 4
  above.
