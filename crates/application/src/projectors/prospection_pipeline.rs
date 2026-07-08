//! Hand-written `ProspectionPipelineCompute` (ADR-0020/0040). `pipeline_status` and `contacts_count` fold
//! from the outreach events; the weighted `score` needs listing signals beyond the event.
#![allow(unused_variables)]

use crate::projections::{Envelope, ProspectionPipelineCompute, ProspectionPipelineRow};
use domain::generated::events::DomainEvent;
use domain::generated::scalars::{ProspectPipelineStatus, ProspectionScore, RestaurantListingStatus};

pub struct ProspectionPipelineProjector;

impl ProspectionPipelineCompute for ProspectionPipelineProjector {
    /// Weighted 0–10 priority (ADR-0020) from listing facts (NAF, rating, reviews, age, aggregator presence)
    /// — most inputs live on the Restaurant read model, not this event. TODO(runtime): compute via the
    /// Restaurant read-model port; preserved / 0 meanwhile.
    fn score(&self, prev: Option<&ProspectionPipelineRow>, env: &Envelope) -> ProspectionScore {
        prev.map(|r| r.score.clone()).unwrap_or(ProspectionScore(0))
    }

    /// Outreach state: NEW → CONTACTED → COLD/REPLIED; CONVERTED once the listing becomes ACTIVE_PARTNER.
    fn pipeline_status(&self, prev: Option<&ProspectionPipelineRow>, env: &Envelope) -> ProspectPipelineStatus {
        match &env.event {
            DomainEvent::ProspectContacted(_) => ProspectPipelineStatus::CONTACTED,
            DomainEvent::ProspectMarkedCold(_) => ProspectPipelineStatus::COLD,
            DomainEvent::ProspectReplied(_) => ProspectPipelineStatus::REPLIED,
            DomainEvent::RestaurantListingStatusChanged(e) => {
                if matches!(e.listing_status, RestaurantListingStatus::ACTIVE_PARTNER) {
                    ProspectPipelineStatus::CONVERTED
                } else {
                    prev.map(|r| r.pipeline_status.clone()).unwrap_or(ProspectPipelineStatus::NEW)
                }
            }
            _ => prev.map(|r| r.pipeline_status.clone()).unwrap_or(ProspectPipelineStatus::NEW),
        }
    }

    /// Count of ProspectContacted (drives the anti-spam ≤3 rule).
    fn contacts_count(&self, prev: Option<&ProspectionPipelineRow>, env: &Envelope) -> i64 {
        let base = prev.map(|r| r.contacts_count).unwrap_or(0);
        match &env.event {
            DomainEvent::ProspectContacted(_) => base + 1,
            _ => base,
        }
    }
}
