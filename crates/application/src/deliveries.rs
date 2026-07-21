//! Recording INBOUND delivery-partner facts on the DeliveryJob aggregate (issue #28, the delivery
//! sibling of [`crate::payments`]): the Avelo37 ACL translates a verified webhook into
//! `DeliveryAcceptedByPartner` / `DeliveryRejectedByPartner` / `DeliveryStatusUpdated`, stages it in
//! `inbound_events` (ADR-20260720-015400), and the drain worker delivers it HERE — no command, the
//! partner reports facts that already happened (CLAUDE.md "Commands vs inbound events").
//!
//! Idempotency is the AGGREGATE's business answer where the fold can give one:
//! - an acceptance is already reflected when the job is assigned to that same `partnerRef`;
//! - a status report is already reflected when the job already sits in the reported status.
//!
//! A rejection folds as a status no-op (it is not in the lifecycle machine) and two successive
//! declines of the same job are legitimately identical payloads — the `(source, external_id)`
//! journal unique is its dedupe (each partner decline is a distinct provider event), and every
//! staged rejection row is recorded. The residual crash-window redelivery (append succeeded,
//! `mark_delivered` lost) can at worst count one extra decline toward the bounded re-offer cap —
//! erring toward `DeliveryDispatchFailed` + manual handling, never an unbounded loop
//! (rules.yaml#/DispatchRetriesAreBounded, ADR-20260720-004556).
//!
//! Unlike payment facts, the lifecycle-bearing facts are GUARDED by the declared status machine
//! (`specs/actors.yaml#/DeliveryJob/lifecycle`, ADR-20260721-093027): the fold applies a recorded
//! fact's target unconditionally, so appending an illegal transition would corrupt the machine.
//! A report that cannot legally apply (e.g. `DELIVERED` on a CANCELLED job) is NOT appended — the
//! drain marks the inbound row FAILED, kept inspectable for ops (the partner is out of sync).
//!
//! A fact for a stream with NO `DeliveryRequested` birth is STILL recorded (facts are never
//! dropped); it is the DeliveryDispatchProcess `DeliveryJobNotFound` guard that flags the orphan
//! run for ops, not this recording path — mirroring the Payment orphan philosophy.

use domain::delivery_job::lifecycle;
use domain::generated::events::DomainEvent;
use domain::generated::scalars::DeliveryJobId;
use domain::shared::errors::DomainError;

use crate::payments::RecordOutcome;
use crate::ports::{Actor, EventStore};
use crate::repository::Repository;

/// The DeliveryJob a partner fact belongs to — the aggregate's stream key. `None` for any event
/// outside the delivery inbox (a routing bug in the caller).
fn delivery_job_of(event: &DomainEvent) -> Option<DeliveryJobId> {
    match event {
        DomainEvent::DeliveryAcceptedByPartner(e) => Some(e.delivery_job_id),
        DomainEvent::DeliveryRejectedByPartner(e) => Some(e.delivery_job_id),
        DomainEvent::DeliveryStatusUpdated(e) => Some(e.delivery_job_id),
        _ => None,
    }
}

/// Record one inbound delivery-partner fact (`DeliveryAcceptedByPartner` | `DeliveryRejectedByPartner`
/// | `DeliveryStatusUpdated`) on its `DeliveryJob-<id>` stream. The `actor` is the ACL's system
/// identity (EXTERNAL, correlation = the webhook's, `cause_id` = the inbound row, ADR-0041).
pub async fn record_inbound_delivery_event(
    store: &dyn EventStore,
    event: DomainEvent,
    actor: &Actor,
) -> Result<RecordOutcome, DomainError> {
    let Some(job_id) = delivery_job_of(&event) else {
        return Err(DomainError::Repository(format!(
            "record_inbound_delivery_event routed a non-delivery event: {event:?}"
        )));
    };
    let stream = crate::commands::delivery_job_stream(&job_id);
    let (events, version) = store.load(&stream).await?;

    if let Some(job) = domain::delivery_job::fold(&events) {
        match &event {
            // Redelivery tail: the job already carries this partner's assignment.
            DomainEvent::DeliveryAcceptedByPartner(e)
                if job.assigned && job.partner_ref.as_ref() == Some(&e.partner_ref) =>
            {
                return Ok(RecordOutcome::AlreadyRecorded);
            }
            // Redelivery tail: the job already sits in the reported status.
            DomainEvent::DeliveryStatusUpdated(e) if job.status == e.status => {
                return Ok(RecordOutcome::AlreadyRecorded);
            }
            // A rejection is outside the lifecycle machine: always recorded (see module docs).
            DomainEvent::DeliveryRejectedByPartner(_) => {}
            // Lifecycle-bearing and not yet reflected: only a declared transition may be appended —
            // the fold applies a recorded fact's target unconditionally, so an illegal append would
            // corrupt the machine. The drain keeps the row FAILED/inspectable.
            _ => {
                if lifecycle::transition(job.status, &event).is_none() {
                    return Err(DomainError::Repository(format!(
                        "inbound {} cannot legally apply to job {} in status {:?} (partner out of \
                         sync; row kept for ops)",
                        event_name(&event),
                        job_id.0,
                        job.status
                    )));
                }
            }
        }
    } else if events.iter().any(|e| e == &event) {
        // Birthless (orphan) stream: no fold to consult, so a redelivery dedups by structural
        // equality — the no-op guarantee holds even before the anomaly is resolved.
        return Ok(RecordOutcome::AlreadyRecorded);
    }
    // No birth on the stream? Record anyway — the fact happened; the DeliveryDispatchProcess
    // `DeliveryJobNotFound` guard is what surfaces the anomaly (never this recording path).
    Repository::new(store).save(&stream, version, &[event], actor).await?;
    Ok(RecordOutcome::Recorded)
}

fn event_name(event: &DomainEvent) -> &'static str {
    match event {
        DomainEvent::DeliveryAcceptedByPartner(_) => "DeliveryAcceptedByPartner",
        DomainEvent::DeliveryStatusUpdated(_) => "DeliveryStatusUpdated",
        DomainEvent::DeliveryRejectedByPartner(_) => "DeliveryRejectedByPartner",
        _ => "unexpected",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_managers::test_support::MemStore;
    use domain::generated::entities::{Address, Courier};
    use domain::generated::events::{
        DeliveryAcceptedByPartner, DeliveryRejectedByPartner, DeliveryRequested,
        DeliveryStatusUpdated,
    };
    use domain::generated::scalars::*;

    fn uid(n: u128) -> uuid::Uuid {
        uuid::Uuid::from_u128(n)
    }
    fn actor() -> Actor {
        Actor { user_id: uid(0xAC), user_type: 6, correlation_id: uid(0xC0), cause_id: None }
    }
    fn job_id() -> DeliveryJobId {
        DeliveryJobId(uid(1))
    }
    fn address() -> Address {
        Address {
            line1: AddressLine("1 rue Nationale".into()),
            line2: None,
            postal_code: PostalCode("37000".into()),
            city: CityName("Tours".into()),
            country: CountryCode("FR".into()),
        }
    }
    fn birth() -> DomainEvent {
        DomainEvent::DeliveryRequested(DeliveryRequested {
            mode: None,
            delivery_job_id: job_id(),
            order_id: OrderId(uid(2)),
            restaurant_id: RestaurantId(uid(3)),
            pickup: address(),
            dropoff: address(),
            provider: None,
        })
    }
    fn accepted() -> DomainEvent {
        DomainEvent::DeliveryAcceptedByPartner(DeliveryAcceptedByPartner {
            delivery_job_id: job_id(),
            partner_ref: ExternalReference("avelo-77".into()),
            courier: Courier {
                display_name: "Léa".into(),
                phone: Some(PhoneNumber("+33611223344".into())),
                rider_id: None,
            },
            estimated_pickup_at: None,
            estimated_dropoff_at: None,
        })
    }
    fn rejected() -> DomainEvent {
        DomainEvent::DeliveryRejectedByPartner(DeliveryRejectedByPartner {
            delivery_job_id: job_id(),
            partner_ref: Some(ExternalReference("avelo-77".into())),
            reason: Some("No courier available".into()),
        })
    }
    fn status(status: DeliveryStatus) -> DomainEvent {
        DomainEvent::DeliveryStatusUpdated(DeliveryStatusUpdated {
            delivery_job_id: job_id(),
            partner_ref: Some(ExternalReference("avelo-77".into())),
            status,
            occurred_at: None,
            note: None,
        })
    }
    fn stream() -> String {
        crate::commands::delivery_job_stream(&job_id())
    }

    /// tests.yaml#/TestDeliveryJobRecordsPartnerAcceptance — the inbound acceptance lands on the
    /// job stream; a webhook redelivery is absorbed by the aggregate's own fold
    /// (rules.yaml#/PartnerAcceptanceRecordsCourier).
    #[tokio::test]
    async fn acceptance_records_once_and_absorbs_redelivery() {
        let store = MemStore::default();
        store.seed(&stream(), vec![birth()]);
        assert_eq!(
            record_inbound_delivery_event(&store, accepted(), &actor()).await.unwrap(),
            RecordOutcome::Recorded
        );
        assert_eq!(
            record_inbound_delivery_event(&store, accepted(), &actor()).await.unwrap(),
            RecordOutcome::AlreadyRecorded
        );
        assert_eq!(store.stream(&stream()).len(), 2, "birth + one acceptance");
    }

    /// tests.yaml#/TestDeliveryJobRecordsPartnerStatusReport — a partner progress report applies as
    /// a valid transition; re-reporting the same status is the redelivery no-op
    /// (rules.yaml#/DeliveryPartnerAssignmentLifecycle).
    #[tokio::test]
    async fn status_report_applies_as_a_valid_transition_and_dedupes_on_status() {
        let store = MemStore::default();
        store.seed(&stream(), vec![birth(), accepted()]);
        assert_eq!(
            record_inbound_delivery_event(&store, status(DeliveryStatus::PICKED_UP), &actor())
                .await
                .unwrap(),
            RecordOutcome::Recorded
        );
        assert_eq!(
            record_inbound_delivery_event(&store, status(DeliveryStatus::PICKED_UP), &actor())
                .await
                .unwrap(),
            RecordOutcome::AlreadyRecorded
        );
        let folded = domain::delivery_job::fold(&store.stream(&stream())).unwrap();
        assert_eq!(folded.status, DeliveryStatus::PICKED_UP);
    }

    /// An illegal transition report (partner out of sync with a terminal job) is never appended —
    /// the caller (drain) keeps the row FAILED/inspectable
    /// (rules.yaml#/DeliveryPartnerAssignmentLifecycle).
    #[tokio::test]
    async fn illegal_status_report_is_rejected_not_recorded() {
        let store = MemStore::default();
        store.seed(&stream(), vec![birth()]);
        // DELIVERED straight from PENDING is not a declared edge of the machine.
        let err = record_inbound_delivery_event(&store, status(DeliveryStatus::DELIVERED), &actor())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("cannot legally apply"), "unexpected: {err}");
        assert_eq!(store.stream(&stream()).len(), 1, "nothing appended");
    }

    /// tests.yaml#/TestDeliveryJobRecordsPartnerRejection — every staged decline is recorded (two
    /// successive declines are distinct provider events with identical payloads; the journal
    /// unique is their dedupe) so the bounded re-offer counter advances
    /// (rules.yaml#/PartnerRejectionReoffers, ADR-20260720-004556).
    #[tokio::test]
    async fn every_rejection_is_recorded_even_when_identical() {
        let store = MemStore::default();
        store.seed(&stream(), vec![birth()]);
        assert_eq!(
            record_inbound_delivery_event(&store, rejected(), &actor()).await.unwrap(),
            RecordOutcome::Recorded
        );
        assert_eq!(
            record_inbound_delivery_event(&store, rejected(), &actor()).await.unwrap(),
            RecordOutcome::Recorded
        );
        assert_eq!(store.stream(&stream()).len(), 3, "birth + two declines");
    }

    /// A fact for a birthless stream is still recorded (facts are never dropped) and structural
    /// equality absorbs its redelivery — the saga's DeliveryJobNotFound guard flags the orphan.
    #[tokio::test]
    async fn orphan_fact_is_recorded_and_redelivery_absorbed_structurally() {
        let store = MemStore::default();
        assert_eq!(
            record_inbound_delivery_event(&store, accepted(), &actor()).await.unwrap(),
            RecordOutcome::Recorded
        );
        assert_eq!(
            record_inbound_delivery_event(&store, accepted(), &actor()).await.unwrap(),
            RecordOutcome::AlreadyRecorded
        );
        assert_eq!(store.stream(&stream()).len(), 1);
    }

    /// A non-delivery event reaching this recorder is a routing bug, surfaced loudly.
    #[tokio::test]
    async fn non_delivery_event_is_a_routing_error() {
        let store = MemStore::default();
        let err = record_inbound_delivery_event(
            &store,
            DomainEvent::DeliveryCancelled(domain::generated::events::DeliveryCancelled {
                delivery_job_id: job_id(),
                reason: None,
            }),
            &actor(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("non-delivery event"), "unexpected: {err}");
    }
}
