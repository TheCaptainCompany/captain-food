//! THE HUMAN-OWNED MAILBOX ROUTER (#771). **This file is NOT generated and must never become
//! generated.**
//!
//! Its counterpart `application::generated::inboxes` emits one `<Actor>Inbox` enum per mailbox
//! actor from that actor's `receives:` set in `specs/*/actors.yaml`. The enum is the CLOSED set of
//! messages a lane can carry; the `match`es below say what each one DOES. Keeping the two halves
//! apart is the entire mechanism: if one emitter walk produced both the variants and the arms, the
//! match would be exhaustive **by construction** and `rustc` would catch exactly nothing.
//!
//! WHAT IT BUYS. Adding a `receives:` entry to actors.yaml adds a variant, and this file then fails
//! to compile with `error[E0004]: non-exhaustive patterns` until a human decides what the new
//! message does. Before #771 the same omission shipped green: the router was a flat `match` over a
//! `&str` across ALL actors ending in `_ => None`, and an unconsumed message surfaced only in
//! production as a `FAILED "unroutable command type"` row. #595 was that, by hand — a replacement
//! order silently never born. The cutover found **ten more** live instances of it — **all ten had
//! their handler already written in `application::commands` and were missing only a router row, so
//! all ten are wired here** — two of them already declared as process-manager `sends:`
//! (`BindCartToCustomer` from `CartBindingProcess`, `GrantCustomerCredit` from
//! `ReclamationProcess`'s GOODWILL_CREDIT arm — a resolved reclamation's goodwill credit, i.e. the
//! money path).
//!
//! **The guarantee is over COMMANDS and the routing decision, not over fact DELIVERY.** E0004
//! proves every declared message reaches a decision in this file; a decision of
//! `InboxOutcome::RecordFact` then hands the message to `mailbox::handler`'s fact route, which is
//! still a string match ending in a catch-all. Extending the same proof to that route is
//! [#780](https://github.com/TheCaptainCompany/captain-food/issues/780).
//!
//! TWO RULES FOR ANYONE EDITING THIS FILE.
//!
//! 1. **Never write a CATCH-ALL arm** — neither `_ =>` nor a named binding (`other =>`, `m =>`,
//!    `_other =>`), which is the same total pattern wearing a name. Typed payloads stop a bare
//!    `Inbox::Foo => {}` from compiling, but a catch-all absorbs every future variant, and a match
//!    that compiles and does nothing is the failure this file exists to remove. The type system
//!    cannot forbid one (there is no "no wildcard may match me", and `#[non_exhaustive]` does the
//!    *opposite*, forcing one), so per ADR-20260803-234035 a check is the legitimate fallback:
//!    `codegen tests::typed_actor_inbox_e0004::every_arm_of_the_human_owned_router_names_an_inbox_variant`
//!    parses this file with `syn` and asserts the POSITIVE property — every arm of a lane match
//!    names an `<Actor>Inbox::` variant, and no arm anywhere is total. (Its first version scanned
//!    for the *spelling* `_ =>`; PR #776's round-1 review bypassed it with `_other =>`.)
//! 2. **A handler you are not ready to write is `InboxOutcome::Deferred`, declared in the DSL** —
//!    `deferred: { reason, issue }` on the `receives:` entry. That replaced the `UNWIRED_MUTATIONS`
//!    const, which was a Rust list in an emitter that nobody read. Never invent a silent arm.
//!    **No message declares a deferral today** (the one candidate turned out to have a handler and
//!    is wired); the grammar is kept for C3's remaining `deliver:` routes, a decision recorded in
//!    ADR-20260830-183000 rather than a leftover.

use std::sync::Arc;

use application::generated::inboxes::{
    ActorFactInbox, ActorInbox, CartFactInbox, CatalogFactInbox, CustomerFactInbox, DeliveryJobFactInbox, OrderFactInbox, PaymentFactInbox, PlaceOrderProcessFactInbox, RefundProcessFactInbox, RestaurantFactInbox, CartInbox, CatalogInbox, ConversationInbox, CustomerCreditInbox, CustomerInbox, DeliveryJobInbox, DeliveryPartnerRegistrationInbox, MailboxSupervisionInbox, OrderInbox, PaymentInbox, PlaceOrderProcessInbox, ProspectInbox, ReclamationInbox, RefundProcessInbox, RestaurantAccountInbox, RestaurantInbox, RiderInbox,
};
use application::ports::Actor;
use domain::shared::errors::DomainError;

/// Every port any wired command handler needs — the worker-side counterpart of the resolvers'
/// ctx.data injections, bundled once at the composition root.
#[derive(Clone)]
pub struct CommandDeps {
    pub store: Arc<dyn application::ports::EventStore>,
    pub restaurants: Arc<dyn application::queries::RestaurantReadRepository>,
    pub slugs: Arc<dyn application::queries::SlugReservationRepository>,
    pub ownership: Arc<dyn application::ports::GoogleOwnershipVerifier>,
    pub probe: Arc<dyn application::ports::GbpOrderLinkProbe>,
    pub prospection: Arc<dyn application::queries::ProspectionReadRepository>,
    pub catalogs: Arc<dyn application::queries::CatalogReadRepository>,
    pub auth: Arc<dyn application::generated::services::IdentityService>,
    pub customers: Arc<dyn application::queries::CustomerReadRepository>,
    pub sessions: Arc<dyn application::auth_sessions::AuthSessionStore>,
    pub payments: Arc<dyn application::generated::services::PaymentService>,
    pub pm_state: Arc<dyn application::pm_state::PaymentProcessStateStore>,
    pub refund_state: Arc<dyn application::pm_state::RefundProcessStateStore>,
    pub mailbox_requeue: Arc<dyn application::queries::MailboxRequeue>,
    /// RSO-1 (`configuration.yaml#/ENFORCE_SERVICE_HOURS_GUARD`): the PlaceOrder service-hours
    /// enforcement gate, resolved ONCE at the composition root -- the handler takes it as a
    /// parameter (the `when_at` style), never reads config/env itself.
    pub enforce_service_hours_guard: bool,
    /// #167 (`configuration.yaml#/ENFORCE_ACCEPTANCE_TIMEOUT`): the acceptance-timeout ACTION
    /// gate, read at DELIVERY time by the kind-MESSAGE OrderAcceptanceTimedOut route -- same
    /// composition-root resolution as `enforce_service_hours_guard`. OFF (the default) is SHADOW
    /// MODE: the full still-PLACED guard runs, only the append is inert.
    pub enforce_acceptance_timeout: bool,
    /// Where EVERY declared lane route's gate stands (#797) -- one field per
    /// [`Route`](application::generated::process_managers::Route), generated from the DSL's
    /// `route_gate:` declarations, resolved once at the composition root exactly like
    /// `enforce_service_hours_guard`.
    ///
    /// Read at DELIVERY time by the PM-fact route, which hands the saga a lane sink plus these
    /// gates; each routed step then consults ITS OWN route. This was the single
    /// `route_order_birth_through_lane` boolean until #797, and one boolean is what made routing a
    /// property of the DELIVERY ROUTE rather than of the route being gated -- so a second route
    /// added here would have inherited the first route's key and could not be rolled back alone.
    /// A gate OFF leaves that route's legacy foreign-stream append untouched, so rollback is a
    /// config flip, never a redeploy. `ROUTE_ORDER_BIRTH_THROUGH_LANE` has been ON since the
    /// ADR-20260830-012200 founder flip.
    pub route_gates: application::generated::process_managers::RouteGates,
}


/// The slice of the request envelope some handler calls read (placeOrder's session scope) — bound
/// once from the mailbox row and handed down, never re-read per arm.
pub struct RouterEnv {
    pub session_id: Option<uuid::Uuid>,
}

/// What one routed message turned out to BE. The router decides; the caller (the mailbox delivery
/// glue, which owns the transaction) performs.
///
/// This split is deliberate: the routing decision is a pure function of the typed message, so it
/// stays testable and transaction-free, while the effects that need a `tx` stay in the delivery
/// glue where the fencing contract lives (PROP-20260728-152752 §3.4).
pub enum InboxOutcome {
    /// A COMMAND this build handles. Its events are staged on `deps.store`.
    Handled(Result<(), DomainError>),
    /// A FACT or promoted REMINDER: the caller records the carried event on the actor's own stream
    /// (the generic record route). No per-message handler exists or should.
    RecordFact,
    /// A leg of a process manager, which the delivery glue runs through its own prepare/commit
    /// path (`mailbox::pm_delivery`) rather than through a command handler. Reaching this arm from
    /// the command door is a wiring bug, not a business outcome.
    ProcessManagerLeg,
    /// DECLARED received, handler deliberately not built yet — `actors.yaml` `receives[].deferred`.
    /// The caller rejects the row with the catalogued `Internal` code; the deferral itself is
    /// reviewable spec content carrying a reason and a tracking issue
    /// (`application::generated::inboxes::DEFERRED_MESSAGES`).
    Deferred,
}

/// Run one command handler under the `command.validate` span and wrap its outcome.
async fn run<F>(handler: F) -> InboxOutcome
where
    F: std::future::Future<Output = Result<(), DomainError>>,
{
    InboxOutcome::Handled(validated(handler).await)
}

/// The `command.validate` handler-boundary wrapper (specs/observability.yaml `place-order`,
/// RSO-1): c4-l3.yaml marks `command-handlers` `instrumented: false`, so the span lives HERE at
/// the dispatch seam, never in the aggregate -- it instruments the handler call and records
/// `business.validation_status` from the ACTUAL outcome. A Repository error is transient
/// infrastructure (the delivery aborts and retries), so it records NO validation verdict --
/// classifying it `rejected` would count a DB blip as a business refusal.
async fn validated<F>(handler: F) -> Result<(), DomainError>
where
    F: std::future::Future<Output = Result<(), DomainError>>,
{
    use tracing::Instrument as _;
    let span = telemetry::spans::command_validate();
    let result = handler.instrument(span.clone()).await;
    match &result {
        Ok(()) => telemetry::spans::record_validation_status(&span, "accepted"),
        Err(DomainError::Repository(_)) => {}
        Err(_) => telemetry::spans::record_validation_status(&span, "rejected"),
    }
    result
}

pub async fn route(
    deps: &CommandDeps,
    message: ActorInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    match message {
        ActorInbox::Cart(m) => cart(deps, m, actor, env).await,
        ActorInbox::Catalog(m) => catalog(deps, m, actor, env).await,
        ActorInbox::Conversation(m) => conversation(deps, m, actor, env).await,
        ActorInbox::Customer(m) => customer(deps, m, actor, env).await,
        ActorInbox::CustomerCredit(m) => customer_credit(deps, m, actor, env).await,
        ActorInbox::DeliveryJob(m) => delivery_job(deps, m, actor, env).await,
        ActorInbox::DeliveryPartnerRegistration(m) => delivery_partner_registration(deps, m, actor, env).await,
        ActorInbox::MailboxSupervision(m) => mailbox_supervision(deps, m, actor, env).await,
        ActorInbox::Order(m) => order(deps, m, actor, env).await,
        ActorInbox::Payment(m) => payment(deps, m, actor, env).await,
        ActorInbox::PlaceOrderProcess(m) => place_order_process(deps, m, actor, env).await,
        ActorInbox::Prospect(m) => prospect(deps, m, actor, env).await,
        ActorInbox::Reclamation(m) => reclamation(deps, m, actor, env).await,
        ActorInbox::RefundProcess(m) => refund_process(deps, m, actor, env).await,
        ActorInbox::Restaurant(m) => restaurant(deps, m, actor, env).await,
        ActorInbox::RestaurantAccount(m) => restaurant_account(deps, m, actor, env).await,
        ActorInbox::Rider(m) => rider(deps, m, actor, env).await,
    }
}

/// The `Cart` lane.
async fn cart(
    deps: &CommandDeps,
    message: CartInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        CartInbox::AddCartLine(cmd) => run(async { application::commands::add_cart_line(deps.store.as_ref(), deps.catalogs.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CartInbox::BindCartToCustomer(cmd) => run(async { application::commands::bind_cart_to_customer(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CartInbox::CartCheckedOut(_) => InboxOutcome::RecordFact,
        CartInbox::ChangeCartLineQuantity(cmd) => run(async { application::commands::change_cart_line_quantity(deps.store.as_ref(), deps.catalogs.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CartInbox::RemoveCartLine(cmd) => run(async { application::commands::remove_cart_line(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
    }
}

/// The `Catalog` lane.
async fn catalog(
    deps: &CommandDeps,
    message: CatalogInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        CatalogInbox::AddCatalogCategory(cmd) => run(async { application::commands::add_catalog_category(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CatalogInbox::AddOptionList(cmd) => run(async { application::commands::add_option_list(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CatalogInbox::AddProduct(cmd) => run(async { application::commands::add_product(deps.store.as_ref(), deps.restaurants.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CatalogInbox::ConfigureCatalogSlug(cmd) => run(async { application::commands::configure_catalog_slug(deps.store.as_ref(), deps.catalogs.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CatalogInbox::CreateCatalog(cmd) => run(async { application::commands::create_catalog(deps.store.as_ref(), deps.restaurants.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CatalogInbox::ImportCatalog(cmd) => run(async { application::commands::import_catalog(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CatalogInbox::OfferStockUpdated(_) => InboxOutcome::RecordFact,
        CatalogInbox::RemoveCatalogCategory(cmd) => run(async { application::commands::remove_catalog_category(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CatalogInbox::RemoveOptionList(cmd) => run(async { application::commands::remove_option_list(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CatalogInbox::RemoveProduct(cmd) => run(async { application::commands::remove_product(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CatalogInbox::UpdateCatalogCategory(cmd) => run(async { application::commands::update_catalog_category(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CatalogInbox::UpdateOfferStock(cmd) => run(async { application::commands::update_offer_stock(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CatalogInbox::UpdateOptionList(cmd) => run(async { application::commands::update_option_list(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CatalogInbox::UpdateProduct(cmd) => run(async { application::commands::update_product(deps.store.as_ref(), deps.restaurants.as_ref(), cmd, actor).await.map(|_| ()) }).await,
    }
}

/// The `Conversation` lane.
async fn conversation(
    deps: &CommandDeps,
    message: ConversationInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        ConversationInbox::EscalateToAdmin(cmd) => run(async { application::commands::escalate_to_admin(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        ConversationInbox::MuteParticipant(cmd) => run(async { application::commands::mute_participant(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        ConversationInbox::OpenConversation(cmd) => run(async { application::commands::open_conversation(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        ConversationInbox::PostMessage(cmd) => run(async { application::commands::post_message(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        ConversationInbox::RecordMessageTranslation(cmd) => run(async { application::commands::record_message_translation(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        ConversationInbox::UnmuteParticipant(cmd) => run(async { application::commands::unmute_participant(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
    }
}

/// The `Customer` lane.
async fn customer(
    deps: &CommandDeps,
    message: CustomerInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        CustomerInbox::CancelCustomerErasure(cmd) => run(async { application::commands::cancel_customer_erasure(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerInbox::ChangeLanguage(cmd) => run(async { application::commands::change_language(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerInbox::ConfirmCustomerErasure(cmd) => run(async { application::commands::confirm_customer_erasure(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerInbox::ConfirmEmailVerification(cmd) => run(async { application::commands::confirm_email_verification(deps.store.as_ref(), deps.auth.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerInbox::ConfirmPhoneChange(cmd) => run(async { application::commands::confirm_phone_change(deps.store.as_ref(), deps.auth.as_ref(), deps.customers.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerInbox::CustomerErasureDue(_) => InboxOutcome::RecordFact,
        CustomerInbox::CustomerIdentityUnlinked(_) => InboxOutcome::RecordFact,
        CustomerInbox::MarkRestaurantAsFavorite(cmd) => run(async { application::commands::mark_restaurant_as_favorite(deps.store.as_ref(), deps.restaurants.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerInbox::RemoveCustomerAddress(cmd) => run(async { application::commands::remove_customer_address(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerInbox::RequestCustomerErasure(cmd) => run(async { application::commands::request_customer_erasure(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerInbox::RequestEmailVerification(cmd) => run(async { application::commands::request_email_verification(deps.store.as_ref(), deps.auth.as_ref(), deps.customers.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerInbox::RequestPhoneChange(cmd) => run(async { application::commands::request_phone_change(deps.store.as_ref(), deps.auth.as_ref(), deps.customers.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerInbox::RequestPhoneVerification(cmd) => run(async { application::commands::request_phone_verification(deps.store.as_ref(), deps.auth.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerInbox::SetCustomerAddress(cmd) => run(async { application::commands::set_customer_address(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerInbox::SetCustomerPaymentMethod(cmd) => run(async { application::commands::set_customer_payment_method(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerInbox::SetCustomerPreferences(cmd) => run(async { application::commands::set_customer_preferences(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerInbox::UnmarkRestaurantAsFavorite(cmd) => run(async { application::commands::unmark_restaurant_as_favorite(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerInbox::UpdateCustomerInfo(cmd) => run(async { application::commands::update_customer_info(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerInbox::VerifyPhone(cmd) => run(async { application::commands::verify_phone(deps.store.as_ref(), deps.auth.as_ref(), deps.customers.as_ref(), deps.sessions.as_ref(), cmd, actor).await.map(|_| ()) }).await,
    }
}

/// The `CustomerCredit` lane.
async fn customer_credit(
    deps: &CommandDeps,
    message: CustomerCreditInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        CustomerCreditInbox::ConsumeCustomerCredit(cmd) => run(async { application::commands::consume_customer_credit(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        CustomerCreditInbox::GrantCustomerCredit(cmd) => run(async { application::commands::grant_customer_credit(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
    }
}

/// The `DeliveryJob` lane.
async fn delivery_job(
    deps: &CommandDeps,
    message: DeliveryJobInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        DeliveryJobInbox::AcceptDelivery(cmd) => run(async { application::commands::accept_delivery(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        DeliveryJobInbox::CancelDelivery(cmd) => run(async { application::commands::cancel_delivery(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        DeliveryJobInbox::CompleteDelivery(cmd) => run(async { application::commands::complete_delivery(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        DeliveryJobInbox::ConfirmPickup(cmd) => run(async { application::commands::confirm_pickup(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        DeliveryJobInbox::DeclineDelivery(cmd) => run(async { application::commands::decline_delivery(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        DeliveryJobInbox::DeliveryAcceptedByPartner(_) => InboxOutcome::RecordFact,
        DeliveryJobInbox::DeliveryDispatchFailed(_) => InboxOutcome::RecordFact,
        DeliveryJobInbox::DeliveryOfferTimedOut(_) => InboxOutcome::RecordFact,
        DeliveryJobInbox::DeliveryRejectedByPartner(_) => InboxOutcome::RecordFact,
        DeliveryJobInbox::DeliveryRequested(_) => InboxOutcome::RecordFact,
        DeliveryJobInbox::DeliveryStatusUpdated(_) => InboxOutcome::RecordFact,
        DeliveryJobInbox::EscalateDelivery(cmd) => run(async { application::commands::escalate_delivery(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        DeliveryJobInbox::ReportDeliveryIssue(cmd) => run(async { application::commands::report_delivery_issue(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        DeliveryJobInbox::ResolveDeliveryIssue(cmd) => run(async { application::commands::resolve_delivery_issue(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        DeliveryJobInbox::UnassignDeliveryFromPartner(cmd) => run(async { application::commands::unassign_delivery_from_partner(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        DeliveryJobInbox::UpdateDeliveryStatus(cmd) => run(async { application::commands::update_delivery_status(deps.store.as_ref(), cmd, actor).await }).await,
    }
}

/// The `DeliveryPartnerRegistration` lane.
async fn delivery_partner_registration(
    deps: &CommandDeps,
    message: DeliveryPartnerRegistrationInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        DeliveryPartnerRegistrationInbox::ApproveDeliveryPartnerAvailability(cmd) => run(async { application::commands::approve_delivery_partner_availability(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        DeliveryPartnerRegistrationInbox::RegisterDeliveryPartnerAvailability(cmd) => run(async { application::commands::register_delivery_partner_availability(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        DeliveryPartnerRegistrationInbox::RevokeDeliveryPartnerAvailability(cmd) => run(async { application::commands::revoke_delivery_partner_availability(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
    }
}

/// The `MailboxSupervision` lane.
async fn mailbox_supervision(
    deps: &CommandDeps,
    message: MailboxSupervisionInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        MailboxSupervisionInbox::RequeueMailboxMessage(cmd) => run(async { application::commands::requeue_mailbox_message(deps.store.as_ref(), deps.mailbox_requeue.as_ref(), cmd, actor).await.map(|_| ()) }).await,
    }
}

/// The `Order` lane.
async fn order(
    deps: &CommandDeps,
    message: OrderInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        OrderInbox::AcceptOrder(cmd) => run(async { application::commands::accept_order(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        OrderInbox::CancelOrderByCustomer(cmd) => run(async { application::commands::cancel_order_by_customer(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        OrderInbox::CancelOrderByRestaurant(cmd) => run(async { application::commands::cancel_order_by_restaurant(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        OrderInbox::MarkOrderDelivered(cmd) => run(async { application::commands::mark_order_delivered(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        OrderInbox::MarkOrderReady(cmd) => run(async { application::commands::mark_order_ready(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        OrderInbox::OrderAcceptanceTimedOut(_) => InboxOutcome::RecordFact,
        OrderInbox::OrderExpired(_) => InboxOutcome::RecordFact,
        OrderInbox::OrderPlaced(_) => InboxOutcome::RecordFact,
        OrderInbox::PlaceReplacementOrder(cmd) => run(async { application::commands::place_replacement_order(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        OrderInbox::RateOrder(cmd) => run(async { application::commands::rate_order(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        OrderInbox::RateRestaurant(cmd) => run(async { application::commands::rate_restaurant(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        OrderInbox::RecordDeliverySatisfaction(cmd) => run(async { application::commands::record_delivery_satisfaction(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        OrderInbox::RejectOrder(cmd) => run(async { application::commands::reject_order(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        OrderInbox::RequestRefund(cmd) => run(async { application::commands::request_refund(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        OrderInbox::StartPreparation(cmd) => run(async { application::commands::start_preparation(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        OrderInbox::TipOrder(cmd) => run(async { application::commands::tip_order(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
    }
}

/// The `Payment` lane.
async fn payment(
    deps: &CommandDeps,
    message: PaymentInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        PaymentInbox::PaymentAuthorized(_) => InboxOutcome::RecordFact,
        PaymentInbox::PaymentCaptureFailed(_) => InboxOutcome::RecordFact,
        PaymentInbox::PaymentCaptured(_) => InboxOutcome::RecordFact,
        PaymentInbox::PaymentFailed(_) => InboxOutcome::RecordFact,
        PaymentInbox::PaymentIntentCreated(_) => InboxOutcome::RecordFact,
        PaymentInbox::PaymentRefunded(_) => InboxOutcome::RecordFact,
        PaymentInbox::PaymentReleased(_) => InboxOutcome::RecordFact,
        PaymentInbox::RefundApproved(_) => InboxOutcome::RecordFact,
        PaymentInbox::RefundDenied(_) => InboxOutcome::RecordFact,
        PaymentInbox::RefundOpened(_) => InboxOutcome::RecordFact,
    }
}

/// The `PlaceOrderProcess` lane.
async fn place_order_process(
    deps: &CommandDeps,
    message: PlaceOrderProcessInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        PlaceOrderProcessInbox::PaymentAuthorized(_) => InboxOutcome::ProcessManagerLeg,
        PlaceOrderProcessInbox::PaymentFailed(_) => InboxOutcome::ProcessManagerLeg,
        PlaceOrderProcessInbox::PlaceOrder(_) => InboxOutcome::ProcessManagerLeg,
    }
}

/// The `Prospect` lane.
async fn prospect(
    deps: &CommandDeps,
    message: ProspectInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        ProspectInbox::MarkProspectCold(cmd) => run(async { application::commands::mark_prospect_cold(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        ProspectInbox::RecordProspectContact(cmd) => run(async { application::commands::record_prospect_contact(deps.store.as_ref(), deps.prospection.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        ProspectInbox::RecordProspectReply(cmd) => run(async { application::commands::record_prospect_reply(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
    }
}

/// The `Reclamation` lane.
async fn reclamation(
    deps: &CommandDeps,
    message: ReclamationInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        ReclamationInbox::AttachReclamationEvidence(cmd) => run(async { application::commands::attach_reclamation_evidence(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        ReclamationInbox::OpenReclamation(cmd) => run(async { application::commands::open_reclamation(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        ReclamationInbox::RejectReclamation(cmd) => run(async { application::commands::reject_reclamation(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        ReclamationInbox::ReopenReclamation(cmd) => run(async { application::commands::reopen_reclamation(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        ReclamationInbox::ResolveReclamation(cmd) => run(async { application::commands::resolve_reclamation(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
    }
}

/// The `RefundProcess` lane.
async fn refund_process(
    deps: &CommandDeps,
    message: RefundProcessInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        RefundProcessInbox::ApproveRefund(_) => InboxOutcome::ProcessManagerLeg,
        RefundProcessInbox::DenyRefund(_) => InboxOutcome::ProcessManagerLeg,
        RefundProcessInbox::PaymentRefunded(_) => InboxOutcome::ProcessManagerLeg,
    }
}

/// The `Restaurant` lane.
async fn restaurant(
    deps: &CommandDeps,
    message: RestaurantInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        RestaurantInbox::ActivateRestaurant(cmd) => run(async { application::commands::activate_restaurant(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RestaurantInbox::ChangeOrderAcceptanceMode(cmd) => run(async { application::commands::change_order_acceptance_mode(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RestaurantInbox::ChangeRestaurantListingStatus(cmd) => run(async { application::commands::change_restaurant_listing_status(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RestaurantInbox::ClaimRestaurantListing(cmd) => run(async { application::commands::claim_restaurant_listing(deps.store.as_ref(), deps.ownership.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RestaurantInbox::ConfigureGoogleBusinessProfileOrderLink(cmd) => run(async { application::commands::configure_gbp_order_link(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RestaurantInbox::ConfigureRestaurantSlug(cmd) => run(async { application::commands::configure_restaurant_slug(deps.store.as_ref(), deps.slugs.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RestaurantInbox::DeactivateRestaurant(cmd) => run(async { application::commands::deactivate_restaurant(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RestaurantInbox::MarkRestaurantClosed(cmd) => run(async { application::commands::mark_restaurant_closed(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RestaurantInbox::OptOutRestaurantListing(cmd) => run(async { application::commands::opt_out_restaurant_listing(deps.store.as_ref(), deps.ownership.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RestaurantInbox::RegisterRestaurant(cmd) => run(async { application::commands::register_restaurant(deps.store.as_ref(), deps.restaurants.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RestaurantInbox::RemoveRestaurant(cmd) => run(async { application::commands::remove_restaurant(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RestaurantInbox::RestaurantRegistered(_) => InboxOutcome::RecordFact,
        RestaurantInbox::UpdateRestaurant(cmd) => run(async { application::commands::update_restaurant(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RestaurantInbox::UpdateRestaurantGoogleBusinessProfile(cmd) => run(async { application::commands::update_restaurant_google_business_profile(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RestaurantInbox::VerifyGoogleBusinessProfileOrderLink(cmd) => run(async { application::commands::verify_gbp_order_link(deps.store.as_ref(), deps.probe.as_ref(), cmd, actor).await.map(|_| ()) }).await,
    }
}

/// The `RestaurantAccount` lane.
async fn restaurant_account(
    deps: &CommandDeps,
    message: RestaurantAccountInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        RestaurantAccountInbox::DeleteRestaurantAccount(cmd) => run(async { application::commands::delete_restaurant_account(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RestaurantAccountInbox::RegisterRestaurantAccount(cmd) => run(async { application::commands::register_restaurant_account(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RestaurantAccountInbox::UpdateRestaurantAccount(cmd) => run(async { application::commands::update_restaurant_account(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
    }
}

/// The `Rider` lane.
async fn rider(
    deps: &CommandDeps,
    message: RiderInbox,
    actor: &Actor,
    env: &RouterEnv,
) -> InboxOutcome {
    let _ = (deps, actor, env);
    match message {
        RiderInbox::ChangeRiderStatus(cmd) => run(async { application::commands::change_rider_status(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RiderInbox::RegisterRider(cmd) => run(async { application::commands::register_rider(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
        RiderInbox::UpdateRiderInfo(cmd) => run(async { application::commands::update_rider_info(deps.store.as_ref(), cmd, actor).await.map(|_| ()) }).await,
    }
}

// ─── THE FACT-RECORD ROUTE (#780) ────────────────────────────────────────────────────────────────

/// What one delivered FACT turns out to BE. Same split as [`InboxOutcome`] and for the same reason:
/// **the decision is a pure total function of the typed fact**, so it is testable without a
/// database, while the effects that need a `tx` stay in `mailbox::handler` where the fencing
/// contract lives.
///
/// Before #780 that decision was a `match` over `DomainEvent` INSIDE the transaction, ending in
/// `_ => Failed("no delivery route for inbound fact type ...")`. Twelve declared facts fell into
/// that arm. A fact reaching it was not appended late — it was LOST, with a terminal verdict, and
/// with `make validate` and `cargo test` both green.
pub enum FactLeg {
    /// Record the carried fact on the addressed aggregate's OWN stream, through the recorder the
    /// leg NAMES BY CARRYING ITS PAYLOAD ([`RecordLeg`]).
    /// One arm, one stream, one transaction — no leg here may reach a second aggregate.
    Record(RecordLeg),
    /// The lane IS a process manager: run its typed event leg, not the record route. The fact is
    /// already on the aggregate's stream; this hop REACTS to it (B2's chained PM-addressed copy).
    ProcessManager(PmFactLeg),
    /// DECLARED received, and the aggregate has NO fold rule for it — so there is no idempotency
    /// anchor and a redelivery would append a second copy. The delivery PARKS (see
    /// [`UnrecordedFact`]); it is never terminally failed, because a fact cannot be refused.
    Unrecorded(UnrecordedFact),
}

/// **WHICH RECORDER OWNS THE APPEND, CARRYING THE PAYLOAD THAT RECORDER TAKES.**
///
/// The shape PR #783's review earned (B1). The first cut of this route was
/// `Record { recorder: FactRecorder, event: DomainEvent }` — a NAME beside an untyped payload — and
/// the pairing was checked by nobody: the Payment arm named
/// `application::payments::record_inbound_payment_fact` in its doc comment while the handler fed the
/// `DomainEvent` to the untyped `record_inbound_payment_event`, whose stream lookup covered only
/// five of the lane's ten declared facts. `RefundOpened` — the sole feeder of `View_PendingRefunds`
/// — did not record; it aborted, retried, and wedged the money lane until the attempts cap. The
/// typed door existed and was dead code, and the whole point of #780 is that a fact with nowhere to
/// go must be a COMPILE error rather than a runtime surprise.
///
/// A struct with two independent fields cannot express "this recorder takes THIS payload"; a sum
/// type can, so the mismatch is now UNSPELLABLE rather than merely absent (CLAUDE.md
/// "compiler first"). The Payment lane carries its generated lane enum; the remaining recorders
/// still take an untyped [`domain::generated::events::DomainEvent`], and typing each of them is a
/// change to ONE variant here plus its recorder's signature — the same move, lane by lane.
pub enum RecordLeg {
    /// **THE MONEY PATH, TYPED.** `application::payments::record_inbound_payment_fact` —
    /// `Payment-{intentId}`, with the stream resolved by `payments::intent_of_fact`, which is total
    /// over `PaymentFactInbox`. An eleventh declared Payment fact is an E0004 there, at the place
    /// that knows how to find its stream.
    Payment(PaymentFactInbox),
    /// `application::deliveries::record_inbound_delivery_event` — `DeliveryJob-{id}`.
    Delivery(domain::generated::events::DomainEvent),
    /// `application::commands::record_inbound_restaurant_registration` — `Restaurant-{id}`.
    RestaurantRegistration(domain::generated::events::DomainEvent),
    /// `application::commands::record_inbound_order_event` — `Order-{id}`.
    Order(domain::generated::events::DomainEvent),
    /// `application::commands::record_inbound_order_placed` — the Order BIRTH (#167).
    OrderPlaced(domain::generated::events::DomainEvent),
    /// `application::commands::record_order_acceptance_timeout` — its own route because its
    /// outcome is richer than `RecordOutcome` and its `schedules:` apply on one arm only (#167).
    OrderAcceptanceTimeout(domain::generated::events::DomainEvent),
}

impl RecordLeg {
    /// The payload-free NAME of the recorder — for the verdict table, which asserts the return
    /// SHAPE and must not be handed a money payload to compare.
    pub fn recorder(&self) -> FactRecorder {
        match self {
            Self::Payment(_) => FactRecorder::Payment,
            Self::Delivery(_) => FactRecorder::Delivery,
            Self::RestaurantRegistration(_) => FactRecorder::RestaurantRegistration,
            Self::Order(_) => FactRecorder::Order,
            Self::OrderPlaced(_) => FactRecorder::OrderPlaced,
            Self::OrderAcceptanceTimeout(_) => FactRecorder::OrderAcceptanceTimeout,
        }
    }

    /// The carried fact as an untyped `DomainEvent`, for the effects that are EVENT-shaped rather
    /// than lane-shaped — the post-commit PM-addressed copy (`chain_pm_copy_in_tx`), which routes
    /// on the event, not on the recording lane. A projection, never the recording path: the
    /// recorders themselves take the typed value where their lane has one.
    pub fn to_domain_event(&self) -> domain::generated::events::DomainEvent {
        match self {
            Self::Payment(fact) => fact.clone().into_domain_event(),
            Self::Delivery(e)
            | Self::RestaurantRegistration(e)
            | Self::Order(e)
            | Self::OrderPlaced(e)
            | Self::OrderAcceptanceTimeout(e) => e.clone(),
        }
    }
}

/// Which recorder owns the append. A closed, human-owned set: the handler performs, this names.
///
/// It carries no payload — it is [`RecordLeg::recorder`]'s codomain, the class key the verdict
/// table compares. The exhaustiveness that matters is over the FACT enums above, never over this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactRecorder {
    /// `application::payments::record_inbound_payment_fact` — `Payment-{intentId}`.
    Payment,
    /// `application::deliveries::record_inbound_delivery_event` — `DeliveryJob-{id}`.
    Delivery,
    /// `application::commands::record_inbound_restaurant_registration` — `Restaurant-{id}`.
    RestaurantRegistration,
    /// `application::commands::record_inbound_order_event` — `Order-{id}`.
    Order,
    /// `application::commands::record_inbound_order_placed` — the Order BIRTH (#167).
    OrderPlaced,
    /// `application::commands::record_order_acceptance_timeout` — its own route because its
    /// outcome is richer than `RecordOutcome` and its `schedules:` apply on one arm only (#167).
    OrderAcceptanceTimeout,
}

/// A process manager's typed EVENT leg. Typed rather than a `(actor_type, DomainEvent)` string
/// pair: the previous shape ended in `(actor, _) => Failed("no PM event leg")`, a catch-all on the
/// money path that no gate could see (`mailbox::handler`'s file is not scanned by the router gate).
pub enum PmFactLeg {
    PlaceOrderOnPaymentAuthorized(domain::generated::events::PaymentAuthorized),
    PlaceOrderOnPaymentFailed(domain::generated::events::PaymentFailed),
    RefundOnPaymentRefunded(domain::generated::events::PaymentRefunded),
}

/// A DECLARED fact the receiving aggregate has no fold rule for.
///
/// **THE CRITERION, and it is a modelling statement rather than a schedule** (evans, #780 briefing:
/// a `deferred:` whose reason is *"lands in C3"* is drift). Every fact routed to a recorder above
/// has a rule in its aggregate's own fold that answers *"is this re-delivered fact already
/// reflected?"* — `domain::payment::already_records`, the DeliveryJob lifecycle's transition
/// table, `record_inbound_order_*`'s status guards. Each of the seven below has none. Without that
/// rule there is no dedupe, and recording the fact would mean a redelivery appends a SECOND copy —
/// so the honest statement is not "no handler yet", it is **"this aggregate does not yet model
/// this fact"**. The fold rule is what a future route move must add FIRST.
///
/// Every member is declared in `specs/*/actors.yaml` as `deferred: { reason, issue }`, and the two
/// sides are held equal in both directions by
/// `codegen tests::fact_route_gate::every_unrecorded_arm_is_a_declared_deferral` — closing, for the
/// fact half, the "nothing binds a `Deferred` ARM to its declaration" gap of
/// [#781](https://github.com/TheCaptainCompany/captain-food/issues/781).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnrecordedFact {
    pub actor_type: &'static str,
    pub message_type: &'static str,
}

impl FactLeg {
    /// The leg's CLASS, for the verdict table — a total projection with no payload, so the table
    /// asserts the return SHAPE rather than any message text.
    pub fn class(&self) -> FactLegClass {
        match self {
            Self::Record(r) => FactLegClass::Record(r.recorder()),
            Self::ProcessManager(_) => FactLegClass::ProcessManager,
            Self::Unrecorded(u) => FactLegClass::Unrecorded(*u),
        }
    }
}

/// [`FactLeg`] without its payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactLegClass {
    Record(FactRecorder),
    ProcessManager,
    Unrecorded(UnrecordedFact),
}

/// THE FACT ROUTE. Pure, total, transaction-free — the twin of [`route`] for the EVENT/MESSAGE door.
///
/// It matches on [`ActorFactInbox`], not on `ActorInbox` and not on `DomainEvent`. That is the
/// whole design: over the fact enums a COMMAND variant is unspellable, so no arm ever needs to say
/// "not a fact" and **no lane wildcard is ever wanted**. The obvious alternative — a match over the
/// composite with lane arms — would have made `ActorInbox::Payment(_) => Failed("no route")` both
/// compilable and gate-clean while absorbing every message the Payment lane can ever carry.
///
/// The same two rules as [`route`] bind here: never a catch-all, and a fact you are not ready to
/// record is [`FactLeg::Unrecorded`] with its `deferred:` declaration in the DSL.
pub fn fact_route(fact: ActorFactInbox) -> FactLeg {
    match fact {
        ActorFactInbox::Cart(m) => cart_fact(m),
        ActorFactInbox::Catalog(m) => catalog_fact(m),
        ActorFactInbox::Customer(m) => customer_fact(m),
        ActorFactInbox::DeliveryJob(m) => delivery_job_fact(m),
        ActorFactInbox::Order(m) => order_fact(m),
        ActorFactInbox::Payment(m) => payment_fact(m),
        ActorFactInbox::PlaceOrderProcess(m) => place_order_process_fact(m),
        ActorFactInbox::RefundProcess(m) => refund_process_fact(m),
        ActorFactInbox::Restaurant(m) => restaurant_fact(m),
    }
}

/// A parked fact, named by its own lane and variant so the two sides of the deferral cannot drift.
fn unrecorded(actor_type: &'static str, message_type: &'static str) -> FactLeg {
    FactLeg::Unrecorded(UnrecordedFact { actor_type, message_type })
}

/// The `Cart` lane's facts.
fn cart_fact(message: CartFactInbox) -> FactLeg {
    match message {
        // The Cart fold has no `checkedOut` rule, so a redelivered checkout would append twice.
        CartFactInbox::CartCheckedOut(_) => {
            unrecorded(CartFactInbox::ACTOR_TYPE, "CartCheckedOut")
        }
    }
}

/// The `Catalog` lane's facts.
fn catalog_fact(message: CatalogFactInbox) -> FactLeg {
    match message {
        // Stock is a DERIVED status over the catalog fold (`StockStatus`), and the fold has no rule
        // for an externally reported stock level — availability, stock and orderability are three
        // different things and which one this fact moves is not modelled.
        CatalogFactInbox::OfferStockUpdated(_) => {
            unrecorded(CatalogFactInbox::ACTOR_TYPE, "OfferStockUpdated")
        }
    }
}

/// The `Customer` lane's facts.
fn customer_fact(message: CustomerFactInbox) -> FactLeg {
    match message {
        // The GDPR erasure clock. The Customer fold has no `erasureDue` rule, so nothing answers
        // "already reflected?" and a redelivery would append a second due-date. PARKED, never
        // terminal: this reminder is the ONLY copy of a legal deadline and a terminal verdict on it
        // is a deadline dropped on the floor (young).
        CustomerFactInbox::CustomerErasureDue(_) => {
            unrecorded(CustomerFactInbox::ACTOR_TYPE, "CustomerErasureDue")
        }
        // Identity is Supabase-side and deliberately NOT business data (CLAUDE.md: the auth wrapper
        // is identity-only). Whether the Customer aggregate records an unlink at all is the open
        // question; it has no fold rule for one.
        CustomerFactInbox::CustomerIdentityUnlinked(_) => {
            unrecorded(CustomerFactInbox::ACTOR_TYPE, "CustomerIdentityUnlinked")
        }
    }
}

/// The `DeliveryJob` lane's facts. The three PARTNER facts are recorded through the lifecycle
/// machine; the three the platform emits itself have no transition declared for them.
fn delivery_job_fact(message: DeliveryJobFactInbox) -> FactLeg {
    use domain::generated::events::DomainEvent as E;
    match message {
        DeliveryJobFactInbox::DeliveryAcceptedByPartner(e) => {
            FactLeg::Record(RecordLeg::Delivery(E::DeliveryAcceptedByPartner(e)))
        }
        DeliveryJobFactInbox::DeliveryRejectedByPartner(e) => {
            FactLeg::Record(RecordLeg::Delivery(E::DeliveryRejectedByPartner(e)))
        }
        DeliveryJobFactInbox::DeliveryStatusUpdated(e) => {
            FactLeg::Record(RecordLeg::Delivery(E::DeliveryStatusUpdated(e)))
        }
        // The `PENDING -> FAILED` transition IS declared (specs/delivery/actors.yaml), but no
        // "already reflected" rule is: on redelivery the job is already FAILED, the transition
        // lookup is `None`, and `record_inbound_delivery_event` turns that into a retry loop
        // instead of a DUPLICATE. Same statement as the DSL `deferred:` reason, which is the side
        // the gate reads — this comment is the side nothing checks, so it is kept literally equal
        // (PR #783 review N1: it used to claim the transition was missing, which is false).
        DeliveryJobFactInbox::DeliveryDispatchFailed(_) => {
            unrecorded(DeliveryJobFactInbox::ACTOR_TYPE, "DeliveryDispatchFailed")
        }
        // Same: an expired OFFER is not a job-status transition, and the fold has no rule for it.
        // PARKED rather than terminal — the platform emits this itself, so there is no second copy.
        DeliveryJobFactInbox::DeliveryOfferTimedOut(_) => {
            unrecorded(DeliveryJobFactInbox::ACTOR_TYPE, "DeliveryOfferTimedOut")
        }
        // The job's BIRTH. `record_inbound_delivery_event` resolves the stream from a job id the
        // fold already holds, which a birth by definition has not established; whether the
        // DeliveryJob is born BY this delivered fact is the aggregate-boundary question, unmodelled.
        DeliveryJobFactInbox::DeliveryRequested(_) => {
            unrecorded(DeliveryJobFactInbox::ACTOR_TYPE, "DeliveryRequested")
        }
    }
}

/// The `Order` lane's facts.
fn order_fact(message: OrderFactInbox) -> FactLeg {
    use domain::generated::events::DomainEvent as E;
    match message {
        // The promoted acceptance deadline (#167): its own recorder because its outcome is richer
        // than `RecordOutcome` (the shadow WouldCancel arm is the flip ADR's evidence) and because
        // its `schedules:` apply on the Recorded/Cancelled arm ONLY.
        OrderFactInbox::OrderAcceptanceTimedOut(e) => {
            FactLeg::Record(RecordLeg::OrderAcceptanceTimeout(E::OrderAcceptanceTimedOut(e)))
        }
        // The promoted GDPR retention deadline (ADR-20260731-153000).
        OrderFactInbox::OrderExpired(e) => FactLeg::Record(RecordLeg::Order(E::OrderExpired(e))),
        // The Order BIRTH as a mailbox delivery (#167/#588) — the one routed `deliver:` in the
        // corpus, and the reason `ROUTED_LANES` and this route are gated against each other.
        OrderFactInbox::OrderPlaced(e) => {
            FactLeg::Record(RecordLeg::OrderPlaced(E::OrderPlaced(e)))
        }
    }
}

/// The `Payment` lane's facts — the money path. Every one records, and every one dedupes through
/// `domain::payment::already_records`, which already carries a rule for all ten: a redelivered
/// capture failure lands DUPLICATE instead of appending a second money event (young).
///
/// **THE LANE VALUE TRAVELS TYPED** (PR #783 review B1). Each arm hands the recorder its
/// `PaymentFactInbox`, not a widened `DomainEvent`, so the stream is resolved by
/// `payments::intent_of_fact` — total over this enum — rather than by a `DomainEvent` match ending
/// in `_ => None`. The first cut widened here and the widening was the whole defect: five of these
/// ten (`PaymentCaptureFailed`, `PaymentIntentCreated`, `RefundApproved`, `RefundDenied`,
/// `RefundOpened`) fell into that wildcard, so they did not record — they aborted and retried until
/// the attempts cap, wedging the money lane head-of-line, while the typed door that would have
/// recorded them sat unreferenced.
fn payment_fact(message: PaymentFactInbox) -> FactLeg {
    // TEN ARMS, NOT `FactLeg::Record(RecordLeg::Payment(message))`. A blanket leg over the whole
    // lane would be a lane-level catch-all wearing a different hat: a new declared Payment fact
    // would be routed to the money-path recorder with no human deciding it should be, which is #780
    // exactly. So each arm re-states its variant, and an eleventh fact is an E0004 here as well as
    // in `intent_of_fact`.
    match message {
        PaymentFactInbox::PaymentAuthorized(e) => {
            record_payment(PaymentFactInbox::PaymentAuthorized(e))
        }
        // Two authors on `Payment-{intentId}`: `PaymentSettlementProcess` records it in-process
        // today, and this lane records it when the route moves. Both go through
        // `already_records` (`state.capture_failed`), so the second copy is absorbed as DUPLICATE
        // rather than double-counted by every downstream fold (young).
        PaymentFactInbox::PaymentCaptureFailed(e) => {
            record_payment(PaymentFactInbox::PaymentCaptureFailed(e))
        }
        PaymentFactInbox::PaymentCaptured(e) => {
            record_payment(PaymentFactInbox::PaymentCaptured(e))
        }
        PaymentFactInbox::PaymentFailed(e) => record_payment(PaymentFactInbox::PaymentFailed(e)),
        // The stream's BIRTH. `already_records` answers `true` whenever a fold exists, and a
        // birthless stream falls back to structural equality — so a redelivered birth is DUPLICATE.
        PaymentFactInbox::PaymentIntentCreated(e) => {
            record_payment(PaymentFactInbox::PaymentIntentCreated(e))
        }
        PaymentFactInbox::PaymentRefunded(e) => {
            record_payment(PaymentFactInbox::PaymentRefunded(e))
        }
        PaymentFactInbox::PaymentReleased(e) => {
            record_payment(PaymentFactInbox::PaymentReleased(e))
        }
        PaymentFactInbox::RefundApproved(e) => {
            record_payment(PaymentFactInbox::RefundApproved(e))
        }
        PaymentFactInbox::RefundDenied(e) => record_payment(PaymentFactInbox::RefundDenied(e)),
        // `View_PendingRefunds` is folded from THIS fact and from nothing else
        // (`specs/database/projection_views.yaml`): losing it means the restaurant is never asked
        // to decide and captured money stays captured. It was one of the five that did not record.
        PaymentFactInbox::RefundOpened(e) => record_payment(PaymentFactInbox::RefundOpened(e)),
    }
}

/// The money lane's record leg — named once so the ten arms above read as ten DECISIONS rather than
/// ten spellings of the same constructor.
fn record_payment(fact: PaymentFactInbox) -> FactLeg {
    FactLeg::Record(RecordLeg::Payment(fact))
}

/// The `PlaceOrderProcess` lane's facts — the saga's own event legs, never the record route.
fn place_order_process_fact(message: PlaceOrderProcessFactInbox) -> FactLeg {
    match message {
        PlaceOrderProcessFactInbox::PaymentAuthorized(e) => {
            FactLeg::ProcessManager(PmFactLeg::PlaceOrderOnPaymentAuthorized(e))
        }
        PlaceOrderProcessFactInbox::PaymentFailed(e) => {
            FactLeg::ProcessManager(PmFactLeg::PlaceOrderOnPaymentFailed(e))
        }
    }
}

/// The `RefundProcess` lane's facts.
fn refund_process_fact(message: RefundProcessFactInbox) -> FactLeg {
    match message {
        RefundProcessFactInbox::PaymentRefunded(e) => {
            FactLeg::ProcessManager(PmFactLeg::RefundOnPaymentRefunded(e))
        }
    }
}

/// The `Restaurant` lane's facts.
fn restaurant_fact(message: RestaurantFactInbox) -> FactLeg {
    use domain::generated::events::DomainEvent as E;
    match message {
        RestaurantFactInbox::RestaurantRegistered(e) => {
            FactLeg::Record(RecordLeg::RestaurantRegistration(E::RestaurantRegistered(e)))
        }
    }
}
