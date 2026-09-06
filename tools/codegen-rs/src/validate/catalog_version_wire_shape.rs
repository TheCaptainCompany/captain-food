//! The catalog coordinate never reaches the wire as a bare scalar (ADR-20260906-192007 D-L,
//! slice 3b of PROP-20260831-134539 "the priced quote token"). `domain::catalog_as_of::
//! CatalogVersion` is a SERVER-CARRIED coordinate only — no `scalars.yaml` declaration exists for
//! it, and none should: a `CatalogVersion` GraphQL scalar would let a client mint or replay a
//! coordinate the server never signed (D1's option 3, refused everywhere at the briefing). The
//! opaque, SIGNED `quote` word (D-A/D-J) is the only wire-carried form of "which coordinate" —
//! this module is the regression guard that keeps the back door shut: `CatalogVersion` must
//! appear in ZERO `input { }` blocks and ZERO field-argument lists of the generated SDL, for
//! good, not merely "not yet".
//!
//! Deliberately its OWN new module (D-L) rather than a tail addition to `validate/core.rs` or
//! `tests.rs`: [#925 "Emit the citation graph"](https://github.com/TheCaptainCompany/captain-food/issues/925)
//! rewrites `tests.rs` in the same window this deliverable lands in, and a shared write-set on the
//! SAME file is exactly the collision the architect's briefing named (holub).
//!
//! Operates on [`crate::emit_schema`]'s own output — the actual SDL the emitter produces, not a
//! hand-maintained mirror of it — so a future emitter change that starts spelling `CatalogVersion`
//! anywhere on the wire fails HERE, at the seam that would let it through, rather than being
//! caught later by a client integration surprise.

use crate::*;

const RULE: &str = "catalog-version-never-on-the-wire";
const FORBIDDEN: &str = "CatalogVersion";

/// Every `input Name { ... }` block's name and body, as [`crate::input_types_block`] emits them:
/// `input {Name} {\n{fields}\n}`, one field per line, no nested braces (fields are scalars, enums
/// or lists of those — never an inline object literal). Blocks are joined by blank lines, so a
/// non-nested `\{([^{}]*)\}` capture is exact for this generator's own output shape, wherever the
/// block starts in the string (start of file or after a blank/prior block).
fn input_blocks(sdl: &str) -> Vec<(String, String)> {
    let re = regex::Regex::new(r"(?m)^input\s+(\w+)\s*\{([^{}]*)\}").expect("static regex compiles");
    re.captures_iter(sdl).map(|c| (c[1].to_string(), c[2].to_string())).collect()
}

/// Every `(...)` argument list in the generated SDL, paired with the field name it follows
/// (`  fieldName(args): ReturnType` — [`crate::query_block`]/[`crate::mutation_block`]/
/// [`crate::subscription_block`]'s own shape). Directive parens (`@auth(requires: [...])`, etc.)
/// are included too — harmless, since `CatalogVersion` is never a role or view-name literal, and
/// excluding them would need directive-name bookkeeping this regression guard does not need.
fn field_argument_lists(sdl: &str) -> Vec<(String, String)> {
    let re = regex::Regex::new(r"([\w@]+)\(([^()]*)\)").expect("static regex compiles");
    re.captures_iter(sdl).map(|c| (c[1].trim_start_matches('@').to_string(), c[2].to_string())).collect()
}

pub(crate) fn check_catalog_version_never_on_the_wire(model: &Model, issues: &mut Vec<Issue>) {
    let sdl = emit_schema(model);

    for (name, body) in input_blocks(&sdl) {
        if body.contains(FORBIDDEN) {
            issues.push(err(
                RULE,
                format!("schema.generated.graphql#input {name}"),
                format!(
                    "input '{name}' names `CatalogVersion` -- the catalog coordinate is \
                     server-carried only (ADR-20260906-192007 D-L); a client-suppliable coordinate \
                     is exactly D1's refused option 3. Use the opaque, signed `quote` (`CartQuote`) \
                     instead."
                ),
            ));
        }
    }

    for (field, args) in field_argument_lists(&sdl) {
        if args.contains(FORBIDDEN) {
            issues.push(err(
                RULE,
                format!("schema.generated.graphql#{field}(...)"),
                format!(
                    "'{field}' takes a `CatalogVersion` argument -- the catalog coordinate is \
                     server-carried only (ADR-20260906-192007 D-L); it must never be a client-\
                     suppliable field argument either."
                ),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Model {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        load_model(&root.join("specs")).expect("load real specs")
    }

    /// The corpus today: no `scalars.yaml#/CatalogVersion` exists, so the guard is silent by
    /// default -- this pins the vacuous-pass case so a future scalar addition alone (with no wire
    /// use yet) does not trip it.
    #[test]
    fn the_real_corpus_never_puts_catalog_version_on_the_wire() {
        let mut issues = Vec::new();
        check_catalog_version_never_on_the_wire(&model(), &mut issues);
        assert!(issues.is_empty(), "unexpected findings: {:?}", issues.iter().map(|i| &i.message).collect::<Vec<_>>());
    }

    /// RED-FIRST (ADR-20260906-192007:34): an `input { }` block naming `CatalogVersion` is caught.
    #[test]
    fn a_catalog_version_typed_input_field_is_caught() {
        let sdl = "input PlaceOrderInput {\n  catalogVersion: CatalogVersion\n  cartId: CartId!\n}\n\ninput Other {\n  x: Int\n}";
        let blocks = input_blocks(sdl);
        assert_eq!(blocks.len(), 2, "both input blocks must be found: {blocks:?}");
        assert!(
            blocks.iter().any(|(name, body)| name == "PlaceOrderInput" && body.contains(FORBIDDEN)),
            "the CatalogVersion-carrying input block must be found: {blocks:?}"
        );
    }

    /// RED-FIRST: a field argument list naming `CatalogVersion` is caught (the back door D-L
    /// forbids even if no `input` block is involved).
    #[test]
    fn a_catalog_version_typed_field_argument_is_caught() {
        let sdl = "type Query {\n  cart(catalogVersion: CatalogVersion): Cart\n}";
        let args = field_argument_lists(sdl);
        assert!(
            args.iter().any(|(field, list)| field == "cart" && list.contains(FORBIDDEN)),
            "the CatalogVersion-carrying argument list must be found: {args:?}"
        );
    }

    /// A field argument list that does NOT name `CatalogVersion` must not be flagged (no false
    /// positive on the ordinary `(input: FooQueryInput)` shape every other query already uses).
    #[test]
    fn an_ordinary_input_wrapped_argument_is_not_flagged() {
        let sdl = "type Query {\n  cart(input: CartQueryInput!): Cart @auth(requires: [CUSTOMER, ADMIN])\n}";
        let args = field_argument_lists(sdl);
        assert!(
            args.iter().all(|(_, list)| !list.contains(FORBIDDEN)),
            "no argument list should contain CatalogVersion here: {args:?}"
        );
    }
}
