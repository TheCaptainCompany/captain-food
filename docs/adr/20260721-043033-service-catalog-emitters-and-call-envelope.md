# ADR-20260721-043033 — Service-catalog emitters (trait + http client + binding wiring) and the service-call envelope

## Status

Accepted — first slice landed with issue #26 (implements ADR-20260719-214500's emitter half,
codegen-roadmap item 4).

## Context

ADR-20260719-214500 made `specs/services.yaml` the spec-owned catalog of the abstract capabilities the
domain calls, with the deployment topology (`binding: local | http`, `expose`) decided in the spec.
The catalog was validated (§2d `svc-*` rules) and referenced by processmanager.yaml `ports:`, but
generated nothing: the port traits (`PaymentGateway`, `DeliveryPartner`, …) and their wiring stayed
hand-written and were already drifting from the catalog's vocabulary (provider verbs like
`create_payment_intent` instead of `payment.request`).

Two facts shaped the emitter design:

1. **The Stripe adapter needs business correlation ids the catalog does not declare.** The inbound
   webhook ACL (adapters/stripe `acl.rs`) can only map `payment_intent.*` facts back onto our
   aggregates through the PaymentIntent's `metadata` (`orderId`/`restaurantId`/`cartId`), which must
   be set at intent creation — but `payment.request`'s spec input is only `{ amount,
   paymentMethodId }`, and the input is the BUSINESS payload (provider-agnostic; a different payment
   provider would not need our Stripe metadata scheme).
2. **`identity` cannot be migrated at parity yet.** The hand-written `AuthProviderGateway` returns
   the PROVEN EMAIL from `verify_email_token` (the handler never trusts client input for the linked
   email), but the catalog's `identity.verify_email_token` output declares only `authRef`; the
   catalog also declares `locale` required where the handlers legitimately have none stored.

## Decision

1. **Four emitters in `tools/codegen-rs`** (all from `specs/services.yaml`, file order):
   - `crates/application/src/generated/services.rs` — per service a `<Base>Service` trait (one
     method per operation) plus serde-`camelCase` `<Base><Op>Input`/`…Output` structs typed by the
     catalog's `$ref`s. Service payload fields are REQUIRED unless `nullable: true` (no `required:`
     lists in services.yaml — the catalog declares the exact call surface).
   - `crates/infrastructure/src/generated/service_clients.rs` — one `Http<Base>Service` per service
     over the DERIVED `POST /services/<service>/<op>` surface (snake→kebab), plus the shared wire
     envelopes: request `{ input, meta }`, success `{ output }`, error `{ error }` kind-tagged so a
     remote `DomainError` rehydrates exactly (Rejected keeps its errors.yaml code + context; statuses
     422 rejected / 409 invariant / 502 repository). Clients are compiled for every service (wire
     path stays covered) but constructed only per the binding.
   - `crates/infrastructure/src/generated/service_bindings.rs` — one resolver per service honoring
     the spec binding: `local` invokes the in-process constructor the composition root supplies;
     `http` ignores it and builds the client from `SERVICE_<NAME>_URL` (missing address = startup
     error). Flipping a binding is a reviewed spec change that regenerates only this wiring.
   - `crates/server/src/generated/services_routes.rs` — the `/services/*` axum routes, emitted ONLY
     for `expose: true` services (V0: none → an empty state-generic router, merged unconditionally).
     The non-default branches (http binding, exposure) are covered by codegen unit tests over an
     inline model.
2. **The service-call ENVELOPE (`ServiceCallMeta`).** Every generated operation takes
   `(input, meta: &ServiceCallMeta)`. The envelope carries `correlation_id` (ADR-0041 propagation)
   plus `refs: BTreeMap<String, String>` — business correlation references an INBOUND adapter needs
   to map provider facts back onto our aggregates. This resolves fact 1 above the same way ADR-0041
   resolved the acting user: correlation metadata is ENVELOPE, never business payload. The checkout
   call site sets `orderId`/`restaurantId`/`cartId`; the Stripe ACL copies `meta.refs` VERBATIM into
   the intent's `metadata` (the webhook side already fails closed on missing keys).
3. **Migrated at parity and deleted:** `PaymentGateway` → `PaymentService` (`request`/`refund`;
   Stripe outbound adapter, fail-closed stand-ins, refund PM, placeOrder, all fakes) and
   `DeliveryPartner` → `DeliveryService` (`offer_job`; dispatch PM, saga runner, noop stand-in).
   The composition root now resolves both through the generated bindings.
4. **`identity` migration deferred** (fact 2): the `IdentityService` trait/structs are generated but
   `AuthProviderGateway` stays hand-written until the catalog gap is fixed by a spec change —
   proposed for the product owner: `verify_email_token.output` gains the proven `email`, and the
   `locale` inputs become `nullable: true`. Spec changes are plan-mode/owner decisions, never made
   by an execution session (CLAUDE.md non-negotiables), so this ADR records the gap instead.

## Alternatives considered

- **Widen `payment.request`'s input with the correlation ids** (the ADR-20260719-214500 example did) —
  rejected here: it would leak Stripe's webhook-mapping scheme into the provider-agnostic catalog, and
  execution sessions must not edit `specs/**`.
- **A typed per-service context struct** — more compile-time safety than the string map, but requires
  spec vocabulary for envelope fields (a DSL extension), revisitable once a second consumer of `refs`
  exists.
- **Emit http clients only for http-bound services** — fewer artifacts, but leaves the client emitter
  path dead until the first flip; compiling clients for all services keeps it continuously proven.

## Consequences

### Positive
- A new integration port is now a services.yaml entry + `make generate` (cheapens #20/#28); the
  local↔http topology flip is a spec-only change, as ADR-20260719-214500 promised.
- Port vocabulary matches the catalog everywhere (`payment.request`, not `create_payment_intent`);
  observability's qualified `service.operation` naming maps 1:1 onto trait methods.
- The wire protocol round-trips `DomainError` losslessly, so behaviour is binding-independent.

### Negative
- `meta.refs` is stringly-typed: a checkout call site that forgets a ref key surfaces at the webhook
  (fail-closed unmappable event), not at compile time — the price of keeping provider correlation out
  of the spec input; documented on the trait and the Stripe encoder.
- `identity` remains split (generated trait unused, hand-written port live) until the spec gap is
  closed — tracked in STATUS and issue #26.

## Sequencing

Remaining for later slices: migrate `identity` once the spec change lands; bind C4-L3 components and
observability span contracts to the catalog (ADR-20260719-214500 point 5); catalog entries + ports for
`catalog_sync` (HubRise, #20) and the Avelo37 ACL (#28) as those land.
