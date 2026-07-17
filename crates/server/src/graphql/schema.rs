//! The GraphQL master schema (ADR-0006). Stage 1a: the schema is built over the GENERATED type layer
//! (`generated/` — wrapper scalars, SimpleObject output types, InputObject inputs, and the QueryRoot
//! exposing every api.yaml query). Resolvers are stubs until the read-model repositories are injected
//! via `.data(...)` in `build_schema`.

use async_graphql::{EmptyMutation, EmptySubscription, Schema};

use super::generated::query::QueryRoot;

pub type CaptainSchema = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

/// Build the master schema served under every role path. Read-model repositories are injected here via
/// `.data(...)` once the read resolvers land.
pub fn build_schema() -> CaptainSchema {
    Schema::build(QueryRoot, EmptyMutation, EmptySubscription).finish()
}

#[cfg(test)]
mod tests {
    /// The generated type layer must produce a VALID registry (`finish()` panics on conflicts) whose
    /// runtime SDL matches the shape of the generated `specs/generated/schema.generated.graphql`:
    /// wrapper scalars, mirror enums, the sanitized `Option` type, FK-nav fields and every query field.
    #[test]
    fn schema_builds_and_matches_generated_sdl_shape() {
        let sdl = super::build_schema().sdl();
        for expected in [
            "scalar RestaurantId",
            "scalar MoneyCents",
            "enum OrderStatus",
            "OUT_FOR_DELIVERY",
            "scalar DateTime",
            "type Option {",                      // sanitized Option_ keeps its GraphQL name
            "type Restaurant {",
            "deliveryJobs: [DeliveryJob!]!",      // FK-derived navigation field
            "input RestaurantsQueryInput {",
            "phoneCountries: [PhoneCountry!]!",
            "restaurantLocationsByAccount(input: RestaurantLocationsByAccountQueryInput!): [Restaurant!]!",
            "operation(input: OperationQueryInput!): Operation\n",
        ] {
            assert!(sdl.contains(expected), "runtime SDL missing `{}`:\n{}", expected, sdl);
        }
        // The scaffold field is gone.
        assert!(!sdl.contains("apiVersion"), "scaffold apiVersion still exposed");
    }
}
