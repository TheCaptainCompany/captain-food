//! Hand-written `RestaurantRosterCompute` (ADR-0040; #639 part C step 6-iv round 2,
//! ADR-20260905-101349 §2 amendment).
//!
//! Every column of `RestaurantRoster` is mechanical — the generated `project_restaurant_roster`
//! dispatch maps them straight off `RestaurantAccessGranted`'s own payload, and `since` is the
//! occurrence timestamp (ColMode::Occurrence). So this impl is deliberately empty: there is no
//! computed, cross-stream or accumulated column to own. It exists because the generated dispatch
//! is generic over the trait (the `MemberCompute`/`SlugAliasCompute` precedent).
//!
//! `RestaurantAccessRevoked` touches NOTHING here (a NAMED gap, see the table's own `note:` in
//! `specs/database/tables/projection_tables.yaml#/RestaurantRoster`): a revoked colleague stays
//! listed until the revoke-removal follow-up lands.

use crate::projections::RestaurantRosterCompute;

pub struct RestaurantRosterProjector;

impl RestaurantRosterCompute for RestaurantRosterProjector {}
