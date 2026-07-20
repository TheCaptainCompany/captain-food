# Production smoke test (Stripe TEST mode)

`prod-smoke.sh` exercises the live deployment end to end — edge, public GraphQL, fixtures, and the
full money path — using **Stripe TEST-mode money only**. It is safe to re-run at any time: it owns one
dedicated tenant (`smoke-test.captain.food`), its fixtures are idempotent (fixed UUIDs, existence-
checked), and every run uses fresh cart/order ids.

Run it with `make smoke-prod`, or on GitHub via the `prod-smoke` workflow (manual dispatch + daily
cron; needs the `STRIPE_SECRET_KEY` and `RENDER_API_KEY` repo secrets).

## Layers

| Layer | What it proves | How |
|-------|----------------|-----|
| L1 edge | the service is up and ready | `GET /ping` = `pong`, `GET /health` = 200 |
| L2 public API | wildcard tenant routing + public GraphQL | introspection on `https://smoke-test.<domain>/public/graphql` |
| L3 fixture | the write path + projections (ADMIN role) | ensures a TEST-mode restaurant `smoke-test` with one AVAILABLE offer, creating it via `registerRestaurant` → `activateRestaurant` → `createCatalog` → `addProduct` when missing |
| L4 money path | checkout → Stripe → webhook → saga → read model | `addCartLine` (PUBLIC) → `placeOrder` (CUSTOMER, `mode: TEST`) → server-side `confirm` of the PaymentIntent with `pm_card_visa` → polls `order(id)` until `paymentStatus: CAPTURED` |

Each layer logs `PASS`/`FAIL`; the script exits non-zero at the first failing layer with the last
observed state.

## Auth

Non-public GraphQL paths require a Supabase JWT whose `app_metadata.captain_role` matches the path
(ADR-0047). The script mints role tokens through the deployment's **own** auth provider: it reads
`SUPABASE_URL`/`SUPABASE_SECRET_KEY` from the Render service env (via `RENDER_API_KEY`), ensures the
dedicated smoke users (`smoke-admin@…` ADMIN, `smoke-customer@…` CUSTOMER) exist, and signs them in
via an admin-generated magic link (nothing is emailed). No secret is ever printed or persisted.

## Environment

| Var | Required | Meaning |
|-----|----------|---------|
| `STRIPE_SECRET_KEY` | yes (L4) | must be `sk_test_…` — the script refuses to confirm payments otherwise |
| `RENDER_API_KEY` | yes (L3/L4) | to read the deployed Supabase creds; or set `SUPABASE_URL` + `SUPABASE_SECRET_KEY` directly |
| `SMOKE_BASE_DOMAIN` | no | default `captain.food` |
| `SMOKE_TENANT_SLUG` | no | default `smoke-test` |
| `RENDER_SERVICE_NAME` | no | default `captain-food` |
| `SMOKE_ORDER_TIMEOUT` | no | seconds to wait for the captured order (default 90) |

## Stripe webhook prerequisite

L4 relies on the inbound webhook (`payment_intent.succeeded` → `PaymentCaptured` → the place-order
saga). The production endpoint `https://api.captain.food/adapters/stripe/webhooks` must be registered
in Stripe (events: `payment_intent.succeeded`, `payment_intent.payment_failed`, `charge.refunded`)
and its signing secret set as `STRIPE_WEBHOOK_SECRET` on the Render service.
