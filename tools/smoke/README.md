# Production smoke test (Stripe TEST mode)

`prod-smoke.sh` exercises the live deployment end to end — edge, public GraphQL, fixtures, and the
full money path — using **Stripe TEST-mode money only**. It is safe to re-run at any time: it owns one
dedicated tenant (`smoke-test.captain.food`), its fixtures are idempotent (fixed UUIDs, existence-
checked), and every run uses fresh cart/order ids.

Run it with `make smoke-prod`, or on GitHub via the `prod-smoke` workflow — **manual dispatch only**:
the daily cron is deliberately off while production is suspended, and the workflow's own header states
what re-enables it (needs the `STRIPE_SECRET_KEY_TEST` and `SUPABASE_SECRET_KEY` repo secrets).

## Layers

| Layer | What it proves | How |
|-------|----------------|-----|
| L1 edge | the service is up and ready | `GET /ping` = `pong`, `GET /health` = 200 |
| L2 public API | wildcard tenant routing + public GraphQL | introspection on `https://smoke-test.<domain>/public/graphql` |
| L3 fixture | the write path + projections (ADMIN role) | ensures a TEST-mode restaurant `smoke-test` with one AVAILABLE offer, creating it via `registerRestaurant` → `activateRestaurant` → `createCatalog` → `addProduct` when missing |
| L4 money path | checkout → Stripe → webhook → saga → read model, **entirely on the storefront host** | `addCartLine` (PUBLIC) → the cart **pair** (`current` non-null and exactly priced on `smoke-test.<domain>`, `null` on `live.<domain>`) → `placeOrder` (CUSTOMER, `mode: TEST`) → server-side `confirm` of the PaymentIntent with `pm_card_visa` → polls `order(id)` until `paymentStatus: AUTHORIZED` (capture happens at fulfilment, ADR-20260808-195315 §1.2) |

**Which host** (#622): L1 and L3's restaurants-by-slug/catalog reads are **marketplace** browse and
stay on `live.<domain>`; L2, L3b and **all** of L4 are **storefront** reads on `{slug}.<domain>`,
because `current` resolves its tenant from the `Host` (#469) and correctly serves `null` off-tenant —
a refusal byte-identical to "the cart never projected". L4 therefore asserts a **pair** that differs
in exactly one input, the host: tenant non-null + marketplace null is the only green; both null means
the cart is genuinely broken; marketplace non-null is a cross-tenant leak. Call sites cannot name a
base — the `marketplace*` / `storefront*` / `admin*` helpers each hardcode theirs.

Each layer logs `PASS`/`FAIL`; the script exits non-zero at the first failing layer with the last
observed state.

## Auth

Non-public GraphQL paths require a Supabase JWT whose `app_metadata.captain_food.role` matches the path
(ADR-0047). The script mints role tokens through the deployment's **own** auth provider. The two Supabase
values have different homes since ADR-20260729-020000 ("non-secret config rides the artifact"):
`SUPABASE_URL` is a non-secret baked per-profile into the image and **removed from the Render env**, so the
script reads it from the baked source of truth `specs/configuration.yaml` (profile `SMOKE_APP_PROFILE`,
default `production`); `SUPABASE_SECRET_KEY` is a real secret and is read from the Render service env (via
`RENDER_API_KEY`). It then ensures the dedicated smoke users (`smoke-admin@…` ADMIN, `smoke-customer@…`
CUSTOMER) exist and signs them in via an admin-generated magic link (nothing is emailed). No secret is
ever printed or persisted. Set `SUPABASE_URL`/`SUPABASE_SECRET_KEY` directly to override either lookup.

## Environment

| Var | Required | Meaning |
|-----|----------|---------|
| `STRIPE_SECRET_KEY` | yes (L4) | must be `sk_test_…` — the script refuses to confirm payments otherwise |
| `RENDER_API_KEY` | yes (L3/L4) | to read the deployed `SUPABASE_SECRET_KEY`; or set `SUPABASE_URL` + `SUPABASE_SECRET_KEY` directly |
| `SMOKE_BASE_DOMAIN` | no | default `captain.food`. May carry a **port** (`captain.food:8080`); may **not** carry another domain — `surface_runtime::hosts::APEX` is a compile-time constant, so under any other apex every host classifies as `Default`, nothing names a tenant, and every tenant-scoped read serves `null`. L4 asserts this up front |
| `SMOKE_TENANT_SLUG` | no | default `smoke-test` |
| `RENDER_SERVICE_NAME` | no | default `captain-food` |
| `SMOKE_APP_PROFILE` | no | baked config profile the deployment runs (default `production`) |
| `SMOKE_ORDER_TIMEOUT` | no | seconds to wait for the captured order (default 90) |

## Stripe webhook prerequisite

L4 relies on the inbound webhook (`payment_intent.succeeded` → `PaymentCaptured` → the place-order
saga). The production endpoint `https://api.captain.food/adapters/stripe/webhooks` must be registered
in Stripe (events: `payment_intent.succeeded`, `payment_intent.payment_failed`, `charge.refunded`)
and its signing secret set as `STRIPE_WEBHOOK_SECRET` on the Render service.
