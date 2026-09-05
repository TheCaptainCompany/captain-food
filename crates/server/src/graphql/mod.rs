//! GraphQL BFF (ADR-0006 "role = path"). The SDL is generated from `api.yaml`; here we host it with
//! async-graphql. Stage 1a: the generated type layer (`generated/` — wrapper scalars, output/input
//! types, QueryRoot) backs the schema; the real read resolvers land next.

pub mod acl;
/// The cart READ seam (#451): two-leg `current` lookup, by-id ownership narrowing, and the
/// one `price_cart` path every cart resolver maps through — hand-written and unit-tested;
/// the generated resolver literals only call it.
pub mod cart_read;
pub mod generated;
/// The request's LOCALE for human-readable GraphQL text (#639 2c-ii): resolved once at the
/// transport boundary, read back by the generated `operationStatus` legs to localize
/// `Operation.message` from the row's typed error context.
pub mod locale;
/// The restriction fact terminates the rider's socket (#639 part C step 5,
/// ADR-20260905-065415): the connection-local standing cell, the watcher task and the gate.
pub mod rider_socket;
/// The per-role GraphQL depth/complexity ceiling (#639 part C step 6-ii round 2, R2-E,
/// ADR-20260905-101349 §9): a `parse_query` extension refusing an over-deep/over-complex document
/// before any resolver runs, keyed on the request's `ActingRole` — every role's schema, never
/// `/public` only.
pub mod query_limits;
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

/// A typed READ-side GraphQL error in the P-10 extensions shape, from a generated errors.yaml
/// `ErrorDef` with an empty typed context: the stable PascalCase code as `extensions.code`, the
/// English message as the error message. The mutation path's `domain_error` (generated/mutation.rs)
/// is the command-rejection twin; this is the seam the GENERATED query preludes call (#749 —
/// `argsExactlyOneOf` violations, tenant-selector mismatches), so the wire contract stays in ONE
/// hand-written, reviewable place.
pub(crate) fn typed_error(def: &domain::generated::errors::ErrorDef) -> async_graphql::Error {
    use async_graphql::ErrorExtensions;
    async_graphql::Error::new(def.message_en).extend_with(|_, ext| ext.set("code", def.code))
}

#[cfg(test)]
mod typed_error_tests {
    /// The P-10 shape: `extensions.code` carries the stable code, the message is the English
    /// template — the contract the #749 one-of/tenant rejections ride.
    #[test]
    fn typed_error_carries_the_code_in_extensions() {
        let err = super::typed_error(&domain::generated::errors::INTERNAL);
        let server_error = err.into_server_error(async_graphql::Pos::default());
        let ext = server_error.extensions.expect("extensions present");
        assert_eq!(ext.get("code").map(|v| v.to_string()), Some("\"Internal\"".into()));
        assert_eq!(server_error.message, "Something went wrong on our side.");
    }
}
