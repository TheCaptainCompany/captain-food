//! Captain.Food application layer (ADR-0035).
//!
//! Orchestrates the domain: CQRS command handlers (`commands/`), query handlers (`queries/`), and process
//! managers / sagas (`process_managers/`, e.g. `PlaceOrderProcess`, `RefundProcess`). It declares **ports**
//! — traits the infrastructure must implement — and depends on `domain` only. It must NOT depend on
//! `infrastructure`, `server`, or `web`: side effects reach it exclusively through the ports below
//! (Ports & Adapters), injected at the `server` composition root.

#[cfg(test)]
pub mod behaviour_support;
pub mod commands;
pub mod deliveries;
pub mod dispatch;
pub mod dispatch_strategy;
pub mod generated;
pub mod journal;
pub mod payments;
pub mod pricing;
pub mod process_managers;
pub mod projections;
pub mod projectors;
pub mod ports;
pub mod queries;
pub mod repository;

// The PM state ports are GENERATED from specs/database/tables/process_managers.yaml (issue #27);
// re-exported here so the stable `application::pm_state` path survives the move into `generated/`.
pub use generated::pm_state;
