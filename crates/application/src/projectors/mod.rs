//! Hand-written projector logic — the `…Compute` trait impls for the materialized read-model tables
//! (ADR-0040). The generator (crates/application/src/generated/projectors.rs) maps the mechanical columns
//! and calls these hooks for the computed / cross-stream / accumulate ones; this is where that business
//! logic lives, tested and out of the DB. One module per table as they are implemented.
//!
//! All six read models have an impl. Columns that fold from the event payload are implemented for real
//! (statuses, tip sums, the OrderPlaced breakdown, delivery mirror, payment status, jsonb accumulations,
//! prospection status/count); columns that need state outside the event — the live catalog + pricing
//! policies (`Cart` prices, `uber_*`), the account read model (`Restaurant.default_currency`), the catalog
//! tree, the prospection score — are `TODO(runtime)` placeholders that preserve the prior value, to be
//! completed once the read-model lookup ports land with the DB layer.

pub mod cart;
pub mod catalog;
pub mod customer;
pub mod order_tracking;
pub mod prospection_pipeline;
pub mod restaurant;
