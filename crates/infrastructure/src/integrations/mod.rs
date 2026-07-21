//! Integrations — the Anti-Corruption Layer (ADR-0035). External systems NEVER talk to the domain
//! directly: each integration translates the partner's shapes/vocabulary into ordinary domain
//! commands (or records inbound facts), keeping HubRise/Stripe/SIRENE idioms out of `domain`.
//!
//! - [`sirene`] — the SIRENE ACL: raw INSEE établissements → `RegisterRestaurant` prospects
//!   (ADR-0019/0020/0027); the HTTP client/wire types live in the `sirene_ingest` crate (ADR-0045).
//! - [`sync_sirene_worker`] — the on-app worker draining the `external_sirene_restaurants` staging
//!   table through the ACL into the ordinary write path (register/close, ADR-0045).
//! - [`google`] — Google Business Profile seams (ownership proof + order-link probe, ADR-0019/0021);
//!   fail-closed stand-ins until the real Google adapters land.
//! - [`supabase_auth`] — the wrapped auth provider seam (phone-OTP + email magic link, ADR-0015);
//!   fail-closed stand-in until the real `supabase-acl` adapter lands.
//! - [`payments`] — the Stripe OUTBOUND seams (create-intent gateway + checkout-snapshot source);
//!   fail-closed stand-ins until the real Stripe adapter lands (integration workstream).
//! - [`retention_sweep_worker`] — the periodic caller of the `sweep_retention()` SQL function
//!   (journal/mirror retention, ADR-20260721-025159); never touches `domain_events`.
//!
//! Partner **webhook** adapters (Stripe, HubRise) now live in their own self-contained crates under
//! `crates/adapters/*` (ADR-20260718-213352) — each an ACL + HTTP shell + standalone binary, so it can
//! deploy as its own web service. They are deliberately NOT part of `infrastructure`. Later: delivery
//! partner.

pub mod delivery_gateway;
pub mod delivery_offer_timeout_worker;
pub mod google;
pub mod inbound_drain_worker;
pub mod payments;
pub mod retention_sweep_worker;
pub mod sirene;
pub mod supabase_auth;
pub mod sync_sirene_worker;
