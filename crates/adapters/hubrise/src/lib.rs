//! HubRise partner adapter (ADR-20260718-213352) — a self-contained vertical slice.
//!
//! - [`acl`] — framework-free Anti-Corruption Layer: `X-HubRise-Hmac-SHA256` verification + callback
//!   envelope parsing.
//! - [`api`] — the OUTBOUND OAuth2 client (domain enrichment): pull catalog/inventory from HubRise after a
//!   (stateless) callback, since those callbacks carry no state.
//! - `http` — the thin axum shell exposing `POST /adapters/hubrise/webhooks`; mount [`routes`] into the monolith
//!   server, or run the standalone `hubrise-webhook` binary (see `main.rs`) as its own web service.
//!
//! - [`enrich`] — the **domain wiring** (now landed): callback → `api` pull → ACL map (deterministic
//!   UUIDv5-of-HubRise-id) → `ImportCatalog` handler / per-SKU `update_offer_stock`. The derived `OfferId`
//!   equals the one `ImportCatalog` assigns, so an inventory update targets the imported offer and the
//!   events project (reconciled with the Catalog aggregate — see the `enrich` module docs).
//!
//! - [`connect`] / [`connections`] — the **connect flow** (issue #20): `GET /adapters/hubrise/connect`
//!   → OAuth authorize → `GET /adapters/hubrise/oauth/callback` → token exchange → provision the
//!   `RestaurantAccount`/`Restaurant`(s)/`Catalog`(s) with the derived ids → persist the
//!   account-scoped token in `hubrise_connections`; the enricher resolves callbacks to that token
//!   (the global `HUBRISE_ACCESS_TOKEN` fallback is retired).
//!
//! Mount [`routes`] (with an optional [`enrich::Enricher`] / [`connect::ConnectService`]) into the
//! monolith, or run the standalone `hubrise-webhook` binary (`main.rs`) as its own web service — which
//! builds the real Postgres-backed enricher + connect flow over the [`api::HubRiseApi`].

pub mod acl;
pub mod api;
pub mod connect;
pub mod connections;
pub mod enrich;
mod http;
pub mod raw;

pub use connect::{ConnectService, HttpHubRiseConnectGateway, HubRiseConnectFlow};
pub use connections::{HubRiseConnections, PgHubRiseConnections};
pub use enrich::{Enricher, HubRiseEnricher};
pub use http::{routes, HubRiseWebhookState};
pub use raw::{PgRawHubRiseCallbacks, RawHubRiseCallbacks};
