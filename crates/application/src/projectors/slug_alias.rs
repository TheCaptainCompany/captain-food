//! Hand-written `SlugAliasCompute` (ADR-0040; ADR-20260728-011344).
//!
//! Every column of `SlugAlias` is mechanical — the generated `project_slug_alias` dispatch maps them
//! straight off `RestaurantSlugReconfigured`, which carries all three (`previousSlug`, `restaurantId`,
//! `slug`). So this impl is deliberately empty: there is no computed, cross-stream or accumulated
//! column to own. It exists because the generated dispatch is generic over the trait.
//!
//! Worth stating explicitly, because the shape is unusual: this projection is **keyed by a slug**, not
//! by an aggregate id. One row per SUPERSEDED label, so a restaurant renamed N times leaves N rows on
//! the same Restaurant stream. That is why the projection worker resolves its row key from the event
//! payload's `previousSlug` rather than from the stream name.
//!
//! It is also the only read model no GraphQL query exposes: `hosts.rs` reads it during host resolution
//! to answer a 301 for a superseded address. Giving it an `fk` to `Restaurant` would make the codegen
//! weave a navigation edge into the API graph, which is why the spec deliberately omits one.

use crate::projections::SlugAliasCompute;

pub struct SlugAliasProjector;

impl SlugAliasCompute for SlugAliasProjector {}
