use crate::*;

// ─── crates/server/src/graphql/generated/ (Stage 1a — async-graphql type layer from api.yaml) ───
//
// The server hosts the GraphQL surface with async-graphql, but `domain` must stay GraphQL-free
// (ADR-0035) and the orphan rule forbids implementing async-graphql's foreign traits on the foreign
// domain newtypes from `server`. So the generator emits a SERVER-SIDE wrapper layer: one wrapper
// newtype (GraphQL scalar) / mirror enum per scalars.yaml type with `From` conversions both ways,
// SimpleObject output types, InputObject inputs, and a QueryRoot exposing every api.yaml query
// (read resolvers stubbed until the read-model repositories land).

/// Rust-safe struct name for a GraphQL type emitted into the server layer: a spec type may collide with
/// a Rust prelude name (the API type `Option` does) — emitted as `<Name>_` plus an explicit
/// `#[graphql(name = "<Name>")]`, so the GraphQL name stays the spec name.
pub(crate) fn gql_rust_name(name: &str) -> String {
    match name {
        "Option" | "Box" | "String" | "Vec" | "Result" => format!("{}_", name),
        _ => name.to_string(),
    }
}

/// Rust type of an inline (non-`$ref`) schema primitive in the GraphQL layer. `integer` → `i64`
/// (async-graphql serializes any Rust integer as the GraphQL `Int`, and the domain uses `i64`);
/// `date-time` strings → `chrono::DateTime<Utc>` (the `DateTime` scalar via async-graphql's `chrono`
/// feature).
pub(crate) fn rust_inline_primitive(t: &str, format: Option<&str>) -> String {
    match t {
        "integer" => "i64".into(),
        "boolean" => "bool".into(),
        "number" => "f64".into(),
        "string" if format == Some("date-time") => "chrono::DateTime<chrono::Utc>".into(),
        _ => "String".into(),
    }
}

/// Rust base type of a spec node in the server GraphQL layer — mirrors `base_type` (the SDL emitter):
/// scalars.yaml refs → the wrapper scalar / mirror enum, other refs → the generated struct
/// (`…Input`-suffixed when `input`), arrays → `Vec<…>`, inline primitives via `rust_inline_primitive`.
pub(crate) fn rust_base_type(node: &Value, ctx: &str, input: bool) -> String {
    if let Some(rf) = node.get("$ref").and_then(|x| x.as_str()) {
        let file = ref_target_file(rf, ctx);
        let name = parse_ref(rf).and_then(|p| p.path.into_iter().next()).unwrap_or_else(|| "String".into());
        if file.as_deref() == Some("scalars.yaml") {
            return gql_rust_name(&name);
        }
        return if input { format!("{}Input", name) } else { gql_rust_name(&name) };
    }
    if node.get("type").and_then(|x| x.as_str()) == Some("array") {
        if let Some(items) = node.get("items") {
            return format!("Vec<{}>", rust_base_type(items, ctx, input));
        }
    }
    rust_inline_primitive(
        node.get("type").and_then(|x| x.as_str()).unwrap_or("string"),
        node.get("format").and_then(|x| x.as_str()),
    )
}

/// Rust base type of an api.yaml field — mirrors `api_field_type` (without the nullability suffix).
pub(crate) fn rust_api_field_base(model: &Model, f: &ApiField, input: bool) -> String {
    let mut base = if f.is_ref {
        if input && !scalar_names(model).contains(&f.ty) {
            format!("{}Input", f.ty)
        } else {
            gql_rust_name(&f.ty)
        }
    } else {
        rust_inline_primitive(&f.ty, f.format.as_deref())
    };
    if f.array {
        base = format!("Vec<{}>", base);
    }
    base
}

/// Spec `description` → Rust `///` doc lines at `indent` (one per non-empty trimmed line). async-graphql
/// turns doc comments into GraphQL descriptions (SimpleObject/InputObject structs + fields, Enum,
/// `#[Object]` resolvers), so the spec documentation reaches introspection/GraphiQL. No description →
/// no lines.
pub(crate) fn push_doc(out: &mut String, indent: &str, desc: Option<&str>) {
    if let Some(d) = desc {
        for line in d.trim().lines() {
            let line = line.trim();
            if !line.is_empty() {
                out.push_str(&format!("{}/// {}\n", indent, line));
            }
        }
    }
}

/// Push one generated GraphQL struct field: the spec description as a `///` doc (→ introspection), an
/// explicit `#[graphql(name = …)]` (the exact SDL name — independent of derive rename rules and raw
/// `r#` idents), `#[serde(default)]` on arrays (lenient jsonb → typed mapping), raw-escaped snake_case
/// ident.
pub(crate) fn push_gql_field(out: &mut String, name: &str, base: &str, non_null: bool, desc: Option<&str>) {
    let ident = rust_ident(&snake_field(name));
    let ty = if non_null { base.to_string() } else { format!("Option<{}>", base) };
    push_doc(out, "    ", desc);
    out.push_str(&format!("    #[graphql(name = \"{}\")]\n", name));
    if ty.starts_with("Vec<") {
        out.push_str("    #[serde(default)]\n");
    }
    out.push_str(&format!("    pub {}: {},\n", ident, ty));
}

/// Open one generated server-side GraphQL struct (`derive` = `SimpleObject` or `InputObject`), with the
/// spec description as a `///` doc (→ the type's introspection description). serde derives use
/// `rename_all = "camelCase"` so the struct (de)serializes to/from the spec wire shape — this is what
/// lets jsonb read-model columns deserialize straight into the typed output structs.
pub(crate) fn push_gql_struct_open(out: &mut String, gql_name: &str, derive: &str, desc: Option<&str>) {
    let rust = gql_rust_name(gql_name);
    out.push('\n');
    push_doc(out, "", desc);
    out.push_str(&format!(
        "#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, async_graphql::{})]\n#[serde(rename_all = \"camelCase\")]\n",
        derive
    ));
    if rust != gql_name {
        out.push_str(&format!("#[graphql(name = \"{}\")]\n", gql_name));
    }
    out.push_str(&format!("pub struct {} {{\n", rust));
}

/// Push the fields of a spec object def (entities.yaml / commands.yaml shape) — mirrors `object_fields`.
pub(crate) fn push_gql_object_fields(out: &mut String, def: &Value, ctx: &str, input: bool) {
    push_gql_object_fields_excluding(out, def, ctx, input, &HashSet::new())
}

/// `push_gql_object_fields`, omitting the named properties entirely — mirrors
/// `api::object_fields_excluding`, the server-side half of #865's `derived:` omission (a derived
/// property carries no Rust struct field at all, so a client attempt to smuggle it fails
/// async-graphql's OWN input-object validation before any resolver code runs).
pub(crate) fn push_gql_object_fields_excluding(out: &mut String, def: &Value, ctx: &str, input: bool, exclude: &HashSet<&str>) {
    let props = match def.get("properties").and_then(|p| p.as_mapping()) {
        Some(m) => m,
        None => return,
    };
    let required: HashSet<&str> = def
        .get("required")
        .and_then(|r| r.as_sequence())
        .map(|s| s.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    for (k, p) in props {
        let name = match k.as_str() {
            Some(s) => s,
            None => continue,
        };
        if exclude.contains(name) {
            continue;
        }
        if input && p.get("readOnly").and_then(|x| x.as_bool()) == Some(true) {
            continue;
        }
        let base = rust_base_type(p, ctx, input);
        let non_null = if input {
            required.contains(name)
        } else {
            p.get("nullable").and_then(|x| x.as_bool()) != Some(true)
        };
        push_gql_field(out, name, &base, non_null, p.get("description").and_then(|x| x.as_str()));
    }
}

/// Emit `crates/server/src/graphql/generated/scalars.rs` — the async-graphql wrapper layer over the
/// domain scalars (orphan rule): non-enum scalars.yaml types become wrapper newtypes registered via
/// `async_graphql::scalar!`, enums become mirror `async_graphql::Enum`s (verbatim variants), each with
/// `From` conversions both ways to `domain::generated::scalars`.
pub(crate) fn emit_server_scalars(model: &Model) -> String {
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/scalars.yaml — do not edit by hand.\n// Server-side async-graphql scalar layer: `domain` stays GraphQL-free (ADR-0035) and the orphan rule\n// forbids implementing async-graphql traits on domain newtypes here, so each scalars.yaml type gets a\n// wrapper newtype (GraphQL scalar) / mirror enum with `From` conversions both ways.\n#![allow(dead_code)]\n#![allow(non_camel_case_types)]\n\nuse domain::generated::scalars as ds;\n",
    );
    if let Some(Value::Mapping(m)) = model.defs.get("scalars.yaml") {
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            out.push('\n');
            out.push_str(&scalar_doc(node));
            if let Some(vals) = node.get("enum").and_then(|e| e.as_sequence()) {
                let variants: Vec<&str> = vals.iter().filter_map(|v| v.as_str()).collect();
                let catch_all = node.get("readOnlyCatchAll").and_then(|c| c.as_str());
                out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, async_graphql::Enum)]\n");
                out.push_str(&format!("pub enum {} {{\n", name));
                for v in &variants {
                    out.push_str(&format!("    #[graphql(name = \"{}\")]\n    {},\n", v, v));
                }
                out.push_str("}\n");
                match catch_all {
                    // `readOnlyCatchAll` (#639 part C step 4-i, ADR-20260904-081527 §3): the SDL
                    // mirror enum has no member for it, so the domain → SDL direction cannot be a
                    // total `From` (and `impl From<Foreign> for Option<Foreign>` violates the
                    // orphan rule — neither side is local) — a plain function instead. The wire
                    // field carrying this type MUST be nullable and the catch-all renders `null`
                    // (never a panic, never a fabricated real value).
                    Some(catch_all) => {
                        out.push_str(&format!("/// `readOnlyCatchAll`: {catch_all} decodes to `None` — unspellable on the wire, renders null.\npub fn {}_from_domain(v: ds::{}) -> Option<{}> {{\n    match v {{\n", snake_type(name), name, name));
                        for v in &variants {
                            out.push_str(&format!("        ds::{}::{} => Some({}::{}),\n", name, v, name, v));
                        }
                        out.push_str(&format!("        ds::{}::{} => None,\n", name, catch_all));
                        out.push_str("    }\n}\n");
                    }
                    None => {
                        out.push_str(&format!("impl From<ds::{}> for {} {{\n    fn from(v: ds::{}) -> Self {{\n        match v {{\n", name, name, name));
                        for v in &variants {
                            out.push_str(&format!("            ds::{}::{} => Self::{},\n", name, v, v));
                        }
                        out.push_str("        }\n    }\n}\n");
                    }
                }
                out.push_str(&format!("impl From<{}> for ds::{} {{\n    fn from(v: {}) -> Self {{\n        match v {{\n", name, name, name));
                for v in &variants {
                    out.push_str(&format!("            {}::{} => Self::{},\n", name, v, v));
                }
                out.push_str("        }\n    }\n}\n");
                continue;
            }
            let ty = node.get("type").and_then(|t| t.as_str()).unwrap_or("string");
            let is_uuid = node.get("format").and_then(|f| f.as_str()) == Some("uuid");
            let (derives, inner) = if is_uuid {
                ("Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize", "uuid::Uuid")
            } else if ty == "integer" {
                ("Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize", "i64")
            } else if ty == "number" {
                ("Debug, Clone, Copy, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize", "f64")
            } else {
                ("Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize", "String")
            };
            out.push_str(&format!("#[derive({})]\n", derives));
            out.push_str(&format!("pub struct {}(pub {});\n", name, inner));
            // The scalar! macro takes the introspection description explicitly (doc comments don't
            // reach it) — whitespace-collapsed to one line, escaped as a Rust string literal.
            match node.get("description").and_then(|d| d.as_str()) {
                Some(d) => out.push_str(&format!("async_graphql::scalar!({}, {:?}, {:?});\n", name, name, ws1(d.trim()))),
                None => out.push_str(&format!("async_graphql::scalar!({});\n", name)),
            }
            out.push_str(&format!(
                "impl From<ds::{}> for {} {{\n    fn from(v: ds::{}) -> Self {{\n        Self(v.0)\n    }}\n}}\n",
                name, name, name
            ));
            out.push_str(&format!(
                "impl From<{}> for ds::{} {{\n    fn from(v: {}) -> Self {{\n        Self(v.0)\n    }}\n}}\n",
                name, name, name
            ));
        }
    }
    out
}

/// Emit `crates/server/src/graphql/generated/types.rs` — the GraphQL output types (SimpleObject),
/// mirroring `output_types_block`: entities.yaml types not registered in api.yaml `types`, then the
/// api.yaml types, each with its FK-derived navigation fields (data fields, resolved empty until the
/// read resolvers land). Includes the worked `From<RestaurantRow> for Restaurant` mapping (Stage 1a).
pub(crate) fn emit_server_types(model: &Model) -> String {
    let api = parse_api(model);
    let views = parse_views(model);
    let registered: HashSet<String> = api.types.iter().map(|t| t.name.clone()).collect();
    let nav = nav_fields(&views, &registered);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/api.yaml + specs/entities.yaml — do not edit by hand.\n// GraphQL output types (async-graphql SimpleObject), mirroring the generated SDL: entities.yaml types\n// not registered as api.yaml projections, then the api.yaml types, each with its FK-derived navigation\n// fields — plain data fields the resolvers hydrate: SINGLE-TARGET edges (Cart.restaurant,\n// DeliveryJob.restaurant, Catalog.restaurant, Prospect.restaurant, Order.restaurant) are built\n// per request via Restaurant::at; reverse COLLECTION edges (Restaurant.carts/orders/…,\n// Order.deliveryJobs) still resolve empty until their read resolvers land.\n#![allow(dead_code)]\n#![allow(non_camel_case_types)]\n\nuse actor_client::supervision::{MailboxLaneRow, PoisonedMessageRow};\nuse application::projections::{CartRow, CatalogRow, CustomerCreditBalanceRow, CustomerRow, OrderConversationRow, OrderTrackingRow, ProspectionPipelineRow, RestaurantRow};\nuse application::queries::{DeliveryJobRow, DeliveryPartnerAvailabilityRow, DeliverySatisfactionRow, PricingPolicyRow, ReclamationRow, RefundRow, UberEstimationPolicyRow, UberSplitPolicyRow};\nuse domain::generated::scalars as ds;\n\nuse super::scalars::*;\n",
    );
    let nav_roles: HashMap<String, HashMap<String, Vec<String>>> = api
        .types
        .iter()
        .map(|t| (t.name.clone(), t.nav_roles.iter().cloned().collect()))
        .collect();
    let push_nav = |out: &mut String, name: &str| {
        if let Some(nfs) = nav.get(name) {
            for n in nfs {
                let base = if n.list { format!("Vec<{}>", gql_rust_name(&n.target)) } else { gql_rust_name(&n.target) };
                let roles = nav_roles.get(name).and_then(|m| m.get(&n.field));
                match roles.and_then(|r| acl_role_set(model, r)) {
                    // Guarded nav edge (#22): the operations' guard/visible pair, fully qualified so
                    // types.rs needs no acl import.
                    Some(set) => {
                        let ident = acl_set_ident(&set);
                        let ty = if n.list || !n.nullable { base.clone() } else { format!("Option<{}>", base) };
                        out.push_str(&format!(
                            "    #[graphql(name = \"{}\", guard = \"super::acl::RoleGuard::new(super::acl::ALLOW_{})\", visible = \"super::acl::visible_{}\")]\n",
                            n.field,
                            ident.to_uppercase(),
                            ident
                        ));
                        if ty.starts_with("Vec<") {
                            out.push_str("    #[serde(default)]\n");
                        }
                        out.push_str(&format!("    pub {}: {},\n", rust_ident(&snake_field(&n.field)), ty));
                    }
                    None => push_gql_field(out, &n.field, &base, n.list || !n.nullable, None),
                }
            }
        }
    };
    if let Some(Value::Mapping(m)) = model.defs.get("entities.yaml") {
        for (k, def) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            if registered.contains(name) {
                continue;
            }
            push_gql_struct_open(&mut out, name, "SimpleObject", def.get("description").and_then(|d| d.as_str()));
            push_gql_object_fields(&mut out, def, "entities.yaml", false);
            push_nav(&mut out, name);
            out.push_str("}\n");
        }
    }
    for t in &api.types {
        push_gql_struct_open(&mut out, &t.name, "SimpleObject", t.description.as_deref());
        for f in &t.properties {
            let base = rust_api_field_base(model, f, false);
            push_gql_field(&mut out, &f.name, &base, !f.nullable, f.description.as_deref());
        }
        push_nav(&mut out, &t.name);
        out.push_str("}\n");
    }
    // Restaurant: NO `From<RestaurantRow>` is emitted, deliberately (RSO-1, DECISIONS §43;
    // compiler-first — ADR-20260803-234035, the Cart precedent below): `serviceWindow` is a
    // verdict AT AN INSTANT, so an infallible clock-less row→Restaurant conversion can only
    // FABRICATE one. The one constructor is `Restaurant::at(row, now, horizon)` — a clock-less
    // Restaurant is unspellable and the compiler enumerates every call site.
    out.push_str(
        "\n/// Read-model row + the REQUEST CLOCK → API type (RSO-1, DECISIONS §43). Deliberately NOT a\n/// `From`: `serviceWindow` is a verdict AT AN INSTANT, so a clock-less `Restaurant` is unspellable —\n/// deleting the old `From<RestaurantRow>` makes the compiler enumerate every call site\n/// (ADR-20260803-234035; the Cart non-impl below is the precedent). `now`/`horizon` are read ONCE\n/// per request at the transport seam (`graphql::service_clock`) and threaded down, so every row of\n/// one request agrees on \"now\" and DeliveryJob's two embedded verdicts cannot disagree. jsonb\n/// columns deserialize into the typed structs; `orderable` = ACTIVE_PARTNER + status ACTIVE +\n/// acceptance != PAUSED (api.yaml); navigation fields resolve empty until the read resolvers land.\nimpl Restaurant {\n    pub fn at(row: RestaurantRow, now: chrono::DateTime<chrono::Utc>, horizon: chrono::Duration) -> Self {\n        // The verdict reads the SAME stored jsonb the `openingHours` field exposes, through the ONE\n        // domain function the checkout guard also calls (`domain::service_window::serving_at`) — the\n        // only construction in which badge and guard cannot disagree. Malformed jsonb parses to []\n        // = HOURS_UNDECLARED, never a panic (PAN-1 is the anti-pattern); the cutoff has no mapped\n        // source today (HubRise `cutoff_time`), so `None` degrades explicitly to door-close.\n        let declared_hours: Vec<domain::generated::entities::OpeningHoursSlot> =\n            serde_json::from_value(row.opening_hours.clone()).unwrap_or_default();\n        let window = domain::service_window::serving_at(&declared_hours, row.timezone.as_ref(), None, now, horizon);\n        Self {\n            id: row.restaurant_id.into(),\n            account_id: row.restaurant_account_id.map(Into::into),\n            listing_status: row.listing_status.into(),\n            orderable: row.listing_status == ds::RestaurantListingStatus::ACTIVE_PARTNER\n                && row.status == ds::RestaurantStatus::ACTIVE\n                && row.order_acceptance != ds::OrderAcceptanceMode::PAUSED,\n            external_identifiers: row\n                .external_identifiers\n                .and_then(|v| serde_json::from_value(v).ok())\n                .unwrap_or_default(),\n            slug: row.slug.map(Into::into),\n            display_name: row.display_name.into(),\n            description: row.description.map(Into::into),\n            tags: row.tags.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),\n            cuisine_category: row.cuisine_category.map(Into::into),\n            rating: row.rating.map(Into::into),\n            reviews_count: row.reviews_count,\n            website: row.website.map(Into::into),\n            gbp_order_url: row.gbp_order_url.map(Into::into),\n            gbp_link_status: row.gbp_link_status.map(Into::into),\n            address: serde_json::from_value(row.address).expect(\"Restaurant.address: invalid jsonb\"),\n            location: row.location.and_then(|v| serde_json::from_value(v).ok()),\n            opening_hours: serde_json::from_value(row.opening_hours).unwrap_or_default(),\n            service_window: ServiceWindow {\n                verdict: window.verdict.into(),\n                opens_at: window.opens_at,\n                last_order_at: window.last_order_at,\n                evaluated_at: window.evaluated_at,\n                valid_until: window.valid_until,\n            },\n            status: row.status.into(),\n            order_acceptance: row.order_acceptance.into(),\n            default_currency: row.default_currency.into(),\n            timezone: row.timezone.map(Into::into),\n            preparation_time_minutes: row.preparation_time_minutes,\n            updated_at: row.updated_at,\n            delivery_jobs: Vec::new(),\n            prospects: Vec::new(),\n            catalogs: Vec::new(),\n            carts: Vec::new(),\n            orders: Vec::new(),\n        }\n    }\n}\n",
    );
    // Prospect: the FK-derived `restaurant` navigation field is NON-NULL, so the mapping takes the
    // joined Restaurant (already clock-evaluated via `Restaurant::at` — RSO-1) alongside the
    // pipeline row (the resolver performs the join and threads the request clock).
    out.push_str(
        "\n/// Read-model rows → API type: the ProspectionPipeline row plus the joined `Restaurant` (built\n/// once by the resolver via `Restaurant::at` — the FK-derived `restaurant` navigation field is\n/// non-null, so the resolver hydrates it from the Restaurant read model with the request clock).\nimpl From<(ProspectionPipelineRow, Restaurant)> for Prospect {\n    fn from((row, restaurant): (ProspectionPipelineRow, Restaurant)) -> Self {\n        Self {\n            restaurant_id: row.restaurant_id.into(),\n            score: row.score.into(),\n            pipeline_status: row.pipeline_status.into(),\n            contacts_count: row.contacts_count,\n            last_contacted_at: row.last_contacted_at,\n            replied_at: row.replied_at,\n            restaurant,\n        }\n    }\n}\n",
    );
    // Catalog: categories/products/optionLists are carried inside the projected `tree` jsonb; the
    // FK-derived `restaurant` navigation field is NON-NULL, so the mapping takes the joined Restaurant
    // row (the resolver performs the join).
    out.push_str(
        "\n/// One section of the projected `Catalog.tree` jsonb (camelCase keys, as folded by the\n/// `CatalogProjector` with the derived per-offer `stockStatus`), leniently parsed: an absent key or\n/// an empty tree (a catalog created before any content event) yields an empty list.\npub(crate) fn catalog_tree_section<T: serde::de::DeserializeOwned>(tree: &serde_json::Value, key: &str) -> Vec<T> {\n    tree.get(key).cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()\n}\n",
    );
    out.push_str(
        "\n/// Read-model rows → API type: the Catalog row plus the joined `Restaurant` (built once by the\n/// resolver via `Restaurant::at` — non-null `restaurant` navigation field, hydrated with the\n/// request clock). categories/products/optionLists deserialize out of the projected `tree` jsonb.\nimpl From<(CatalogRow, Restaurant)> for Catalog {\n    fn from((row, restaurant): (CatalogRow, Restaurant)) -> Self {\n        Self {\n            id: row.catalog_id.into(),\n            restaurant_id: row.restaurant_id.into(),\n            slug: row.slug.map(Into::into),\n            name: row.name.into(),\n            categories: catalog_tree_section(&row.tree, \"categories\"),\n            products: catalog_tree_section(&row.tree, \"products\"),\n            option_lists: catalog_tree_section(&row.tree, \"optionLists\"),\n            updated_at: row.updated_at,\n            restaurant,\n        }\n    }\n}\n",
    );
    // Cart: NO `From<(CartRow, RestaurantRow)>` is emitted, deliberately (#451 Phase 2b,
    // compiler-first — CLAUDE.md/ADR-20260803-234035). The row is a MONEY-FREE pure fold
    // (ADR-20260810-112836), so any infallible row→Cart conversion can only FABRICATE the priced
    // fields: the impl that used to live here filled `lines: Vec::new()`, `MoneyCents(0)` and
    // `breakdown: None` — exactly the 0,00 EUR payable that ADR forbids, kept alive only by the
    // file's `#![allow(dead_code)]`. Pricing a cart is FALLIBLE (the catalog read can miss an
    // offer) and therefore cannot be a `From`. The one path is
    // `crates/server/src/graphql/cart_read.rs::priced`, which returns a Result. Not emitting the
    // impl makes the fabrication unspellable rather than merely discouraged: a future resolver
    // that reaches for `Cart::from((row, restaurant))` fails to compile.
    // Order: minor-units columns + the row currency rebuild the Money values; the breakdown's
    // `restaurantContribution` is re-derived from the stored leaves; the Uber comparison needs every
    // uber_* column; paymentStatus is the projector's TEXT fold parsed leniently.
    out.push_str(
        "\n/// Minor-units column + the row's currency → the Money value object.\nfn order_money(cents: ds::MoneyCents, currency: &ds::CurrencyCode) -> Money {\n    Money { amount_cents: cents.into(), currency: currency.clone().into() }\n}\n",
    );
    out.push_str(
        "\n/// Read-model rows → API type: the OrderTracking row plus the joined `Restaurant` (built once by\n/// the resolver via `Restaurant::at` — non-null `restaurant` navigation field, request clock). The breakdown's `restaurantContribution` is re-derived as\n/// articles − restaurantPayout (the projection stores the split's leaves); the Uber comparison is\n/// rebuilt only when every `uber_*` column is present; `paymentStatus` is folded as TEXT by the\n/// projector and parsed leniently (unknown → PENDING); nav `deliveryJobs` resolve empty until that\n/// read model lands.\nimpl From<(OrderTrackingRow, Restaurant)> for Order {\n    fn from((row, restaurant): (OrderTrackingRow, Restaurant)) -> Self {\n        let currency = row.currency.clone();\n        let breakdown = PaymentBreakdown {\n            articles: order_money(row.articles_cents.clone(), &currency),\n            delivery: order_money(row.delivery_cents.clone(), &currency),\n            service_fee: order_money(row.service_fee_cents.clone(), &currency),\n            total: order_money(row.total_amount_cents.clone(), &currency),\n            restaurant_contribution: order_money(\n                ds::MoneyCents(row.articles_cents.0 - row.restaurant_payout_cents.0),\n                &currency,\n            ),\n            restaurant_payout: order_money(row.restaurant_payout_cents.clone(), &currency),\n            rider_payout: order_money(row.rider_payout_cents.clone(), &currency),\n            captain_net: order_money(row.captain_net_cents.clone(), &currency),\n        };\n        let uber_comparison = match (\n            row.uber_total_cents,\n            row.uber_restaurant_cents,\n            row.uber_rider_cents,\n            row.uber_platform_cents,\n            row.uber_basis,\n        ) {\n            (Some(total), Some(restaurant_share), Some(rider_share), Some(platform_share), Some(basis)) => {\n                Some(UberComparison {\n                    total: order_money(total, &currency),\n                    restaurant_share: order_money(restaurant_share, &currency),\n                    rider_share: order_money(rider_share, &currency),\n                    platform_share: order_money(platform_share, &currency),\n                    basis: basis.into(),\n                })\n            }\n            _ => None,\n        };\n        Self {\n            id: row.order_id.into(),\n            r#ref: row.r#ref.into(),\n            restaurant_id: row.restaurant_id.into(),\n            customer_id: row.customer_id.map(Into::into),\n            status: row.status.into(),\n            service_type: row.service_type.into(),\n            items: serde_json::from_value(row.items).unwrap_or_default(),\n            total_amount: order_money(row.total_amount_cents, &currency),\n            breakdown,\n            delivery_address: row.delivery_address.and_then(|v| serde_json::from_value(v).ok()),\n            estimated_ready_at: row.estimated_ready_at,\n            placed_at: row.placed_at,\n            status_changed_at: row.status_changed_at,\n            payment_status: match row.payment_status.as_str() {\n                \"CAPTURED\" => PaymentStatus::CAPTURED,\n                \"FAILED\" => PaymentStatus::FAILED,\n                \"REFUNDED\" => PaymentStatus::REFUNDED,\n                _ => PaymentStatus::PENDING,\n            },\n            restaurant_stars: row.restaurant_stars.map(Into::into),\n            rating_comment: row.rating_comment.map(Into::into),\n            rider_thumb: row.rider_thumb.map(Into::into),\n            delivery_timeliness: row.delivery_timeliness.map(Into::into),\n            rider_tip: row.rider_tip_cents.map(|c| order_money(c, &currency)),\n            restaurant_tip: row.restaurant_tip_cents.map(|c| order_money(c, &currency)),\n            captain_tip: row.captain_tip_cents.map(|c| order_money(c, &currency)),\n            uber_comparison,\n            delivery_status: row.delivery_status.map(Into::into),\n            courier: row.courier.and_then(|v| serde_json::from_value(v).ok()),\n            estimated_dropoff_at: row.estimated_dropoff_at,\n            delivery_handed_back: row.delivery_handed_back,\n            rated_at: row.rated_at,\n            delivery_jobs: Vec::new(),\n            restaurant,\n        }\n    }\n}\n",
    );
    // OrderConversation (#131, epic #129): the one projection row backs BOTH conversation API types.
    // OrderConversation exposes the PUBLIC `messages` thread + the folded order status; the jsonb
    // `messages` column deserializes into the typed ConversationMessage list. Rows map 1:1 (no nav).
    out.push_str(
        "\n/// Read-model row → API type: the OrderConversation projection row → the PUBLIC thread\n/// (#131, epic #129). The customer-visible `messages` jsonb deserializes into the typed\n/// ConversationMessage list; the woven claim lifecycle `claim_events` jsonb into the ClaimTimelineEntry\n/// list (§2.5, #155); the folded order `status` and `customer_chat_enabled` ride along. The INTERNAL\n/// notes stay in the separate `ConversationInternalNotes` type (the visibility guarantee).\nimpl From<OrderConversationRow> for OrderConversation {\n    fn from(row: OrderConversationRow) -> Self {\n        Self {\n            order_id: row.order_id.into(),\n            restaurant_id: row.restaurant_id.into(),\n            status: row.status.into(),\n            customer_chat_enabled: row.customer_chat_enabled,\n            opened_at: row.opened_at,\n            messages: serde_json::from_value(row.messages).unwrap_or_default(),\n            claim_events: serde_json::from_value(row.claim_events).unwrap_or_default(),\n        }\n    }\n}\n",
    );
    // ConversationInternalNotes: the staff-only view of the SAME OrderConversation row — the INTERNAL
    // `internal_notes` thread, the `admin_invited` flag and the current muted set. Deliberately a
    // separate type, absent from the CUSTOMER schema (the visibility guarantee, #129).
    out.push_str(
        "\n/// Read-model row → API type: the OrderConversation projection row → the INTERNAL staff notes\n/// (#131, epic #129). The staff-only `internal_notes` jsonb deserializes into the typed\n/// ConversationMessage list; `adminInvited` and the current `mutedParticipants` set ride along.\nimpl From<OrderConversationRow> for ConversationInternalNotes {\n    fn from(row: OrderConversationRow) -> Self {\n        Self {\n            order_id: row.order_id.into(),\n            notes: serde_json::from_value(row.internal_notes).unwrap_or_default(),\n            admin_invited: row.admin_invited,\n            muted_participants: serde_json::from_value(row.muted).unwrap_or_default(),\n        }\n    }\n}\n",
    );
    // DeliveryJob: the View_DeliveryJob fold-view row (hand-written DTO — view-backed read models get
    // no generated row); both nav fields are NON-NULL, so the mapping takes the joined OrderTracking +
    // Restaurant rows (the resolver performs the joins).
    out.push_str(
        "\n/// Read-model rows → API type: the `View_DeliveryJob` row (ADR-0031/0039) plus the joined\n/// OrderTracking row and `Restaurant` (built once by the resolver via `Restaurant::at`, then\n/// CLONED into the embedded Order — one evaluation, so the job's two embedded serviceWindow\n/// verdicts agree by construction, RSO-1). Addresses and the courier deserialize out of the\n/// view's jsonb columns.\nimpl From<(DeliveryJobRow, OrderTrackingRow, Restaurant)> for DeliveryJob {\n    fn from((row, order, restaurant): (DeliveryJobRow, OrderTrackingRow, Restaurant)) -> Self {\n        Self {\n            id: row.delivery_job_id.into(),\n            order_id: row.order_id.into(),\n            restaurant_id: row.restaurant_id.into(),\n            status: row.status.into(),\n            provider: row.provider.map(Into::into),\n            courier: row.courier.and_then(|v| serde_json::from_value(v).ok()),\n            pickup_address: serde_json::from_value(row.pickup_address)\n                .expect(\"DeliveryJob.pickupAddress: invalid jsonb\"),\n            dropoff_address: serde_json::from_value(row.dropoff_address)\n                .expect(\"DeliveryJob.dropoffAddress: invalid jsonb\"),\n            estimated_pickup_at: row.estimated_pickup_at,\n            estimated_dropoff_at: row.estimated_dropoff_at,\n            requested_at: row.requested_at,\n            picked_up_at: row.picked_up_at,\n            delivered_at: row.delivered_at,\n            open_issue: row.open_issue_kind.map(Into::into),\n            food_location: row.food_location.map(Into::into),\n            handed_back_at: row.handed_back_at,\n            order: (order, restaurant.clone()).into(),\n            restaurant,\n        }\n    }\n}\n",
    );
    // Refund: the View_PendingRefunds fold-view row (hand-written DTO — view-backed read models get
    // no generated row). Minor-units columns + the row currency rebuild the Money values, like Order.
    out.push_str(
        "\n/// Read-model row → API type: the `View_PendingRefunds` fold-view row (the refund queue —\n/// RefundOpened/RefundApproved/RefundDenied/PaymentRefunded folded on the Payment stream). The\n/// minor-units columns + the row currency rebuild the Money values (`approvedAmount` only once a\n/// possibly-partial approval is recorded).\nimpl From<RefundRow> for Refund {\n    fn from(row: RefundRow) -> Self {\n        let currency = row.currency.clone();\n        Self {\n            order_id: row.order_id.into(),\n            restaurant_id: row.restaurant_id.into(),\n            status: row.status.into(),\n            amount: order_money(row.amount_cents, &currency),\n            approved_amount: row.approved_amount_cents.map(|c| order_money(c, &currency)),\n            reason: row.reason,\n            refund_id: row.refund_id.map(Into::into),\n            requested_at: row.requested_at,\n            decided_at: row.decided_at,\n        }\n    }\n}\n",
    );
    // DeliverySatisfaction: the View_DeliverySatisfaction fold-view row (#62; hand-written DTO). Rows
    // map 1:1 — no navigation fields (the survey view carries no FK edges), so no joins.
    out.push_str(
        "\n/// Read-model row → API type: the `View_DeliverySatisfaction` fold-view row (#62) — one\n/// customer delivery-delay answer (`DeliverySatisfactionRecorded` folded on the Order stream). Rows\n/// map 1:1 (no navigation fields), so no joins.\nimpl From<DeliverySatisfactionRow> for DeliverySatisfaction {\n    fn from(row: DeliverySatisfactionRow) -> Self {\n        Self {\n            order_id: row.order_id.into(),\n            restaurant_id: row.restaurant_id.into(),\n            timeliness: row.timeliness.into(),\n            reason: row.reason.map(Into::into),\n            recorded_at: row.recorded_at,\n        }\n    }\n}\n",
    );
    // DeliveryPartnerAvailability (#61): the `View_DeliveryPartnerAvailability` fold-view row. Rows map
    // 1:1 — set-once identity from the Requested birth fact, status derived, decided_at null while PENDING.
    out.push_str(
        "\n/// Read-model row → API type: the `View_DeliveryPartnerAvailability` fold-view row (delivery partner\n/// self-registration, #61 — Requested/Approved/Revoked folded on the DeliveryPartnerRegistration stream).\nimpl From<DeliveryPartnerAvailabilityRow> for DeliveryPartnerAvailability {\n    fn from(row: DeliveryPartnerAvailabilityRow) -> Self {\n        Self {\n            registration_id: row.registration_id.into(),\n            channel: row.channel.into(),\n            city_id: row.city_id.into(),\n            partner_name: row.partner_name.into(),\n            contact_email: row.contact_email.into(),\n            status: row.status.into(),\n            requested_at: row.requested_at,\n            decided_at: row.decided_at,\n        }\n    }\n}\n",
    );
    // Reclamation (#154): the `View_Reclamation` fold-view row (customer claims). Rows map 1:1 — the
    // set-once identity from the ReclamationOpened birth fact, status derived, the decision fields null
    // while OPEN; the optional refund Money rebuilds from the minor-units column + the row currency.
    out.push_str(
        "\n/// Read-model row → API type: the `View_Reclamation` fold-view row (customer claims, #154 —\n/// Opened/Resolved/Rejected/Reopened folded on the Reclamation stream). The decision fields are null\n/// while OPEN; `refundAmount` rebuilds from the minor-units column + the row currency (both present\n/// only when a refund amount was recorded).\nimpl From<ReclamationRow> for Reclamation {\n    fn from(row: ReclamationRow) -> Self {\n        Self {\n            reclamation_id: row.reclamation_id.into(),\n            order_id: row.order_id.into(),\n            customer_id: row.customer_id.into(),\n            restaurant_id: row.restaurant_id.into(),\n            category: row.category.into(),\n            description: row.description.into(),\n            requested_resolution: row.requested_resolution.map(Into::into),\n            status: row.status.into(),\n            resolution: row.resolution.map(Into::into),\n            refund_amount: match (row.refund_amount_cents, row.currency) {\n                (Some(cents), Some(currency)) => Some(order_money(cents, &currency)),\n                _ => None,\n            },\n            reject_reason: row.reject_reason.map(Into::into),\n            opened_at: row.opened_at,\n            decided_at: row.decided_at,\n            overdue: row.overdue,\n        }\n    }\n}\n",
    );
    // CustomerCredit (#158, Part B of #207): the materialized CustomerCreditBalance projection row →
    // the customer's spendable store-credit balance. Rows map 1:1 (no navigation).
    out.push_str(
        "\n/// Read-model row → API type: the `CustomerCreditBalance` projection row (#158, Part B of #207) →\n/// the customer's spendable store-credit balance (Σ granted − Σ consumed, never negative).\nimpl From<CustomerCreditBalanceRow> for CustomerCredit {\n    fn from(row: CustomerCreditBalanceRow) -> Self {\n        Self {\n            customer_id: row.customer_id.into(),\n            balance_cents: row.balance_cents.into(),\n            currency: row.currency.into(),\n        }\n    }\n}\n",
    );
    // CustomerProfile: the `me` query's projection of the Customer identity row — only the profile
    // surface; the jsonb accumulation columns (ratings/favorites/preferences/addresses) stay internal.
    out.push_str(
        "\n/// Read-model row → API type: the Customer identity row behind the `me` query. Only the profile\n/// surface is exposed — the jsonb accumulation columns (ratings/favorites/preferences/addresses)\n/// stay internal to the read model.\nimpl From<CustomerRow> for CustomerProfile {\n    fn from(row: CustomerRow) -> Self {\n        Self {\n            customer_id: row.customer_id.into(),\n            display_name: row.display_name.map(Into::into),\n            email: row.email.map(Into::into),\n            email_verified: row.email_verified,\n            phone: row.phone.into(),\n            locale: row.locale.map(Into::into),\n            timezone: row.timezone.map(Into::into),\n        }\n    }\n}\n",
    );
    // Referential rows → API types (ADR-0037): the policy tables are seeded configuration, not
    // projections, so their hand-written rows live in `application::queries`.
    out.push_str(
        "\n/// Referential row → API type: the seeded `pricingpolicy` table (ADR-0016/0017).\nimpl From<PricingPolicyRow> for PricingPolicy {\n    fn from(row: PricingPolicyRow) -> Self {\n        Self {\n            currency: row.currency.into(),\n            fee_rate: row.fee_rate,\n            buyer_share: row.buyer_share,\n            margin_low: row.margin_low,\n            margin_high: row.margin_high,\n            effective_from: row.effective_from,\n        }\n    }\n}\n",
    );
    out.push_str(
        "\n/// Referential row → API type: the seeded `uberestimationpolicy` table (ADR-0024/0030).\nimpl From<UberEstimationPolicyRow> for UberEstimationPolicy {\n    fn from(row: UberEstimationPolicyRow) -> Self {\n        Self {\n            cuisine_category: row.cuisine_category.into(),\n            price_coefficient: row.price_coefficient,\n            effective_from: row.effective_from,\n        }\n    }\n}\n",
    );
    out.push_str(
        "\n/// Referential row → API type: the seeded `ubersplitpolicy` table (ADR-0024/0025/0030).\nimpl From<UberSplitPolicyRow> for UberSplitPolicy {\n    fn from(row: UberSplitPolicyRow) -> Self {\n        Self {\n            currency: row.currency.into(),\n            uber_commission_pct: row.uber_commission_pct,\n            rider_base_cents: row.rider_base_cents,\n            rider_per_km_cents: row.rider_per_km_cents,\n            avg_delivery_fee_cents: row.avg_delivery_fee_cents,\n            platform_fee_pct: row.platform_fee_pct,\n            effective_from: row.effective_from,\n        }\n    }\n}\n",
    );
    out.push_str(
        "\n/// Supervision row → API type: a mailbox lane (#242 Runtime B). The BIGINT counters\n/// (ownershipVersion/checkpoint) render as decimal strings — GraphQL Int is 32-bit.\nimpl From<MailboxLaneRow> for MailboxLane {\n    fn from(row: MailboxLaneRow) -> Self {\n        Self {\n            actor_type: row.actor_type,\n            partition: i64::from(row.partition),\n            ownership_version: row.ownership_version.to_string(),\n            claimed_by: row.claimed_by,\n            lease_until: row.lease_until,\n            checkpoint: row.checkpoint.to_string(),\n            pending: row.pending,\n            scheduled: row.scheduled,\n            oldest_pending_at: row.oldest_pending_at,\n            retrying_attempts: row.retrying_attempts,\n            poisoned: row.poisoned,\n            registration: row.registration.into(),\n        }\n    }\n}\n",
    );
    out.push_str(
        "\n/// Supervision row → API type: one cap-poisoned mailbox row (#315) — the per-row detail\n/// behind MailboxLane.poisoned's count, carrying the messageId the requeue recovery needs.\nimpl From<PoisonedMessageRow> for PoisonedMailboxMessage {\n    fn from(row: PoisonedMessageRow) -> Self {\n        Self {\n            message_id: row.message_id.to_string(),\n            actor_type: row.actor_type,\n            partition: i64::from(row.partition),\n            message_type: row.message_type,\n            attempts: i64::from(row.attempts),\n            error_code: row.error_code,\n            correlation_id: row.correlation_id.map(|c| c.to_string()),\n            received_at: row.received_at,\n            completed_at: row.completed_at,\n        }\n    }\n}\n",
    );
    out
}

/// Emit `crates/server/src/graphql/generated/inputs.rs` — the GraphQL input types (InputObject),
/// mirroring `input_types_block`: one `<Command>Input` per mutation command, one `<Query>QueryInput`
/// per query with args, one `<Name>SubscriptionInput` per subscription with args, plus every entity
/// reachable from those payloads as `<Name>Input` (recursive, deduped).
pub(crate) fn emit_server_inputs(model: &Model) -> String {
    let api = parse_api(model);
    let mut needed: Vec<(String, String)> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/api.yaml + specs/commands.yaml — do not edit by hand.\n// GraphQL input types (async-graphql InputObject), mirroring the generated SDL: command payloads,\n// query/subscription args, and every entity reachable from them as `<Name>Input`.\n#![allow(dead_code)]\n\nuse super::scalars::*;\n",
    );

    for m in &api.mutations {
        if let Some(def) = model.defs.get("commands.yaml").and_then(|d| d.get(&m.command)) {
            // #865: a `derived:` property carries no field on the generated InputObject at all —
            // its description states where the id comes from, APPENDED to the command's own
            // description (never replacing it -- the command's description is still the primary
            // documentation of what the mutation DOES).
            let exclude: HashSet<&str> = m.derived.iter().map(|(p, _)| p.as_str()).collect();
            let base_desc = def.get("description").and_then(|d| d.as_str());
            let desc = match (base_desc, derived_doc(&m.derived)) {
                (Some(base), Some(derived)) => Some(format!("{base} {derived}")),
                (Some(base), None) => Some(base.to_string()),
                (None, Some(derived)) => Some(derived),
                (None, None) => None,
            };
            push_gql_struct_open(&mut out, &format!("{}Input", m.command), "InputObject", desc.as_deref());
            push_gql_object_fields_excluding(&mut out, def, "commands.yaml", true, &exclude);
            out.push_str("}\n");
            visit_inputs(model, &m.command, "commands.yaml", &mut needed, &mut visited);
        }
    }

    let scalars = scalar_names(model);
    for q in &api.queries {
        if q.args.is_empty() {
            continue;
        }
        // `argsExactlyOneOf` (#749): the one-of contract lands as the input type's DESCRIPTION
        // (→ introspection/SDL), generated from the same declaration as the resolver check.
        let one_of_doc = q.exactly_one_of.as_ref().map(|x| x.sentence());
        push_gql_struct_open(&mut out, &format!("{}QueryInput", pascal(&q.name)), "InputObject", one_of_doc.as_deref());
        for a in &q.args {
            let base = rust_api_field_base(model, a, true);
            push_gql_field(&mut out, &a.name, &base, a.required, a.description.as_deref());
        }
        out.push_str("}\n");
        for a in &q.args {
            if a.is_ref && !scalars.contains(&a.ty) {
                visit_inputs(model, &a.ty, "entities.yaml", &mut needed, &mut visited);
            }
        }
    }

    for s in &api.subscriptions {
        if s.args.is_empty() {
            continue;
        }
        push_gql_struct_open(&mut out, &format!("{}SubscriptionInput", pascal(&s.name)), "InputObject", None);
        for a in &s.args {
            let base = rust_api_field_base(model, a, true);
            push_gql_field(&mut out, &a.name, &base, a.required, a.description.as_deref());
        }
        out.push_str("}\n");
        for a in &s.args {
            if a.is_ref && !scalars.contains(&a.ty) {
                visit_inputs(model, &a.ty, "entities.yaml", &mut needed, &mut visited);
            }
        }
    }

    let mut emitted: HashSet<String> = HashSet::new();
    for (name, file) in &needed {
        if emitted.contains(name) {
            continue;
        }
        emitted.insert(name.clone());
        if let Some(def) = model.defs.get(file).and_then(|d| d.get(name)) {
            push_gql_struct_open(&mut out, &format!("{}Input", name), "InputObject", def.get("description").and_then(|d| d.as_str()));
            push_gql_object_fields(&mut out, def, file, true);
            out.push_str("}\n");
        }
    }

    // Generator-injected inputs (api.yaml `inputs:` — MetadataInput, ADR-20260720-015500).
    for (name, fields) in &api.inputs {
        push_gql_struct_open(&mut out, name, "InputObject", None);
        for f in fields {
            let base = rust_api_field_base(model, f, true);
            push_gql_field(&mut out, &f.name, &base, f.required, f.description.as_deref());
        }
        out.push_str("}\n");
    }
    out
}

/// The api.yaml role name → the server's `RequestRole` variant (`RESTAURANT_ACCOUNT` →
/// `RestaurantAccount`).
pub(crate) fn acl_role_variant(role: &str) -> String {
    role.split('_').map(|seg| pascal(&seg.to_lowercase())).collect()
}

/// An operation's allowed-role set in canonical `scalars.yaml#/UserType` declaration order, or `None`
/// when the operation is open to everyone (`roles` OMITTED — ADR-20260720-191500 literal lists; a
/// present list is guarded verbatim, PUBLIC in it being just the anonymous path).
pub(crate) fn acl_role_set(model: &Model, roles: &[String]) -> Option<Vec<String>> {
    if roles.is_empty() {
        return None;
    }
    let order = model
        .defs
        .get("scalars.yaml")
        .and_then(|v| v.get("UserType"))
        .and_then(|v| v.get("enum"))
        .and_then(|v| v.as_sequence())
        .map(|s| s.iter().filter_map(|x| x.as_str().map(|r| r.to_string())).collect::<Vec<_>>())
        .unwrap_or_default();
    Some(order.into_iter().filter(|r| roles.contains(r)).collect())
}

/// The identifier stem shared by a role set's generated const/fn (`[RESTAURANT_ACCOUNT, ADMIN]` →
/// `restaurant_account_admin` → `ALLOW_RESTAURANT_ACCOUNT_ADMIN` / `visible_restaurant_account_admin`).
pub(crate) fn acl_set_ident(set: &[String]) -> String {
    set.join("_").to_lowercase()
}

/// Mutations that ADDITIONALLY chain `.and(AuthorityGuard::new(MemberAuthority::…))` beyond
/// `RoleGuard`/`StandingGuard` (#639 part C step 6-iv round 2, ADR-20260905-101349 §2 amendment) —
/// a SECOND, orthogonal, SELECTIVE question (unlike `StandingGuard`'s blanket application to every
/// role-guarded field): only the two ops a MANAGER-only authority actually gates. Hand-maintained,
/// the `BT_GATE_CONSUMING` precedent (`tools/codegen-rs/src/emit/behaviour_tests.rs`) — a
/// mutation named here that does not exist, or a `MemberAuthority` variant that does not exist,
/// fails to compile the GENERATED code, never silently.
pub(crate) const AUTHORITY_GUARDED_MUTATIONS: &[(&str, &str)] =
    &[("inviteRestaurantMember", "MANAGER"), ("revokeRestaurantInvitation", "MANAGER")];

/// The `guard`/`visible` additions to a generated QueryRoot/MutationRoot field's `#[graphql(...)]`
/// attribute, from the operation's api.yaml `roles`. Empty for public operations. `while_restricted`
/// (#639 part C step 4-i, ADR-20260904-081527 §4): `RoleGuard.and(StandingGuard)` on EVERY
/// role-guarded operation, with an EMPTY carve set when the key is absent — fail-closed by absence
/// lives here, in the emitter, never in the author's memory. [`AUTHORITY_GUARDED_MUTATIONS`] adds a
/// THIRD, selective link only for the named ops.
pub(crate) fn acl_field_attr(model: &Model, roles: &[String], while_restricted: &[String], op_name: &str) -> String {
    match acl_role_set(model, roles) {
        Some(set) => {
            let ident = acl_set_ident(&set);
            let carve: Vec<String> =
                while_restricted.iter().map(|r| format!("RequestRole::{}", acl_role_variant(r))).collect();
            let authority = AUTHORITY_GUARDED_MUTATIONS
                .iter()
                .find(|(name, _)| *name == op_name)
                .map(|(_, level)| {
                    format!(".and(AuthorityGuard::new(domain::generated::scalars::MemberAuthority::{}))", level)
                })
                .unwrap_or_default();
            format!(
                ", guard = \"RoleGuard::new(ALLOW_{}).and(StandingGuard::new(&[{}], \\\"{}\\\")){}\", visible = \"visible_{}\"",
                ident.to_uppercase(),
                carve.join(", "),
                op_name,
                authority,
                ident
            )
        }
        None => String::new(),
    }
}

/// Emit `crates/server/src/graphql/generated/acl.rs` — the spec-derived ACL data (ADR-0006): one
/// allowed-role const + one `visible` fn per distinct non-public role set found on api.yaml
/// queries/mutations. The generated QueryRoot/MutationRoot fields reference them as
/// `guard = "RoleGuard::new(ALLOW_…)"` (execution) and `visible = "visible_…"` (introspection); the
/// guard/lookup logic itself is the hand-written `graphql::acl` seam.
pub(crate) fn emit_server_acl(model: &Model) -> String {
    let api = parse_api(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/api.yaml — do not edit by hand.\n// Per-operation ACL role sets (ADR-0006 role-as-path): each distinct non-public `roles:` set on an\n// api.yaml query/mutation/subscription becomes an allowed-role const + a `visible` fn. The generated\n// QueryRoot/MutationRoot/SubscriptionRoot fields wire them as `guard = \"RoleGuard::new(ALLOW_…).and(StandingGuard::new(&[…]))\"`\n// (execution — unauthorized roles get a FORBIDDEN error, then a RESTRICTED rider not in the\n// operation's carve set gets one too, #639 part C step 4-i) and `visible = \"visible_…\"`\n// (introspection — the field is hidden from unauthorized roles, and async-graphql's\n// `find_visible_types` then hides every type reachable only through hidden fields, so per-role\n// introspection/Voyager expose only that role's surface). Operations with `roles:` OMITTED carry no\n// guard/visible: open to every role path (LITERAL roles, ADR-20260720-191500 — PUBLIC in a list is\n// just the anonymous path) — and therefore no StandingGuard either (`operations with roles: omitted\n// are unaffected by restriction`, ADR-20260904-081527 §4).\n#![allow(dead_code)]\n\npub(crate) use super::super::acl::{AuthorityGuard, RequestRole, RoleGuard, StandingGuard};\nuse super::super::acl::role_allows;\nuse async_graphql::GuardExt as _;\n",
    );
    // Distinct non-public role sets across queries + mutations + subscriptions (the generated
    // SubscriptionRoot carries the same guard/visible pairs), keyed by identifier for a
    // deterministic, deduped emission order.
    let mut sets: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for roles in api
        .queries
        .iter()
        .map(|q| &q.roles)
        .chain(api.mutations.iter().map(|m| &m.roles))
        .chain(api.subscriptions.iter().map(|s| &s.roles))
    {
        if let Some(set) = acl_role_set(model, roles) {
            sets.insert(acl_set_ident(&set), set);
        }
    }
    // Guarded FK-derived nav edges (#22) share the same const/visible pairs.
    for t in &api.types {
        for (_field, roles) in &t.nav_roles {
            if let Some(set) = acl_role_set(model, roles) {
                sets.insert(acl_set_ident(&set), set);
            }
        }
    }
    for (ident, set) in &sets {
        let variants: Vec<String> =
            set.iter().map(|r| format!("RequestRole::{}", acl_role_variant(r))).collect();
        out.push_str(&format!(
            "\n/// roles: [{}]\npub(crate) const ALLOW_{}: &[RequestRole] = &[{}];\npub(crate) fn visible_{}(ctx: &async_graphql::Context<'_>) -> bool {{\n    role_allows(ctx, ALLOW_{})\n}}\n",
            set.join(", "),
            ident.to_uppercase(),
            variants.join(", "),
            ident,
            ident.to_uppercase()
        ));
    }
    out
}

/// Emit `crates/server/src/graphql/generated/query.rs` — the `QueryRoot`, mirroring `query_block`:
/// one async resolver per api.yaml query with the SDL argument/return shape. Every resolver returns
/// `Err("not implemented")` until the read-model repositories are injected (a later stage).
pub(crate) fn emit_server_query(model: &Model) -> String {
    let api = parse_api(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/api.yaml — do not edit by hand.\n// The GraphQL QueryRoot: one resolver per api.yaml query, matching the generated SDL shape. Resolvers\n// whose read-model repository is wired delegate to it (via ctx.data); the rest stub `not implemented`\n// until their repos land. Each non-public field carries its api.yaml `roles` as a `guard` (execution)\n// + `visible` (introspection) pair from the generated acl module (ADR-0006 role-as-path).\n#![allow(unused_variables)]\n#![allow(dead_code)]\n\nuse super::acl::*;\nuse super::inputs::*;\nuse super::types::*;\n\npub struct QueryRoot;\n\n#[async_graphql::Object(name = \"Query\")]\nimpl QueryRoot {\n",
    );
    for q in &api.queries {
        let fnname = rust_ident(&snake_field(&q.name));
        let acl = acl_field_attr(model, &q.roles, &q.while_restricted, &q.name);
        let arg = if q.args.is_empty() {
            String::new()
        } else {
            let ty = format!("{}QueryInput", pascal(&q.name));
            let ty = if q.args.iter().any(|a| a.required) { ty } else { format!("Option<{}>", ty) };
            format!(", input: {}", ty)
        };
        let inner = gql_rust_name(&q.returns_type);
        let mut ret = if q.returns_list { format!("Vec<{}>", inner) } else { inner };
        if q.returns_nullable {
            ret = format!("Option<{}>", ret);
        }
        // The `argsExactlyOneOf` check (#749), GENERATED from the declaration — never ad-hoc
        // resolver code: exactly one of the named selector args must be provided; zero or both
        // reject with the DECLARED typed error (P-10 extensions shape) before any repository read.
        let one_of = q
            .exactly_one_of
            .as_ref()
            .map(|x| {
                let input_is_opt = !q.args.iter().any(|a| a.required);
                let checks: Vec<String> = x
                    .args
                    .iter()
                    .map(|a| {
                        let f = rust_ident(&snake_field(a));
                        if input_is_opt {
                            format!("input.as_ref().is_some_and(|i| i.{}.is_some())", f)
                        } else {
                            format!("input.{}.is_some()", f)
                        }
                    })
                    .collect();
                format!(
                    "        // GENERATED from `argsExactlyOneOf` (#749): exactly one selector arg must be\n        // provided; zero or both reject with the declared typed error (P-10 shape).\n        if [{}].into_iter().filter(|p| *p).count() != 1 {{\n            return Err(crate::graphql::typed_error(&domain::generated::errors::{}));\n        }}\n",
                    checks.join(", "),
                    screaming_snake(&x.throws)
                )
            })
            .unwrap_or_default();
        push_doc(&mut out, "    ", q.description.as_deref());
        match wired_query_body(&q.name) {
            // Wired: delegate to the injected read-model repo (ctx.data); takes &Context.
            Some(body) => out.push_str(&format!(
                "    #[graphql(name = \"{}\"{})]\n    async fn {}(&self, ctx: &async_graphql::Context<'_>{}) -> async_graphql::Result<{}> {{\n{}{}\n    }}\n",
                q.name, acl, fnname, arg, ret, one_of, body
            )),
            None => out.push_str(&format!(
                "    #[graphql(name = \"{}\"{})]\n    async fn {}(&self{}) -> async_graphql::Result<{}> {{\n{}        Err(async_graphql::Error::new(\"not implemented\"))\n    }}\n",
                q.name, acl, fnname, arg, ret, one_of
            )),
        }
    }
    out.push_str("}\n");
    out
}

/// Resolver bodies for queries whose read-model repository is wired (injected via `ctx.data`). Returned as
/// the fn body (8-space indent); `None` → the `not implemented` stub. Extend as read repos land.
pub(crate) fn wired_query_body(name: &str) -> Option<&'static str> {
    match name {
        // GDPR erasure status (#708). WIRED DELIBERATELY TO A TYPED REFUSAL, not left as the
        // generic `not implemented` stub and not answered `Ok(None)`. Three reasons, in order of
        // how badly the alternatives fail:
        //   1. `Ok(None)` is the dangerous one. The field's contract is "null when no request
        //      exists", so null does not mean "we cannot tell you" — it means "nothing is
        //      happening to your account". The moment the write side starts accepting requests,
        //      a resolver still hard-coded to null would tell a subject with a pending erasure
        //      that they never asked. That is the mutations' "scheduled forever" failure with the
        //      sign flipped, and it is worse, because a subject who is told nothing is scheduled
        //      stops chasing the right entirely.
        //   2. `not implemented` is untyped: a client cannot distinguish it from a server bug,
        //      and it carries no code a screen can branch on.
        //   3. The mutations for this journey refuse with `ErasureEngineUnavailable`. A read that
        //      failed differently from the writes would make the surface incoherent exactly where
        //      the subject is most anxious.
        // The refusal is UNCONDITIONAL here for the same reason it is in the command handlers:
        // `View_CustomerErasure` has no repository yet. It becomes a real read in the runtime
        // chunk, and this arm is deleted when `wired_query_body` gains the repo delegation.
        // No disclosure risk either way -- the guard is CUSTOMER-only and this arm reads nothing.
        "erasureStatus" => Some(
            "        Err(crate::graphql::typed_error(&domain::generated::errors::ERASURE_ENGINE_UNAVAILABLE))",
        ),
        // The actor-mailbox supervision lanes (#242 Runtime B): ADMIN-only via the field guard;
        // no args, rows map 1:1 — write-path infrastructure, no View_*. Read through the
        // actor_client supervision DOOR (#510): the port method demands the mailbox witness,
        // which only that crate mints — the resolver holds the port but not the door.
        "mailboxLanes" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn actor_client::supervision::MailboxLaneRepository>>()?;\n        let rows = actor_client::supervision::mailbox_lanes(repo.as_ref()).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(MailboxLane::from).collect())",
        ),
        // The cap-poisoned rows behind each lane's `poisoned` count (#315): ADMIN-only via the
        // field guard; optional lane filter, page clamped to 200 — same supervision read port,
        // same actor_client door (#510).
        "poisonedMailboxMessages" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn actor_client::supervision::MailboxLaneRepository>>()?;\n        let (actor_type, limit) = input\n            .map(|i| (i.actor_type.map(|v| v.0), i.limit.map(|v| v.0)))\n            .unwrap_or((None, None));\n        let rows = actor_client::supervision::poisoned_messages(repo.as_ref(), actor_type, limit.unwrap_or(200).clamp(1, 200))\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(PoisonedMailboxMessage::from).collect())",
        ),
        // The command status poll (ADR-20260720-015500): PUBLIC, ownership-scoped — a
        // non-owned/unknown messageId resolves null (no existence oracle). MAILBOX-ONLY since
        // #242 Runtime D: every acceptance lives in inbound_messages, read through the ONE
        // generic ActorClient — the D4 read door (PROP-20260802-130500: status is an
        // envelope-level outcome keyed by message_id alone, so the read side is actor-agnostic
        // while the write side stays per-actor). There is no second journal to fall back to.
        "operationStatus" => Some(
            "        let mailbox = ctx.data::<std::sync::Arc<dyn actor_client::mailbox::Mailbox>>()?.clone();\n        let status_door = actor_client::ActorClient::new(mailbox, ctx.data::<actor_client::OperationStatusBus>()?.clone());\n        if let Some(row) = status_door\n            .get_operation_status(input.message_id.0)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n        {\n            if !super::mutation::mailbox_operation_owned(ctx, &row) {\n                return Ok(None);\n            }\n            return Ok(Some(super::mutation::operation_from_mailbox(&row, super::mutation::request_locale(ctx))));\n        }\n        Ok(None)",
        ),
        // The checkout payment state (ADR-20260720-015500): served from the PlaceOrderProcess run
        // row (the declared PM-privacy exception); initiator-scoped — ADMIN, the checkout's
        // customer (JWT subject → Customer row), or the checkout's session.
        "paymentStatus" => Some(
            "        let pm = ctx.data::<std::sync::Arc<dyn application::pm_state::PaymentProcessStateStore>>()?;\n        let Some(row) = pm\n            .by_order(input.order_id.into())\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n        else {\n            return Ok(None);\n        };\n        let admin = matches!(\n            crate::graphql::acl::request_role(ctx),\n            crate::graphql::acl::RequestRole::Admin\n        );\n        let session = ctx.data_opt::<crate::graphql::session::SessionHeader>().and_then(|s| s.0);\n        let session_owned = session.is_some() && session == row.session_id.as_ref().map(|s| s.0);\n        // Per-instance authorization (#144/#433): the ReadScope was resolved ONCE at the edge from the token's verified claims (CARD-11) and injected into the context -- the same identity source as every other guarded read; the by_auth_ref bridge is gone from authorization. Absent => Public -- fail closed.\n        let scope = ctx.data_opt::<application::queries::ReadScope>().cloned().unwrap_or(application::queries::ReadScope::Public);\n        let customer_owned = matches!(\n            (&scope, row.customer_id.as_ref()),\n            (application::queries::ReadScope::Customer(c), Some(row_customer)) if c == row_customer\n        );\n        if !(admin || customer_owned || session_owned) {\n            return Ok(None);\n        }\n        Ok(Some(PaymentIntent {\n            payment_intent_id: row.payment_intent_id.into(),\n            client_secret: row.client_secret,\n            status: row.payment_status.into(),\n        }))",
        ),
        // The two Customer-vertical queries resolve through the Customer identity read model: `me`
        // maps the verified session Principal's authRef (ADR-0047/0015) to its Customer row;
        // `favoriteRestaurants` joins the row's projected favorite ids to the Restaurant read model.
        "me" => Some(
            "        // The verified session identity (ADR-0047), injected per-request by the HTTP layer. No\n        // principal (schema executed outside a request) or an anonymous one → no profile, not an error.\n        let Some(auth_ref) = ctx.data_opt::<crate::auth::Principal>().and_then(|p| p.user_id().map(str::to_string)) else {\n            return Ok(None);\n        };\n        let customers = ctx.data::<std::sync::Arc<dyn application::queries::CustomerReadRepository>>()?;\n        let row = customers\n            .by_auth_ref(domain::generated::scalars::ExternalReference(auth_ref))\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(row.map(CustomerProfile::from))",
        ),
        "favoriteRestaurants" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let customers = ctx.data::<std::sync::Arc<dyn application::queries::CustomerReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let Some(row) = customers.by_id(input.customer_id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else {\n            return Ok(Vec::new());\n        };\n        // The projected favorite set is a jsonb array of restaurant-id strings (CustomerProjector);\n        // resolve each against the Restaurant read model (an unknown id simply drops out).\n        let ids: Vec<uuid::Uuid> = row\n            .favorite_restaurant_ids\n            .as_array()\n            .map(|a| a.iter().filter_map(|v| v.as_str().and_then(|s| uuid::Uuid::parse_str(s).ok())).collect())\n            .unwrap_or_default();\n        let mut out = Vec::new();\n        for id in ids {\n            let found = restaurants\n                .by_id(domain::generated::scalars::RestaurantId(id))\n                .await\n                .map_err(|e| async_graphql::Error::new(e.to_string()))?;\n            if let Some(r) = found {\n                out.push(Restaurant::at(r, now, horizon));\n            }\n        }\n        Ok(out)",
        ),
        // #639 part C step 4-i (ADR-20260904-081527 §4): the ONE additive query the carve-out
        // needs, so a restricted rider bootstraps the /restricted screen with no `orderId` in
        // hand. `heldDelivery` keys on `ReadScope::Rider.id` — never the JWT `sub` (#869).
        "myStanding" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let Some(application::queries::ReadScope::Rider { id: rider_id, standing }) = ctx.data_opt::<application::queries::ReadScope>() else {\n            return Err(super::mutation::forbidden_error());\n        };\n        let restrictions = ctx.data::<std::sync::Arc<dyn application::queries::RiderRestrictionReadRepository>>()?;\n        let row = restrictions.by_rider_id(*rider_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        let restriction = if *standing == domain::generated::scalars::RiderStanding::RESTRICTED { row.as_ref().and_then(|r| match (r.ground, r.decided_at, r.effective_at) {\n            (Some(ground), Some(decided_at), Some(effective_at)) => Some(RiderRestrictionInfo {\n                ground: super::scalars::rider_restriction_ground_from_domain(ground),\n                decided_at,\n                effective_at,\n            }),\n            _ => None,\n        }) } else { None };\n        let deliveries = ctx.data::<std::sync::Arc<dyn application::queries::DeliveryReadRepository>>()?;\n        let orders = ctx.data::<std::sync::Arc<dyn application::queries::OrderReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        // #639 part C step 4-ii (ADR-20260904-124600 §5, #879): the held-job read is a PER-PAINT\n        // port keyed on the standing carve-out, not `for_rider`'s whole-history-plus-PENDING-pool\n        // scan over a view of the log.\n        let held = deliveries.held_by_rider(*rider_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        let mut held_delivery = None;\n        if let Some(job) = held {\n            if let Some(order) = orders.by_id(job.order_id, &application::queries::ReadScope::System).await.map_err(|e| async_graphql::Error::new(e.to_string()))? {\n                if let Some(restaurant) = restaurants.by_id(job.restaurant_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))? {\n                    held_delivery = Some(DeliveryJob::from((job, order, Restaurant::at(restaurant, now, horizon))));\n                }\n            }\n        }\n        // #639 4-ii (ADR-20260904-124600 §4): SUPPORT_CONTACT, bound ONCE from the composition\n        // root's ReadDeps (the 2c refusal-screen precedent) -- never a translation-string literal.\n        let contest_contact = ctx.data_opt::<Option<domain::generated::scalars::EmailAddress>>().cloned().flatten().map(Into::into);\n        Ok(RiderStandingInfo { standing: (*standing).into(), restriction, held_delivery, contest_contact })",
        ),
        // #639 part C step 4-iii-A (ADR-20260904-152807 SS3-4): the admin roster triage --
        // riders holding a job first, then RESTRICTED, then ACTIVE, each by displayName then
        // riderId, computed over the WHOLE roster before paging (the page boundary must not
        // split the held-first group).
        "riders" => Some(
            "        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let roster = ctx.data::<std::sync::Arc<dyn application::queries::RiderRosterReadRepository>>()?;\n        let deliveries = ctx.data::<std::sync::Arc<dyn application::queries::DeliveryReadRepository>>()?;\n        let orders = ctx.data::<std::sync::Arc<dyn application::queries::OrderReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let restriction_door_open = ctx.data::<crate::graphql::schema::RunRiderRestrictionDoor>()?.0;\n        let limit = input.as_ref().and_then(|i| i.limit).map(|v| v.0).filter(|l| *l > 0).unwrap_or(50).min(200) as usize;\n        let offset = input.and_then(|i| i.offset).map(|v| v.0).filter(|o| *o >= 0).unwrap_or(0) as usize;\n        // The ORDER is the contract (riders holding a job first, then RESTRICTED, then ACTIVE, each\n        // by displayName then riderId) and must not be split across a page boundary, so the held\n        // set is computed for the WHOLE roster BEFORE paging -- tens/hundreds of rows, the honest\n        // cost of a rider POPULATION, never an order log (ADR-20260904-152807 SS2/SS4).\n        let rows = roster.all().await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        let all_ids: Vec<domain::generated::scalars::RiderId> = rows.iter().map(|r| r.rider_id).collect();\n        let held_rows = deliveries.held_by_riders(&all_ids).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        // Round 2 item 3 (dba): FIRST-WINS over `held_by_riders`\'s now-deterministic `requested_at DESC, delivery_job_id DESC` order -- a plain `.collect()` here was LAST-wins, silently keeping the OLDEST held job per rider while `held_by_rider` (the detail\'s own `LIMIT 1`) picks the NEWEST: the list and the detail could name two different held jobs for the SAME rider.\n        let mut held_by: std::collections::HashMap<domain::generated::scalars::RiderId, application::queries::DeliveryJobRow> = std::collections::HashMap::new();\n        for j in held_rows { if let Some(r) = j.rider_id { held_by.entry(r).or_insert(j); } }\n        let mut held_group = Vec::new();\n        let mut restricted_group = Vec::new();\n        let mut active_group = Vec::new();\n        for row in rows {\n            if held_by.contains_key(&row.rider_id) {\n                held_group.push(row);\n            } else if row.standing == domain::generated::scalars::RiderStanding::RESTRICTED {\n                restricted_group.push(row);\n            } else {\n                active_group.push(row);\n            }\n        }\n        held_group.extend(restricted_group);\n        held_group.extend(active_group);\n        let mut out = Vec::new();\n        for row in held_group.into_iter().skip(offset).take(limit) {\n            let mut held_delivery = None;\n            if let Some(job) = held_by.get(&row.rider_id).cloned() {\n                if let Some(order) = orders.by_id(job.order_id, &application::queries::ReadScope::System).await.map_err(|e| async_graphql::Error::new(e.to_string()))? {\n                    if let Some(restaurant) = restaurants.by_id(job.restaurant_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))? {\n                        held_delivery = Some(DeliveryJob::from((job, order, Restaurant::at(restaurant, now, horizon))));\n                    }\n                }\n            }\n            out.push(RiderRosterEntry {\n                rider_id: row.rider_id.into(),\n                display_name: row.display_name,\n                phone: row.phone.into(),\n                status: row.status.into(),\n                standing: row.standing.into(),\n                ground: row.ground.and_then(super::scalars::rider_restriction_ground_from_domain),\n                decided_at: row.decided_at,\n                effective_at: row.effective_at,\n                reinstated_at: row.reinstated_at,\n                held_delivery,\n                restriction_door_open,\n            });\n        }\n        Ok(out)",
        ),
        // The admin detail behind /system/riders/:riderId (#639 part C step 4-iii-A).
        "rider" => Some(
            "        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let roster = ctx.data::<std::sync::Arc<dyn application::queries::RiderRosterReadRepository>>()?;\n        let Some(row) = roster.by_id(input.rider_id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else {\n            return Ok(None);\n        };\n        let deliveries = ctx.data::<std::sync::Arc<dyn application::queries::DeliveryReadRepository>>()?;\n        let orders = ctx.data::<std::sync::Arc<dyn application::queries::OrderReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let mut held_delivery = None;\n        if let Some(job) = deliveries.held_by_rider(row.rider_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))? {\n            if let Some(order) = orders.by_id(job.order_id, &application::queries::ReadScope::System).await.map_err(|e| async_graphql::Error::new(e.to_string()))? {\n                if let Some(restaurant) = restaurants.by_id(job.restaurant_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))? {\n                    held_delivery = Some(DeliveryJob::from((job, order, Restaurant::at(restaurant, now, horizon))));\n                }\n            }\n        }\n        let restriction_door_open = ctx.data::<crate::graphql::schema::RunRiderRestrictionDoor>()?.0;\n        Ok(Some(RiderRosterEntry {\n            rider_id: row.rider_id.into(),\n            display_name: row.display_name,\n            phone: row.phone.into(),\n            status: row.status.into(),\n            standing: row.standing.into(),\n            ground: row.ground.and_then(super::scalars::rider_restriction_ground_from_domain),\n            decided_at: row.decided_at,\n            effective_at: row.effective_at,\n            reinstated_at: row.reinstated_at,\n            held_delivery,\n            restriction_door_open,\n        }))",
        ),
        "restaurants" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let filter = input\n            .map(|i| application::queries::RestaurantFilter { search: i.search, orderable_only: i.orderable_only, limit: i.limit.map(|v| v.0), offset: i.offset.map(|v| v.0) })\n            .unwrap_or_default();\n        let rows = repo.list(filter).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(|r| Restaurant::at(r, now, horizon)).collect())",
        ),
        "restaurant" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let row = repo.by_slug(input.slug.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(row.map(|r| Restaurant::at(r, now, horizon)))",
        ),
        // The storefront MENU read (#749): selector = restaurantId OR restaurantSlug (the
        // generated exactly-one-of prelude runs first). The slug resolves through the SAME path
        // as the tenant host — current slug, then the SlugAlias fallback for a superseded label
        // (ADR-20260728-011344) — so a renamed storefront behaves identically on both paths. Two
        // indexed point lookups (restaurant, then catalog by restaurant id), never a cross-scope
        // join; the resolved restaurant row doubles as the non-null `restaurant` navigation
        // target (both rows are projections of the same domain log).
        "catalog" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::CatalogReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        // The selector (post the generated exactly-one-of check, #749): an id, or a storefront\n        // slug resolved through the SAME path as the tenant host -- current slug first, then the\n        // SlugAlias fallback for a superseded label (ADR-20260728-011344). Two indexed point\n        // lookups, never a cross-scope join.\n        let (selector_id, selector_slug) = match input {\n            Some(i) => (i.restaurant_id, i.restaurant_slug),\n            None => (None, None),\n        };\n        let restaurant = match (selector_id, selector_slug) {\n            (Some(id), _) => restaurants.by_id(id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))?,\n            (None, Some(slug)) => {\n                let slug: domain::generated::scalars::Slug = slug.into();\n                match restaurants.by_slug(slug.clone()).await.map_err(|e| async_graphql::Error::new(e.to_string()))? {\n                    Some(r) => Some(r),\n                    None => restaurants.by_previous_slug(slug).await.map_err(|e| async_graphql::Error::new(e.to_string()))?,\n                }\n            }\n            // Unreachable past the generated one-of check; answers null defensively.\n            (None, None) => None,\n        };\n        // An unknown selector answers null -- the read is nullable, never an existence error.\n        let Some(restaurant) = restaurant else {\n            return Ok(None);\n        };\n        // Host precedence (#749 hard rule): on a tenant host the Host is the tenant selector\n        // (#469) -- a client selector naming ANOTHER restaurant REJECTS with the typed error,\n        // never a silent pick in either direction (a silent pick is a cross-tenant read).\n        if let Some(crate::graphql::tenant::TenantScope::Restaurant(tenant)) = ctx.data_opt::<crate::graphql::tenant::TenantScope>() {\n            if *tenant != restaurant.restaurant_id {\n                return Err(crate::graphql::typed_error(&domain::generated::errors::TENANT_SELECTOR_MISMATCH));\n            }\n        }\n        let Some(row) = repo.by_restaurant(restaurant.restaurant_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else {\n            return Ok(None);\n        };\n        Ok(Some(Catalog::from((row, Restaurant::at(restaurant, now, horizon)))))",
        ),
        "categories" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::CatalogReadRepository>>()?;\n        let row = repo.by_restaurant(input.restaurant_id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        // Categories live inside the projected Catalog.tree jsonb; an absent catalog or an empty\n        // tree (a catalog created before any content event) yields an empty list.\n        Ok(row.map(|r| catalog_tree_section::<CatalogCategory>(&r.tree, \"categories\")).unwrap_or_default())",
        ),
        "carts" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::CartReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let catalogs = ctx.data::<std::sync::Arc<dyn application::queries::CatalogReadRepository>>()?;\n        let authority = ctx.data::<std::sync::Arc<dyn application::ports::AsOfPriceAuthority>>()?;\n        let door_open = ctx.data::<crate::graphql::schema::RunFoldPricedCartRead>()?.0;\n        // The ONE request-scoped correlation id (#451, contract `request.correlation_id`): every\n        // cart.price span of THIS request shares it. Absent = the schema was executed OUTSIDE a\n        // request (no transport, e.g. a direct unit-test execution), and the NIL uuid says exactly\n        // that: a random id would be indistinguishable from a real one in a trace, sending an\n        // operator hunting for a request that never existed. All three real paths -- HTTP POST, the\n        // WS connection_init and the SSR render -- inject one, so this is unreachable in production.\n        let correlation_id = ctx.data_opt::<crate::graphql::session::RequestCorrelationId>().map(|c| c.0).unwrap_or(uuid::Uuid::nil());\n        // Per-instance authorization (#144): the ReadScope was resolved ONCE at the edge from the verified Principal and injected into the context. Absent (schema executed outside a request) => Public, i.e. no tenant rows -- fail closed.\n        let scope = ctx.data_opt::<application::queries::ReadScope>().cloned().unwrap_or(application::queries::ReadScope::Public);\n        // Ownership enforced server-side (#144): a CUSTOMER caller's customerId argument is IGNORED\n        // and forced to the caller's own identity; only ADMIN reads another customer's carts (the\n        // query is guarded [CUSTOMER, ADMIN], so no other tenant role can arrive). An unresolvable\n        // identity yields an empty list -- fail closed, never a fall-through to the client filter.\n        let customer_id: domain::generated::scalars::CustomerId = match &scope {\n            application::queries::ReadScope::Customer(id) => *id,\n            application::queries::ReadScope::Admin | application::queries::ReadScope::System => input.customer_id.into(),\n            _ => return Ok(Vec::new()),\n        };\n        let rows = repo.by_customer(customer_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        // The non-null `restaurant` navigation field: join against the Restaurant read model in memory\n        // (a cart is only ever started against a projected restaurant, so a match always exists).\n        let by_id: std::collections::HashMap<_, _> = restaurants\n            .list(application::queries::RestaurantFilter::default())\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .into_iter()\n            .map(|r| (r.restaurant_id.0, r))\n            .collect();\n        // Priced through the ONE `price_cart` seam (#451): each cart from ITS restaurant's live\n        // catalog, one memoized catalog read per cart; an unresolvable price errors the read\n        // (fail-closed, cart-price contract) rather than lying with a partial list.\n        let mut out = Vec::new();\n        for c in rows {\n            let Some(r) = by_id.get(&c.restaurant_id.0).cloned() else { continue };\n            out.push(crate::graphql::cart_read::priced(&**catalogs, &**authority, door_open, c, Restaurant::at(r, now, horizon), correlation_id).await?);\n        }\n        Ok(out)",
        ),
        "cart" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::CartReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let catalogs = ctx.data::<std::sync::Arc<dyn application::queries::CatalogReadRepository>>()?;\n        let authority = ctx.data::<std::sync::Arc<dyn application::ports::AsOfPriceAuthority>>()?;\n        let door_open = ctx.data::<crate::graphql::schema::RunFoldPricedCartRead>()?.0;\n        // The ONE request-scoped correlation id (#451, contract `request.correlation_id`): every\n        // cart.price span of THIS request shares it. Absent = the schema was executed OUTSIDE a\n        // request (no transport, e.g. a direct unit-test execution), and the NIL uuid says exactly\n        // that: a random id would be indistinguishable from a real one in a trace, sending an\n        // operator hunting for a request that never existed. All three real paths -- HTTP POST, the\n        // WS connection_init and the SSR render -- inject one, so this is unreachable in production.\n        let correlation_id = ctx.data_opt::<crate::graphql::session::RequestCorrelationId>().map(|c| c.0).unwrap_or(uuid::Uuid::nil());\n        let Some(row) = repo.by_id(input.id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else {\n            return Ok(None);\n        };\n        // Claim-ownership narrowing (#144/#434, the #451 DONE-WHEN): a CUSTOMER reads only a cart\n        // bound to their claim-resolved id — anyone else's (or an unbound session cart) resolves\n        // null, no existence oracle; ADMIN reads any cart. Scope absent => Public => nothing.\n        let scope = ctx.data_opt::<application::queries::ReadScope>().cloned().unwrap_or(application::queries::ReadScope::Public);\n        if !crate::graphql::cart_read::readable_by(&row, &scope) {\n            return Ok(None);\n        }\n        let restaurant = restaurants\n            .by_id(row.restaurant_id)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .ok_or_else(|| async_graphql::Error::new(\"cart references an unknown restaurant\"))?;\n        Ok(Some(crate::graphql::cart_read::priced(&**catalogs, &**authority, door_open, row, Restaurant::at(restaurant, now, horizon), correlation_id).await?))",
        ),
        // The TWO-LEG current-cart read (#451, ADR-20260810-120531): claim leg, then session leg
        // with the NULL-or-claim ownership filter — all semantics live in the hand-written,
        // unit-tested `cart_read` seam; this literal only assembles the request context.
        "current" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let carts = ctx.data::<std::sync::Arc<dyn application::queries::CartReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let catalogs = ctx.data::<std::sync::Arc<dyn application::queries::CatalogReadRepository>>()?;\n        let authority = ctx.data::<std::sync::Arc<dyn application::ports::AsOfPriceAuthority>>()?;\n        let door_open = ctx.data::<crate::graphql::schema::RunFoldPricedCartRead>()?.0;\n        // The ONE request-scoped correlation id (#451, contract `request.correlation_id`): every\n        // cart.price span of THIS request shares it. Absent = the schema was executed OUTSIDE a\n        // request (no transport, e.g. a direct unit-test execution), and the NIL uuid says exactly\n        // that: a random id would be indistinguishable from a real one in a trace, sending an\n        // operator hunting for a request that never existed. All three real paths -- HTTP POST, the\n        // WS connection_init and the SSR render -- inject one, so this is unreachable in production.\n        let correlation_id = ctx.data_opt::<crate::graphql::session::RequestCorrelationId>().map(|c| c.0).unwrap_or(uuid::Uuid::nil());\n        // Per-instance authorization (#144): the ReadScope was resolved ONCE at the edge from the verified Principal and injected into the context. Absent (schema executed outside a request) => Public, i.e. the session leg only -- fail closed.\n        let scope = ctx.data_opt::<application::queries::ReadScope>().cloned().unwrap_or(application::queries::ReadScope::Public);\n        // The anonymous-session correlator (validated at transport, `session.rs`): leg 2's key.\n        let session = ctx.data_opt::<crate::graphql::session::SessionHeader>().and_then(|s| s.0);\n        // The request's TENANT (#469), resolved ONCE at the edge from the Host -- its own datum\n        // beside the ReadScope. Absent (the marketplace host, or a schema executed outside a\n        // request) => no tenant, and a tenant-scoped read serves NOTHING rather than everything.\n        let tenant = ctx.data_opt::<crate::graphql::tenant::TenantScope>().copied().unwrap_or(crate::graphql::tenant::TenantScope::None);\n        let Some(row) = crate::graphql::cart_read::current_open_cart(&**carts, &scope, session, tenant)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n        else {\n            return Ok(None);\n        };\n        let restaurant = restaurants\n            .by_id(row.restaurant_id)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .ok_or_else(|| async_graphql::Error::new(\"cart references an unknown restaurant\"))?;\n        Ok(Some(crate::graphql::cart_read::priced(&**catalogs, &**authority, door_open, row, Restaurant::at(restaurant, now, horizon), correlation_id).await?))",
        ),
        "orders" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::OrderReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        // Per-instance authorization (#144): the ReadScope was resolved ONCE at the edge from the verified Principal and injected into the context. Absent (schema executed outside a request) => Public, i.e. no tenant rows -- fail closed.\n        let scope = ctx.data_opt::<application::queries::ReadScope>().cloned().unwrap_or(application::queries::ReadScope::Public);\n        let filter = input\n            .map(|i| application::queries::OrderFilter {\n                customer_id: i.customer_id.map(Into::into),\n                restaurant_id: i.restaurant_id.map(Into::into),\n                status: i.status.map(Into::into),\n            })\n            .unwrap_or_default();\n        let rows = repo.list(filter, &scope).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        // The non-null `restaurant` navigation field: join against the Restaurant read model in memory\n        // (an order is only ever placed against a projected restaurant, so a match always exists).\n        let by_id: std::collections::HashMap<_, _> = restaurants\n            .list(application::queries::RestaurantFilter::default())\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .into_iter()\n            .map(|r| (r.restaurant_id.0, r))\n            .collect();\n        Ok(rows\n            .into_iter()\n            .filter_map(|o| by_id.get(&o.restaurant_id.0).cloned().map(|r| Order::from((o, Restaurant::at(r, now, horizon)))))\n            .collect())",
        ),
        "order" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::OrderReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        // Per-instance authorization (#144): the ReadScope was resolved ONCE at the edge from the verified Principal and injected into the context. Absent (schema executed outside a request) => Public, i.e. no tenant rows -- fail closed.\n        let scope = ctx.data_opt::<application::queries::ReadScope>().cloned().unwrap_or(application::queries::ReadScope::Public);\n        let Some(row) = repo.by_id(input.id.into(), &scope).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else {\n            return Ok(None);\n        };\n        let restaurant = restaurants\n            .by_id(row.restaurant_id)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .ok_or_else(|| async_graphql::Error::new(\"order references an unknown restaurant\"))?;\n        Ok(Some(Order::from((row, Restaurant::at(restaurant, now, horizon)))))",
        ),
        "restaurantLocationsByAccount" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let rows = repo.by_account(input.account_id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(|r| Restaurant::at(r, now, horizon)).collect())",
        ),
        // The two Order-conversation queries read the OrderConversation projection table (#131, epic
        // #129) by order id; both map the one row into their type (the PUBLIC/INTERNAL split is a
        // column split). Rows map 1:1 — no navigation joins. Null when the conversation is unopened.
        "orderConversation" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::OrderConversationReadRepository>>()?;\n        let row = repo.by_order(input.order_id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(row.map(OrderConversation::from))",
        ),
        "orderConversationInternalNotes" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::OrderConversationReadRepository>>()?;\n        let row = repo.by_order(input.order_id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(row.map(ConversationInternalNotes::from))",
        ),
        // The three DeliveryJob queries read the View_DeliveryJob fold view (ADR-0031/0039). The
        // non-null `order`/`restaurant` navigation fields hydrate from their read models — all three
        // rows are projections of the same domain log.
        "delivery" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let deliveries = ctx.data::<std::sync::Arc<dyn application::queries::DeliveryReadRepository>>()?;\n        let orders = ctx.data::<std::sync::Arc<dyn application::queries::OrderReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n                // Per-instance authorization (#144): the ReadScope was resolved ONCE at the edge from the verified Principal and injected into the context. Absent (schema executed outside a request) => Public, i.e. no tenant rows -- fail closed.\n        let scope = ctx.data_opt::<application::queries::ReadScope>().cloned().unwrap_or(application::queries::ReadScope::Public);\n        let Some(job) = deliveries.by_order(input.order_id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else {\n            return Ok(None);\n        };\n        // The order hydration carries the caller's scope, and a miss degrades to None like the\n        // by-id query does (#144): out-of-scope (or a not-yet-folded membership after acceptance)\n        // must not become a GraphQL error -- that would be an existence oracle plus error noise.\n        let Some(order) = orders\n            .by_id(job.order_id, &scope)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))? else {\n            return Ok(None);\n        };\n        let restaurant = restaurants\n            .by_id(job.restaurant_id)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .ok_or_else(|| async_graphql::Error::new(\"delivery references an unknown restaurant\"))?;\n        Ok(Some(DeliveryJob::from((job, order, Restaurant::at(restaurant, now, horizon)))))",
        ),
        "myDeliveries" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        // The rider's identity is the verified session principal (ADR-0047): the rider app acts\n        // under its Supabase subject, which serves as the RiderId until a dedicated rider identity\n        // read model lands. No principal (schema executed outside a request) or an anonymous one →\n        // no jobs, not an error.\n        let Some(rider_id) = ctx\n            .data_opt::<crate::auth::Principal>()\n            .and_then(|p| p.user_id())\n            .and_then(|s| uuid::Uuid::parse_str(s).ok())\n        else {\n            return Ok(Vec::new());\n        };\n        let deliveries = ctx.data::<std::sync::Arc<dyn application::queries::DeliveryReadRepository>>()?;\n        let orders = ctx.data::<std::sync::Arc<dyn application::queries::OrderReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let rows = deliveries\n            .for_rider(domain::generated::scalars::RiderId(rider_id), input.and_then(|i| i.status).map(Into::into))\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        // Non-null `order`/`restaurant` navigation fields: join by id (a job is only dispatched for a\n        // projected order+restaurant, so a missing target simply drops the job).\n        // The order join runs as SYSTEM deliberately (#144): the row-level authorization decision\n        // for this list is for_rider(rider_id) itself, and the PENDING offer pool it returns is\n        // jobs the rider has NOT accepted yet -- no membership exists until acceptance, so\n        // threading the caller scope here would silently drop every offered job and no rider\n        // would ever see (or accept) new work: a self-sealing dispatch outage whose only symptom\n        // is an empty list.\n        let mut out = Vec::new();\n        for job in rows {\n            let Some(order) = orders.by_id(job.order_id, &application::queries::ReadScope::System).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else { continue };\n            let Some(restaurant) = restaurants.by_id(job.restaurant_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else { continue };\n            out.push(DeliveryJob::from((job, order, Restaurant::at(restaurant, now, horizon))));\n        }\n        Ok(out)",
        ),
        "restaurantDeliveries" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let deliveries = ctx.data::<std::sync::Arc<dyn application::queries::DeliveryReadRepository>>()?;\n        let orders = ctx.data::<std::sync::Arc<dyn application::queries::OrderReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let restaurant_id: domain::generated::scalars::RestaurantId = input.restaurant_id.into();\n        let rows = deliveries\n            .by_restaurant(restaurant_id, input.status.map(Into::into))\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        if rows.is_empty() {\n            return Ok(Vec::new());\n        }\n        // One board = one restaurant: hydrate the non-null `restaurant` navigation target once.\n        let restaurant = restaurants\n            .by_id(restaurant_id)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .ok_or_else(|| async_graphql::Error::new(\"delivery references an unknown restaurant\"))?;\n        // ONE board = ONE restaurant = ONE service-window evaluation, cloned per job (RSO-1).\n        let restaurant = Restaurant::at(restaurant, now, horizon);\n                // Per-instance authorization (#144): the ReadScope was resolved ONCE at the edge from the verified Principal and injected into the context. Absent (schema executed outside a request) => Public, i.e. no tenant rows -- fail closed.\n        let scope = ctx.data_opt::<application::queries::ReadScope>().cloned().unwrap_or(application::queries::ReadScope::Public);\n        let mut out = Vec::new();\n        for job in rows {\n            // Caller-scoped join (#144): a restaurant is granted on its own orders at placement, so\n            // its own board hydrates; a caller passing ANOTHER restaurant's id gets the jobs'\n            // orders dropped and an empty board -- filtered, not an error, no oracle.\n            let Some(order) = orders.by_id(job.order_id, &scope).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else { continue };\n            out.push(DeliveryJob::from((job, order, restaurant.clone())));\n        }\n        Ok(out)",
        ),
        // The refund queue reads the View_PendingRefunds fold view (RefundProcess). Rows map 1:1 —
        // no navigation fields (the Payment aggregate is not a registered API type), so no joins.
        "pendingRefunds" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::RefundReadRepository>>()?;\n        let filter = input\n            .map(|i| application::queries::RefundFilter {\n                restaurant_id: i.restaurant_id.map(Into::into),\n                status: i.status.map(Into::into),\n            })\n            .unwrap_or_default();\n        let rows = repo.list(filter).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(Refund::from).collect())",
        ),
        // The restaurant timeliness insight reads the View_DeliverySatisfaction fold view (#62). Rows
        // map 1:1 — no navigation fields, so no joins.
        "restaurantDeliverySatisfaction" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::DeliverySatisfactionReadRepository>>()?;\n        let rows = repo\n            .by_restaurant(input.restaurant_id.into(), input.timeliness.map(Into::into))\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(DeliverySatisfaction::from).collect())",
        ),
        // Delivery-partner self-registration (#61): the EXTERNAL/admin review queue reads the
        // View_DeliveryPartnerAvailability fold view. Rows map 1:1 — no navigation joins.
        "deliveryPartnerAvailabilities" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::DeliveryPartnerAvailabilityReadRepository>>()?;\n        let filter = input\n            .map(|i| application::queries::DeliveryPartnerAvailabilityFilter {\n                city_id: i.city_id.map(Into::into),\n                channel: i.channel.map(Into::into),\n                status: i.status.map(Into::into),\n            })\n            .unwrap_or_default();\n        let rows = repo.list(filter).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(DeliveryPartnerAvailability::from).collect())",
        ),
        // Reclamations / customer claims (#154): the three claim reads over the View_Reclamation fold
        // view. `myReclamations` scopes to the caller's Customer identity (verified Principal → Customer
        // row, like `me`); `restaurantReclamations` filters the queue by status/category (restaurant
        // narrowing is a recorded follow-up gap); `reclamation` is claim detail by id. Rows map 1:1.
        "myReclamations" => Some(
            "        // Per-instance authorization (#144/#433): the ReadScope was resolved ONCE at the edge from the token's verified claims (CARD-11) and injected into the context -- the same identity source as every other guarded read; the by_auth_ref bridge is gone from authorization. Absent => Public -- fail closed.\n        let scope = ctx.data_opt::<application::queries::ReadScope>().cloned().unwrap_or(application::queries::ReadScope::Public);\n        // \"Which customer am I\" is exactly what the claim answers -- no bridge row, no lookup.\n        let application::queries::ReadScope::Customer(customer_id) = scope else {\n            return Ok(Vec::new());\n        };\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::ReclamationReadRepository>>()?;\n        let rows = repo.by_customer(customer_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(Reclamation::from).collect())",
        ),
        "restaurantReclamations" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::ReclamationReadRepository>>()?;\n        let filter = input\n            .map(|i| application::queries::ReclamationFilter {\n                status: i.status.map(Into::into),\n                category: i.category.map(Into::into),\n                overdue: i.overdue,\n            })\n            .unwrap_or_default();\n        let rows = repo.list(filter).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(Reclamation::from).collect())",
        ),
        "reclamation" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::ReclamationReadRepository>>()?;\n        let row = repo.by_id(input.reclamation_id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(row.map(Reclamation::from))",
        ),
        // Customer store-credit balance (#158, Part B of #207): scoped to the caller's Customer
        // identity (verified Principal → Customer row, the same me-pattern as `myReclamations`); reads
        // the materialized CustomerCreditBalance projection table. Null for an anonymous caller or a
        // customer with no ledger yet.
        "customerCredit" => Some(
            "        // Per-instance authorization (#144/#433): the ReadScope was resolved ONCE at the edge from the token's verified claims (CARD-11) and injected into the context -- the same identity source as every other guarded read; the by_auth_ref bridge is gone from authorization. Absent => Public -- fail closed.\n        let scope = ctx.data_opt::<application::queries::ReadScope>().cloned().unwrap_or(application::queries::ReadScope::Public);\n        // \"Which customer am I\" is exactly what the claim answers -- no bridge row, no lookup.\n        let application::queries::ReadScope::Customer(customer_id) = scope else {\n            return Ok(None);\n        };\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::CustomerCreditReadRepository>>()?;\n        let row = repo.by_customer(customer_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(row.map(CustomerCredit::from))",
        ),
        "prospectionPipeline" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::ProspectionReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let filter = input\n            .map(|i| application::queries::ProspectFilter {\n                min_score: i.min_score.map(|s| s.0 as i32),\n                status: i.status.map(Into::into),\n            })\n            .unwrap_or_default();\n        let rows = repo.list(filter).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        // The non-null `restaurant` navigation field: join against the Restaurant read model in memory\n        // (both rows are folded from the same Restaurant-stream events, so a match always exists).\n        let by_id: std::collections::HashMap<_, _> = restaurants\n            .list(application::queries::RestaurantFilter::default())\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .into_iter()\n            .map(|r| (r.restaurant_id.0, r))\n            .collect();\n        Ok(rows\n            .into_iter()\n            .filter_map(|p| by_id.get(&p.restaurant_id.0).cloned().map(|r| Prospect::from((p, Restaurant::at(r, now, horizon)))))\n            .collect())",
        ),
        // The three admin policy queries read seeded REFERENTIAL tables (ADR-0037) — no args, no input.
        "pricingPolicy" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::PricingPolicyReadRepository>>()?;\n        let rows = repo.list().await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(PricingPolicy::from).collect())",
        ),
        "uberEstimationPolicy" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::UberEstimationPolicyReadRepository>>()?;\n        let rows = repo.list().await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(UberEstimationPolicy::from).collect())",
        ),
        "uberSplitPolicy" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::UberSplitPolicyReadRepository>>()?;\n        let rows = repo.list().await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(UberSplitPolicy::from).collect())",
        ),
        // #639 part C step 6-iv round 2 (ADR-20260905-101349 §2 amendment, PROP §6.5): flat, no
        // args beyond paging -- the restaurant comes from `ReadScope::Restaurant`, never an
        // argument. `viewerAuthority` rides on the CONNECTION (ux/graphql: the ONLY expressible
        // MANAGER condition), resolved through the SAME `MemberAuthorityRepository` the
        // `AuthorityGuard` uses -- never the roster row itself (a roster rebuild must never change
        // what a DIFFERENT member's own authority reads as).
        "restaurantRoster" => Some(
"        let Some(application::queries::ReadScope::Restaurant(scope_id)) = ctx.data_opt::<application::queries::ReadScope>() else {
            return Err(super::mutation::forbidden_error());
        };
        let Some(subject) = ctx.data_opt::<crate::auth::Principal>().and_then(|p| p.user_id()) else {
            return Err(super::mutation::forbidden_error());
        };
        let authority_repo = ctx.data::<std::sync::Arc<dyn application::queries::MemberAuthorityRepository>>()?;
        let Some(viewer_authority) = authority_repo
            .authority_for_subject(domain::generated::scalars::AuthSubject(subject.to_string()), *scope_id)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?
        else {
            return Err(super::mutation::forbidden_error());
        };
        let roster = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantRosterReadRepository>>()?;
        let limit = input.as_ref().and_then(|i| i.limit).map(|v| v.0).filter(|l| *l > 0).unwrap_or(50).min(200);
        let offset = input.and_then(|i| i.offset).map(|v| v.0).filter(|o| *o >= 0).unwrap_or(0);
        let rows = roster.by_scope(*scope_id, limit, offset).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;
        let items = rows
            .into_iter()
            .map(|r| RestaurantRosterEntry {
                membership_id: r.membership_id.into(),
                member_id: r.member_id.into(),
                authority: r.authority.into(),
                since: r.since,
            })
            .collect();
        Ok(RestaurantRosterConnection { items, viewer_authority: viewer_authority.into() })",
        ),
        "restaurantInvitations" => Some(
"        let Some(application::queries::ReadScope::Restaurant(scope_id)) = ctx.data_opt::<application::queries::ReadScope>() else {
            return Err(super::mutation::forbidden_error());
        };
        let invitations = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantInvitationListReadRepository>>()?;
        let limit = input.as_ref().and_then(|i| i.limit).map(|v| v.0).filter(|l| *l > 0).unwrap_or(50).min(200);
        let offset = input.and_then(|i| i.offset).map(|v| v.0).filter(|o| *o >= 0).unwrap_or(0);
        let rows = invitations.by_scope(*scope_id, limit, offset).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|r| RestaurantInvitationEntry {
                invitation_id: r.invitation_id.into(),
                invited_email: r.invited_email.into(),
                authority: r.authority.into(),
                status: r.status.into(),
                expires_at: r.expires_at,
                created_at: r.created_at,
            })
            .collect())",
        ),
        _ => None,
    }
}

/// Emit `crates/server/src/graphql/generated/mutation.rs` — the `MutationRoot`, mirroring
/// `mutation_block`: one async resolver per api.yaml mutation, ACCEPTANCE-FIRST
/// (ADR-20260720-015500). Since #242 Runtime D there is exactly ONE resolver shape: the command is
/// ENQUEUED on its actor's mailbox lane through the generated typed client (durable RECEIVED row,
/// idempotent by messageId + payload hash) and the uniform `MutationAcceptance` is returned
/// immediately; the partitioned worker delivers it and completes the row, publishing the
/// transition on the `OperationStatusBus` for `operationStatus`/`operationStatusChanged`. Nothing
/// is spawned in the request path and no resolver writes a second journal.

/// One `derived:` property's Rust injection statement (#865, ADR-20260904-015903 §6, realized by
/// `auth.rs::resolve_rider_scope` — #849 "#639 part C step 2b" / ADR-20260830-191457 parts A+B):
/// the
/// resolver reads the caller's OWN `ReadScope` (never a client-suppliable claim) and writes the
/// domain id straight into the mailbox payload — the ONE seam a client cannot smuggle past, since
/// the property carries no field on the generated Input type at all. A REQUIRED derived property
/// fails CLOSED (`errors.yaml#/Forbidden`, the sync path) before the mailbox is ever touched — the
/// caller's `api-derived-role-mismatch`-checked `roles: [RIDER]` means the ONLY legitimate scope
/// here is `ReadScope::Rider`, so anything else (Public, an unbound rider row, System) is refused
/// rather than silently minted from nothing. A NULLABLE derived property injects nothing on any
/// other scope — the mutation's OTHER paths (e.g. ADMIN on `reportDeliveryIssue`) carry no id at
/// all, which is the D2 "ops reported" contract, not a bug.
pub(crate) fn derived_injection_block(prop: &str, source: &str, required: bool) -> String {
    // Only `rider` exists today (the closed set lives in `validate/api_derived.rs`); widening adds
    // an arm here in the SAME change as that closed set and the loader that reads it.
    let scope_arm = match source {
        "rider" => "application::queries::ReadScope::Rider",
        other => panic!(
            "derived source '{other}' has no server-side injection arm in emit/server_graphql.rs -- \
             add one here in the same change as validate/api_derived.rs's closed source set."
        ),
    };
    // #639 part C step 4-i (ADR-20260904-081527 §1): `ReadScope::Rider` is a STRUCT variant
    // (`{ id, standing }`) so a guard that ignores standing does not compile — the `Identity` and
    // this injection still match `Rider { id, .. }` on purpose: a restricted rider on the
    // carve-out still resolves to a rider scope (`handBackDelivery` derives its `riderId` from it).
    let pattern = format!("{scope_arm} {{ id: __derived_id, .. }}");
    if required {
        format!(
            "        let __derived_scope = ctx.data_opt::<application::queries::ReadScope>();\n        let Some({pattern}) = __derived_scope else {{\n            return Err(forbidden_error());\n        }};\n        payload_json[\"{prop}\"] = serde_json::json!(__derived_id.0);\n"
        )
    } else {
        format!(
            "        if let Some({pattern}) = ctx.data_opt::<application::queries::ReadScope>() {{\n            payload_json[\"{prop}\"] = serde_json::json!(__derived_id.0);\n        }}\n"
        )
    }
}

pub(crate) fn emit_server_mutation(model: &Model) -> String {
    let api = parse_api(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/api.yaml — do not edit by hand.\n// The GraphQL MutationRoot, ACCEPTANCE-FIRST (ADR-20260720-015500): one resolver per api.yaml\n// mutation, `(input: <Command>Input!, metadata: MetadataInput) -> MutationAcceptance!`. The resolver\n// ENQUEUES the command on its actor's mailbox lane (`inbound_messages`, THE only journal since #242\n// Runtime D — durable RECEIVED, idempotent by messageId: same payload hash replays the original\n// acceptance, a different one is a Conflict) and answers with the effective envelope + PENDING.\n// The partitioned mailbox worker delivers it and completes the row (SUCCEEDED | REJECTED | FAILED),\n// publishing the transition on the OperationStatusBus; post-acceptance rejections surface as\n// Operation.errorCode, never as GraphQL errors (the sync path — input/metadata validation,\n// duplicate-payload Conflict — still uses them).\n// Each non-public field carries its api.yaml `roles` as a `guard` + `visible` pair (ADR-0006).\n//\n// OBSERVABILITY (issue #191): every resolver emits the `command-acceptance` contract's three spans —\n// `command.receive` (SERVER) -> `command.journal` (INTERNAL) -> `command.dispatch` (INTERNAL) — plus its\n// four metrics. The span field names live in `crates/telemetry`, not here: inlining `info_span!` would\n// copy the contract's attribute list into every one of these resolvers, so a contract change could land\n// in some and not others with nothing to catch it.\n#![allow(unused_variables)]\n#![allow(dead_code)]\n\nuse tracing::Instrument as _;\n\nuse super::acl::*;\nuse super::inputs::*;\nuse super::scalars::*;\nuse super::types::*;\n",
    );
    out.push_str("\npub struct MutationRoot;\n\n#[async_graphql::Object(name = \"Mutation\")]\nimpl MutationRoot {\n");
    let addressing = command_addressing(model);
    for m in &api.mutations {
        let fnname = rust_ident(&snake_field(&m.name));
        let acl = acl_field_attr(model, &m.roles, &m.while_restricted, &m.name);
        push_doc(&mut out, "    ", m.description.as_deref());
        // THE MAILBOX (#242 Runtime C3/D, PROP-20260728-152752): EVERY command is ENQUEUED on its
        // actor's `inbound_messages` lane and answered PENDING — the partitioned worker delivers
        // it; nothing is spawned in the request path. Process-manager legs (placeOrder,
        // approveRefund, denyRefund) are addressed exactly like aggregate commands since Runtime D
        // retired the gate and the journal+spawn arm behind it.
        // THE ADDRESSING IS THE GATE (#771). It used to be `wired_mutation_dispatch(&m.name)` — a
        // Rust table of handler-call strings — which asked the WRONG question: a resolver only
        // ENQUEUES, so whether a handler exists says nothing about whether the resolver can be
        // written. The right question is whether the command has an actors.yaml mailbox address,
        // and that is what `addressing` answers. "Does the enqueued message reach a handler?" is
        // now the compiler's question, over `infrastructure::inbox` (E0004), across ALL received
        // commands rather than only those a mutation happens to reach.
        {
            if let Some(addr) = addressing.get(&m.command) {
                let actor_id_expr = match &addr.identity_prop {
                    // A DECLARED identity property that is missing or unparsable fails AT THE
                    // DOOR — the same rule the worker-channel enqueue helper enforces (#272 D3,
                    // id-minting unification): silently minting a random lane id would park the
                    // command on an arbitrary lane and break the per-aggregate serialization the
                    // mailbox exists to give. (The typed input normally catches this first; this
                    // is the door's own guarantee, not the input layer's.)
                    Some(prop) => format!(
                        "let actor_id = payload_json.get(\"{prop}\").and_then(|v| v.as_str()).and_then(|s| uuid::Uuid::parse_str(s).ok()).ok_or_else(|| async_graphql::Error::new(\"{command}: identity property '{prop}' missing or not a uuid -- unaddressable\"))?;",
                        command = m.command,
                    ),
                    None => "// Birth command: the handler mints the aggregate id; this one only routes the mailbox lane.\n        let actor_id = uuid::Uuid::now_v7();".to_string(),
                };
                // #865: one derived-field injection block per `derived:` property, BETWEEN
                // `command_payload` and the typed deserialize (young's trap -- after it, every
                // rider mutation would fail deserialization on a REQUIRED derived property). Empty
                // for the ~90 mutations with no `derived:` -- no behaviour change there.
                let required: HashSet<&str> = model
                    .defs
                    .get("commands.yaml")
                    .and_then(|d| d.get(&m.command))
                    .and_then(|c| c.get("required"))
                    .and_then(|r| r.as_sequence())
                    .map(|s| s.iter().filter_map(|x| x.as_str()).collect())
                    .unwrap_or_default();
                let derived_injection: String = m
                    .derived
                    .iter()
                    .map(|(prop, source)| derived_injection_block(prop, source, required.contains(prop.as_str())))
                    .collect();
                out.push_str(&format!(
                    "    #[graphql(name = \"{name}\"{acl})]\n    async fn {fnname}(&self, ctx: &async_graphql::Context<'_>, input: {command}Input, metadata: Option<MetadataInput>) -> async_graphql::Result<MutationAcceptance> {{\n        // command.receive (SERVER). Opened before any fallible work so an input that fails to\n        // deserialize still leaves a span naming the command that was attempted.\n        let __receive = telemetry::spans::command_receive(\n            \"{command}\",\n            crate::graphql::acl::request_role(ctx).api_name(),\n            telemetry::CHANNEL_GRAPHQL,\n        );\n        let __rx = __receive.clone();\n        async move {{\n        let mailbox = ctx.data::<std::sync::Arc<dyn actor_client::mailbox::Mailbox>>()?.clone();\n        let mut payload_json = command_payload(&input)?;\n{derived_injection}        // SYNC input validation (fail fast as a GraphQL error) AND the typed value the actor client\n        // sends (#284 slice 2) -- the mailbox payload is the domain command's own serde form.\n        let cmd: domain::generated::commands::{command} = serde_json::from_value(payload_json.clone())\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        let env = request_envelope(ctx, &metadata);\n        // run_identity: both ids are mandatory in every contract and both may be server-generated.\n        telemetry::spans::record_envelope(&__rx, &env.message_id.to_string(), &env.correlation_id.to_string());\n        {actor_id_expr}\n        // The TYPED DOOR (#284 slice 2, PROP-20260728-152752 §2.1): the generated {actor}Client\n        // assembles the mailbox row through the SAME shared constructors the worker-channel\n        // enqueue uses (lane, partition, kind, payload hash), so the GraphQL door can never\n        // drift from any other door and no resolver builds a mailbox entry inline.\n        let __client = {actor_crate}::{actor}Client::new(mailbox, actor_id);\n        let __envelope = actor_client::mailbox::Envelope {{\n            message_id: env.message_id,\n            correlation_id: env.correlation_id,\n            cause_id: env.cause_id,\n            session_id: env.session_id,\n            trace_id: env.trace_id.clone(),\n            user_id: env.user_id,\n            user_type: env.user_type.clone(),\n            channel: \"GRAPHQL\".into(),\n        }};\n        // command.journal (INTERNAL) — the typed send IS the durable acceptance now; the\n        // span keeps its contract name (the acceptance contract is unchanged, ADR-20260720-015500).\n        let __journal = telemetry::spans::command_journal(&env.message_id.to_string());\n        let __outcome = __client.send(cmd, __envelope).instrument(__journal.clone()).await.map_err(domain_error)?;\n        match __outcome {{\n            actor_client::EnqueueOutcome::PayloadConflict(_) => {{\n                // A reused messageId with a DIFFERENT payload is a client bug, and the only\n                // acceptance outcome the contract does NOT count as success.\n                telemetry::spans::record_journal_status(&__journal, telemetry::journal_status::CONFLICT);\n                telemetry::meters::acceptance::sync_conflict(\"{command}\");\n                return Err(conflict_error(env.message_id));\n            }}\n            actor_client::EnqueueOutcome::Deduplicated(status) => {{\n                telemetry::spans::record_journal_status(&__journal, telemetry::journal_status::DUPLICATE);\n                let _ = telemetry::spans::command_dispatch(\n                    &env.message_id.to_string(),\n                    telemetry::dispatch_outcome::DUPLICATE_SKIPPED,\n                );\n                telemetry::meters::acceptance::duplicate(telemetry::CHANNEL_GRAPHQL);\n                return Ok(acceptance(&env, mailbox_status_api(status), true));\n            }}\n            actor_client::EnqueueOutcome::Enqueued => {{\n                telemetry::spans::record_journal_status(&__journal, telemetry::journal_status::RECEIVED);\n            }}\n        }}\n        // command.dispatch (INTERNAL): ENQUEUED — the mailbox worker owns delivery and completion\n        // (its StatusBusObserver publishes the terminal transition post-commit).\n        let _ = telemetry::spans::command_dispatch(\n            &env.message_id.to_string(),\n            telemetry::dispatch_outcome::ENQUEUED,\n        );\n        telemetry::meters::acceptance::accepted(telemetry::CHANNEL_GRAPHQL);\n        Ok(acceptance(&env, OperationStatus::PENDING, false))\n        }}\n        .instrument(__receive)\n        .await\n    }}\n",
                    name = m.name,
                    acl = acl,
                    fnname = fnname,
                    command = m.command,
                    actor = addr.actor_type,
                    actor_crate = client_crate_ident(&addr.actor_type),
                    actor_id_expr = actor_id_expr,
                    derived_injection = derived_injection
                ));
                continue;
            }
        }
        // UNADDRESSED = UNGENERATABLE (#242 Runtime D). Every mutation's command is addressed to
        // an actor mailbox above; with `command_journal` gone there is no second arm to fall back
        // to, so an api.yaml mutation whose command carries no actors.yaml addressing FAILS
        // GENERATION here instead of silently emitting a resolver that writes nowhere. Since #771
        // there is no unwired-stub arm either: a deferral belongs in the DSL, not in a stub body.
        panic!(
            "mutation `{}` (command `{}`) has no mailbox addressing -- declare `identity` + \
             `mailbox.partitions` on the actor that receives it in actors.yaml. Every mutation \
             ENQUEUES on a lane; there is no second arm to fall back to, and emitting an \
             `Err(\"not implemented\")` stub would ship a control that renders and does nothing. \
             A handler you are not ready to write is declared in the DSL as \
             `receives[].deferred: {{ reason, issue }}` (#771), which keeps the door real and the \
             deferral reviewable.",
            m.name, m.command
        );
    }
    out.push_str("}\n");
    // The `UNWIRED_MUTATIONS` const and its `assert_eq!` retired with #771, SUBSUMED BY THE
    // COMPILER (ADR-20260803-234035: deleting a gate the compiler subsumes is a correct outcome).
    // The assert asked "does this mutation have a row in the handler-call table?". After #771 there
    // is no handler-call table, and the question it protected — does an addressed message actually
    // reach a handler — is answered by E0004 over `infrastructure::inbox`, across a STRICTLY LARGER
    // set: all 100 commands some actor receives, not only the 90 reachable from a mutation. The ten
    // in that gap were exactly the ones the assert could not see, two of them live PM `sends:`.
    // Shared write-side plumbing for the wired resolvers.
    out.push_str(
        "\n/// The stripped serde wire shape of the GraphQL input — both the mailbox `payload` column and the\n/// domain command deserialize from it (generated from the same commands.yaml, camelCase). `null`s\n/// are stripped first — an unset GraphQL optional serializes as an explicit null, while the domain\n/// payloads model absence as a MISSING key (`Option` fields / `#[serde(default)]` arrays).\nfn command_payload(input: &impl serde::Serialize) -> async_graphql::Result<serde_json::Value> {\n    let mut value = serde_json::to_value(input).map_err(|e| async_graphql::Error::new(e.to_string()))?;\n    strip_nulls(&mut value);\n    Ok(value)\n}\n\nfn strip_nulls(value: &mut serde_json::Value) {\n    match value {\n        serde_json::Value::Object(map) => {\n            map.retain(|_, v| !v.is_null());\n            for v in map.values_mut() {\n                strip_nulls(v);\n            }\n        }\n        serde_json::Value::Array(items) => {\n            for v in items.iter_mut() {\n                strip_nulls(v);\n            }\n        }\n        _ => {}\n    }\n}\n\n/// `RequestRole` → the scalars.yaml UserType TEXT value (ADR-20260728: enums are stored verbatim).\nfn role_text(role: &crate::graphql::acl::RequestRole) -> &'static str {\n    use crate::graphql::acl::RequestRole as R;\n    match role {\n        R::Public => \"PUBLIC\",\n        R::Customer => \"CUSTOMER\",\n        R::RestaurantAccount => \"RESTAURANT_ACCOUNT\",\n        R::Restaurant => \"RESTAURANT\",\n        R::Rider => \"RIDER\",\n        R::Admin => \"ADMIN\",\n        R::External => \"EXTERNAL\",\n    }\n}\n\n/// The EFFECTIVE technical envelope of one mutation request (ADR-20260720-015500): what the client\n/// supplied via MetadataInput/headers, completed server-side (UUIDv7) and echoed back verbatim in\n/// the MutationAcceptance.\npub(crate) struct RequestEnvelope {\n    pub message_id: uuid::Uuid,\n    pub correlation_id: uuid::Uuid,\n    pub cause_id: Option<uuid::Uuid>,\n    pub session_id: Option<uuid::Uuid>,\n    pub trace_id: Option<String>,\n    pub user_id: Option<uuid::Uuid>,\n    pub user_type: String,\n}\n\nfn request_envelope(ctx: &async_graphql::Context<'_>, metadata: &Option<MetadataInput>) -> RequestEnvelope {\n    let principal = ctx.data_opt::<crate::auth::Principal>();\n    let user_id = principal\n        .and_then(|p| p.user_id())\n        .and_then(|s| uuid::Uuid::parse_str(s).ok());\n    let user_type = principal.map(|p| role_text(&p.recorded_role())).unwrap_or(\"PUBLIC\").to_string();\n    let session_id = ctx.data_opt::<crate::graphql::session::SessionHeader>().and_then(|s| s.0);\n    let trace_id = ctx.data_opt::<crate::graphql::session::TraceContext>().and_then(|t| t.0.clone());\n    // Client-suppliable ids validate structurally at scalar parse time; anything missing is\n    // server-generated (time-ordered UUIDv7) and the correlation defaults to the messageId.\n    let message_id = metadata\n        .as_ref()\n        .and_then(|m| m.message_id.as_ref())\n        .map(|v| v.0)\n        .unwrap_or_else(uuid::Uuid::now_v7);\n    let correlation_id = metadata\n        .as_ref()\n        .and_then(|m| m.correlation_id.as_ref())\n        .map(|v| v.0)\n        .unwrap_or(message_id);\n    let cause_id = metadata.as_ref().and_then(|m| m.cause_id.as_ref()).map(|v| v.0);\n    RequestEnvelope { message_id, correlation_id, cause_id, session_id, trace_id, user_id, user_type }\n}\n\n/// The uniform acceptance payload from the effective envelope.\nfn acceptance(env: &RequestEnvelope, status: OperationStatus, duplicate: bool) -> MutationAcceptance {\n    MutationAcceptance {\n        message_id: MessageId(env.message_id),\n        correlation_id: CorrelationId(env.correlation_id),\n        cause_id: env.cause_id.map(CauseId),\n        session_id: env.session_id.map(SessionId),\n        trace_id: env.trace_id.clone().map(TraceId),\n        operation_status: status,\n        duplicate,\n    }\n}\n\n\n/// `inbound_messages` lifecycle → the caller-facing OperationStatus (the ONE journal, #242):\n/// RECEIVED/SCHEDULED read as PENDING; IGNORED/DUPLICATE are the aggregate's no-change/redelivery\n/// verdicts — success from the caller's seat; CANCELLED can only be a withdrawn reminder — a\n/// command never reaches it, mapped FAILED defensively.\npub(crate) fn mailbox_status_api(s: domain::generated::scalars::InboundMessageStatus) -> OperationStatus {\n    use domain::generated::scalars::InboundMessageStatus as M;\n    match s {\n        M::RECEIVED | M::SCHEDULED => OperationStatus::PENDING,\n        M::SUCCEEDED | M::IGNORED | M::DUPLICATE => OperationStatus::SUCCEEDED,\n        M::REJECTED => OperationStatus::REJECTED,\n        M::FAILED | M::CANCELLED => OperationStatus::FAILED,\n    }\n}\n\n/// The caller's locale for the human-readable `Operation.message` (#639 part C step 2c-ii): the\n/// transport injects `crate::graphql::locale::RequestLocale` (cookie → Accept-Language → the\n/// platform default); a context with none — a direct schema execution in a test — keeps the\n/// pre-locale contract (English). The CODE is the contract (`errorCode`); the message is\n/// presentation, derived at READ time from the row's `{ code, context }` — never stored.\npub(crate) fn request_locale(ctx: &async_graphql::Context<'_>) -> crate::graphql::locale::RequestLocale {\n    ctx.data_opt::<crate::graphql::locale::RequestLocale>().copied().unwrap_or_default()\n}\n\n/// A mailbox status row → the API Operation shape (`operationStatus` / `operationStatusChanged`),\n/// its message interpolated from the row's typed error context in the caller's locale — so a\n/// French rider reads `RiderNotRegistered` as the French catalogue sentence naming the support\n/// contact, on the poll path AND the push path (both build from the durable row).\npub(crate) fn operation_from_mailbox(\n    row: &actor_client::mailbox::MailboxStatusRow,\n    locale: crate::graphql::locale::RequestLocale,\n) -> Operation {\n    let error_code = row\n        .error\n        .as_ref()\n        .and_then(|e| e.get(\"code\"))\n        .and_then(|c| c.as_str())\n        .map(str::to_owned);\n    let message = match (&error_code, row.error.as_ref().and_then(|e| e.get(\"context\"))) {\n        (Some(code), Some(context)) => locale.message(code, context),\n        _ => None,\n    };\n    Operation {\n        message_id: MessageId(row.message_id),\n        correlation_id: CorrelationId(row.correlation_id),\n        status: mailbox_status_api(row.status),\n        error_code,\n        message,\n        occurred_at: row.completed_at.unwrap_or(row.received_at),\n    }\n}\n\n/// The mailbox row's ownership scope (ADR-20260720-015500): ADMIN, the accepting actor (JWT\n/// subject), or the accepting session (X-SESSION-ID). Callers resolve null / an empty stream on\n/// false — the PUBLIC surface must not become an existence oracle.\npub(crate) fn mailbox_operation_owned(\n    ctx: &async_graphql::Context<'_>,\n    row: &actor_client::mailbox::MailboxStatusRow,\n) -> bool {\n    let admin = matches!(\n        crate::graphql::acl::request_role(ctx),\n        crate::graphql::acl::RequestRole::Admin\n    );\n    let principal_uuid = ctx\n        .data_opt::<crate::auth::Principal>()\n        .and_then(|p| p.user_id())\n        .and_then(|s| uuid::Uuid::parse_str(s).ok());\n    let session = ctx.data_opt::<crate::graphql::session::SessionHeader>().and_then(|s| s.0);\n    admin\n        || (principal_uuid.is_some() && principal_uuid == row.user_id)\n        || (session.is_some() && session == row.session_id)\n}\n\n/// The synchronous Conflict for a replayed messageId whose payload differs — a client bug, not a\n/// retry (ADR-20260720-015300); errors.yaml cross-cutting `Conflict`, P-10 extensions shape.\nfn conflict_error(message_id: uuid::Uuid) -> async_graphql::Error {\n    use async_graphql::ErrorExtensions;\n    let def = domain::generated::errors::CONFLICT;\n    async_graphql::Error::new(format!(\n        \"messageId {message_id} was already used with a different payload\"\n    ))\n    .extend_with(|_, ext| ext.set(\"code\", def.code))\n}\n\n/// Map a SYNCHRONOUS failure (mailbox enqueue, input deserialization) onto the GraphQL error\n/// contract (P-10): an anticipated errors.yaml rejection surfaces `extensions.code` = the stable\n/// PascalCase code, the interpolated English message as the error message, and its typed context\n/// fields under the extensions; anything unexpected (repository/adapter failures) surfaces as the\n/// generic catalogued `Internal` — never leaking adapter details to the client.\nfn domain_error(e: domain::shared::errors::DomainError) -> async_graphql::Error {\n",
    );
    out.push_str(
        "    use async_graphql::ErrorExtensions;\n    use domain::shared::errors::DomainError;\n    match e {\n        DomainError::Rejected { code, context } => {\n            let message = domain::generated::errors::message_en(&code, &context)\n                .unwrap_or_else(|| code.clone());\n            async_graphql::Error::new(message).extend_with(|_, ext| {\n                ext.set(\"code\", code.as_str());\n                if let Some(fields) = context.as_object() {\n                    for (key, value) in fields {\n                        if key == \"code\" {\n                            continue; // never let a context field shadow the wire code\n                        }\n                        ext.set(\n                            key.as_str(),\n                            async_graphql::Value::from_json(value.clone())\n                                .unwrap_or(async_graphql::Value::Null),\n                        );\n                    }\n                }\n            })\n        }\n        // Legacy \"<Code>: <detail>\" string invariants (interim adapters, e.g. the fail-closed\n        // payment stand-in): surface the prefix when it is a catalogued code, else it is unexpected.\n        DomainError::Invariant(msg) => {\n            let code = msg.split(':').next().map(str::trim).unwrap_or(\"\").to_string();\n            if domain::generated::errors::find(&code).is_some() {\n                async_graphql::Error::new(msg).extend_with(|_, ext| ext.set(\"code\", code.as_str()))\n            } else {\n                internal_error()\n            }\n        }\n        DomainError::Repository(_) => internal_error(),\n    }\n}\n\n/// The generic catalogued `Internal` fallback (errors.yaml): unexpected/infrastructure failures\n/// never leak their detail to the client.\nfn internal_error() -> async_graphql::Error {\n    use async_graphql::ErrorExtensions;\n    let def = domain::generated::errors::INTERNAL;\n    async_graphql::Error::new(def.message_en).extend_with(|_, ext| ext.set(\"code\", def.code))\n}\n\n/// The synchronous Forbidden for a `derived:` property whose REQUIRED source scope did not resolve\n/// (#865): the caller's `roles:` already narrows to the source's role (`api-derived-role-mismatch`),\n/// so reaching here with no matching `ReadScope` means an unbound identity (no row, System, or an\n/// absent context in a direct schema execution) -- refused BEFORE the mailbox is ever touched,\n/// never a Public default that enqueues. errors.yaml cross-cutting `Forbidden`, P-10 extensions shape\n/// (mirrors `conflict_error` exactly).\npub(crate) fn forbidden_error() -> async_graphql::Error {\n    use async_graphql::ErrorExtensions;\n    let def = domain::generated::errors::FORBIDDEN;\n    async_graphql::Error::new(def.message_en).extend_with(|_, ext| ext.set(\"code\", def.code))\n}\n",
    );
    out
}

// `wired_mutation_dispatch` retired with #771. It held the 90 handler CALLS as Rust source in
// STRING LITERALS -- source no compiler ever checked until the emitted file was built downstream.
// Those calls are ordinary source in the human-owned `crates/infrastructure/src/inbox.rs` now,
// matched against the generated per-actor inbox enums.

/// The mailbox ADDRESSING of one command: which actor receives it, through which payload property
/// its instance id is read, over how many partitions. `identity_prop` is `None` when the command's
/// payload does not carry the actor's identity (a birth command whose id the handler mints) — the
/// edge then mints an ADDRESSING-ONLY actor_id (payload untouched; unification is a D item).
pub(crate) struct CommandAddress {
    pub(crate) actor_type: String,
    pub(crate) identity_prop: Option<String>,
    pub(crate) partitions: u16,
}

/// command name → its mailbox address, from actors.yaml (`identity` + `mailbox.partitions` +
/// `receives`). A command received by several actors takes the FIRST declaring actor (commands are
/// 1:1 in practice; events are the fan-out kind).
pub(crate) fn command_addressing(model: &Model) -> BTreeMap<String, CommandAddress> {
    let mut map: BTreeMap<String, CommandAddress> = BTreeMap::new();
    let Some(Value::Mapping(actors)) = model.defs.get("actors.yaml") else { return map };
    for (k, def) in actors {
        let Some(actor) = k.as_str().filter(|s| *s != "principals") else { continue };
        // The TYPED identity form (`identity: { $ref: '#/<Actor>/state/<field>' }`,
        // ADR-20260731-214500 consequences) — the bare-string form is a hard validator error
        // (`identity-untyped`), and generation only runs on a 0-error catalog.
        let Some(identity) = actor_identity_field(def, actor) else { continue };
        let Some(width) = def
            .get("mailbox")
            .and_then(|m| m.get("partitions"))
            .and_then(|p| p.as_u64())
        else {
            continue;
        };
        let Some(receives) = def.get("receives").and_then(|r| r.as_sequence()) else { continue };
        for entry in receives {
            let Some(r) = entry.get("message").and_then(|m| m.get("$ref")).and_then(|r| r.as_str())
            else {
                continue;
            };
            let Some(rest) = r.strip_prefix("commands.yaml#/") else { continue };
            map.entry(rest.to_string()).or_insert_with(|| CommandAddress {
                actor_type: actor.to_string(),
                identity_prop: message_property_exists(model, r, &identity)
                    .then(|| identity.clone()),
                partitions: width as u16,
            });
        }
    }
    map
}

/// Emit `crates/infrastructure/src/generated/command_router.rs`.
///
/// SINCE #771 THIS FILE NO LONGER CONTAINS A ROUTER. The `match command_type { … }` that used to
/// live here — a flat match over a `&str` across ALL actors, ending in `_ => None` → a `FAILED
/// "unroutable command type"` row — is replaced by the generated per-actor `<Actor>Inbox` enums
/// (`application::generated::inboxes`) matched in the HUMAN-OWNED `infrastructure::inbox`. What
/// stays here is the frozen addressing surface the composition root and the workers read.
///
/// The 90 handler calls this emitter used to hold as Rust source in STRING LITERALS
/// (`wired_mutation_dispatch`) are ordinary source in `infrastructure::inbox` now, where the
/// compiler checks them.
pub(crate) fn emit_infra_command_router(model: &Model) -> String {
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/*/actors.yaml — do not edit by hand.\n// The worker-side addressing surface of the actor mailbox.\n//\n// The command ROUTER moved out of this file with #771: `message_type` → handler is no longer a\n// flat generated match over a string. It is a generated per-actor `<Actor>Inbox` enum\n// (`application::generated::inboxes`, emitted from each actor's `receives:`) matched in the\n// HUMAN-OWNED `infrastructure::inbox` — so a message an actor declares it receives and nobody\n// consumes is an E0004 build failure instead of a `FAILED \"unroutable command type\"` row.\n\n/// Every port any command handler needs — the worker-side counterpart of the resolvers' ctx.data\n/// injections, bundled once at the composition root. DEFINED with the router it feeds; re-exported\n/// here so every existing consumer (composition root, standalone workers, tests) keeps one import\n/// path.\npub use crate::inbox::CommandDeps;\n\n// The frozen command-addressing tables (mailbox_address, ACTOR_MAILBOXES) live in the\n// actor_client boundary crate since #290 phase 1 — re-exported for the worker-side consumers.\npub use actor_client::generated::addresses::{mailbox_address, ACTOR_MAILBOXES};\n",
    );
    let widths = actor_mailbox_widths(model);
    // The per-actor activation policy (PROP-20260728-152752 §3.5, #272 D3): rendered for EVERY
    // mailbox actor so the composition root resolves policy from the spec, never from code
    // defaults it can drift from. Absent block = enabled under the global gate, global idle.
    out.push_str(
        "\n/// Per-actor ACTIVATION policy (actors.yaml `mailbox.activations`, gated globally by\n/// configuration.yaml `ACTOR_ACTIVATIONS`): `(actor_type, enabled, idle-seconds override)`.\n/// An absent spec block renders as `(true, None)` — enabled under the global gate, passivating\n/// at the global `ACTOR_ACTIVATION_IDLE_SECONDS`.\npub const ACTOR_ACTIVATIONS: &[(&str, bool, Option<i64>)] = &[\n",
    );
    for (actor, _) in &widths {
        let (enabled, idle) = model
            .defs
            .get("actors.yaml")
            .and_then(|m| m.get(actor.as_str()))
            .and_then(|def| def.get("mailbox"))
            .and_then(|m| m.get("activations"))
            .map(|act| match act {
                Value::Bool(b) => (*b, None),
                Value::Mapping(m) => (
                    m.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
                    m.get("idle_seconds").and_then(|v| v.as_i64()),
                ),
                _ => (true, None),
            })
            .unwrap_or((true, None));
        let idle = idle.map(|n| format!("Some({})", n)).unwrap_or_else(|| "None".to_string());
        out.push_str(&format!("    (\"{}\", {}, {}),\n", actor, enabled, idle));
    }
    out.push_str("];\n");
    out
}

/// Emit `crates/infrastructure/src/generated/deletion_policy.rs` — the GENERIC deletion engine's
/// parameter table (ADR-20260731-214500 §4): the decided journey (checkpoint verification, window,
/// tombstone, stream deletion, receipt) is implemented ONCE in infrastructure and parameterized
/// entirely by the actors.yaml `deletion:` declarations rendered here. Returns `None` when NO actor
/// declares `deletion:` — the file is then not emitted at all, so the artifact set stays byte-stable
/// until the first spec delta lands (zero-drift gate).
pub(crate) fn emit_infra_deletion_policy(model: &Model) -> Option<String> {
    let deletions = parse_deletions(model);
    if deletions.is_empty() {
        return None;
    }
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/actors.yaml `deletion:` blocks\n// (ADR-20260731-214500) — do not edit by hand. The generic deletion engine's parameter table:\n// per actor type, the triggers that start (or cancel) its deletion journey and the receipt fact\n// recorded on the ledger when the journey completes. Event/config/property names are the spec's\n// own vocabulary — the engine resolves them against the event store, the typed configuration and\n// the child projection at runtime.\n\n/// One deletion trigger. `after_config_key = None` means PROPAGATION: the engine reacts to the\n/// recorded fact immediately, enumerating child instances through the typed `match` pair.\npub struct DeletionTrigger {\n    /// Event types that start the journey.\n    pub on: &'static [&'static str],\n    /// configuration.yaml key naming the window (a generated reminder, reschedule in place) —\n    /// `None` = immediate propagation.\n    pub after_config_key: Option<&'static str>,\n    /// Event types that CANCEL a pending scheduled deletion (SCHEDULED -> CANCELLED).\n    pub cancelled_on: &'static [&'static str],\n    /// Property of the triggering event carrying the parent key (propagation).\n    pub match_event_property: Option<&'static str>,\n    /// This actor's state field the parent key matches (propagation).\n    pub match_state_field: Option<&'static str>,\n}\n\n/// One actor's declared deletion policy.\npub struct DeletionPolicy {\n    pub actor_type: &'static str,\n    /// The actor's identity property (its typed `identity` ref) — the stream key: a trigger whose\n    /// `match_state_field` equals it identifies the doomed instance DIRECTLY by the event property\n    /// (self-trigger / direct key); anything else needs child-projection enumeration.\n    pub identity: &'static str,\n    pub triggers: &'static [DeletionTrigger],\n    /// The business fact recorded on the deletion ledger when the journey completes\n    /// (pseudonymous references, never erased payloads — ADR-20260731-160000 §6).\n    pub receipt: &'static str,\n}\n\npub const DELETION_POLICIES: &[DeletionPolicy] = &[\n",
    );
    let str_list = |refs: &[String]| -> String {
        refs.iter()
            .filter_map(|r| ref_name(r))
            .map(|n| format!("\"{}\"", n))
            .collect::<Vec<_>>()
            .join(", ")
    };
    for d in &deletions {
        let identity = model
            .defs
            .get("actors.yaml")
            .and_then(|m| m.get(d.actor.as_str()))
            .and_then(|def| actor_identity_field(def, &d.actor))
            .unwrap_or_default();
        out.push_str(&format!(
            "    DeletionPolicy {{\n        actor_type: \"{}\",\n        identity: \"{}\",\n        triggers: &[\n",
            d.actor, identity
        ));
        for t in &d.triggers {
            let after = t
                .after_ref
                .as_deref()
                .and_then(config_key_ref_name)
                .map(|k| format!("Some(\"{}\")", k))
                .unwrap_or_else(|| "None".to_string());
            let m_event = t
                .match_event_ref
                .as_deref()
                .and_then(lineage_parts)
                .and_then(|(_, p)| p.map(|p| format!("Some(\"{}\")", p)))
                .unwrap_or_else(|| "None".to_string());
            let m_state = t
                .match_state_ref
                .as_deref()
                .and_then(parse_ref)
                .and_then(|pr| pr.path.last().cloned())
                .map(|f| format!("Some(\"{}\")", f))
                .unwrap_or_else(|| "None".to_string());
            out.push_str(&format!(
                "            DeletionTrigger {{ on: &[{}], after_config_key: {}, cancelled_on: &[{}], match_event_property: {}, match_state_field: {} }},\n",
                str_list(&t.on),
                after,
                str_list(&t.cancelled_on),
                m_event,
                m_state
            ));
        }
        out.push_str(&format!(
            "        ],\n        receipt: \"{}\",\n    }},\n",
            d.receipt_ref.as_deref().and_then(ref_name).unwrap_or_default()
        ));
    }
    out.push_str("];\n");
    Some(out)
}

/// Emit `crates/application/src/generated/reminders.rs` — the scheduling table the `schedules:`
/// declarations expand to (ADR-20260731-214500 §2): per (actor, received message), the reminders a
/// SUCCESSFUL delivery (re)declares. The mailbox delivery glue applies it inside the completion
/// transaction; the generated behaviour tests assert it as the handler's third observable effect.
/// Always emitted (an empty catalog renders an empty table) so the hand-written
/// `application::reminders` runtime compiles against a stable module.
pub(crate) fn emit_app_reminders(model: &Model) -> String {
    let actors = parse_actors(model);
    let reminders = parse_reminders(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/actors.yaml `reminders:` + `schedules:`\n// declarations (ADR-20260731-214500 §2) — do not edit by hand. One row per (actor, received\n// message, reminder): a SUCCESSFUL delivery of `on_message` (re)declares the reminder — the\n// handler's third observable effect, applied by the mailbox delivery glue inside the completion\n// transaction and asserted by the generated behaviour tests (schedule + the declared reschedule\n// policy, ADR-20260731-150500 / #167).\n\n/// Re-declaration semantics for one reminder (#167 Phase 0): what a second `schedules:` of the\n/// SAME (actor, purpose) identity does while the row is still SCHEDULED.\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum ReschedulePolicy {\n    /// `reschedule: in-place` (ADR-20260731-150500): the pending occurrence moves — the LAST\n    /// declaration wins the clock (the retention-window shape: a later terminal fact postpones).\n    InPlace,\n    /// `reschedule: keep` (#167): the FIRST scheduled_at wins — re-declaring never extends a\n    /// deadline (the acceptance-timeout shape: a redelivered birth fact must not push it out).\n    Keep,\n}\n\n/// One declared scheduling effect.\n///\n/// `#[non_exhaustive]` ON PURPOSE (#290 review BLOCKING-2): the fields are readable everywhere,\n/// but a LITERAL can be built only in this crate — i.e. only by this generated table. Without\n/// it, any crate could forge a spec (arbitrary actor_type/message_type/reminder identity) and\n/// feed it to `actor_client::reminders::scheduled_entry`, minting a real mailbox row around the\n/// sealed clients. Specs come from `REMINDER_SCHEDULES` / `reminder_schedules_for`, period.\n#[non_exhaustive]\npub struct ReminderSchedule {\n    pub actor_type: &'static str,\n    /// The receives message (command or event name) whose successful delivery schedules.\n    pub on_message: &'static str,\n    /// The reminder's name — the identity axis: `message_id = UUIDv5(actor_id, reminder)`.\n    pub reminder: &'static str,\n    /// The payload FACT type (events.yaml vocabulary) recorded at delivery (ADR-20260731-153000 §1a).\n    pub payload_event: &'static str,\n    /// The actor's identity property — the single payload field of the reminder fact.\n    pub identity_prop: &'static str,\n    /// configuration.yaml key naming the window. The duration unit is the KEY's typed `unit:`\n    /// field, already applied to `after_default` below and to `Config::reminder_windows()` —\n    /// never parsed from this name (#167; the SCREAMING suffix is for humans).\n    pub after_key: &'static str,\n    /// The key's spec default as a typed Duration — what the behaviour tests schedule with;\n    /// production reads Config (`reminder_windows()`), which is Duration-typed too.\n    pub after_default: std::time::Duration,\n    /// The declared re-declaration semantics (`reschedule:`, default `in-place`).\n    pub reschedule: ReschedulePolicy,\n}\n\npub const REMINDER_SCHEDULES: &[ReminderSchedule] = &[\n",
    );
    for actor in &actors {
        for entry in &actor.receives {
            let on_message = match reminder_ref_parts(&entry.message_ref) {
                Some((_, rname)) => {
                    reminder_payload_event(model, &entry.message_ref).unwrap_or(rname)
                }
                None => match ref_name(&entry.message_ref) {
                    Some(m) => m,
                    None => continue,
                },
            };
            for sref in &entry.schedules {
                let Some((ractor, rname)) = reminder_ref_parts(sref) else { continue };
                let Some(rem) = reminders.iter().find(|r| r.actor == ractor && r.name == rname)
                else {
                    continue; // §2f `schedules-unresolved` owns the report; generation stays quiet.
                };
                let payload_event = ref_name(&rem.payload_ref).unwrap_or_else(|| {
                    panic!("reminders: {}/{} payload ref unresolved", ractor, rname)
                });
                // The generic payload builder fills the identity property ALONE — a reminder fact
                // requiring more would be silently under-filled at delivery, so fail generation.
                let identity = model
                    .defs
                    .get("actors.yaml")
                    .and_then(|m| m.get(actor.name.as_str()))
                    .and_then(|def| actor_identity_field(def, &actor.name))
                    .unwrap_or_else(|| {
                        panic!("reminders: actor {} schedules {} but declares no typed identity", actor.name, rname)
                    });
                if let Some(props) = resolve_ref(model, &rem.payload_ref, "actors.yaml")
                    .and_then(|d| d.get("required"))
                    .and_then(|r| r.as_sequence())
                {
                    for p in props.iter().filter_map(|v| v.as_str()) {
                        assert_eq!(
                            p,
                            identity.as_str(),
                            "reminders: {}'s payload fact {} requires property '{}' — the generic builder fills only the identity ('{}'); extend the builder before declaring it",
                            rname, payload_event, p, identity
                        );
                    }
                }
                let after_key = rem
                    .after_ref
                    .as_deref()
                    .and_then(config_key_ref_name)
                    .unwrap_or_else(|| {
                        panic!("reminders: {}/{} declares no `after` window — a scheduled receive needs one until dynamic scheduling has a use case", ractor, rname)
                    });
                let key_node = model
                    .defs
                    .get("configuration.yaml")
                    .and_then(|c| c.get("keys"))
                    .and_then(|k| k.get(after_key.as_str()));
                let default = key_node
                    .and_then(|k| k.get("default"))
                    .and_then(|d| d.as_i64())
                    .unwrap_or_else(|| {
                        panic!("reminders: configuration key {} has no integer default", after_key)
                    });
                // The duration unit is the key's TYPED `unit:` field (#167) — the deleted
                // `ends_with("_DAYS")` suffix parse must never come back. Rule
                // `reminder-window-unit` reports the spec hole first; this panic keeps
                // generation honest if it is ever bypassed.
                let default_expr = match key_node.and_then(|k| k.get("unit")).and_then(|u| u.as_str())
                {
                    Some("days") => format!("std::time::Duration::from_secs({} * 86_400)", default),
                    Some("seconds") => format!("std::time::Duration::from_secs({})", default),
                    other => panic!(
                        "reminders: window key {} declares unit {:?} — the closed set is days|seconds (#167, rule reminder-window-unit)",
                        after_key, other
                    ),
                };
                let policy = match rem.reschedule.as_deref() {
                    Some("keep") => "ReschedulePolicy::Keep",
                    _ => "ReschedulePolicy::InPlace", // `in-place` (or absent) — the historical default
                };
                out.push_str(&format!(
                    "    ReminderSchedule {{ actor_type: \"{}\", on_message: \"{}\", reminder: \"{}\", payload_event: \"{}\", identity_prop: \"{}\", after_key: \"{}\", after_default: {}, reschedule: {} }},\n",
                    actor.name, on_message, rname, payload_event, identity, after_key, default_expr, policy
                ));
            }
        }
    }
    out.push_str(
        "];\n\n/// The reminders a successful delivery of `on_message` to `actor_type` (re)declares.\npub fn reminder_schedules_for(\n    actor_type: &str,\n    on_message: &str,\n) -> impl Iterator<Item = &'static ReminderSchedule> {\n    let (actor_type, on_message) = (actor_type.to_owned(), on_message.to_owned());\n    REMINDER_SCHEDULES\n        .iter()\n        .filter(move |s| s.actor_type == actor_type && s.on_message == on_message)\n}\n",
    );
    out
}

/// Emit `crates/server/src/graphql/generated/subscription.rs` — the `SubscriptionRoot`, mirroring
/// `subscription_block`: one stream resolver per api.yaml subscription with the SDL argument/return
/// shape. Wired resolvers subscribe to the in-process `infrastructure::EventBus` (each envelope is
/// published by `PgEventStore::append` AFTER a successful commit) and map matching envelopes onto the
/// declared return type — re-resolving the read models rather than exposing raw `domain_events`. Each
/// non-public field carries the same generated `guard`/`visible` ACL pair as queries/mutations.
pub(crate) fn emit_server_subscription(model: &Model) -> String {
    let api = parse_api(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/api.yaml — do not edit by hand.\n// The GraphQL SubscriptionRoot: one stream resolver per api.yaml subscription, matching the generated\n// SDL shape. `operationStatusChanged` streams the operation lifecycle through the typed\n// `ActorClient::watch` over the in-process response bus, #303 (snapshot-first, ownership-scoped — ADR-20260720-015500); the domain-fact\n// subscriptions (`orderStatusChanged`, `paymentStatusChanged`) subscribe to the in-process EventBus\n// (each envelope is published by PgEventStore::append AFTER a successful commit) and re-resolve the\n// read models / saga row rather than exposing raw domain_events (ADR-0005/0035). Each non-public\n// field carries its api.yaml `roles` as a `guard` (execution) + `visible` (introspection) pair from\n// the generated acl module (ADR-0006 role-as-path).\n//\n// Free-tier caveat: the buses are IN-PROCESS and a GraphQL-over-WebSocket connection lives only while\n// the app instance is warm (the uptimerobot ping keeps it so); after a restart/redeploy clients must\n// resubscribe and re-sync via the pull queries (`order`, `operationStatus`, `paymentStatus`).\n#![allow(unused_variables)]\n#![allow(dead_code)]\n\nuse async_graphql::futures_util::Stream;\n\nuse super::acl::*;\nuse super::inputs::*;\nuse super::scalars::*;\nuse super::types::*;\n\npub struct SubscriptionRoot;\n\n#[async_graphql::Subscription(name = \"Subscription\")]\nimpl SubscriptionRoot {\n",
    );
    for s in &api.subscriptions {
        let fnname = rust_ident(&snake_field(&s.name));
        let acl = acl_field_attr(model, &s.roles, &s.while_restricted, &s.name);
        let arg = if s.args.is_empty() {
            String::new()
        } else {
            let ty = format!("{}SubscriptionInput", pascal(&s.name));
            let ty = if s.args.iter().any(|a| a.required) { ty } else { format!("Option<{}>", ty) };
            format!(", input: {}", ty)
        };
        let inner = gql_rust_name(&s.returns_type);
        let mut ret = if s.returns_list { format!("Vec<{}>", inner) } else { inner };
        if s.returns_nullable {
            ret = format!("Option<{}>", ret);
        }
        push_doc(&mut out, "    ", s.description.as_deref());
        match wired_subscription_body(&s.name) {
            // Wired: stream over the injected EventBus (+ read repos) from ctx.data.
            Some(body) => out.push_str(&format!(
                "    #[graphql(name = \"{}\"{})]\n    async fn {}(&self, ctx: &async_graphql::Context<'_>{}) -> async_graphql::Result<impl Stream<Item = async_graphql::Result<{}>>> {{\n{}\n    }}\n",
                s.name, acl, fnname, arg, ret, body
            )),
            None => out.push_str(&format!(
                "    #[graphql(name = \"{}\"{})]\n    async fn {}(&self{}) -> async_graphql::Result<impl Stream<Item = async_graphql::Result<{}>>> {{\n        Err::<async_graphql::futures_util::stream::Empty<async_graphql::Result<{}>>, _>(async_graphql::Error::new(\"not implemented\"))\n    }}\n",
                s.name, acl, fnname, arg, ret, ret
            )),
        }
    }
    out.push_str("}\n");
    out
}

/// Resolver bodies for subscriptions wired over the EventBus + read models. Returned as the fn body
/// (8-space indent); `None` → the `not implemented` stub. `orderStatusChanged` re-resolves the Order
/// read row per matching envelope (dedupe + terminal completion); `operationStatusChanged` maps each
/// matching envelope onto a SUCCEEDED `Operation` tick (the transient, non-projected type).
pub(crate) fn wired_subscription_body(name: &str) -> Option<&'static str> {
    match name {
        // The operation status stream (ADR-20260720-015500): snapshot-first from the durable
        // row (closes the subscribe/complete race), then every response-bus transition for this
        // messageId through the typed `ActorClient::watch` (#303); completes on a terminal
        // status. Ownership is checked at setup — a non-owned/unknown messageId yields an EMPTY
        // stream (no existence oracle).
        "operationStatusChanged" => Some(
            r#"        // The D4 read door (PROP-20260802-130500, #303): the snapshot reads AND the response
        // stream resolve through the ONE generic ActorClient — same door as `operationStatus`;
        // the bus lives behind it now, so nobody subscribes raw.
        let status_door = actor_client::ActorClient::new(
            ctx.data::<std::sync::Arc<dyn actor_client::mailbox::Mailbox>>()?.clone(),
            ctx.data::<actor_client::OperationStatusBus>()?.clone(),
        );
        let wanted = input.message_id.0;
        let admin = matches!(
            crate::graphql::acl::request_role(ctx),
            crate::graphql::acl::RequestRole::Admin
        );
        let principal_uuid = ctx
            .data_opt::<crate::auth::Principal>()
            .and_then(|p| p.user_id())
            .and_then(|s| uuid::Uuid::parse_str(s).ok());
        let session = ctx.data_opt::<crate::graphql::session::SessionHeader>().and_then(|s| s.0);
        // Watch BEFORE the snapshot read (the subscribe/complete race stays closed): the typed
        // §2.1 response stream (#303), filtered to this messageId, lag made explicit.
        // `watch` is None only on a pull-only door (ActorClient::pull_only); this one is built
        // from the context's real bus two statements up, so None here is a wiring bug, surfaced
        // rather than silently degrading the subscription to nothing.
        let mut watch = status_door
            .watch(wanted)
            .ok_or_else(|| async_graphql::Error::new("operationStatusChanged: the status door carries no response stream"))?;
        let locale = super::mutation::request_locale(ctx);
        Ok(async_stream::stream! {
            use domain::generated::scalars::InboundMessageStatus as M;
            // Snapshot-first (#242 Runtime D): the acceptance already inserted the row on
            // inbound_messages -- the only journal, so an unknown messageId ends the stream.
            let Ok(Some(row)) = status_door.get_operation_status(wanted).await else { return };
            let owned = admin
                || (principal_uuid.is_some() && principal_uuid == row.user_id)
                || (session.is_some() && session == row.session_id);
            if !owned {
                return;
            }
            {
                let terminal = !matches!(row.status, M::RECEIVED | M::SCHEDULED);
                yield Ok(super::mutation::operation_from_mailbox(&row, locale));
                if terminal {
                    return;
                }
            }
            loop {
                match watch.next().await {
                    Some(actor_client::OperationWatchEvent::Update(update)) => {
                        let terminal = !matches!(update.status, M::RECEIVED | M::SCHEDULED);
                        // A terminal REJECTED/FAILED push carries the bus's English summary; the
                        // durable row carries the typed context, so re-read it and localize
                        // exactly as the poll leg does (#639 2c-ii) — the row is the pull truth,
                        // but ONLY once it says so: a bus frame can be observed before the row's
                        // completion is (the subscriptions suite scripts exactly that), and a
                        // still-open row must never turn a terminal push back into PENDING.
                        if terminal && update.error_code.is_some() {
                            if let Ok(Some(row)) = status_door.get_operation_status(wanted).await {
                                if !matches!(row.status, M::RECEIVED | M::SCHEDULED) {
                                    yield Ok(super::mutation::operation_from_mailbox(&row, locale));
                                    break;
                                }
                            }
                        }
                        yield Ok(Operation {
                            message_id: MessageId(update.message_id),
                            correlation_id: CorrelationId(update.correlation_id),
                            status: super::mutation::mailbox_status_api(update.status),
                            error_code: update.error_code.clone(),
                            message: update.message.clone(),
                            occurred_at: chrono::Utc::now(),
                        });
                        if terminal {
                            break;
                        }
                    }
                    // Lagged: the durable row is the pull truth — re-read and finish if terminal.
                    Some(actor_client::OperationWatchEvent::Lagged) => {
                        if let Ok(Some(row)) = status_door.get_operation_status(wanted).await {
                            let terminal = !matches!(row.status, M::RECEIVED | M::SCHEDULED);
                            yield Ok(super::mutation::operation_from_mailbox(&row, locale));
                            if terminal {
                                break;
                            }
                        }
                    }
                    None => break,
                }
            }
        })"#,
        ),
        // Push-based checkout payment tracking (ADR-20260720-015500): initial resolve + re-resolve
        // of the PlaceOrderProcess run row on every Payment-stream envelope; dedupes identical
        // states and completes when the run resolves. Initiator-scoped like queries/paymentStatus.
        "paymentStatusChanged" => Some(
            r#"        let bus = ctx.data::<infrastructure::EventBus>()?.clone();
        let pm = ctx.data::<std::sync::Arc<dyn application::pm_state::PaymentProcessStateStore>>()?.clone();
        let order_id: domain::generated::scalars::OrderId = input.order_id.into();
        let admin = matches!(
            crate::graphql::acl::request_role(ctx),
            crate::graphql::acl::RequestRole::Admin
        );
        let session = ctx.data_opt::<crate::graphql::session::SessionHeader>().and_then(|s| s.0);
        // The caller's Customer identity comes from the ReadScope resolved ONCE at connection
        // init from the token's verified claims (#433, CARD-11) — the same identity source as
        // queries/paymentStatus, so the checkout's order read and its payment stream can never
        // disagree on who the customer is. No bridge row, no lookup.
        let caller_customer: Option<domain::generated::scalars::CustomerId> =
            match ctx.data_opt::<application::queries::ReadScope>() {
                Some(application::queries::ReadScope::Customer(id)) => Some(*id),
                _ => None,
            };
        let mut rx = bus.subscribe();
        Ok(async_stream::stream! {
            use domain::generated::scalars as ds;
            let owned = |row: &application::pm_state::PaymentProcessRow| {
                admin
                    || (caller_customer.is_some() && caller_customer == row.customer_id)
                    || (session.is_some() && session == row.session_id.as_ref().map(|s| s.0))
            };
            // (payment_status, clientSecret presence): dedupe key + what the checkout cares about.
            let mut last: Option<(ds::PaymentStatus, bool)> = None;
            if let Ok(Some(row)) = pm.by_order(order_id).await {
                if !owned(&row) {
                    return;
                }
                let terminal = row.process_status != ds::PaymentProcessStatus::AWAITING_PAYMENT_RESULT;
                last = Some((row.payment_status, row.client_secret.is_some()));
                yield Ok(PaymentIntent {
                    payment_intent_id: row.payment_intent_id.into(),
                    client_secret: row.client_secret,
                    status: row.payment_status.into(),
                });
                if terminal {
                    return;
                }
            }
            loop {
                let evt = match rx.recv().await {
                    Ok(evt) => evt,
                    // Lagged: the next Payment envelope re-resolves the CURRENT row anyway.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                if !evt.stream_name.starts_with("Payment-") {
                    continue;
                }
                let Ok(Some(row)) = pm.by_order(order_id).await else { continue };
                if !owned(&row) {
                    continue;
                }
                let key = (row.payment_status, row.client_secret.is_some());
                if last.as_ref() == Some(&key) {
                    continue;
                }
                last = Some(key);
                let terminal = row.process_status != ds::PaymentProcessStatus::AWAITING_PAYMENT_RESULT;
                yield Ok(PaymentIntent {
                    payment_intent_id: row.payment_intent_id.into(),
                    client_secret: row.client_secret,
                    status: row.payment_status.into(),
                });
                if terminal {
                    break;
                }
            }
        })"#,
        ),
        // Push-based order tracking: each matching envelope re-resolves the CURRENT Order from the
        // read model (queries never read raw domain_events), dedupes on the row's own `updated_at`
        // fold clock (#420 — a `status`-keyed dedupe swallowed every delivery movement) and
        // completes on a terminal status. Matching envelopes are the order's stream AND its
        // delivery job's, because the rider hops are appended to the latter.
        "orderStatusChanged" => Some(
            r#"        let bus = ctx.data::<infrastructure::EventBus>()?.clone();
        let orders = ctx.data::<std::sync::Arc<dyn application::queries::OrderReadRepository>>()?.clone();
        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?.clone();
        let deliveries = ctx.data::<std::sync::Arc<dyn application::queries::DeliveryReadRepository>>()?.clone();
        // Tracked by orderId (#14, ADR-20260720-220000) — the key the confirmation screen has —
        // replacing the pre-acceptance-first correlationId convention.
        let order_id: domain::generated::scalars::OrderId = input.order_id.into();
        let wanted_stream = format!("Order-{}", order_id.0);
        // Per-instance authorization (#144): the ReadScope was resolved ONCE at connection init,
        // from the same bridge the query path uses — the stream must not widen what a query would
        // refuse. Every row read below is scoped, so ownership for EVERY role (customer,
        // restaurant, account, rider) comes from the ScopeMembership index — this closes the
        // "RESTAURANT paths are trusted" gap recorded on ADR-20260720-220000. Absent scope
        // (schema executed outside a transport) => Public, i.e. no rows — fail closed.
        let scope = ctx.data_opt::<application::queries::ReadScope>().cloned().unwrap_or(application::queries::ReadScope::Public);
        // RSO-1: the validity horizon is connection-scoped CONFIGURATION; "now" is read PER YIELD
        // below through the blessed streaming-clock symbol (`service_clock::evaluate_now` —
        // api.yaml ServiceWindow.evaluatedAt: "per pushed update, not per subscribe") — a
        // subscribe-time instant would serve every later push a stale serviceWindow.
        let service_window_horizon = ctx.data_opt::<crate::graphql::service_clock::ServiceWindowHorizon>().copied().unwrap_or_default().0;
        let mut rx = bus.subscribe();
        Ok(async_stream::stream! {
            use domain::generated::scalars as ds;
            // The last state pushed to this subscriber: the projected row's `updated_at`, which the
            // projector advances on EVERY fold of this row. Deliberately NOT keyed on `status`
            // (#420): a rider's pickup/dropoff folds `delivery_status` / `courier` /
            // `estimated_dropoff_at` onto this same row (#424) and leaves `status` untouched, so a
            // status-keyed dedupe SWALLOWED exactly the movement the tracking screen exists to
            // show. `updated_at` is the row's own "something changed" clock, so the key is the
            // rendered state rather than one field of it.
            let mut last: Option<chrono::DateTime<chrono::Utc>> = None;
            // THIS order's delivery job stream, learned lazily: before dispatch there is no job, so
            // the lookup repeats only across the (short) window between subscribing and dispatch,
            // and costs nothing once bound. Without it, every `DeliveryJob-` envelope on the
            // platform would make every tracking subscriber re-read its row.
            let mut delivery_stream: Option<String> = None;
            'events: loop {
                let evt = match rx.recv().await {
                    Ok(evt) => evt,
                    // Lagged: skipped envelopes are harmless — the next matching one re-resolves the
                    // CURRENT state anyway.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                // THIS order's own stream (`Order-<uuid>`), or THIS order's delivery job
                // (`DeliveryJob-<uuid>`) — the rider hops are appended there, never to the Order
                // stream, so an Order-only filter is a confirmation page that goes quiet at exactly
                // the moment the customer is watching hardest.
                if evt.stream_name != wanted_stream {
                    if !evt.stream_name.starts_with("DeliveryJob-") {
                        continue;
                    }
                    if delivery_stream.is_none() {
                        delivery_stream = match deliveries.by_order(order_id).await {
                            Ok(Some(job)) => Some(format!("DeliveryJob-{}", job.delivery_job_id.0)),
                            // No job yet (or the read failed): ignore this envelope and try again on
                            // the next one — never bind to someone else's job.
                            _ => None,
                        };
                    }
                    if delivery_stream.as_deref() != Some(evt.stream_name.as_str()) {
                        continue;
                    }
                }
                // The row is folded ASYNCHRONOUSLY by the projection worker (ADR-0040): give it a
                // bounded window to absorb this event before treating it as a no-op.
                for attempt in 0..12u32 {
                    if attempt > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    }
                    // The scoped read makes "not projected yet" and "not this caller's order"
                    // both `None` — deliberately indistinguishable (no oracle). A stranger
                    // therefore pays this bounded re-poll window per envelope and then goes
                    // silent; the denial itself is counted by the adapter's telemetry.
                    let row = match orders.by_id(order_id, &scope).await {
                        Ok(Some(row)) => row,
                        Ok(None) => continue, // not projected yet, or out of scope — re-poll
                        Err(e) => {
                            yield Err(async_graphql::Error::new(e.to_string()));
                            continue 'events;
                        }
                    };
                    if last == Some(row.updated_at) {
                        continue; // fold not visible yet — re-poll within the bounded window
                    }
                    last = Some(row.updated_at);
                    let terminal = matches!(
                        row.status,
                        ds::OrderStatus::REJECTED
                            | ds::OrderStatus::DELIVERED
                            | ds::OrderStatus::CANCELLED_BY_CUSTOMER
                            | ds::OrderStatus::CANCELLED_BY_RESTAURANT
                    );
                    // The non-null `restaurant` navigation field: hydrate like the `order` query
                    // does — with THIS push's clock (see the horizon note above).
                    match restaurants.by_id(row.restaurant_id).await {
                        Ok(Some(restaurant)) => yield Ok(Order::from((row, Restaurant::at(restaurant, crate::graphql::service_clock::evaluate_now(), service_window_horizon)))),
                        Ok(None) => {}
                        Err(e) => yield Err(async_graphql::Error::new(e.to_string())),
                    }
                    if terminal {
                        break 'events; // terminal status — complete the subscription
                    }
                    continue 'events;
                }
            }
        })"#,
        ),
        _ => None,
    }
}


/// Emit `crates/server/src/graphql/generated/operation_scopes.rs` — the COMPOSITION TABLE of the
/// API tier (#385, PROP-20260807-174246 D8): every api.yaml operation with the scope whose
/// `specs/{scope}/api.yaml` fragment declares it. Two consumers, one derivation:
/// - the subgraph SCOPE SLICE (`graphql::scope_slice`): a `graphql-{scope}` bin rejects
///   top-level fields owned by another scope;
/// - the generated `gateway-{role}` mains embed the same rows to route each top-level field to
///   its owning subgraph service (static stitching — composition failures are build failures).
pub(crate) fn emit_server_operation_scopes(model: &Model) -> String {
    let api = parse_api(model);
    let origin = |section: &str, name: &str| -> String {
        model
            .origins
            .get(&("api.yaml".to_string(), format!("{section}/{name}")))
            .cloned()
            .unwrap_or_else(|| KERNEL_SCOPE.to_string())
    };
    let mut rows = String::new();
    for q in &api.queries {
        rows.push_str(&format!("    (\"query\", \"{}\", \"{}\"),\n", q.name, origin("queries", &q.name)));
    }
    for m in &api.mutations {
        rows.push_str(&format!("    (\"mutation\", \"{}\", \"{}\"),\n", m.name, origin("mutations", &m.name)));
    }
    for s in &api.subscriptions {
        rows.push_str(&format!("    (\"subscription\", \"{}\", \"{}\"),\n", s.name, origin("subscriptions", &s.name)));
    }
    format!(
        "// GENERATED by the Captain.Food codegen from the per-scope specs/{{scope}}/api.yaml fragments\n// (#385, PROP-20260807-174246 D8) — do not edit by hand. The API tier's composition table: every\n// operation with its OWNING scope. The subgraph scope slice rejects foreign fields against it; the\n// generated gateway mains embed the same rows to route top-level fields to the owning subgraph.\n\n/// `(kind, field, owning scope)` — kind ∈ query | mutation | subscription; field = the GraphQL\n/// top-level field name; scope = the specs/{{scope}}/ folder whose api fragment declares it.\npub const OPERATION_SCOPES: &[(&str, &str, &str)] = &[\n{rows}];\n\n/// The owning scope of one top-level field, if declared.\npub fn operation_scope(kind: &str, field: &str) -> Option<&'static str> {{\n    OPERATION_SCOPES\n        .iter()\n        .find(|(k, f, _)| *k == kind && *f == field)\n        .map(|(_, _, s)| *s)\n}}\n"
    )
}
