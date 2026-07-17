//! The GraphQL master schema (ADR-0006), built over the GENERATED type layer (`generated/` — wrapper
//! scalars, SimpleObject output types, InputObject inputs, and the QueryRoot exposing every api.yaml
//! query). Read-model repositories are injected via `.data(...)` so the wired resolvers (e.g. `restaurants`)
//! resolve them from `ctx`; unwired queries still stub `not implemented`.

use std::sync::Arc;

use async_graphql::{EmptyMutation, EmptySubscription, Schema};
use application::queries::{
    CartReadRepository, CatalogReadRepository, OrderReadRepository, PricingPolicyReadRepository,
    ProspectionReadRepository, RestaurantReadRepository, UberEstimationPolicyReadRepository,
    UberSplitPolicyReadRepository,
};

use super::generated::query::QueryRoot;

pub type CaptainSchema = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

/// Read-model repositories injected into every resolver's context (ADR-0035 composition root). Grows as
/// more read models are wired.
pub struct ReadDeps {
    pub restaurants: Arc<dyn RestaurantReadRepository>,
    pub prospection: Arc<dyn ProspectionReadRepository>,
    pub pricing_policy: Arc<dyn PricingPolicyReadRepository>,
    pub uber_estimation_policy: Arc<dyn UberEstimationPolicyReadRepository>,
    pub uber_split_policy: Arc<dyn UberSplitPolicyReadRepository>,
    pub catalogs: Arc<dyn CatalogReadRepository>,
    pub carts: Arc<dyn CartReadRepository>,
    pub orders: Arc<dyn OrderReadRepository>,
}

/// Build the master schema served under every role path. With `Some(deps)` the read-model repos are
/// attached, so wired resolvers return data; with `None` (no DB) the schema still builds/introspects and
/// those resolvers error at runtime.
pub fn build_schema(deps: Option<ReadDeps>) -> CaptainSchema {
    let mut builder = Schema::build(QueryRoot, EmptyMutation, EmptySubscription);
    if let Some(d) = deps {
        builder = builder.data(d.restaurants);
        builder = builder.data(d.prospection);
        builder = builder.data(d.pricing_policy);
        builder = builder.data(d.uber_estimation_policy);
        builder = builder.data(d.uber_split_policy);
        builder = builder.data(d.catalogs);
        builder = builder.data(d.carts);
        builder = builder.data(d.orders);
    }
    builder.finish()
}

#[cfg(test)]
mod tests {
    /// The generated type layer must produce a VALID registry (`finish()` panics on conflicts) whose
    /// runtime SDL matches the shape of the generated `specs/generated/schema.generated.graphql`:
    /// wrapper scalars, mirror enums, the sanitized `Option` type, FK-nav fields and every query field.
    #[test]
    fn schema_builds_and_matches_generated_sdl_shape() {
        let sdl = super::build_schema(None).sdl();
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
