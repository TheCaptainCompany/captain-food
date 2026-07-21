//! Uber Direct delivery-partner adapter (issue #57, ADR-20260721-172500) — a self-contained vertical
//! slice and a `DeliveryProvider = PARTNER` implementation via the Uber **Direct** delivery API (NOT
//! the Uber Eats marketplace; distinct from the price-comparison ADRs 0022/0023/0024/0030), mirroring
//! `crates/adapters/avelo37` and `crates/adapters/coopcycle`.
//!
//! - [`config`] — the single-endpoint config + OAuth2 client-credentials, parsed from `UBER_DIRECT_*`
//!   (fully-set ⇒ configured; unset ⇒ no-op stand-in; partial ⇒ misconfiguration error).
//! - [`acl`] — framework-free Anti-Corruption Layer: `X-Uber-Signature` raw-body HMAC verification,
//!   Uber→domain event mapping (courier assignment → `DeliveryAcceptedByPartner`, undeliverable →
//!   `DeliveryRejectedByPartner`, progress → `DeliveryStatusUpdated`), and the idempotent
//!   [`acl::UberDirectWebhookIngestor`] over the two-layer inbox (ADR-20260720-015400).
//! - `http` — the thin axum shell exposing `POST /adapters/uber-direct/webhooks`; mount [`routes`]
//!   into the monolith server, or run the standalone `uber-direct-webhook` binary (main.rs).
//! - [`outbound`] — the OUTBOUND client [`UberDirectDeliveryGateway`], the real adapter behind the
//!   generated `DeliveryService` port: fetch the OAuth2 token, then POST the offered job (Create
//!   Delivery) with our `deliveryJobId` carried as `external_id`.
//!
//! Uber's answers NEVER come back through the outbound call — they arrive asynchronously as verified
//! webhooks, recorded as inbound facts (CLAUDE.md "Commands vs inbound events").

pub mod acl;
pub mod config;
mod http;
pub mod outbound;
pub mod raw;

pub use acl::{RawUberDirectEvents, UberDirectWebhookIngestor};
pub use config::UberDirectConfig;
pub use http::{routes, UberDirectWebhookState};
pub use outbound::UberDirectDeliveryGateway;
pub use raw::PgRawUberDirectEvents;
