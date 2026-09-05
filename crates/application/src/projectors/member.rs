//! Hand-written `MemberCompute` (ADR-0040; #639 part C step 6-i, ADR-20260905-101349 §5).
//!
//! Every column of `Member` is mechanical — the generated `project_member` dispatch maps them
//! straight off `RestaurantAccessGranted`'s `memberId`/`authSubject`. So this impl is deliberately
//! empty: there is no computed, cross-stream or accumulated column to own. It exists because the
//! generated dispatch is generic over the trait (the `SlugAliasCompute` precedent).
//!
//! Worth stating explicitly, because the shape is unusual: this projection is **keyed by the
//! payload's `memberId`**, not by the stream's own aggregate id (`RestaurantMembership-{membershipId}`)
//! — the same cross-stream-key shape `ScopeMembership`'s `RestaurantListingClaimed` arm resolves
//! from a payload property rather than the stream name.

use crate::projections::MemberCompute;

pub struct MemberProjector;

impl MemberCompute for MemberProjector {}
