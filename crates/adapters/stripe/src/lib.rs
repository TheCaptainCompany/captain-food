//! Stripe partner adapter (ADR-20260718-213352) — a self-contained vertical slice.
//!
//! - [`acl`] — framework-free Anti-Corruption Layer: `Stripe-Signature` verification, Stripe→domain event
//!   mapping, and the idempotent [`acl::StripeWebhookIngestor`] over the `application` EventStore port.
//! - `http` — the thin axum shell exposing `POST /adapters/stripe/webhooks`; mount [`routes`] into the monolith
//!   server, or run the standalone `stripe-webhook` binary (see `main.rs`) as its own web service.

pub mod acl;
mod http;

pub use acl::StripeWebhookIngestor;
pub use http::routes;
