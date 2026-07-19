//! Captain.Food application layer (ADR-0035).
//!
//! Orchestrates the domain: CQRS command handlers (`commands/`), query handlers (`queries/`), and process
//! managers / sagas (`process_managers/`, e.g. `PlaceOrderProcess`, `RefundProcess`). It declares **ports**
//! — traits the infrastructure must implement — and depends on `domain` only. It must NOT depend on
//! `infrastructure`, `server`, or `web`: side effects reach it exclusively through the ports below
//! (Ports & Adapters), injected at the `server` composition root.

pub mod commands;
pub mod generated;
pub mod payments;
pub mod pm_state;
pub mod process_managers;
pub mod projections;
pub mod projectors;
pub mod ports;
pub mod queries;
pub mod repository;
