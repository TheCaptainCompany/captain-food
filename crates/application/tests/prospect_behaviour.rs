//! BEHAVIOUR tests for the Prospect aggregate (ADR-0020) — the executable form of the
//! `specs/tests.yaml` Given/When/Then cases whose `when` is a Prospect command (ADR-0032: each test
//! cites the `specs/rules.yaml` rule it asserts). Given = pre-seeded stream events (in-memory event
//! store), When = the command handler, Then = the emitted event(s) / the errors.yaml rejection code.
//!
//! The ≥7-days anti-spam check reads the `ProspectionPipeline` projection's `last_contacted_at`
//! (contact TIME is envelope metadata, invisible to the stream fold), so the fake read repo seeds it.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use application::commands::{
    mark_prospect_cold, record_prospect_contact, record_prospect_reply, rejection_code,
};
use application::ports::{version_conflict, Actor, EventStore};
use application::queries::{ProspectFilter, ProspectionPipelineRow, ProspectionReadRepository};
use domain::generated::commands::{MarkProspectCold, RecordProspectContact, RecordProspectReply};
use domain::generated::events::{DomainEvent, ProspectContacted};
use domain::generated::scalars::*;
use domain::shared::errors::DomainError;

// ------------------------------------------------------------------------------------------------
// Test doubles
// ------------------------------------------------------------------------------------------------

/// In-memory [`EventStore`]: version = number of events on the stream, same optimistic-concurrency
/// semantics as `PgEventStore` (a clash → the canonical `version_conflict`).
#[derive(Default)]
struct MemStore {
    streams: Mutex<HashMap<String, Vec<DomainEvent>>>,
}

impl MemStore {
    /// GIVEN: pre-seed a stream with already-recorded facts.
    fn seed(&self, stream: &str, events: Vec<DomainEvent>) {
        self.streams.lock().unwrap().insert(stream.to_string(), events);
    }

    /// THEN: the full stream after the command ran.
    fn stream(&self, stream: &str) -> Vec<DomainEvent> {
        self.streams.lock().unwrap().get(stream).cloned().unwrap_or_default()
    }
}

#[async_trait]
impl EventStore for MemStore {
    async fn append(
        &self,
        stream_name: &str,
        expected_version: i64,
        events: &[DomainEvent],
        _actor: &Actor,
    ) -> Result<i64, DomainError> {
        let mut streams = self.streams.lock().unwrap();
        let stream = streams.entry(stream_name.to_string()).or_default();
        if stream.len() as i64 != expected_version {
            return Err(version_conflict(stream_name, expected_version));
        }
        stream.extend(events.iter().cloned());
        Ok(stream.len() as i64)
    }

    async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
        let events = self.stream(stream_name);
        let version = events.len() as i64;
        Ok((events, version))
    }
}

/// Fake `ProspectionPipeline` read model: at most one projected row (seeding `last_contacted_at`
/// mirrors "the projector already folded the previous ProspectContacted with its occurred_at").
#[derive(Default)]
struct FakeProspection {
    row: Option<ProspectionPipelineRow>,
}

#[async_trait]
impl ProspectionReadRepository for FakeProspection {
    async fn list(&self, _filter: ProspectFilter) -> Result<Vec<ProspectionPipelineRow>, DomainError> {
        Ok(self.row.clone().into_iter().collect())
    }
}

// ------------------------------------------------------------------------------------------------
// Fixtures (tests.yaml `fixtures`, with UUIDs instead of the sample string ids)
// ------------------------------------------------------------------------------------------------

fn actor() -> Actor {
    Actor {
        user_id: uuid::Uuid::new_v4(),
        user_type: 6, // UserType::EXTERNAL ordinal (the prospection-acl worker)
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    }
}

fn stream(id: RestaurantId) -> String {
    format!("Prospect-{}", id.0)
}

/// Fixtures `prospectContacted` / `prospectContactedRelance` / `prospectContactedFinal`.
fn contacted_event(id: RestaurantId, channel: OutreachChannel, step: i64) -> DomainEvent {
    DomainEvent::ProspectContacted(ProspectContacted {
        restaurant_id: id,
        channel,
        sequence_step: step,
    })
}

fn contact_cmd(id: RestaurantId, step: i64) -> RecordProspectContact {
    RecordProspectContact { restaurant_id: id, channel: OutreachChannel::EMAIL, sequence_step: step }
}

/// A projected `prospectionpipeline` row whose last contact happened `days_ago`.
fn projected_row(id: RestaurantId, contacts: i64, days_ago: i64) -> ProspectionPipelineRow {
    let last = chrono::Utc::now() - chrono::Duration::days(days_ago);
    ProspectionPipelineRow {
        restaurant_id: id,
        score: ProspectionScore(70),
        pipeline_status: ProspectPipelineStatus::CONTACTED,
        contacts_count: contacts,
        last_contacted_at: Some(last),
        replied_at: None,
        created_at: last,
        updated_at: last,
    }
}

fn rid() -> RestaurantId {
    RestaurantId(uuid::Uuid::new_v4())
}

// ------------------------------------------------------------------------------------------------
// Contact schedule & anti-spam (rules.yaml#/ProspectContactScheduleAndLimit)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestProspectContacted — rules.yaml#/ProspectContactScheduleAndLimit
#[tokio::test]
async fn records_the_first_contact_which_births_the_prospect() {
    let store = MemStore::default();
    let id = rid();

    record_prospect_contact(&store, &FakeProspection::default(), contact_cmd(id, 0), &actor())
        .await
        .expect("first contact");

    let events = store.stream(&stream(id));
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        DomainEvent::ProspectContacted(e)
            if e.restaurant_id == id && e.channel == OutreachChannel::EMAIL && e.sequence_step == 0
    ));
}

/// tests.yaml#/cases/TestProspectContactedTooRecently —
/// rules.yaml#/ProspectContactScheduleAndLimit (≥ 7 days apart)
#[tokio::test]
async fn rejects_a_new_contact_too_soon_after_the_previous_one() {
    let store = MemStore::default();
    let id = rid();
    store.seed(&stream(id), vec![contacted_event(id, OutreachChannel::EMAIL, 0)]);
    // The previous contact was projected 1 day ago — well inside the 7-day window.
    let prospection = FakeProspection { row: Some(projected_row(id, 1, 1)) };

    let err = record_prospect_contact(&store, &prospection, contact_cmd(id, 7), &actor())
        .await
        .expect_err("too recent");
    assert_eq!(rejection_code(&err), Some("ProspectContactedTooRecently"));
    assert_eq!(store.stream(&stream(id)).len(), 1, "no event on rejection");
}

/// A relance a full week after the previous contact is accepted (the schedule's happy path).
#[tokio::test]
async fn accepts_a_relance_seven_or_more_days_later() {
    let store = MemStore::default();
    let id = rid();
    store.seed(&stream(id), vec![contacted_event(id, OutreachChannel::EMAIL, 0)]);
    let prospection = FakeProspection { row: Some(projected_row(id, 1, 8)) };

    record_prospect_contact(&store, &prospection, contact_cmd(id, 7), &actor())
        .await
        .expect("relance");
    assert_eq!(store.stream(&stream(id)).len(), 2);
}

/// tests.yaml#/cases/TestProspectContactLimitReached —
/// rules.yaml#/ProspectContactScheduleAndLimit (anti-spam: ≤ 3, counted from the fold)
#[tokio::test]
async fn rejects_a_fourth_contact() {
    let store = MemStore::default();
    let id = rid();
    store.seed(
        &stream(id),
        vec![
            contacted_event(id, OutreachChannel::EMAIL, 0),
            contacted_event(id, OutreachChannel::EMAIL, 7),
            contacted_event(id, OutreachChannel::SLACK, 21),
        ],
    );
    let prospection = FakeProspection { row: Some(projected_row(id, 3, 30)) };

    let err = record_prospect_contact(&store, &prospection, contact_cmd(id, 28), &actor())
        .await
        .expect_err("limit reached");
    assert_eq!(rejection_code(&err), Some("ProspectContactLimitReached"));
    assert_eq!(store.stream(&stream(id)).len(), 3, "no event on rejection");
}

// ------------------------------------------------------------------------------------------------
// Outreach state transitions (rules.yaml#/ProspectOutreachStateTransitions)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestProspectMarkedCold — rules.yaml#/ProspectOutreachStateTransitions
#[tokio::test]
async fn marks_a_contacted_prospect_cold() {
    let store = MemStore::default();
    let id = rid();
    store.seed(&stream(id), vec![contacted_event(id, OutreachChannel::EMAIL, 0)]);

    mark_prospect_cold(
        &store,
        MarkProspectCold { restaurant_id: id, reason: Some("No reply by J+21".into()) },
        &actor(),
    )
    .await
    .expect("mark cold");

    let events = store.stream(&stream(id));
    assert!(matches!(
        &events[1],
        DomainEvent::ProspectMarkedCold(e) if e.reason.as_deref() == Some("No reply by J+21")
    ));
}

/// tests.yaml#/cases/TestProspectReplied — rules.yaml#/ProspectOutreachStateTransitions
#[tokio::test]
async fn records_a_prospect_reply() {
    let store = MemStore::default();
    let id = rid();
    store.seed(&stream(id), vec![contacted_event(id, OutreachChannel::EMAIL, 0)]);

    record_prospect_reply(
        &store,
        RecordProspectReply { restaurant_id: id, note: Some("Interested — call back".into()) },
        &actor(),
    )
    .await
    .expect("reply");

    let events = store.stream(&stream(id));
    assert!(matches!(
        &events[1],
        DomainEvent::ProspectReplied(e) if e.note.as_deref() == Some("Interested — call back")
    ));
}

/// tests.yaml#/cases/TestProspectMarkColdNotFound — rules.yaml#/ProspectOutreachStateTransitions
#[tokio::test]
async fn rejects_marking_cold_a_prospect_that_was_never_contacted() {
    let store = MemStore::default();

    let err = mark_prospect_cold(
        &store,
        MarkProspectCold { restaurant_id: rid(), reason: Some("n/a".into()) },
        &actor(),
    )
    .await
    .expect_err("never contacted");
    assert_eq!(rejection_code(&err), Some("ProspectNotFound"));

    // Same for a reply on a never-contacted prospect (actors.yaml throws).
    let err = record_prospect_reply(
        &store,
        RecordProspectReply { restaurant_id: rid(), note: None },
        &actor(),
    )
    .await
    .expect_err("never contacted");
    assert_eq!(rejection_code(&err), Some("ProspectNotFound"));
}
