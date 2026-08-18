//! GraphQL BFF (ADR-0006 "role = path"). The SDL is generated from `api.yaml`; here we host it with
//! async-graphql. Stage 1a: the generated type layer (`generated/` — wrapper scalars, output/input
//! types, QueryRoot) backs the schema; the real read resolvers land next.

pub mod acl;
/// The cart READ seam (#451): two-leg `current` lookup, by-id ownership narrowing, and the
/// one `price_cart` path every cart resolver maps through — hand-written and unit-tested;
/// the generated resolver literals only call it.
pub mod cart_read;
pub mod generated;
/// The read-surface scope binding mode (#618): `READ_SCOPE_BINDING_MODE` as per-request data,
/// absent ⇒ `Enforce`. The generated resolvers read it through
/// [`ReadScopeBindingMode::from_context`](read_binding::ReadScopeBindingMode::from_context).
pub mod read_binding;
pub mod routes;
pub mod schema;
/// The subgraph scope slice (#385 API-tier wiring, D8): a `graphql-{scope}` bin serves the
/// master schema restricted to its own scope's operations via the generated composition table.
pub mod scope_slice;
/// The request clock for service-window evaluation (RSO-1): `RequestNow` minted ONCE per
/// request at the transport boundary (beside the correlation id) + the configured validity
/// horizon — the pair every `Restaurant::at` call threads down.
pub mod service_clock;
pub mod session;
/// The request's TENANT (#469): `Host` -> `{slug}` -> `RestaurantId`, resolved ONCE at the
/// transport boundary and injected beside the `ReadScope`. Multi-tenancy by host reached the SSR
/// page renderer and nothing else until this module existed.
pub mod tenant;

