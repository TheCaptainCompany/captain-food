//! Hand-written `ScopeMembership` projector (#144, PROP-20260725-185140 §3.4).
//!
//! Unlike every other read model, this one is **set-shaped**: one event yields N row changes, not one
//! row. `OrderPlaced` grants three memberships (customer, restaurant, account); `DeliveryCancelled`
//! revokes however many rider rows the order carries. That is why the generated
//! `project_<table>(state: Option<Row>) -> Option<Row>` dispatch cannot express it (`emit_projectors`
//! skips the table) and this fold is hand-written instead.
//!
//! The fold stays **pure**: two resolutions need I/O the application layer must not do, so the
//! infrastructure worker performs them and passes the answers in via [`Resolved`]:
//!   * `Delivery*` events carry only a `deliveryJobId` — the order must be looked up;
//!   * `OrderPlaced` carries no `restaurantAccountId` — the owning account must be looked up.
//!
//! SAFETY (the asymmetry that drives every decision here): a MISSING row denies — visible and safe.
//! A STALE row grants — a silent breach. So revokes are deliberately **broad** (drop every rider on
//! the order, not "the rider named in this event") and grants are deliberately **narrow**.

use domain::generated::events::DomainEvent;
use domain::generated::scalars::{ScopeType, UserType};
use uuid::Uuid;

use crate::projections::Envelope;

/// The DNS-namespace-derived constant this projector hashes membership keys under. Fixed forever:
/// changing it would re-key every existing row and silently orphan the old ones.
const MEMBERSHIP_NAMESPACE: Uuid = Uuid::from_u128(0x6ca4_1f0e_9b27_4d55_a3e1_7c88_5b02_9df4);

/// One change the projector wants applied. Grants are per-principal; revokes drop a whole ROLE on a
/// scope rather than a named principal — see the module note on the missing-vs-stale asymmetry.
#[derive(Debug, Clone, PartialEq)]
pub enum MembershipChange {
    Grant { scope_type: ScopeType, scope_id: Uuid, principal_type: UserType, principal_id: Uuid },
    RevokeRole { scope_type: ScopeType, scope_id: Uuid, principal_type: UserType },
}

/// Lookups the infrastructure worker resolves before folding (the pure layer cannot do I/O).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Resolved {
    /// The order a `Delivery*` event's job belongs to. `None` when the job is unknown.
    pub order_id: Option<Uuid>,
    /// The account owning the restaurant named by the event. `None` when unresolved.
    pub restaurant_account_id: Option<Uuid>,
}

/// `membership_id` — UUIDv5 over the natural key, so the same membership always derives the same id.
///
/// This is what buys idempotent re-projection (a replayed grant upserts onto itself) and lookup-free
/// revocation, and it is why the table needs no composite primary key — which the DSL cannot express
/// anyway (the emitter writes `PRIMARY KEY` inline per column). Same trick as
/// `hubrise_connections.restaurant_account_id`.
pub fn membership_id(
    scope_type: ScopeType,
    scope_id: Uuid,
    principal_type: UserType,
    principal_id: Uuid,
) -> Uuid {
    let key = format!("{scope_type:?}|{scope_id}|{principal_type:?}|{principal_id}");
    Uuid::new_v5(&MEMBERSHIP_NAMESPACE, key.as_bytes())
}

/// Derive the membership changes one event implies. An event with no authorization meaning yields an
/// empty vec — the overwhelmingly common case, so this stays cheap on the hot projection path.
pub fn membership_changes(env: &Envelope, resolved: &Resolved) -> Vec<MembershipChange> {
    match &env.event {
        // An order's participants at birth. The rider is NOT here — it is granted on acceptance,
        // because at placement time no rider exists yet.
        DomainEvent::OrderPlaced(e) => {
            let order_id = e.order_id.0;
            let mut out = Vec::with_capacity(3);

            // `customer_id` is optional, so an order may carry no Captain customer at all. This is
            // NOT guest checkout — the product requires a verified phone at checkout, which creates
            // the Customer — it is the externally-originated order. No Captain identity, nothing to
            // grant; the restaurant grant below still applies.
            if let Some(c) = &e.customer_id {
                out.push(MembershipChange::Grant {
                    scope_type: ScopeType::ORDER,
                    scope_id: order_id,
                    principal_type: UserType::CUSTOMER,
                    principal_id: c.0,
                });
            }
            out.push(MembershipChange::Grant {
                scope_type: ScopeType::ORDER,
                scope_id: order_id,
                principal_type: UserType::RESTAURANT,
                principal_id: e.restaurant_id.0,
            });
            if let Some(account_id) = resolved.restaurant_account_id {
                out.push(MembershipChange::Grant {
                    scope_type: ScopeType::ORDER,
                    scope_id: order_id,
                    principal_type: UserType::RESTAURANT_ACCOUNT,
                    principal_id: account_id,
                });
            }
            out
        }

        // A rider took the job: grant that rider on the job's order. The event carries only
        // `deliveryJobId`, so the order arrives pre-resolved.
        DomainEvent::DeliveryAcceptedByRider(e) => match resolved.order_id {
            Some(order_id) => vec![MembershipChange::Grant {
                scope_type: ScopeType::ORDER,
                scope_id: order_id,
                principal_type: UserType::RIDER,
                principal_id: e.rider_id.0,
            }],
            None => Vec::new(),
        },

        // The job ended without delivery: no rider should retain access. Revoking the ROLE rather
        // than a named rider is deliberate — if reassignment left two rider rows on the order, a
        // targeted revoke would strip one and leave the other, which is the stale-grant breach.
        DomainEvent::DeliveryCancelled(_) | DomainEvent::DeliveryDispatchFailed(_) => {
            match resolved.order_id {
                Some(order_id) => vec![MembershipChange::RevokeRole {
                    scope_type: ScopeType::ORDER,
                    scope_id: order_id,
                    principal_type: UserType::RIDER,
                }],
                None => Vec::new(),
            }
        }

        // A restaurant's own scope (back-office reads, restaurant-owned documents): the location
        // itself and its owning account.
        DomainEvent::RestaurantRegistered(e) => {
            let restaurant_id = e.restaurant_id.0;
            let mut out = vec![MembershipChange::Grant {
                scope_type: ScopeType::RESTAURANT,
                scope_id: restaurant_id,
                principal_type: UserType::RESTAURANT,
                principal_id: restaurant_id,
            }];
            // Unlike OrderPlaced, this event CARRIES its owning account — no lookup needed.
            if let Some(account_id) = e.account_id.as_ref().map(|a| a.0) {
                out.push(MembershipChange::Grant {
                    scope_type: ScopeType::RESTAURANT,
                    scope_id: restaurant_id,
                    principal_type: UserType::RESTAURANT_ACCOUNT,
                    principal_id: account_id,
                });
            }
            out
        }

        // ADMIN holds no rows at all — the guard short-circuits on the role. Storing them would mean
        // a row per admin per instance, unbounded and pointless.
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::generated::events::{
        DeliveryAcceptedByRider, DeliveryCancelled, OrderPlaced,
    };
    use domain::generated::scalars::{CustomerId, DeliveryJobId, OrderId, RestaurantId, RiderId};

    fn env(event: DomainEvent) -> Envelope {
        Envelope {
            stream_name: "Order-x".into(),
            position: 1,
            occurred_at: chrono::Utc::now(),
            event,
        }
    }

    /// The breakdown is required in full by serde; its values are irrelevant to authorization.
    fn money() -> serde_json::Value {
        serde_json::json!({ "amountCents": 0, "currency": "EUR" })
    }

    fn order_placed(order: Uuid, restaurant: Uuid, customer: Option<Uuid>) -> DomainEvent {
        let mut e: OrderPlaced = serde_json::from_value(serde_json::json!({
            "orderId": order,
            "restaurantId": restaurant,
            "customerContact": { "displayName": "T", "phone": "+33600000000" },
            "serviceType": "DELIVERY",
            "items": [],
            "totalAmount": money(),
            "breakdown": {
                "articles": money(), "delivery": money(), "serviceFee": money(), "total": money(),
                "restaurantContribution": money(), "restaurantPayout": money(),
                "riderPayout": money(), "captainNet": money(),
            },
            "paymentIntentId": "pi_test",
        }))
        .expect("OrderPlaced fixture");
        e.customer_id = customer.map(CustomerId);
        DomainEvent::OrderPlaced(e)
    }

    const ORDER: Uuid = Uuid::from_u128(1);
    const RESTAURANT: Uuid = Uuid::from_u128(2);
    const CUSTOMER: Uuid = Uuid::from_u128(3);
    const ACCOUNT: Uuid = Uuid::from_u128(4);
    const RIDER_A: Uuid = Uuid::from_u128(5);
    const RIDER_B: Uuid = Uuid::from_u128(6);

    /// The membership key must be a pure function of the natural key — this is what makes a replayed
    /// grant an idempotent upsert instead of a duplicate row.
    #[test]
    fn membership_id_is_deterministic_and_distinguishes_every_part() {
        let base = membership_id(ScopeType::ORDER, ORDER, UserType::CUSTOMER, CUSTOMER);
        assert_eq!(base, membership_id(ScopeType::ORDER, ORDER, UserType::CUSTOMER, CUSTOMER));
        // Each component participates: changing any one yields a different key.
        assert_ne!(base, membership_id(ScopeType::RESTAURANT, ORDER, UserType::CUSTOMER, CUSTOMER));
        assert_ne!(base, membership_id(ScopeType::ORDER, RESTAURANT, UserType::CUSTOMER, CUSTOMER));
        assert_ne!(base, membership_id(ScopeType::ORDER, ORDER, UserType::RIDER, CUSTOMER));
        assert_ne!(base, membership_id(ScopeType::ORDER, ORDER, UserType::CUSTOMER, RESTAURANT));
    }

    /// A principal who is BOTH a customer and a rider must hold two distinct memberships, or their
    /// customer row would let them fetch rider-audience data. This is why principal_type is in the key.
    #[test]
    fn same_person_as_customer_and_rider_gets_distinct_memberships() {
        let person = Uuid::from_u128(42);
        assert_ne!(
            membership_id(ScopeType::ORDER, ORDER, UserType::CUSTOMER, person),
            membership_id(ScopeType::ORDER, ORDER, UserType::RIDER, person),
        );
    }

    #[test]
    fn order_placed_grants_customer_restaurant_and_account() {
        let changes = membership_changes(
            &env(order_placed(ORDER, RESTAURANT, Some(CUSTOMER))),
            &Resolved { order_id: None, restaurant_account_id: Some(ACCOUNT) },
        );
        assert_eq!(changes.len(), 3);
        assert!(changes.contains(&MembershipChange::Grant {
            scope_type: ScopeType::ORDER,
            scope_id: ORDER,
            principal_type: UserType::CUSTOMER,
            principal_id: CUSTOMER,
        }));
        assert!(changes.contains(&MembershipChange::Grant {
            scope_type: ScopeType::ORDER,
            scope_id: ORDER,
            principal_type: UserType::RESTAURANT,
            principal_id: RESTAURANT,
        }));
        assert!(changes.contains(&MembershipChange::Grant {
            scope_type: ScopeType::ORDER,
            scope_id: ORDER,
            principal_type: UserType::RESTAURANT_ACCOUNT,
            principal_id: ACCOUNT,
        }));
    }

    /// An order with NO Captain customer. Not guest checkout — the product spec requires a verified
    /// phone at checkout (PRODUCT_SPEC_WEB_CLIENT.md §3.5), which creates the Customer, so the Captain
    /// flow always has one. `customerId` is optional for orders that originate OUTSIDE that flow
    /// (externally-channeled / imported). Granting no CUSTOMER membership there is correct, not a gap:
    /// someone who ordered through the restaurant's own channel has no Captain identity to grant to.
    /// The order still grants the restaurant — an unidentified order is not an unowned one.
    #[test]
    fn order_without_a_captain_customer_grants_no_customer_membership() {
        let changes = membership_changes(
            &env(order_placed(ORDER, RESTAURANT, None)),
            &Resolved { order_id: None, restaurant_account_id: None },
        );
        assert!(!changes
            .iter()
            .any(|c| matches!(c, MembershipChange::Grant { principal_type: UserType::CUSTOMER, .. })));
        assert_eq!(changes.len(), 1, "only the restaurant is granted");
    }

    /// THE REASSIGNMENT PAIR. Asserting only that the new rider is granted would pass against a
    /// broken rule — the previous rider losing access is the half that matters.
    #[test]
    fn reassignment_grants_the_new_rider_and_revokes_the_previous_one() {
        let accept = |rider: Uuid| {
            let e: DeliveryAcceptedByRider = serde_json::from_value(serde_json::json!({
                "deliveryJobId": Uuid::from_u128(99),
                "riderId": rider,
            }))
            .expect("DeliveryAcceptedByRider fixture");
            env(DomainEvent::DeliveryAcceptedByRider(e))
        };
        let resolved = Resolved { order_id: Some(ORDER), restaurant_account_id: None };

        // Rider A takes the job.
        assert_eq!(
            membership_changes(&accept(RIDER_A), &resolved),
            vec![MembershipChange::Grant {
                scope_type: ScopeType::ORDER,
                scope_id: ORDER,
                principal_type: UserType::RIDER,
                principal_id: RIDER_A,
            }]
        );

        // The job is cancelled — EVERY rider on the order loses access, not just a named one.
        let cancelled: DeliveryCancelled = serde_json::from_value(serde_json::json!({
            "deliveryJobId": Uuid::from_u128(99),
            "reason": "bike broke",
        }))
        .expect("DeliveryCancelled fixture");
        assert_eq!(
            membership_changes(&env(DomainEvent::DeliveryCancelled(cancelled)), &resolved),
            vec![MembershipChange::RevokeRole {
                scope_type: ScopeType::ORDER,
                scope_id: ORDER,
                principal_type: UserType::RIDER,
            }],
            "a targeted revoke would strip one rider and leave the other — the stale-grant breach"
        );

        // Rider B takes the replacement job.
        assert_eq!(
            membership_changes(&accept(RIDER_B), &resolved),
            vec![MembershipChange::Grant {
                scope_type: ScopeType::ORDER,
                scope_id: ORDER,
                principal_type: UserType::RIDER,
                principal_id: RIDER_B,
            }]
        );
    }

    /// A delivery event whose job cannot be resolved to an order yields NOTHING. Missing rows deny,
    /// which is the safe direction — inventing a scope id would grant against the wrong order.
    #[test]
    fn unresolved_delivery_job_yields_no_changes() {
        let e: DeliveryAcceptedByRider = serde_json::from_value(serde_json::json!({
            "deliveryJobId": Uuid::from_u128(99),
            "riderId": RIDER_A,
        }))
        .expect("fixture");
        assert!(membership_changes(
            &env(DomainEvent::DeliveryAcceptedByRider(e)),
            &Resolved::default()
        )
        .is_empty());
    }

    /// The overwhelmingly common case on the projection hot path: an event with no authorization
    /// meaning contributes nothing.
    #[test]
    fn unrelated_events_yield_no_changes() {
        let e: DeliveryCancelled = serde_json::from_value(serde_json::json!({
            "deliveryJobId": Uuid::from_u128(99),
            "reason": "x",
        }))
        .expect("fixture");
        // Same event, but with no resolvable order -> still nothing.
        assert!(
            membership_changes(&env(DomainEvent::DeliveryCancelled(e)), &Resolved::default())
                .is_empty()
        );
    }
}
