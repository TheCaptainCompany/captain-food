# ADR-20260719-214500 — Service catalog with spec-declared binding (local | http)

## Status

Accepted — catalog (`specs/services.yaml`) + validator wiring landed 2026-07-19; the trait/client/binding/route EMITTERS landed 2026-07-21 with issue #26 (ADR-20260721-043033; `identity` migration + C4/observability catalog binding remain).

## Context

The process managers and command handlers call outbound capabilities through application-layer port
traits (`PaymentGateway`, `DeliveryPartner`, `AuthProviderGateway`, …) implemented by adapter crates
(`crates/adapters/stripe`, `hubrise`) that today are hard-wired in-process into the Axum server. The
ports exist, but (a) they are declared ad hoc (inline `ports:` per process manager, hand-written
traits), and (b) the local-vs-remote topology decision is buried in code instead of being a reviewed
spec declaration. Calling our own
adapter over HTTP inside one deployable would be a useless network hop; hard-wiring it in-process
forever blocks splitting the deployable later.

## Decision

Introduce **services** — a spec-level catalog of the abstract APIs the domain calls — with the
implementation **binding and exposure declared in the spec** (environment config carries only
addresses):

1. **`specs/services.yaml`** (new DSL source, deliberately SEPARATE from api.yaml): each service
   declares typed operations in the house style — `payment: { operations: { request: {input/output/
   errors as $refs}, refund: {…} } }`, `delivery: { offer_job, cancel_job }`, `identity:
   { verify_phone_otp, verify_email_token }`, `catalog_sync: { import_catalog, sync_inventory }`,
   `listing_enrichment: { … }`. The HTTP binding's surface is **`/services/<service>/<operation>`**
   (e.g. `/services/payment/request`, `/services/payment/refund`); provider adapters keep their own
   surface in the provider's vocabulary (`/adapters/stripe/payment-intents`,
   `/adapters/stripe/refunds`, `/adapters/avelo37/…`).
   NOTE the namespace: `/external/*` is NOT used — `EXTERNAL` is an api.yaml ROLE, so
   `/external/graphql` is already the external partners' GraphQL endpoint (role-as-path); the
   service transport must not overload it.
2. **processmanager.yaml `ports:` become `$ref`s into services.yaml** — the validator proves every
   `call` step against the catalog (operation exists, error set declared), same as every other ref.
3. **Codegen emits per service**: the Rust trait (application), and — per the spec's declared
   `binding`/`expose` — the composition-root wiring (local → the adapter crate in-process; http →
   the generated client reading `SERVICE_<NAME>_URL`), the adapter-side Axum routes, and the
   `/services/*` routes when exposed. Splitting the monolith becomes a reviewed spec change plus an
   address, not a rewrite.
4. **The workers follow the same model**: the projector and the saga runner already toggle
   in-process vs (future) dedicated deployable via `RUN_PROJECTOR`/`RUN_PROCESS_MANAGERS` — they are
   services under this decision, not exceptions.
5. **C4-L3 and observability bind to the catalog**: service components/relations and the span
   contracts around service calls derive from services.yaml instead of being maintained by hand.
6. **Relation to the GraphQL API (api.yaml)**: no overlap by design. api.yaml is the PRODUCT API —
   GraphQL, role-filtered (`/{role}/graphql`), consumed by UIs and external partners. services.yaml
   is the INTERNAL capability catalog — consumed by the domain through generated traits, with the
   HTTP binding as a spec-declared option. GraphQL never fronts a service call, and services never
   appear in the GraphQL schema.

## Naming & exposure convention (agreed 2026-07-19)

- **Operations are short domain verbs, snake_case**, grouped under their service — the service
  carries the noun, the operation is the bare intention (`payment.request`, `payment.refund`,
  `delivery.offer_job`, `identity.verify_phone_otp`). An operation name must be unambiguous WITHIN
  its service; observability always emits the qualified `service.operation` form, never the bare op.
- **Provider vocabulary never appears at the service level** (no `payment.create_payment_intent`) —
  the ACL translates names as well as payloads.
- **HTTP binding paths are DERIVED, never hand-picked**: `POST /services/<service>/<op>` with
  snake_case → kebab-case (`payment.request` → `POST /services/payment/request`,
  `delivery.offer_job` → `POST /services/delivery/offer-job`). All service operations are `POST` —
  they are commands with typed bodies; queries stay on GraphQL.
- **Adapter routes speak the provider's vocabulary** (`/adapters/stripe/payment-intents`, like
  `/adapters/stripe/webhooks` already does), and the service-op → adapter-route mapping is DECLARED
  in the spec per implementation — the name-level ACL is spec, not code.
- **Binding and exposure are DECIDED IN THE SPEC (agreed: everything in the spec for now).**
  Each service declares its chosen `binding: local | http` and `expose: true | false` — changing
  the deployment topology is a SPEC change (reviewed, validated, regenerated), not an environment
  knob. `binding: local` + `expose: false` (the V0 default for every service) means in-process
  calls only and the `/services/*` routes are not emitted at all. When a service flips to
  `binding: http`, environment configuration supplies ONLY the address
  (`SERVICE_<NAME>_URL=<base-url>` — an address book, never a decision); a missing address for an
  http-bound service is a startup error. Per-environment overriding of the binding itself may be
  revisited later, as a new ADR.

### Example — the emitter's input contract (`specs/services.yaml`)

Operations are grouped by service, and the mapping onto the provider adapter's own API is part of
the declaration:

```yaml
payment:
  description: "Payments capability the domain calls — provider-agnostic (ACL: Stripe behind it)."
  operations:
    request:
      description: "Create the payment intent for a priced checkout."
      input:
        orderId: { $ref: 'scalars.yaml#/OrderId' }
        cartId:  { $ref: 'scalars.yaml#/CartId' }
        amount:  { $ref: 'entities.yaml#/Money' }
      output:
        paymentIntentId: { $ref: 'scalars.yaml#/PaymentIntentId' }
      errors:
        - { $ref: 'errors.yaml#/PaymentDeclined' }
    refund:
      description: "Request a (possibly partial) refund of a captured intent."
      input:
        paymentIntentId: { $ref: 'scalars.yaml#/PaymentIntentId' }
        amount:          { $ref: 'entities.yaml#/Money' }
      errors: []
  binding: local                   # THE decision (spec-owned): local | http. V0: everything local
  expose: false                    # whether /services/payment/* routes are emitted at all
  implementations:
    stripe:                        # the provider adapter (ACL) — translates names AND payloads
      routes:                      # service op → adapter route, in the PROVIDER's vocabulary
        request: 'POST /adapters/stripe/payment-intents'
        refund:  'POST /adapters/stripe/refunds'
```

From this one block the codegen emits: the `Payment` service trait (`request`/`refund` with the
typed input/output/error signatures), the composition-root wiring for the DECLARED binding (local →
the adapter crate in-process; http → the generated `POST /services/payment/request` client reading
`SERVICE_PAYMENT_URL`), the `/services/payment/*` server routes ONLY when `expose: true`, and the
Stripe-side route mapping — and the validator proves every processmanager.yaml `call` step against
the catalog (`port: payment, operation: refund` must exist, with its declared errors ⊆ the leg's
error surface) plus the spec's own coherence (`expose: true` requires `binding`-consumers to exist;
an http binding with no implementation routes is an error).

## Alternatives considered

- **Keep ad-hoc port traits** — works (it is today's state) but leaves the catalog implicit, the
  deployment topology hard-coded, and C4/observability hand-maintained.
- **Always-HTTP internal APIs (microservices first)** — pays the network/ops tax on day one for a
  V0 that fits one deployable; the whole point is to defer that choice to configuration.

## Consequences

### Positive
- Process managers/handlers stay ignorant of transport and provider; local mode has zero HTTP
  overhead inside one app; remote mode is a config flip per service.
- One more hand-maintained surface (trait + client + routes + wiring) becomes generated from spec.

### Negative
- One more DSL source file and emitter to maintain; when the first service flips to `binding:
  http`, CI needs coverage of the http path (client + routes) in addition to local.

## Sequencing

See docs/codegen-roadmap.md — the service catalog is item 4; the aggregate-lifecycle DSL and the
generated behaviour-test harness land first because they shrink the hand-written (misinterpretable)
surface the most.
