//! Hand-written `ScopeMembership` projector (#144, PROP-20260725-185140 §3.4).
//!
//! Unlike every other read model, this one is **set-shaped**: one event yields N row changes, not one
//! row. `OrderPlaced` grants three memberships (customer, restaurant, account); `DeliveryCancelled`
//! revokes however many rider rows the order carries. That is why the generated
//! `project_<table>(state: Option<Row>) -> Option<Row>` dispatch cannot express it (`emit_projectors`
//! skips a table whose pk lineage carries no property path) and this fold is hand-written instead.
//!
//! The fold stays **pure**: `DeliveryCancelled` carries only a `deliveryJobId` (unlike
//! `DeliveryAcceptedByRider`/`DeliveryDispatchFailed`, which carry `orderId` since D-QW1 option b,
//! ADR-20260808-234907), so the infrastructure worker resolves its order and passes the answer in via
//! [`Resolved`]. Same for `OrderPlaced`'s owning restaurant account, which no order event carries.
//!
//! SAFETY (the asymmetry that drives every decision here): a MISSING row denies — visible and safe.
//! A STALE row grants — a silent breach. So revokes are deliberately **broad** (drop every rider on
//! the order, not "the rider named in this event") and grants are deliberately **narrow**.
//!
//! ERASURE (#194, ADR-20260731-160000): this is an Order-fed read model holding a customer-to-order
//! link, so it OWES an `OrderExpired` tombstone fold (delete the order scope's rows) when the deletion
//! engine lands — named in the table spec's rules so the #194 sweep cannot miss an app-projected table
//! the generated dispatch skips.

use domain::generated::events::DomainEvent;
use domain::generated::scalars::{ScopeType, UserType};
use uuid::Uuid;

use crate::projections::Envelope;

/// The namespace constant this projector derives membership keys under. Fixed forever: changing it
/// would re-key every existing row and silently orphan the old ones.
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
    /// The order a `DeliveryCancelled` job belongs to (`View_DeliveryJob` lookup). `None` when the
    /// job is unknown — an orphan stream, which yields no change.
    pub order_id: Option<Uuid>,
    /// The account owning the restaurant named by `OrderPlaced`. `None` when unresolved.
    pub restaurant_account_id: Option<Uuid>,
}

/// `membership_id` — UUIDv5 over the natural key, so the same membership always derives the same id.
///
/// This is what buys idempotent re-projection (a replayed grant upserts onto itself) and lookup-free
/// revocation, and it is why the table needs no composite primary key — which the DSL cannot express
/// anyway (the emitter writes `PRIMARY KEY` inline per column). Same trick as
/// `hubrise_connections.restaurant_account_id`.
///
/// The key hashes the enums' WIRE TEXT (`EnumText` stores the variant name verbatim), which the
/// `membership_id_is_pinned` test freezes — a variant rename would re-key every row.
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
            // `customer_id` is REQUIRED as of #144 — checkout registers or resolves the Customer, and
            // OrderPlaced is emitted only by PlaceOrderProcess, so every order has one by
            // construction. The authorization index is therefore complete: there is no "order nobody
            // owns" class the guard would have to deny.
            out.push(MembershipChange::Grant {
                scope_type: ScopeType::ORDER,
                scope_id: order_id,
                principal_type: UserType::CUSTOMER,
                principal_id: e.customer_id.0,
            });
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

        // A rider took the job: grant that rider on the job's order — carried in the payload
        // (required) since D-QW1 option b, so no lookup is needed.
        DomainEvent::DeliveryAcceptedByRider(e) => vec![MembershipChange::Grant {
            scope_type: ScopeType::ORDER,
            scope_id: e.order_id.0,
            principal_type: UserType::RIDER,
            principal_id: e.rider_id.0,
        }],

        // The job ended without delivery: no rider should retain access. Revoking the ROLE rather
        // than a named rider is deliberate — if reassignment left two rider rows on the order, a
        // targeted revoke would strip one and leave the other, which is the stale-grant breach.
        //
        // DeliveryDispatchFailed carries the order; DeliveryCancelled does not, so its order arrives
        // pre-resolved. An unresolvable cancel yields NO change — note this is ALLOW-STALE, not
        // deny-safe (a lost revoke would leave a rider grant standing). It is acceptable only
        // because a non-orphan stream always resolves: the job's birth fact precedes its cancel in
        // position order and `View_DeliveryJob` folds straight off `domain_events`, and an ORPHAN
        // stream never had an acceptance grant to revoke in the first place.
        DomainEvent::DeliveryDispatchFailed(e) => vec![MembershipChange::RevokeRole {
            scope_type: ScopeType::ORDER,
            scope_id: e.order_id.0,
            principal_type: UserType::RIDER,
        }],
        DomainEvent::DeliveryCancelled(_) => match resolved.order_id {
            Some(order_id) => vec![MembershipChange::RevokeRole {
                scope_type: ScopeType::ORDER,
                scope_id: order_id,
                principal_type: UserType::RIDER,
            }],
            None => Vec::new(),
        },

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

        // The POST-registration account attachment (review finding, ADR-20260809-160000 addendum):
        // a Sirene-seeded listing registers with NO accountId and gains one when its owner proves
        // ownership and claims it. Without this fold that account never holds RESTAURANT
        // membership and `resolve_restaurant_account` finds nothing for every later OrderPlaced —
        // deny-safe, but a coverage hole that would need a checkpoint-reset replay to repair once
        // discovered (the #424 lesson).
        DomainEvent::RestaurantListingClaimed(e) => match e.account_id.as_ref() {
            Some(account_id) => vec![MembershipChange::Grant {
                scope_type: ScopeType::RESTAURANT,
                scope_id: e.restaurant_id.0,
                principal_type: UserType::RESTAURANT_ACCOUNT,
                principal_id: account_id.0,
            }],
            None => Vec::new(),
        },

        // ADMIN holds no rows at all — the guard short-circuits on the role. Storing them would mean
        // a row per admin per instance, unbounded and pointless.
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::generated::events::{DeliveryAcceptedByRider, DeliveryCancelled, OrderPlaced};

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

    fn order_placed(order: Uuid, restaurant: Uuid, customer: Uuid) -> DomainEvent {
        let e: OrderPlaced = serde_json::from_value(serde_json::json!({
            "orderId": order,
            "restaurantId": restaurant,
            "customerId": customer,
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

    /// The derived key is PINNED to a literal: the UUIDv5 input hashes the enum variant names, so a
    /// rename (or a namespace change) re-keys every stored row — grants would survive as orphans that
    /// `revoke_role`'s column-keyed DELETE still finds, but replay idempotence and `is_member` would
    /// silently split. This failing is the alarm.
    #[test]
    fn membership_id_is_pinned() {
        assert_eq!(
            membership_id(ScopeType::ORDER, ORDER, UserType::CUSTOMER, CUSTOMER).to_string(),
            "bf9b9d8f-84c5-5d73-8a90-e0ebdac66d94",
        );
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
            &env(order_placed(ORDER, RESTAURANT, CUSTOMER)),
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

    /// The customer grant is unconditional: `OrderPlaced.customerId` is REQUIRED as of #144, so the
    /// earlier "order with no Captain customer" case is unrepresentable. Every order yields a
    /// CUSTOMER grant — the authorization index is complete by construction and the guard has no
    /// "order nobody owns" class to deny.
    #[test]
    fn every_order_grants_its_customer() {
        let changes =
            membership_changes(&env(order_placed(ORDER, RESTAURANT, CUSTOMER)), &Resolved::default());
        assert!(changes.contains(&MembershipChange::Grant {
            scope_type: ScopeType::ORDER,
            scope_id: ORDER,
            principal_type: UserType::CUSTOMER,
            principal_id: CUSTOMER,
        }));
        // No account resolved -> customer + restaurant only.
        assert_eq!(changes.len(), 2);
    }

    /// THE REASSIGNMENT PAIR. Asserting only that the new rider is granted would pass against a
    /// broken rule — the previous rider losing access is the half that matters.
    #[test]
    fn reassignment_grants_the_new_rider_and_revokes_the_previous_one() {
        let accept = |rider: Uuid| {
            let e: DeliveryAcceptedByRider = serde_json::from_value(serde_json::json!({
                "deliveryJobId": Uuid::from_u128(99),
                "orderId": ORDER,
                "riderId": rider,
            }))
            .expect("DeliveryAcceptedByRider fixture");
            env(DomainEvent::DeliveryAcceptedByRider(e))
        };
        let resolved = Resolved { order_id: Some(ORDER), restaurant_account_id: None };

        // Rider A takes the job — the grant keys on the payload's own orderId, no lookup.
        assert_eq!(
            membership_changes(&accept(RIDER_A), &Resolved::default()),
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
            membership_changes(&accept(RIDER_B), &Resolved::default()),
            vec![MembershipChange::Grant {
                scope_type: ScopeType::ORDER,
                scope_id: ORDER,
                principal_type: UserType::RIDER,
                principal_id: RIDER_B,
            }]
        );
    }

    /// A cancel whose job cannot be resolved to an order yields NOTHING — and that is ALLOW-STALE,
    /// not deny-safe (see the module note): acceptable only because an orphan stream never received
    /// an acceptance grant, so there is nothing standing to revoke.
    #[test]
    fn unresolved_cancel_yields_no_changes() {
        let e: DeliveryCancelled = serde_json::from_value(serde_json::json!({
            "deliveryJobId": Uuid::from_u128(99),
            "reason": "x",
        }))
        .expect("fixture");
        assert!(
            membership_changes(&env(DomainEvent::DeliveryCancelled(e)), &Resolved::default())
                .is_empty()
        );
    }

    /// The post-registration attachment: a claimed listing grants its account RESTAURANT
    /// membership — the Sirene-seeded path, where registration carried no account (review finding).
    /// A claim WITHOUT an account (nullable) grants nothing.
    #[test]
    fn listing_claim_grants_the_claiming_account() {
        let claim = |account: Option<Uuid>| {
            let e: domain::generated::events::RestaurantListingClaimed =
                serde_json::from_value(serde_json::json!({
                    "restaurantId": RESTAURANT,
                    "accountId": account,
                }))
                .expect("RestaurantListingClaimed fixture");
            env(DomainEvent::RestaurantListingClaimed(e))
        };
        assert_eq!(
            membership_changes(&claim(Some(ACCOUNT)), &Resolved::default()),
            vec![MembershipChange::Grant {
                scope_type: ScopeType::RESTAURANT,
                scope_id: RESTAURANT,
                principal_type: UserType::RESTAURANT_ACCOUNT,
                principal_id: ACCOUNT,
            }]
        );
        assert!(membership_changes(&claim(None), &Resolved::default()).is_empty());
    }

    /// The overwhelmingly common case on the projection hot path: an event with no authorization
    /// meaning contributes nothing.
    #[test]
    fn unrelated_events_yield_no_changes() {
        let e: domain::generated::events::OrderAcceptedByRestaurant =
            serde_json::from_value(serde_json::json!({
                "orderId": ORDER,
                "restaurantId": RESTAURANT,
            }))
            .expect("fixture");
        assert!(membership_changes(
            &env(DomainEvent::OrderAcceptedByRestaurant(e)),
            &Resolved::default()
        )
        .is_empty());
    }
}
