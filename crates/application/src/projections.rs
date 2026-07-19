//! Read-model projections (ADR-0040): the write side appends business events to `domain_events`; a
//! projector folds each event into a materialized read-model row. This module hosts the hand-written glue
//! — the [`Envelope`] a projector receives — and re-exports the GENERATED row types (`generated::rows`)
//! and projector wiring (`generated::projectors`: a `<Table>Handlers` trait + a `project_<table>`
//! dispatch per table). Projection LOGIC is the hand-written `…Handlers` impl (tested app code), never in
//! generated code or SQL — see ADR-0040.

use domain::generated::events::DomainEvent;

/// One event as delivered to a projector: the typed business event plus the log metadata a fold needs.
/// The technical envelope lives in infrastructure (`domain_events`); this is its in-memory projection.
#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    /// The aggregate-instance stream this event belongs to (`domain_events.stream_name`).
    pub stream_name: String,
    /// Global total order / projection checkpoint (`domain_events.position`).
    pub position: i64,
    /// When the event occurred — the row-write time stamped onto `updated_at` by the dispatch.
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    /// The typed business event.
    pub event: DomainEvent,
}

pub use crate::generated::projectors::*;
pub use crate::generated::rows::*;

#[cfg(test)]
mod projector_dispatch_tests {
    //! Prove the GENERATED hybrid projector is usable end-to-end: the generator builds the row on the
    //! creation event (mechanical columns inline + `Compute` hooks for the complex ones), routes each
    //! event, stamps created_at/updated_at from the envelope, and leaves the row untouched for events the
    //! table is not fed. The hand-written part is just the small `…Compute` impl.
    use super::*;
    use domain::generated::events::{CartStarted, DomainEvent, RestaurantAccountDeleted};
    use domain::generated::scalars::{
        CartId, CartStatus, CurrencyCode, CustomerId, MoneyCents, RestaurantAccountId, RestaurantId,
        SessionId,
    };

    const NIL: &str = "00000000-0000-0000-0000-000000000000";
    fn ts(secs: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(secs, 0).unwrap()
    }
    fn env(event: DomainEvent, at: i64) -> Envelope {
        Envelope { stream_name: "cart-1".into(), position: 1, occurred_at: ts(at), event }
    }

    // The hand-written business logic — only Cart's complex columns; the mechanical ones are generated.
    struct Compute;
    impl CartCompute for Compute {
        fn status(&self, _p: Option<&CartRow>, _e: &Envelope) -> CartStatus { CartStatus::OPEN }
        fn lines(&self, _p: Option<&CartRow>, _e: &Envelope) -> serde_json::Value { serde_json::json!([]) }
        fn total_amount_cents(&self, _p: Option<&CartRow>, _e: &Envelope) -> MoneyCents { MoneyCents(0) }
        fn currency(&self, _p: Option<&CartRow>, _e: &Envelope) -> CurrencyCode { CurrencyCode("EUR".into()) }
        fn estimated_breakdown(&self, _p: Option<&CartRow>, _e: &Envelope) -> Option<serde_json::Value> { None }
        fn uber_comparison(&self, _p: Option<&CartRow>, _e: &Envelope) -> Option<serde_json::Value> { None }
    }

    #[test]
    fn creation_event_builds_row_and_stamps_timestamps() {
        let started = DomainEvent::CartStarted(CartStarted {
            cart_id: CartId(NIL.parse().unwrap()),
            restaurant_id: RestaurantId(NIL.parse().unwrap()),
            session_id: SessionId(NIL.parse().unwrap()),
            customer_id: None,
        });
        let out = project_cart(&Compute, None, &env(started, 1_700_000_000)).unwrap();
        // mechanical column mapped from the event by the generator:
        assert_eq!(out.restaurant_id, RestaurantId(NIL.parse().unwrap()));
        // both technical timestamps stamped from the envelope (first event → created = updated = occurred):
        assert_eq!(out.created_at, ts(1_700_000_000));
        assert_eq!(out.updated_at, ts(1_700_000_000));
    }

    #[test]
    fn unrelated_event_passes_through_untouched() {
        // RestaurantAccountDeleted is not fed to Cart → the incoming row is returned as-is, NOT stamped.
        let unrelated = DomainEvent::RestaurantAccountDeleted(RestaurantAccountDeleted {
            restaurant_account_id: RestaurantAccountId(NIL.parse().unwrap()),
            reason: None,
        });
        let started = DomainEvent::CartStarted(CartStarted {
            cart_id: CartId(NIL.parse().unwrap()),
            restaurant_id: RestaurantId(NIL.parse().unwrap()),
            session_id: SessionId(NIL.parse().unwrap()),
            customer_id: None,
        });
        let row = project_cart(&Compute, None, &env(started, 42)).unwrap();
        let out = project_cart(&Compute, Some(row), &env(unrelated, 1_700_000_000)).unwrap();
        assert_eq!(out.updated_at, ts(42)); // unchanged — the `_ => state` arm skips stamping
        assert_eq!(out.created_at, ts(42));
    }
}
