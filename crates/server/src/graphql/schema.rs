//! The GraphQL master schema (ADR-0006), built over the GENERATED type layer (`generated/` — wrapper
//! scalars, SimpleObject output types, InputObject inputs, the QueryRoot exposing every api.yaml
//! query, the MutationRoot exposing every api.yaml mutation, and the SubscriptionRoot exposing every
//! api.yaml subscription). Read-model repositories, the write-side ports and the in-process
//! `EventBus` (the appended-event feed the subscription resolvers stream over) are injected via
//! `.data(...)` so the wired resolvers resolve them from `ctx`; unwired operations still stub
//! `not implemented`.

use std::sync::Arc;

use async_graphql::Schema;
use application::journal::CommandJournal;
use application::pm_state::{PaymentProcessStateStore, RefundProcessStateStore};
use application::generated::services::{IdentityService, PaymentService};
use application::ports::{EventStore, GbpOrderLinkProbe, GoogleOwnershipVerifier};
use application::queries::{
    CartReadRepository, CatalogReadRepository, CustomerReadRepository,
    DeliveryPartnerAvailabilityReadRepository, DeliverySatisfactionReadRepository,
    DeliveryReadRepository, OrderReadRepository, PricingPolicyReadRepository,
    ProspectionReadRepository, RefundReadRepository, RestaurantReadRepository,
    UberEstimationPolicyReadRepository, UberSplitPolicyReadRepository,
};

use infrastructure::{EventBus, OperationStatusBus};

use super::generated::mutation::MutationRoot;
use super::generated::query::QueryRoot;
use super::generated::subscription::SubscriptionRoot;

pub type CaptainSchema = Schema<QueryRoot, MutationRoot, SubscriptionRoot>;

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
    pub deliveries: Arc<dyn DeliveryReadRepository>,
    pub refunds: Arc<dyn RefundReadRepository>,
    pub delivery_satisfaction: Arc<dyn DeliverySatisfactionReadRepository>,
    pub delivery_partner_availabilities: Arc<dyn DeliveryPartnerAvailabilityReadRepository>,
}

/// Write-side ports injected into the mutation resolvers' context (ADR-0035 composition root): the
/// event store the command handlers append to, plus the Google seams the listing commands need
/// (ownership proof, ADR-0019; order-link probe, ADR-0021) and the wrapped auth provider the Customer
/// identity commands need (phone-OTP / email magic link, ADR-0015).
pub struct WriteDeps {
    pub event_store: Arc<dyn EventStore>,
    pub ownership: Arc<dyn GoogleOwnershipVerifier>,
    pub gbp_probe: Arc<dyn GbpOrderLinkProbe>,
    pub auth_provider: Arc<dyn IdentityService>,
    /// The generated `payment` service port (services.yaml, issue #26) `placeOrder`/`approveRefund`
    /// call (fail-closed stand-in until the real Stripe adapter is configured — a checkout is
    /// declined, never silently "paid").
    pub payments: Arc<dyn PaymentService>,
    /// The `payment_process_manager` state rows `placeOrder` opens (AWAITING_PAYMENT_RESULT) and
    /// single-flights concurrent checkouts of the same cart on (ADR-20260719-193500).
    pub pm_state: Arc<dyn PaymentProcessStateStore>,
    /// The `refund_process_manager` state rows the refund DECISION legs (`approveRefund` /
    /// `denyRefund`) resolve the pending run on (rules.yaml#/RefundRequiresApproval).
    pub refund_state: Arc<dyn RefundProcessStateStore>,
    /// The durable command journal every mutation writes BEFORE handling (acceptance-first,
    /// ADR-20260720-015300/-015500) — also the `operationStatus` read.
    pub journal: Arc<dyn CommandJournal>,
    /// The in-process journal-transition broadcast feeding `operationStatusChanged`.
    pub status_bus: OperationStatusBus,
}

/// Build the master schema served under every role path. With `Some(deps)`/`Some(writes)` the
/// read-model repos and write-side ports are attached, so wired resolvers work; with `None` (no DB)
/// the schema still builds/introspects and those resolvers error at runtime. `events` is the
/// in-process appended-event bus the subscription resolvers stream over (the same bus handed to
/// `PgEventStore::with_bus`); `None` makes subscriptions error at runtime like any missing dep.
pub fn build_schema(
    deps: Option<ReadDeps>,
    writes: Option<WriteDeps>,
    events: Option<EventBus>,
) -> CaptainSchema {
    let mut builder = Schema::build(QueryRoot, MutationRoot, SubscriptionRoot);
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
        builder = builder.data(d.deliveries);
        builder = builder.data(d.refunds);
        builder = builder.data(d.delivery_satisfaction);
        builder = builder.data(d.delivery_partner_availabilities);
    }
    if let Some(w) = writes {
        builder = builder.data(w.event_store);
        builder = builder.data(w.ownership);
        builder = builder.data(w.gbp_probe);
        builder = builder.data(w.auth_provider);
        builder = builder.data(w.payments);
        builder = builder.data(w.pm_state);
        builder = builder.data(w.refund_state);
        builder = builder.data(w.journal);
        builder = builder.data(w.status_bus);
    }
    if let Some(bus) = events {
        builder = builder.data(bus);
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
        let sdl = super::build_schema(None, None, None).sdl();
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
            "operationStatus(input: OperationStatusQueryInput!): Operation\n",
            "paymentStatus(input: PaymentStatusQueryInput!): PaymentIntent\n",
            // Write side — acceptance-first (ADR-20260720-015500): every mutation takes the optional
            // metadata envelope and returns the ONE shared MutationAcceptance.
            "type Mutation {",
            "type MutationAcceptance {",
            "messageId: MessageId!",
            "operationStatus: OperationStatus!",
            "input MetadataInput {",
            "registerRestaurant(input: RegisterRestaurantInput!, metadata: MetadataInput): MutationAcceptance!",
            "verifyPhone(input: VerifyPhoneInput!, metadata: MetadataInput): MutationAcceptance!",
            // Subscriptions (subscription_block's runtime mirror).
            "type Subscription {",
            "orderStatusChanged(input: OrderStatusChangedSubscriptionInput!): Order!",
            "operationStatusChanged(input: OperationStatusChangedSubscriptionInput!): Operation!",
            "paymentStatusChanged(input: PaymentStatusChangedSubscriptionInput!): PaymentIntent!",
        ] {
            assert!(sdl.contains(expected), "runtime SDL missing `{}`:\n{}", expected, sdl);
        }
        // The scaffold field is gone.
        assert!(!sdl.contains("apiVersion"), "scaffold apiVersion still exposed");
    }
}
