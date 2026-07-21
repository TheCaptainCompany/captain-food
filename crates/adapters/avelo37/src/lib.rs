//! Avelo37 delivery-partner adapter (ADR-20260718-213352, issue #28) — a self-contained vertical slice.
//!
//! - [`acl`] — framework-free Anti-Corruption Layer: `Avelo37-Signature` verification, partner→domain
//!   event mapping (`delivery.accepted` / `delivery.declined` / `delivery.status_updated` →
//!   `DeliveryAcceptedByPartner` / `DeliveryRejectedByPartner` / `DeliveryStatusUpdated`), and the
//!   idempotent [`acl::Avelo37WebhookIngestor`] over the two-layer inbox (ADR-20260720-015400).
//! - `http` — the thin axum shell exposing `POST /adapters/avelo37/webhooks`; mount [`routes`] into the
//!   monolith server, or run the standalone `avelo37-webhook` binary (see `main.rs`) as its own web
//!   service.
//! - [`outbound`] — the OUTBOUND Avelo37 client: [`Avelo37DeliveryGateway`], the real adapter behind
//!   the generated `DeliveryService` port (services.yaml `delivery`): POST the offered job to the
//!   partner API with the `deliveryJobId` reference the webhook facts echo back.
//!
//! The partner's answers NEVER come back through the outbound call — they arrive asynchronously as
//! verified webhooks, recorded as inbound facts (CLAUDE.md "Commands vs inbound events").

pub mod acl;
mod http;
pub mod outbound;
pub mod raw;

pub use acl::{Avelo37WebhookIngestor, RawAvelo37Events};
pub use http::routes;
pub use outbound::Avelo37DeliveryGateway;
pub use raw::PgRawAvelo37Events;
