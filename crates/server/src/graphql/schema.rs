//! The GraphQL master schema (ADR-0006), built over the GENERATED type layer (`generated/` — wrapper
//! scalars, SimpleObject output types, InputObject inputs, the QueryRoot exposing every api.yaml
//! query, and the MutationRoot exposing every api.yaml mutation). Read-model repositories and the
//! write-side ports are injected via `.data(...)` so the wired resolvers (e.g. `restaurants`,
//! `registerRestaurant`) resolve them from `ctx`; unwired operations still stub `not implemented`.

use std::sync::Arc;

use async_graphql::{EmptySubscription, Schema};
use application::ports::{
    AuthProviderGateway, EventStore, GbpOrderLinkProbe, GoogleOwnershipVerifier, PaymentGateway,
};
use application::queries::{
    CartReadRepository, CatalogReadRepository, CustomerReadRepository, OrderReadRepository,
    PricingPolicyReadRepository, ProspectionReadRepository, RestaurantReadRepository,
    UberEstimationPolicyReadRepository, UberSplitPolicyReadRepository,
};

use super::generated::mutation::MutationRoot;
use super::generated::query::QueryRoot;

pub type CaptainSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

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
    pub customers: Arc<dyn CustomerReadRepository>,
}

/// Write-side ports injected into the mutation resolvers' context (ADR-0035 composition root): the
/// event store the command handlers append to, plus the Google seams the listing commands need
/// (ownership proof, ADR-0019; order-link probe, ADR-0021) and the wrapped auth provider the Customer
/// identity commands need (phone-OTP / email magic link, ADR-0015).
pub struct WriteDeps {
    pub event_store: Arc<dyn EventStore>,
    pub ownership: Arc<dyn GoogleOwnershipVerifier>,
    pub gbp_probe: Arc<dyn GbpOrderLinkProbe>,
    pub auth_provider: Arc<dyn AuthProviderGateway>,
    /// The Stripe create-intent seam `placeOrder` needs (fail-closed stand-in until the real Stripe
    /// adapter lands — a checkout is declined, never silently "paid").
    pub payments: Arc<dyn PaymentGateway>,
}

/// Build the master schema served under every role path. With `Some(deps)`/`Some(writes)` the
/// read-model repos and write-side ports are attached, so wired resolvers work; with `None` (no DB)
/// the schema still builds/introspects and those resolvers error at runtime.
pub fn build_schema(deps: Option<ReadDeps>, writes: Option<WriteDeps>) -> CaptainSchema {
    let mut builder = Schema::build(QueryRoot, MutationRoot, EmptySubscription);
    if let Some(d) = deps {
        builder = builder.data(d.restaurants);
        builder = builder.data(d.prospection);
        builder = builder.data(d.pricing_policy);
        builder = builder.data(d.uber_estimation_policy);
        builder = builder.data(d.uber_split_policy);
        builder = builder.data(d.catalogs);
        builder = builder.data(d.carts);
        builder = builder.data(d.orders);
        builder = builder.data(d.customers);
    }
    if let Some(w) = writes {
        builder = builder.data(w.event_store);
        builder = builder.data(w.ownership);
        builder = builder.data(w.gbp_probe);
        builder = builder.data(w.auth_provider);
        builder = builder.data(w.payments);
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
        let sdl = super::build_schema(None, None).sdl();
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
            // Write side (mutation_block/payloads_block runtime mirror).
            "type Mutation {",
            "registerRestaurant(input: RegisterRestaurantInput!): RegisterRestaurantPayload!",
            "correlationId: CorrelationId!",
            "verifyPhone(input: VerifyPhoneInput!): VerifyPhonePayload!",
        ] {
            assert!(sdl.contains(expected), "runtime SDL missing `{}`:\n{}", expected, sdl);
        }
        // The scaffold field is gone.
        assert!(!sdl.contains("apiVersion"), "scaffold apiVersion still exposed");
    }
}
