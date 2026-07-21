//! Stripe partner adapter (ADR-20260718-213352) — a self-contained vertical slice.
//!
//! - [`acl`] — framework-free Anti-Corruption Layer: `Stripe-Signature` verification, Stripe→domain event
//!   mapping, and the idempotent [`acl::StripeWebhookIngestor`] over the `application` EventStore port.
//! - `http` — the thin axum shell exposing `POST /adapters/stripe/webhooks`; mount [`routes`] into the monolith
//!   server, or run the standalone `stripe-webhook` binary (see `main.rs`) as its own web service.
//! - [`outbound`] — the OUTBOUND Stripe client: [`StripePaymentGateway`], the real adapter behind the
//!   generated `PaymentService` port (services.yaml `payment`, issue #26): create PaymentIntent with
//!   the envelope's `orderId`/`restaurantId` metadata refs, request refunds.

pub mod acl;
mod http;
pub mod outbound;
pub mod raw;

pub use acl::{RawStripeEvents, StripeWebhookIngestor};
pub use http::routes;
pub use outbound::StripePaymentGateway;
pub use raw::PgRawStripeEvents;
