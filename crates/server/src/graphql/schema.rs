//! The GraphQL master schema (ADR-0006), built over the GENERATED type layer (`generated/` — wrapper
//! scalars, SimpleObject output types, InputObject inputs, the QueryRoot exposing every api.yaml
//! query, the MutationRoot exposing every api.yaml mutation, and the SubscriptionRoot exposing every
//! api.yaml subscription). Read-model repositories, the write-side ports and the in-process
//! `EventBus` (the appended-event feed the subscription resolvers stream over) are injected via
//! `.data(...)` so the wired resolvers resolve them from `ctx`; unwired operations still stub
//! `not implemented`.

use std::sync::Arc;

use async_graphql::Schema;
use actor_client::mailbox::Mailbox;
use application::pm_state::{PaymentProcessStateStore, RefundProcessStateStore};
use application::generated::services::{IdentityService, PaymentService};
use application::ports::{EventStore, GbpOrderLinkProbe, GoogleOwnershipVerifier};
use actor_client::supervision::MailboxLaneRepository;
use application::queries::{
    CartReadRepository, CatalogReadRepository, CustomerCreditReadRepository, CustomerReadRepository,
    DeliveryPartnerAvailabilityReadRepository, DeliverySatisfactionReadRepository,
    DeliveryReadRepository, OrderConversationReadRepository, OrderReadRepository,
    PricingPolicyReadRepository, ProspectionReadRepository, ReclamationReadRepository,
    RefundReadRepository, RestaurantReadRepository, UberEstimationPolicyReadRepository,
    UberSplitPolicyReadRepository,
};

use actor_client::OperationStatusBus;
use infrastructure::EventBus;

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
    pub order_conversations: Arc<dyn OrderConversationReadRepository>,
    pub customers: Arc<dyn CustomerReadRepository>,
    pub deliveries: Arc<dyn DeliveryReadRepository>,
    pub refunds: Arc<dyn RefundReadRepository>,
    pub delivery_satisfaction: Arc<dyn DeliverySatisfactionReadRepository>,
    pub delivery_partner_availabilities: Arc<dyn DeliveryPartnerAvailabilityReadRepository>,
    pub reclamations: Arc<dyn ReclamationReadRepository>,
    pub customer_credit: Arc<dyn CustomerCreditReadRepository>,
    /// The ADMIN `mailboxLanes`/`poisonedMailboxMessages` supervision read (#242 Runtime B): the
    /// mailbox partition registry joined with its live backlog — write-path infrastructure, not a
    /// business read model. The port lives in `actor_client::supervision` (#510): its methods
    /// demand the mailbox witness, so the resolvers read through that crate's door functions.
    pub mailbox_lanes: Arc<dyn MailboxLaneRepository>,
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
    /// THE acceptance door (acceptance-first, ADR-20260720-015300/-015500; the only journal
    /// since #242 Runtime D): every mutation's command is an `inbound_messages` insert the
    /// partitioned worker delivers, and `operationStatus` reads the same row back.
    pub mailbox: Arc<dyn Mailbox>,
    /// The in-process operation-response broadcast (behind the actor_client boundary, #303)
    /// feeding `operationStatusChanged` through `ActorClient::watch`.
    pub status_bus: OperationStatusBus,
    /// Cookie-pickup parking (#112): VerifyPhone/verify-email park the provider session here for
    /// `POST /auth/session` to claim. Fail-closed stand-in (refuses) until AUTH_SESSION_KEY + DB.
    pub auth_sessions: Arc<dyn application::auth_sessions::AuthSessionStore>,
    /// The write-side arbiter of storefront-slug uniqueness (ADR-20260728-011344 D3) that
    /// `configureRestaurantSlug` reserves through. NOT a read repository: a reservation is a write-path
    /// decision backed by a real `UNIQUE` constraint, never a lookup against the eventually-consistent
    /// `Restaurant` projection.
    pub slug_reservations: Arc<dyn application::queries::SlugReservationRepository>,
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
    build_schema_for_scope(deps, writes, events, None)
}

/// [`build_schema`] with an optional SUBGRAPH SCOPE SLICE (#385 API-tier wiring, D8): with
/// `Some(scope)` the schema rejects any document whose top-level fields belong to another
/// scope's `specs/{scope}/api.yaml` fragment (see `scope_slice`). The monolith passes `None`
/// and keeps serving everything; a `graphql-{scope}` bin passes its own scope.
pub fn build_schema_for_scope(
    deps: Option<ReadDeps>,
    writes: Option<WriteDeps>,
    events: Option<EventBus>,
    scope: Option<&'static str>,
) -> CaptainSchema {
    let mut builder = Schema::build(QueryRoot, MutationRoot, SubscriptionRoot);
    if let Some(scope) = scope {
        builder = builder.extension(super::scope_slice::ScopeSlice::new(scope));
    }
    if let Some(d) = deps {
        // EXHAUSTIVE on purpose — no `..` (#510, the #529 class): `ctx.data` is TypeId-keyed, so
        // a ReadDeps field added without a matching `.data()` registration compiles clean and
        // 500s at resolve time, on the supervision screen mid-incident in the worst case. This
        // destructure forces every new field to be named here, and the unused-variable lint then
        // points straight at the missing registration.
        let ReadDeps {
            restaurants,
            prospection,
            pricing_policy,
            uber_estimation_policy,
            uber_split_policy,
            catalogs,
            carts,
            orders,
            order_conversations,
            customers,
            deliveries,
            refunds,
            delivery_satisfaction,
            delivery_partner_availabilities,
            reclamations,
            customer_credit,
            mailbox_lanes,
        } = d;
        builder = builder.data(restaurants);
        builder = builder.data(prospection);
        builder = builder.data(pricing_policy);
        builder = builder.data(uber_estimation_policy);
        builder = builder.data(uber_split_policy);
        builder = builder.data(catalogs);
        builder = builder.data(carts);
        builder = builder.data(orders);
        builder = builder.data(order_conversations);
        builder = builder.data(customers);
        builder = builder.data(deliveries);
        builder = builder.data(refunds);
        builder = builder.data(delivery_satisfaction);
        builder = builder.data(delivery_partner_availabilities);
        builder = builder.data(reclamations);
        builder = builder.data(customer_credit);
        builder = builder.data(mailbox_lanes);
    }
    if let Some(w) = writes {
        builder = builder.data(w.event_store);
        builder = builder.data(w.ownership);
        builder = builder.data(w.gbp_probe);
        builder = builder.data(w.auth_provider);
        builder = builder.data(w.payments);
        builder = builder.data(w.pm_state);
        builder = builder.data(w.refund_state);
        builder = builder.data(w.mailbox);
        builder = builder.data(w.status_bus);
        builder = builder.data(w.auth_sessions);
        builder = builder.data(w.slug_reservations);
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
