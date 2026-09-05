//! Hand-written `RestaurantRosterCompute` (ADR-0040; #639 part C step 6-iv round 2,
//! ADR-20260905-101349 §2 amendment).
//!
//! Every column of `RestaurantRoster` is mechanical — the generated `project_restaurant_roster`
//! dispatch maps them straight off `RestaurantAccessGranted`'s own payload, and `since` is the
//! occurrence timestamp (ColMode::Occurrence). So this impl is deliberately empty: there is no
//! computed, cross-stream or accumulated column to own. It exists because the generated dispatch
//! is generic over the trait (the `MemberCompute`/`SlugAliasCompute` precedent).
//!
//! Round 3 (dba BLOCKING): `RestaurantAccessRevoked` DELETEs the row instead — handled OUTSIDE
//! this mechanical dispatch (`crates/infrastructure/src/projection/worker.rs`'s `RestaurantRoster`
//! arm, the `ScopeMembership` targeted-revoke precedent), because `RestaurantRosterCompute`'s
//! generated dispatch only ever returns a row to upsert and has no DELETE shape to hand a
//! `Compute` trait a hook into.

use crate::projections::RestaurantRosterCompute;

pub struct RestaurantRosterProjector;

impl RestaurantRosterCompute for RestaurantRosterProjector {}
