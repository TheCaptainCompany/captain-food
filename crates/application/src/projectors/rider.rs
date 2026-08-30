//! Hand-written `RiderCompute` (ADR-0040) for the rider identity read model (#639 part A).
//!
//! Deliberately empty, like [`crate::projectors::slug_alias`]: every column of `Rider` is
//! mechanical, and the generated `project_rider` dispatch maps all five straight off the three
//! rider facts. `RiderRegistered` carries `riderId`, `authRef`, `displayName`, `phone` and `status`
//! and all are required, so the creation arm needs no computation; `RiderInfoUpdated` and
//! `RiderStatusChanged` only overwrite. There is no computed, cross-stream or accumulated column to
//! own. This impl exists because the generated dispatch is generic over the trait.
//!
//! Worth stating, because the emptiness is the interesting part: `RiderInfoUpdated` is a PARTIAL
//! update (both of its fields are optional) despite the `*Updated` replace convention, and the fold
//! still needs no hand-written arm — the emitter derives the `if let Some(v)` guard from the
//! COLUMN's NOT NULL-ness, which `projection_tables.yaml#/Rider` declares for exactly that reason.
//! A nullable `display_name` there would have made a phone-only update erase the rider's name, and
//! the repair would have landed here instead of in the spec, where it belongs.

use crate::projections::RiderCompute;

pub struct RiderProjector;

impl RiderCompute for RiderProjector {}
