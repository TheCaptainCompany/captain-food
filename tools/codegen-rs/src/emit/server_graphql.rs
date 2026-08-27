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
                out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, async_graphql::Enum)]\n");
                out.push_str(&format!("pub enum {} {{\n", name));
                for v in &variants {
                    out.push_str(&format!("    #[graphql(name = \"{}\")]\n    {},\n", v, v));
                }
                out.push_str("}\n");
                out.push_str(&format!("impl From<ds::{}> for {} {{\n    fn from(v: ds::{}) -> Self {{\n        match v {{\n", name, name, name));
                for v in &variants {
                    out.push_str(&format!("            ds::{}::{} => Self::{},\n", name, v, v));
                }
                out.push_str("        }\n    }\n}\n");
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
        "// GENERATED by the Captain.Food codegen from specs/api.yaml + specs/entities.yaml — do not edit by hand.\n// GraphQL output types (async-graphql SimpleObject), mirroring the generated SDL: entities.yaml types\n// not registered as api.yaml projections, then the api.yaml types, each with its FK-derived navigation\n// fields (plain data fields for now — resolved empty until the read resolvers land).\n#![allow(dead_code)]\n#![allow(non_camel_case_types)]\n\nuse actor_client::supervision::{MailboxLaneRow, PoisonedMessageRow};\nuse application::projections::{CartRow, CatalogRow, CustomerCreditBalanceRow, CustomerRow, OrderConversationRow, OrderTrackingRow, ProspectionPipelineRow, RestaurantRow};\nuse application::queries::{DeliveryJobRow, DeliveryPartnerAvailabilityRow, DeliverySatisfactionRow, PricingPolicyRow, ReclamationRow, RefundRow, UberEstimationPolicyRow, UberSplitPolicyRow};\nuse domain::generated::scalars as ds;\n\nuse super::scalars::*;\n",
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
        "\n/// Read-model rows → API type: the OrderTracking row plus the joined `Restaurant` (built once by\n/// the resolver via `Restaurant::at` — non-null `restaurant` navigation field, request clock). The breakdown's `restaurantContribution` is re-derived as\n/// articles − restaurantPayout (the projection stores the split's leaves); the Uber comparison is\n/// rebuilt only when every `uber_*` column is present; `paymentStatus` is folded as TEXT by the\n/// projector and parsed leniently (unknown → PENDING); nav `deliveryJobs` resolve empty until that\n/// read model lands.\nimpl From<(OrderTrackingRow, Restaurant)> for Order {\n    fn from((row, restaurant): (OrderTrackingRow, Restaurant)) -> Self {\n        let currency = row.currency.clone();\n        let breakdown = PaymentBreakdown {\n            articles: order_money(row.articles_cents.clone(), &currency),\n            delivery: order_money(row.delivery_cents.clone(), &currency),\n            service_fee: order_money(row.service_fee_cents.clone(), &currency),\n            total: order_money(row.total_amount_cents.clone(), &currency),\n            restaurant_contribution: order_money(\n                ds::MoneyCents(row.articles_cents.0 - row.restaurant_payout_cents.0),\n                &currency,\n            ),\n            restaurant_payout: order_money(row.restaurant_payout_cents.clone(), &currency),\n            rider_payout: order_money(row.rider_payout_cents.clone(), &currency),\n            captain_net: order_money(row.captain_net_cents.clone(), &currency),\n        };\n        let uber_comparison = match (\n            row.uber_total_cents,\n            row.uber_restaurant_cents,\n            row.uber_rider_cents,\n            row.uber_platform_cents,\n            row.uber_basis,\n        ) {\n            (Some(total), Some(restaurant_share), Some(rider_share), Some(platform_share), Some(basis)) => {\n                Some(UberComparison {\n                    total: order_money(total, &currency),\n                    restaurant_share: order_money(restaurant_share, &currency),\n                    rider_share: order_money(rider_share, &currency),\n                    platform_share: order_money(platform_share, &currency),\n                    basis: basis.into(),\n                })\n            }\n            _ => None,\n        };\n        Self {\n            id: row.order_id.into(),\n            r#ref: row.r#ref.into(),\n            restaurant_id: row.restaurant_id.into(),\n            customer_id: row.customer_id.map(Into::into),\n            status: row.status.into(),\n            service_type: row.service_type.into(),\n            items: serde_json::from_value(row.items).unwrap_or_default(),\n            total_amount: order_money(row.total_amount_cents, &currency),\n            breakdown,\n            delivery_address: row.delivery_address.and_then(|v| serde_json::from_value(v).ok()),\n            estimated_ready_at: row.estimated_ready_at,\n            placed_at: row.placed_at,\n            status_changed_at: row.status_changed_at,\n            payment_status: match row.payment_status.as_str() {\n                \"CAPTURED\" => PaymentStatus::CAPTURED,\n                \"FAILED\" => PaymentStatus::FAILED,\n                \"REFUNDED\" => PaymentStatus::REFUNDED,\n                _ => PaymentStatus::PENDING,\n            },\n            restaurant_stars: row.restaurant_stars.map(Into::into),\n            rating_comment: row.rating_comment.map(Into::into),\n            rider_thumb: row.rider_thumb.map(Into::into),\n            delivery_timeliness: row.delivery_timeliness.map(Into::into),\n            rider_tip: row.rider_tip_cents.map(|c| order_money(c, &currency)),\n            restaurant_tip: row.restaurant_tip_cents.map(|c| order_money(c, &currency)),\n            captain_tip: row.captain_tip_cents.map(|c| order_money(c, &currency)),\n            uber_comparison,\n            delivery_status: row.delivery_status.map(Into::into),\n            courier: row.courier.and_then(|v| serde_json::from_value(v).ok()),\n            estimated_dropoff_at: row.estimated_dropoff_at,\n            rated_at: row.rated_at,\n            delivery_jobs: Vec::new(),\n            restaurant,\n        }\n    }\n}\n",
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
        "\n/// Read-model rows → API type: the `View_DeliveryJob` row (ADR-0031/0039) plus the joined\n/// OrderTracking row and `Restaurant` (built once by the resolver via `Restaurant::at`, then\n/// CLONED into the embedded Order — one evaluation, so the job's two embedded serviceWindow\n/// verdicts agree by construction, RSO-1). Addresses and the courier deserialize out of the\n/// view's jsonb columns.\nimpl From<(DeliveryJobRow, OrderTrackingRow, Restaurant)> for DeliveryJob {\n    fn from((row, order, restaurant): (DeliveryJobRow, OrderTrackingRow, Restaurant)) -> Self {\n        Self {\n            id: row.delivery_job_id.into(),\n            order_id: row.order_id.into(),\n            restaurant_id: row.restaurant_id.into(),\n            status: row.status.into(),\n            provider: row.provider.map(Into::into),\n            courier: row.courier.and_then(|v| serde_json::from_value(v).ok()),\n            pickup_address: serde_json::from_value(row.pickup_address)\n                .expect(\"DeliveryJob.pickupAddress: invalid jsonb\"),\n            dropoff_address: serde_json::from_value(row.dropoff_address)\n                .expect(\"DeliveryJob.dropoffAddress: invalid jsonb\"),\n            estimated_pickup_at: row.estimated_pickup_at,\n            estimated_dropoff_at: row.estimated_dropoff_at,\n            requested_at: row.requested_at,\n            picked_up_at: row.picked_up_at,\n            delivered_at: row.delivered_at,\n            order: (order, restaurant.clone()).into(),\n            restaurant,\n        }\n    }\n}\n",
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
            push_gql_struct_open(&mut out, &format!("{}Input", m.command), "InputObject", def.get("description").and_then(|d| d.as_str()));
            push_gql_object_fields(&mut out, def, "commands.yaml", true);
            out.push_str("}\n");
            visit_inputs(model, &m.command, "commands.yaml", &mut needed, &mut visited);
        }
    }

    let scalars = scalar_names(model);
    for q in &api.queries {
        if q.args.is_empty() {
            continue;
        }
        push_gql_struct_open(&mut out, &format!("{}QueryInput", pascal(&q.name)), "InputObject", None);
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

/// The `guard`/`visible` additions to a generated QueryRoot/MutationRoot field's `#[graphql(...)]`
/// attribute, from the operation's api.yaml `roles`. Empty for public operations.
pub(crate) fn acl_field_attr(model: &Model, roles: &[String]) -> String {
    match acl_role_set(model, roles) {
        Some(set) => {
            let ident = acl_set_ident(&set);
            format!(
                ", guard = \"RoleGuard::new(ALLOW_{})\", visible = \"visible_{}\"",
                ident.to_uppercase(),
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
        "// GENERATED by the Captain.Food codegen from specs/api.yaml — do not edit by hand.\n// Per-operation ACL role sets (ADR-0006 role-as-path): each distinct non-public `roles:` set on an\n// api.yaml query/mutation/subscription becomes an allowed-role const + a `visible` fn. The generated\n// QueryRoot/MutationRoot/SubscriptionRoot fields wire them as `guard = \"RoleGuard::new(ALLOW_…)\"`\n// (execution — unauthorized roles get a FORBIDDEN error) and `visible = \"visible_…\"` (introspection —\n// the field is hidden from unauthorized roles, and async-graphql's `find_visible_types` then hides\n// every type reachable only through hidden fields, so per-role introspection/Voyager expose only that\n// role's surface). Operations with `roles:` OMITTED carry no guard/visible: open to every role path\n// (LITERAL roles, ADR-20260720-191500 — PUBLIC in a list is just the anonymous path).\n#![allow(dead_code)]\n\npub(crate) use super::super::acl::RoleGuard;\nuse super::super::acl::{role_allows, RequestRole};\n",
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

// ─── The read-scope binding table (#618) ─────────────────────────────────────────────────────────
//
// Before this table, the `ReadScope` prelude was HAND-COPIED into each bound arm: `myReclamations`
// and `customerCredit` each carried a verbatim copy of the same lines, comment and all, while
// `restaurantReclamations` — three arms away in the same match — had none, with a comment conceding
// that the narrowing was a follow-up gap. Same defect, three arms apart: second occurrence ⇒ a rule,
// not a comment (ADR-20260803-234035, compiler-first).
//
// What the table removes is the PRELUDE, which is exactly what was copied wrong. The BODIES stay
// per-arm and the table does not pretend otherwise: `myReclamations` / `customerCredit` bind by
// REPLACING the read (`by_customer(...)`), `pendingRefunds` binds by INTERSECTING a caller-supplied
// filter with the bound scope. Those are different operations on different shapes.
//
// `validate::read_scope_binding` calls THIS function — same binary, same table — so a query's
// binding decision and the gate that audits it cannot drift. #649 later swaps this table's SOURCE
// (literal here → derived from the DSL) without touching the emit path or a single test.

/// How one query's resolver binds its read to the caller's `ReadScope`.
pub(crate) enum ScopeBinding {
    /// The read is REPLACED by a scope-derived one. The emitted prelude destructures `ReadScope`
    /// into `variant(bind)`; **every** other scope — `Admin` and `System` included — short-circuits
    /// with `otherwise`. That is correct for the two `me`-pattern reads: "which customer am I" has
    /// no answer for a caller that is not a customer, and an admin is not one.
    Replace {
        /// The `ReadScope` variant this read is only meaningful for, e.g. `Customer`.
        variant: &'static str,
        /// The identifier the variant's payload is bound to, for the arm body to use.
        bind: &'static str,
        /// The expression returned for any other scope — the resolver's own "nothing" shape.
        otherwise: &'static str,
    },
    /// The read's caller-supplied filter is INTERSECTED with the caller's bound scope (#618). The
    /// prelude brings `scope` AND `binding_mode` into the body; the arm performs the intersection
    /// itself, because the filter's shape is the arm's own business.
    Intersect,
}

/// The binding a query's resolver applies, or `None` for a query that binds elsewhere (see
/// [`READ_SCOPE_BINDING_EXEMPT`]).
pub(crate) fn read_scope_binding(op_name: &str) -> Option<ScopeBinding> {
    match op_name {
        // "Which customer am I" is exactly what the verified claim answers — no bridge row, no
        // lookup (#144/#433, CARD-11).
        "myReclamations" => Some(ScopeBinding::Replace {
            variant: "Customer",
            bind: "customer_id",
            otherwise: "Vec::new()",
        }),
        "customerCredit" => {
            Some(ScopeBinding::Replace { variant: "Customer", bind: "customer_id", otherwise: "None" })
        }
        // The refund queue: `restaurantId` is a SELECTOR WITHIN the caller's scope, never a grant
        // (#618, DECISIONS §39 IDOR-1).
        "pendingRefunds" => Some(ScopeBinding::Intersect),
        _ => None,
    }
}

/// Tenant-scoped queries that are deliberately NOT in [`read_scope_binding`], each with a
/// MECHANICAL reason: a `file:line` where the scoping actually happens, or the literal token
/// [`UNSCOPED_TOKEN`]. Prose is not admissible here and `validate::read_scope_binding` rejects it —
/// a reason a reader cannot check is how a gate becomes a formality.
///
/// The `UNSCOPED — #618` rows are the authoritative enumeration of #618's remaining read surfaces:
/// the count is DERIVED from this list rather than measured into a record and left to rot.
pub(crate) const READ_SCOPE_BINDING_EXEMPT: &[(&str, &str)] = &[
    // --- Scoped, but not by this emit path -------------------------------------------------
    // The resolver hands the row and the scope to a hand-written predicate.
    ("cart", "crates/server/src/graphql/cart_read.rs:87"),
    ("current", "crates/server/src/graphql/cart_read.rs:48"),
    // The scope is threaded into the repository and fused into the SQL as an EXISTS predicate, so
    // out-of-scope rows never enter the result set at all.
    ("order", "crates/infrastructure/src/persistence/order.rs:85"),
    ("orders", "crates/infrastructure/src/persistence/order.rs:45"),
    ("delivery", "crates/infrastructure/src/persistence/order.rs:85"),
    ("restaurantDeliveries", "crates/infrastructure/src/persistence/order.rs:85"),
    // Scoped inline in the emitted body — the arm maps the scope to its own read key or performs
    // its own ownership comparison, rather than delegating either.
    ("carts", "crates/server/src/graphql/generated/query.rs:346"),
    ("me", "crates/server/src/graphql/generated/query.rs:101"),
    ("myDeliveries", "crates/server/src/graphql/generated/query.rs:152"),
    ("paymentStatus", "crates/server/src/graphql/generated/query.rs:557"),
    // --- NOT scoped. These are #618's remaining surfaces ------------------------------------
    // Every one takes a caller-supplied id or filter and consults no identity whatsoever.
    ("favoriteRestaurants", UNSCOPED_TOKEN),
    ("orderConversation", UNSCOPED_TOKEN),
    ("orderConversationInternalNotes", UNSCOPED_TOKEN),
    ("reclamation", UNSCOPED_TOKEN),
    ("restaurantDeliverySatisfaction", UNSCOPED_TOKEN),
    ("restaurantLocationsByAccount", UNSCOPED_TOKEN),
    ("restaurantReclamations", UNSCOPED_TOKEN),
];

/// The one admissible non-`file:line` exempt reason: this surface is not scoped at all, and #618
/// owns it. Written as a constant so the list cannot drift into near-spellings of it.
pub(crate) const UNSCOPED_TOKEN: &str = "UNSCOPED — #618";

/// The `ReadScope` prelude, emitted from the table instead of hand-copied into each arm.
fn scope_binding_prelude(binding: &ScopeBinding) -> String {
    let mut out = String::from(
        "        // Per-instance authorization (#144/#433): the ReadScope was resolved ONCE at the edge from the token's verified claims (CARD-11) and injected into the context -- the same identity source as every other guarded read. Absent => Public -- fail closed.\n        let scope = ctx.data_opt::<application::queries::ReadScope>().cloned().unwrap_or(application::queries::ReadScope::Public);\n",
    );
    match binding {
        ScopeBinding::Replace { variant, bind, otherwise } => {
            out.push_str(&format!(
                "        let application::queries::ReadScope::{variant}({bind}) = scope else {{\n            return Ok({otherwise});\n        }};\n"
            ));
        }
        ScopeBinding::Intersect => {
            out.push_str(
                "        // The binding MODE, per request (#618). Absent from the context => ENFORCE: \"forgot to wire it\" must never be indistinguishable from \"chose the permissive value\".\n        let binding_mode = crate::graphql::read_binding::ReadScopeBindingMode::from_context(ctx);\n",
            );
        }
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
        let acl = acl_field_attr(model, &q.roles);
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
        push_doc(&mut out, "    ", q.description.as_deref());
        // The `ReadScope` prelude comes from the BINDING TABLE, never hand-copied into an arm
        // (#618): three arms carrying two verbatim copies and one silent omission is how the
        // omission happened. The arm body follows it and consumes `scope` (and, for an
        // intersecting binding, `binding_mode`).
        let prelude = read_scope_binding(&q.name).map(|b| scope_binding_prelude(&b)).unwrap_or_default();
        match wired_query_body(&q.name) {
            // Wired: delegate to the injected read-model repo (ctx.data); takes &Context.
            Some(body) => out.push_str(&format!(
                "    #[graphql(name = \"{}\"{})]\n    async fn {}(&self, ctx: &async_graphql::Context<'_>{}) -> async_graphql::Result<{}> {{\n{}{}\n    }}\n",
                q.name, acl, fnname, arg, ret, prelude, body
            )),
            None => out.push_str(&format!(
                "    #[graphql(name = \"{}\"{})]\n    async fn {}(&self{}) -> async_graphql::Result<{}> {{\n        Err(async_graphql::Error::new(\"not implemented\"))\n    }}\n",
                q.name, acl, fnname, arg, ret
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
            "        let mailbox = ctx.data::<std::sync::Arc<dyn actor_client::mailbox::Mailbox>>()?.clone();\n        let status_door = actor_client::ActorClient::new(mailbox, ctx.data::<actor_client::OperationStatusBus>()?.clone());\n        if let Some(row) = status_door\n            .get_operation_status(input.message_id.0)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n        {\n            if !super::mutation::mailbox_operation_owned(ctx, &row) {\n                return Ok(None);\n            }\n            return Ok(Some(super::mutation::operation_from_mailbox(&row)));\n        }\n        Ok(None)",
        ),
        // The checkout payment state (ADR-20260720-015500): served from the PlaceOrderProcess run
        // row (the declared PM-privacy exception); initiator-scoped — ADMIN, the checkout's
        // customer (JWT subject → Customer row), or the checkout's session.
        "paymentStatus" => Some(
            "        let pm = ctx.data::<std::sync::Arc<dyn application::pm_state::PaymentProcessStateStore>>()?;\n        let Some(row) = pm\n            .by_order(input.order_id.into())\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n        else {\n            return Ok(None);\n        };\n        let admin = matches!(\n            ctx.data_opt::<crate::graphql::acl::RequestRole>(),\n            Some(crate::graphql::acl::RequestRole::Admin)\n        );\n        let session = ctx.data_opt::<crate::graphql::session::SessionHeader>().and_then(|s| s.0);\n        let session_owned = session.is_some() && session == row.session_id.as_ref().map(|s| s.0);\n        // Per-instance authorization (#144/#433): the ReadScope was resolved ONCE at the edge from the token's verified claims (CARD-11) and injected into the context -- the same identity source as every other guarded read; the by_auth_ref bridge is gone from authorization. Absent => Public -- fail closed.\n        let scope = ctx.data_opt::<application::queries::ReadScope>().cloned().unwrap_or(application::queries::ReadScope::Public);\n        let customer_owned = matches!(\n            (&scope, row.customer_id.as_ref()),\n            (application::queries::ReadScope::Customer(c), Some(row_customer)) if c == row_customer\n        );\n        if !(admin || customer_owned || session_owned) {\n            return Ok(None);\n        }\n        Ok(Some(PaymentIntent {\n            payment_intent_id: row.payment_intent_id.into(),\n            client_secret: row.client_secret,\n            status: row.payment_status.into(),\n        }))",
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
        "restaurants" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let filter = input\n            .map(|i| application::queries::RestaurantFilter { search: i.search, orderable_only: i.orderable_only, limit: i.limit.map(|v| v.0), offset: i.offset.map(|v| v.0) })\n            .unwrap_or_default();\n        let rows = repo.list(filter).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(|r| Restaurant::at(r, now, horizon)).collect())",
        ),
        "restaurant" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let row = repo.by_slug(input.slug.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(row.map(|r| Restaurant::at(r, now, horizon)))",
        ),
        "catalog" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::CatalogReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let Some(row) = repo.by_restaurant(input.restaurant_id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else {\n            return Ok(None);\n        };\n        // The non-null `restaurant` navigation field: hydrate from the Restaurant read model (both rows\n        // are projections of the same domain log, so the FK target always exists).\n        let restaurant = restaurants\n            .by_id(row.restaurant_id)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .ok_or_else(|| async_graphql::Error::new(\"catalog references an unknown restaurant\"))?;\n        Ok(Some(Catalog::from((row, Restaurant::at(restaurant, now, horizon)))))",
        ),
        "categories" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::CatalogReadRepository>>()?;\n        let row = repo.by_restaurant(input.restaurant_id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        // Categories live inside the projected Catalog.tree jsonb; an absent catalog or an empty\n        // tree (a catalog created before any content event) yields an empty list.\n        Ok(row.map(|r| catalog_tree_section::<CatalogCategory>(&r.tree, \"categories\")).unwrap_or_default())",
        ),
        "carts" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::CartReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let catalogs = ctx.data::<std::sync::Arc<dyn application::queries::CatalogReadRepository>>()?;\n        // The ONE request-scoped correlation id (#451, contract `request.correlation_id`): every\n        // cart.price span of THIS request shares it. Absent = the schema was executed OUTSIDE a\n        // request (no transport, e.g. a direct unit-test execution), and the NIL uuid says exactly\n        // that: a random id would be indistinguishable from a real one in a trace, sending an\n        // operator hunting for a request that never existed. All three real paths -- HTTP POST, the\n        // WS connection_init and the SSR render -- inject one, so this is unreachable in production.\n        let correlation_id = ctx.data_opt::<crate::graphql::session::RequestCorrelationId>().map(|c| c.0).unwrap_or(uuid::Uuid::nil());\n        // Per-instance authorization (#144): the ReadScope was resolved ONCE at the edge from the verified Principal and injected into the context. Absent (schema executed outside a request) => Public, i.e. no tenant rows -- fail closed.\n        let scope = ctx.data_opt::<application::queries::ReadScope>().cloned().unwrap_or(application::queries::ReadScope::Public);\n        // Ownership enforced server-side (#144): a CUSTOMER caller's customerId argument is IGNORED\n        // and forced to the caller's own identity; only ADMIN reads another customer's carts (the\n        // query is guarded [CUSTOMER, ADMIN], so no other tenant role can arrive). An unresolvable\n        // identity yields an empty list -- fail closed, never a fall-through to the client filter.\n        let customer_id: domain::generated::scalars::CustomerId = match &scope {\n            application::queries::ReadScope::Customer(id) => *id,\n            application::queries::ReadScope::Admin | application::queries::ReadScope::System => input.customer_id.into(),\n            _ => return Ok(Vec::new()),\n        };\n        let rows = repo.by_customer(customer_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        // The non-null `restaurant` navigation field: join against the Restaurant read model in memory\n        // (a cart is only ever started against a projected restaurant, so a match always exists).\n        let by_id: std::collections::HashMap<_, _> = restaurants\n            .list(application::queries::RestaurantFilter::default())\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .into_iter()\n            .map(|r| (r.restaurant_id.0, r))\n            .collect();\n        // Priced through the ONE `price_cart` seam (#451): each cart from ITS restaurant's live\n        // catalog, one memoized catalog read per cart; an unresolvable price errors the read\n        // (fail-closed, cart-price contract) rather than lying with a partial list.\n        let mut out = Vec::new();\n        for c in rows {\n            let Some(r) = by_id.get(&c.restaurant_id.0).cloned() else { continue };\n            out.push(crate::graphql::cart_read::priced(&**catalogs, c, Restaurant::at(r, now, horizon), correlation_id).await?);\n        }\n        Ok(out)",
        ),
        "cart" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::CartReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let catalogs = ctx.data::<std::sync::Arc<dyn application::queries::CatalogReadRepository>>()?;\n        // The ONE request-scoped correlation id (#451, contract `request.correlation_id`): every\n        // cart.price span of THIS request shares it. Absent = the schema was executed OUTSIDE a\n        // request (no transport, e.g. a direct unit-test execution), and the NIL uuid says exactly\n        // that: a random id would be indistinguishable from a real one in a trace, sending an\n        // operator hunting for a request that never existed. All three real paths -- HTTP POST, the\n        // WS connection_init and the SSR render -- inject one, so this is unreachable in production.\n        let correlation_id = ctx.data_opt::<crate::graphql::session::RequestCorrelationId>().map(|c| c.0).unwrap_or(uuid::Uuid::nil());\n        let Some(row) = repo.by_id(input.id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else {\n            return Ok(None);\n        };\n        // Claim-ownership narrowing (#144/#434, the #451 DONE-WHEN): a CUSTOMER reads only a cart\n        // bound to their claim-resolved id — anyone else's (or an unbound session cart) resolves\n        // null, no existence oracle; ADMIN reads any cart. Scope absent => Public => nothing.\n        let scope = ctx.data_opt::<application::queries::ReadScope>().cloned().unwrap_or(application::queries::ReadScope::Public);\n        if !crate::graphql::cart_read::readable_by(&row, &scope) {\n            return Ok(None);\n        }\n        let restaurant = restaurants\n            .by_id(row.restaurant_id)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .ok_or_else(|| async_graphql::Error::new(\"cart references an unknown restaurant\"))?;\n        Ok(Some(crate::graphql::cart_read::priced(&**catalogs, row, Restaurant::at(restaurant, now, horizon), correlation_id).await?))",
        ),
        // The TWO-LEG current-cart read (#451, ADR-20260810-120531): claim leg, then session leg
        // with the NULL-or-claim ownership filter — all semantics live in the hand-written,
        // unit-tested `cart_read` seam; this literal only assembles the request context.
        "current" => Some(
            "        // ONE request clock (RSO-1): (now, horizon) read once at the transport seam and threaded\n        // down -- every serviceWindow this request builds agrees on \"now\".\n        let (now, horizon) = crate::graphql::service_clock::evaluation(ctx);\n        let carts = ctx.data::<std::sync::Arc<dyn application::queries::CartReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let catalogs = ctx.data::<std::sync::Arc<dyn application::queries::CatalogReadRepository>>()?;\n        // The ONE request-scoped correlation id (#451, contract `request.correlation_id`): every\n        // cart.price span of THIS request shares it. Absent = the schema was executed OUTSIDE a\n        // request (no transport, e.g. a direct unit-test execution), and the NIL uuid says exactly\n        // that: a random id would be indistinguishable from a real one in a trace, sending an\n        // operator hunting for a request that never existed. All three real paths -- HTTP POST, the\n        // WS connection_init and the SSR render -- inject one, so this is unreachable in production.\n        let correlation_id = ctx.data_opt::<crate::graphql::session::RequestCorrelationId>().map(|c| c.0).unwrap_or(uuid::Uuid::nil());\n        // Per-instance authorization (#144): the ReadScope was resolved ONCE at the edge from the verified Principal and injected into the context. Absent (schema executed outside a request) => Public, i.e. the session leg only -- fail closed.\n        let scope = ctx.data_opt::<application::queries::ReadScope>().cloned().unwrap_or(application::queries::ReadScope::Public);\n        // The anonymous-session correlator (validated at transport, `session.rs`): leg 2's key.\n        let session = ctx.data_opt::<crate::graphql::session::SessionHeader>().and_then(|s| s.0);\n        // The request's TENANT (#469), resolved ONCE at the edge from the Host -- its own datum\n        // beside the ReadScope. Absent (the marketplace host, or a schema executed outside a\n        // request) => no tenant, and a tenant-scoped read serves NOTHING rather than everything.\n        let tenant = ctx.data_opt::<crate::graphql::tenant::TenantScope>().copied().unwrap_or(crate::graphql::tenant::TenantScope::None);\n        let Some(row) = crate::graphql::cart_read::current_open_cart(&**carts, &scope, session, tenant)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n        else {\n            return Ok(None);\n        };\n        let restaurant = restaurants\n            .by_id(row.restaurant_id)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .ok_or_else(|| async_graphql::Error::new(\"cart references an unknown restaurant\"))?;\n        Ok(Some(crate::graphql::cart_read::priced(&**catalogs, row, Restaurant::at(restaurant, now, horizon), correlation_id).await?))",
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
            "        let (requested_restaurant, requested_status): (Option<domain::generated::scalars::RestaurantId>, Option<domain::generated::scalars::RefundStatus>) = match input {\n            Some(i) => (i.restaurant_id.map(Into::into), i.status.map(Into::into)),\n            None => (None, None),\n        };\n        // BOUND SCOPE INTERSECT REQUESTED FILTER (#618, DECISIONS ss39 IDOR-1). `restaurantId` is a\n        // SELECTOR WITHIN the caller's scope, never a grant: no filter => the caller's own queue,\n        // the caller's own id => the same queue, any OTHER id => EMPTY. Substitution -- serving the\n        // caller its own rows when it asked for another tenant's -- is a DEFECT, not a kindness: the\n        // client asked for R2, got HTTP 200 with R1's rows, no error, and no way to tell.\n        let bound_restaurant = match (&scope, binding_mode) {\n            // OFF -- the incident rollback rung. Today's behaviour restored EXACTLY: the\n            // caller-supplied filter, whatever the caller's scope, including none at all. This is the\n            // one value of the three that re-opens the only hole production can reach today. It is\n            // PROBED, deliberately: an untested rollback rung is a rollback that fails at 20:00 on a\n            // Friday with the operator holding the flag.\n            (_, crate::graphql::read_binding::ReadScopeBindingMode::Off) => requested_restaurant,\n            // The ADMIN arbitrates ACROSS restaurants (stories.yaml, ArbitrateRefunds: \"the\n            // cross-restaurant refund queue\"). Unchanged in every mode -- and unbounded: the view\n            // folds every RefundOpened ever recorded and the query has no LIMIT.\n            (application::queries::ReadScope::Admin | application::queries::ReadScope::System, _) => requested_restaurant,\n            (application::queries::ReadScope::Restaurant(bound), _) => match requested_restaurant {\n                None => Some(*bound),\n                Some(want) if want == *bound => Some(*bound),\n                Some(_) => {\n                    // ONE execution. The comparison is Option<RestaurantId> against RestaurantId, in\n                    // memory, BEFORE the repository is called -- never two reads diffed, which would\n                    // double the most expensive read on the money path at the hour the queue is longest.\n                    telemetry::meters::read_authorization::scope_mismatch(\"pendingRefunds\", \"RESTAURANT\", binding_mode.as_str());\n                    if binding_mode.is_enforcing() {\n                        // The intersection is EMPTY, and the repository is never touched.\n                        return Ok(Vec::new());\n                    }\n                    requested_restaurant\n                }\n            },\n            // No bound scope at all -- an unbound RESTAURANT credential, which is the ONLY RESTAURANT\n            // credential that can exist today. This is not a binding, it is the ABSENCE of one, and the\n            // system has a recorded answer for that: a missing claim fails closed inside read_scope.\n            // NOT gated between OBSERVE and ENFORCE -- only OFF restores it.\n            _ => return Ok(Vec::new()),\n        };\n        let repo = ctx.data::<std::sync::Arc<dyn application::queries::RefundReadRepository>>()?;\n        let filter = application::queries::RefundFilter { restaurant_id: bound_restaurant, status: requested_status };\n        let rows = repo.list(filter).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(Refund::from).collect())",
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
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::ReclamationReadRepository>>()?;\n        let rows = repo.by_customer(customer_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(Reclamation::from).collect())",
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
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::CustomerCreditReadRepository>>()?;\n        let row = repo.by_customer(customer_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(row.map(CustomerCredit::from))",
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

pub(crate) fn emit_server_mutation(model: &Model) -> String {
    // Names that hit the unwired arm below -- checked against UNWIRED_MUTATIONS once the loop ends.
    let mut unwired: Vec<String> = Vec::new();
    let api = parse_api(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/api.yaml — do not edit by hand.\n// The GraphQL MutationRoot, ACCEPTANCE-FIRST (ADR-20260720-015500): one resolver per api.yaml\n// mutation, `(input: <Command>Input!, metadata: MetadataInput) -> MutationAcceptance!`. The resolver\n// ENQUEUES the command on its actor's mailbox lane (`inbound_messages`, THE only journal since #242\n// Runtime D — durable RECEIVED, idempotent by messageId: same payload hash replays the original\n// acceptance, a different one is a Conflict) and answers with the effective envelope + PENDING.\n// The partitioned mailbox worker delivers it and completes the row (SUCCEEDED | REJECTED | FAILED),\n// publishing the transition on the OperationStatusBus; post-acceptance rejections surface as\n// Operation.errorCode, never as GraphQL errors (the sync path — input/metadata validation,\n// duplicate-payload Conflict — still uses them).\n// Each non-public field carries its api.yaml `roles` as a `guard` + `visible` pair (ADR-0006).\n//\n// OBSERVABILITY (issue #191): every resolver emits the `command-acceptance` contract's three spans —\n// `command.receive` (SERVER) -> `command.journal` (INTERNAL) -> `command.dispatch` (INTERNAL) — plus its\n// four metrics. The span field names live in `crates/telemetry`, not here: inlining `info_span!` would\n// copy the contract's attribute list into every one of these resolvers, so a contract change could land\n// in some and not others with nothing to catch it.\n#![allow(unused_variables)]\n#![allow(dead_code)]\n\nuse tracing::Instrument as _;\n\nuse super::acl::*;\nuse super::inputs::*;\nuse super::scalars::*;\nuse super::types::*;\n",
    );
    out.push_str("\npub struct MutationRoot;\n\n#[async_graphql::Object(name = \"Mutation\")]\nimpl MutationRoot {\n");
    let addressing = command_addressing(model);
    for m in &api.mutations {
        let fnname = rust_ident(&snake_field(&m.name));
        let acl = acl_field_attr(model, &m.roles);
        push_doc(&mut out, "    ", m.description.as_deref());
        // THE MAILBOX (#242 Runtime C3/D, PROP-20260728-152752): EVERY command is ENQUEUED on its
        // actor's `inbound_messages` lane and answered PENDING — the partitioned worker delivers
        // it; nothing is spawned in the request path. Process-manager legs (placeOrder,
        // approveRefund, denyRefund) are addressed exactly like aggregate commands since Runtime D
        // retired the gate and the journal+spawn arm behind it.
        if wired_mutation_dispatch(&m.name).is_some() {
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
                out.push_str(&format!(
                    "    #[graphql(name = \"{name}\"{acl})]\n    async fn {fnname}(&self, ctx: &async_graphql::Context<'_>, input: {command}Input, metadata: Option<MetadataInput>) -> async_graphql::Result<MutationAcceptance> {{\n        // command.receive (SERVER). Opened before any fallible work so an input that fails to\n        // deserialize still leaves a span naming the command that was attempted.\n        let __receive = telemetry::spans::command_receive(\n            \"{command}\",\n            crate::graphql::acl::request_role(ctx).api_name(),\n            telemetry::CHANNEL_GRAPHQL,\n        );\n        let __rx = __receive.clone();\n        async move {{\n        let mailbox = ctx.data::<std::sync::Arc<dyn actor_client::mailbox::Mailbox>>()?.clone();\n        let payload_json = command_payload(&input)?;\n        // SYNC input validation (fail fast as a GraphQL error) AND the typed value the actor client\n        // sends (#284 slice 2) -- the mailbox payload is the domain command's own serde form.\n        let cmd: domain::generated::commands::{command} = serde_json::from_value(payload_json.clone())\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        let env = request_envelope(ctx, &metadata);\n        // run_identity: both ids are mandatory in every contract and both may be server-generated.\n        telemetry::spans::record_envelope(&__rx, &env.message_id.to_string(), &env.correlation_id.to_string());\n        {actor_id_expr}\n        // The TYPED DOOR (#284 slice 2, PROP-20260728-152752 §2.1): the generated {actor}Client\n        // assembles the mailbox row through the SAME shared constructors the worker-channel\n        // enqueue uses (lane, partition, kind, payload hash), so the GraphQL door can never\n        // drift from any other door and no resolver builds a mailbox entry inline.\n        let __client = {actor_crate}::{actor}Client::new(mailbox, actor_id);\n        let __envelope = actor_client::mailbox::Envelope {{\n            message_id: env.message_id,\n            correlation_id: env.correlation_id,\n            cause_id: env.cause_id,\n            session_id: env.session_id,\n            trace_id: env.trace_id.clone(),\n            user_id: env.user_id,\n            user_type: env.user_type.clone(),\n            channel: \"GRAPHQL\".into(),\n        }};\n        // command.journal (INTERNAL) — the typed send IS the durable acceptance now; the\n        // span keeps its contract name (the acceptance contract is unchanged, ADR-20260720-015500).\n        let __journal = telemetry::spans::command_journal(&env.message_id.to_string());\n        let __outcome = __client.send(cmd, __envelope).instrument(__journal.clone()).await.map_err(domain_error)?;\n        match __outcome {{\n            actor_client::EnqueueOutcome::PayloadConflict(_) => {{\n                // A reused messageId with a DIFFERENT payload is a client bug, and the only\n                // acceptance outcome the contract does NOT count as success.\n                telemetry::spans::record_journal_status(&__journal, telemetry::journal_status::CONFLICT);\n                telemetry::meters::acceptance::sync_conflict(\"{command}\");\n                return Err(conflict_error(env.message_id));\n            }}\n            actor_client::EnqueueOutcome::Deduplicated(status) => {{\n                telemetry::spans::record_journal_status(&__journal, telemetry::journal_status::DUPLICATE);\n                let _ = telemetry::spans::command_dispatch(\n                    &env.message_id.to_string(),\n                    telemetry::dispatch_outcome::DUPLICATE_SKIPPED,\n                );\n                telemetry::meters::acceptance::duplicate(telemetry::CHANNEL_GRAPHQL);\n                return Ok(acceptance(&env, mailbox_status_api(status), true));\n            }}\n            actor_client::EnqueueOutcome::Enqueued => {{\n                telemetry::spans::record_journal_status(&__journal, telemetry::journal_status::RECEIVED);\n            }}\n        }}\n        // command.dispatch (INTERNAL): ENQUEUED — the mailbox worker owns delivery and completion\n        // (its StatusBusObserver publishes the terminal transition post-commit).\n        let _ = telemetry::spans::command_dispatch(\n            &env.message_id.to_string(),\n            telemetry::dispatch_outcome::ENQUEUED,\n        );\n        telemetry::meters::acceptance::accepted(telemetry::CHANNEL_GRAPHQL);\n        Ok(acceptance(&env, OperationStatus::PENDING, false))\n        }}\n        .instrument(__receive)\n        .await\n    }}\n",
                    name = m.name,
                    acl = acl,
                    fnname = fnname,
                    command = m.command,
                    actor = addr.actor_type,
                    actor_crate = client_crate_ident(&addr.actor_type),
                    actor_id_expr = actor_id_expr
                ));
                continue;
            }
        }
        // UNADDRESSED = UNGENERATABLE (#242 Runtime D). Every mutation's command is addressed to
        // an actor mailbox above; with `command_journal` gone there is no second arm to fall back
        // to, so an api.yaml mutation whose command carries no actors.yaml addressing FAILS
        // GENERATION here instead of silently emitting a resolver that writes nowhere. The only
        // legitimate other outcome is the DECLARED unwired stub below.
        match wired_mutation_dispatch(&m.name) {
            Some(_) => panic!(
                "mutation `{}` (command `{}`) is wired but has no mailbox addressing -- declare \
                 `identity` + `mailbox.partitions` on the actor that receives it in actors.yaml. \
                 The journal+spawn fallback retired with command_journal (#242 Runtime D).",
                m.name, m.command
            ),
            None => {
                unwired.push(m.name.clone());
                out.push_str(&format!(
                "    #[graphql(name = \"{}\"{})]\n    async fn {}(&self, input: {}Input, metadata: Option<MetadataInput>) -> async_graphql::Result<MutationAcceptance> {{\n        Err(async_graphql::Error::new(\"not implemented\"))\n    }}\n",
                m.name, acl, fnname, m.command
                ))
            }
        }
    }
    out.push_str("}\n");
    // A mutation missing from `wired_mutation_dispatch` degrades SILENTLY -- a stub body and no
    // router arm, with every spec gate green. Fail generation instead, so the omission has to be
    // either fixed or declared in UNWIRED_MUTATIONS on purpose.
    let declared: std::collections::BTreeSet<&str> = UNWIRED_MUTATIONS.iter().copied().collect();
    let found: std::collections::BTreeSet<&str> = unwired.iter().map(String::as_str).collect();
    assert_eq!(
        found, declared,
        "api.yaml mutations with no handler in `wired_mutation_dispatch` must be declared in \
         UNWIRED_MUTATIONS. Undeclared (would ship as `Err(\"not implemented\")`): {:?}. Declared but \
         now wired (remove them): {:?}.",
        found.difference(&declared).collect::<Vec<_>>(),
        declared.difference(&found).collect::<Vec<_>>(),
    );
    // Shared write-side plumbing for the wired resolvers.
    out.push_str(
        "\n/// The stripped serde wire shape of the GraphQL input — both the mailbox `payload` column and the\n/// domain command deserialize from it (generated from the same commands.yaml, camelCase). `null`s\n/// are stripped first — an unset GraphQL optional serializes as an explicit null, while the domain\n/// payloads model absence as a MISSING key (`Option` fields / `#[serde(default)]` arrays).\nfn command_payload(input: &impl serde::Serialize) -> async_graphql::Result<serde_json::Value> {\n    let mut value = serde_json::to_value(input).map_err(|e| async_graphql::Error::new(e.to_string()))?;\n    strip_nulls(&mut value);\n    Ok(value)\n}\n\nfn strip_nulls(value: &mut serde_json::Value) {\n    match value {\n        serde_json::Value::Object(map) => {\n            map.retain(|_, v| !v.is_null());\n            for v in map.values_mut() {\n                strip_nulls(v);\n            }\n        }\n        serde_json::Value::Array(items) => {\n            for v in items.iter_mut() {\n                strip_nulls(v);\n            }\n        }\n        _ => {}\n    }\n}\n\n/// `RequestRole` → the scalars.yaml UserType TEXT value (ADR-20260728: enums are stored verbatim).\nfn role_text(role: &crate::graphql::acl::RequestRole) -> &'static str {\n    use crate::graphql::acl::RequestRole as R;\n    match role {\n        R::Public => \"PUBLIC\",\n        R::Customer => \"CUSTOMER\",\n        R::RestaurantAccount => \"RESTAURANT_ACCOUNT\",\n        R::Restaurant => \"RESTAURANT\",\n        R::Rider => \"RIDER\",\n        R::Admin => \"ADMIN\",\n        R::External => \"EXTERNAL\",\n    }\n}\n\n/// The EFFECTIVE technical envelope of one mutation request (ADR-20260720-015500): what the client\n/// supplied via MetadataInput/headers, completed server-side (UUIDv7) and echoed back verbatim in\n/// the MutationAcceptance.\npub(crate) struct RequestEnvelope {\n    pub message_id: uuid::Uuid,\n    pub correlation_id: uuid::Uuid,\n    pub cause_id: Option<uuid::Uuid>,\n    pub session_id: Option<uuid::Uuid>,\n    pub trace_id: Option<String>,\n    pub user_id: Option<uuid::Uuid>,\n    pub user_type: String,\n}\n\nfn request_envelope(ctx: &async_graphql::Context<'_>, metadata: &Option<MetadataInput>) -> RequestEnvelope {\n    let principal = ctx.data_opt::<crate::auth::Principal>();\n    let user_id = principal\n        .and_then(|p| p.user_id())\n        .and_then(|s| uuid::Uuid::parse_str(s).ok());\n    let user_type = principal.map(|p| role_text(&p.role())).unwrap_or(\"PUBLIC\").to_string();\n    let session_id = ctx.data_opt::<crate::graphql::session::SessionHeader>().and_then(|s| s.0);\n    let trace_id = ctx.data_opt::<crate::graphql::session::TraceContext>().and_then(|t| t.0.clone());\n    // Client-suppliable ids validate structurally at scalar parse time; anything missing is\n    // server-generated (time-ordered UUIDv7) and the correlation defaults to the messageId.\n    let message_id = metadata\n        .as_ref()\n        .and_then(|m| m.message_id.as_ref())\n        .map(|v| v.0)\n        .unwrap_or_else(uuid::Uuid::now_v7);\n    let correlation_id = metadata\n        .as_ref()\n        .and_then(|m| m.correlation_id.as_ref())\n        .map(|v| v.0)\n        .unwrap_or(message_id);\n    let cause_id = metadata.as_ref().and_then(|m| m.cause_id.as_ref()).map(|v| v.0);\n    RequestEnvelope { message_id, correlation_id, cause_id, session_id, trace_id, user_id, user_type }\n}\n\n/// The uniform acceptance payload from the effective envelope.\nfn acceptance(env: &RequestEnvelope, status: OperationStatus, duplicate: bool) -> MutationAcceptance {\n    MutationAcceptance {\n        message_id: MessageId(env.message_id),\n        correlation_id: CorrelationId(env.correlation_id),\n        cause_id: env.cause_id.map(CauseId),\n        session_id: env.session_id.map(SessionId),\n        trace_id: env.trace_id.clone().map(TraceId),\n        operation_status: status,\n        duplicate,\n    }\n}\n\n\n/// `inbound_messages` lifecycle → the caller-facing OperationStatus (the ONE journal, #242):\n/// RECEIVED/SCHEDULED read as PENDING; IGNORED/DUPLICATE are the aggregate's no-change/redelivery\n/// verdicts — success from the caller's seat; CANCELLED can only be a withdrawn reminder — a\n/// command never reaches it, mapped FAILED defensively.\npub(crate) fn mailbox_status_api(s: domain::generated::scalars::InboundMessageStatus) -> OperationStatus {\n    use domain::generated::scalars::InboundMessageStatus as M;\n    match s {\n        M::RECEIVED | M::SCHEDULED => OperationStatus::PENDING,\n        M::SUCCEEDED | M::IGNORED | M::DUPLICATE => OperationStatus::SUCCEEDED,\n        M::REJECTED => OperationStatus::REJECTED,\n        M::FAILED | M::CANCELLED => OperationStatus::FAILED,\n    }\n}\n\n/// A mailbox status row → the API Operation shape (`operationStatus` / `operationStatusChanged`).\npub(crate) fn operation_from_mailbox(row: &actor_client::mailbox::MailboxStatusRow) -> Operation {\n    let error_code = row\n        .error\n        .as_ref()\n        .and_then(|e| e.get(\"code\"))\n        .and_then(|c| c.as_str())\n        .map(str::to_owned);\n    let message = match (&error_code, row.error.as_ref().and_then(|e| e.get(\"context\"))) {\n        (Some(code), Some(context)) => domain::generated::errors::message_en(code, context),\n        _ => None,\n    };\n    Operation {\n        message_id: MessageId(row.message_id),\n        correlation_id: CorrelationId(row.correlation_id),\n        status: mailbox_status_api(row.status),\n        error_code,\n        message,\n        occurred_at: row.completed_at.unwrap_or(row.received_at),\n    }\n}\n\n/// The mailbox row's ownership scope (ADR-20260720-015500): ADMIN, the accepting actor (JWT\n/// subject), or the accepting session (X-SESSION-ID). Callers resolve null / an empty stream on\n/// false — the PUBLIC surface must not become an existence oracle.\npub(crate) fn mailbox_operation_owned(\n    ctx: &async_graphql::Context<'_>,\n    row: &actor_client::mailbox::MailboxStatusRow,\n) -> bool {\n    let admin = matches!(\n        ctx.data_opt::<crate::graphql::acl::RequestRole>(),\n        Some(crate::graphql::acl::RequestRole::Admin)\n    );\n    let principal_uuid = ctx\n        .data_opt::<crate::auth::Principal>()\n        .and_then(|p| p.user_id())\n        .and_then(|s| uuid::Uuid::parse_str(s).ok());\n    let session = ctx.data_opt::<crate::graphql::session::SessionHeader>().and_then(|s| s.0);\n    admin\n        || (principal_uuid.is_some() && principal_uuid == row.user_id)\n        || (session.is_some() && session == row.session_id)\n}\n\n/// The synchronous Conflict for a replayed messageId whose payload differs — a client bug, not a\n/// retry (ADR-20260720-015300); errors.yaml cross-cutting `Conflict`, P-10 extensions shape.\nfn conflict_error(message_id: uuid::Uuid) -> async_graphql::Error {\n    use async_graphql::ErrorExtensions;\n    let def = domain::generated::errors::CONFLICT;\n    async_graphql::Error::new(format!(\n        \"messageId {message_id} was already used with a different payload\"\n    ))\n    .extend_with(|_, ext| ext.set(\"code\", def.code))\n}\n\n/// Map a SYNCHRONOUS failure (mailbox enqueue, input deserialization) onto the GraphQL error\n/// contract (P-10): an anticipated errors.yaml rejection surfaces `extensions.code` = the stable\n/// PascalCase code, the interpolated English message as the error message, and its typed context\n/// fields under the extensions; anything unexpected (repository/adapter failures) surfaces as the\n/// generic catalogued `Internal` — never leaking adapter details to the client.\nfn domain_error(e: domain::shared::errors::DomainError) -> async_graphql::Error {\n",
    );
    out.push_str(
        "    use async_graphql::ErrorExtensions;\n    use domain::shared::errors::DomainError;\n    match e {\n        DomainError::Rejected { code, context } => {\n            let message = domain::generated::errors::message_en(&code, &context)\n                .unwrap_or_else(|| code.clone());\n            async_graphql::Error::new(message).extend_with(|_, ext| {\n                ext.set(\"code\", code.as_str());\n                if let Some(fields) = context.as_object() {\n                    for (key, value) in fields {\n                        if key == \"code\" {\n                            continue; // never let a context field shadow the wire code\n                        }\n                        ext.set(\n                            key.as_str(),\n                            async_graphql::Value::from_json(value.clone())\n                                .unwrap_or(async_graphql::Value::Null),\n                        );\n                    }\n                }\n            })\n        }\n        // Legacy \"<Code>: <detail>\" string invariants (interim adapters, e.g. the fail-closed\n        // payment stand-in): surface the prefix when it is a catalogued code, else it is unexpected.\n        DomainError::Invariant(msg) => {\n            let code = msg.split(':').next().map(str::trim).unwrap_or(\"\").to_string();\n            if domain::generated::errors::find(&code).is_some() {\n                async_graphql::Error::new(msg).extend_with(|_, ext| ext.set(\"code\", code.as_str()))\n            } else {\n                internal_error()\n            }\n        }\n        DomainError::Repository(_) => internal_error(),\n    }\n}\n\n/// The generic catalogued `Internal` fallback (errors.yaml): unexpected/infrastructure failures\n/// never leak their detail to the client.\nfn internal_error() -> async_graphql::Error {\n    use async_graphql::ErrorExtensions;\n    let def = domain::generated::errors::INTERNAL;\n    async_graphql::Error::new(def.message_en).extend_with(|_, ext| ext.set(\"code\", def.code))\n}\n",
    );
    out
}

/// The HANDLER CALL for mutations whose command handler is wired: the awaited expression the
/// generated `command_router` runs, normalized to `Result<(), DomainError>` (business return
/// values are discarded — acceptance-first results are reads, ADR-20260720-015500). The local
/// names it binds (`store`, `catalogs`, `mailbox_requeue`, `env`, …) are the ones the router's
/// prelude clones out of `CommandDeps`. `None` → the `not implemented` stub.
///
/// Until #510 this returned a second, FIRST tuple element — per-port `ctx.data` resolution
/// strings for a resolver-side spawn that retired with #242 Runtime D (mutations only ENQUEUE
/// now); the router at the sole call site discarded it. Deleted rather than kept: a dead
/// `ctx.data` path is exactly where a stale port type hides (the #529 class).
/// Mutations DELIBERATELY emitted without a handler, i.e. whose resolver body is
/// `Err("not implemented")` and which get no `command_router` arm.
///
/// This list exists because the failure mode is SILENT: [`wired_mutation_dispatch`] returns `None`
/// for any mutation missing from its table, and the emitter then writes a stub while `make validate`
/// stays green -- the api.yaml mutation is declared, story-covered and role-guarded, and simply does
/// nothing. `phoneCountries` was the query-side twin of this and was deleted for it;
/// `recordDeliverySatisfaction` and `escalateDelivery` sat here with their handlers ALREADY WRITTEN,
/// missing only a table row.
///
/// Empty is the correct state. Adding a mutation to api.yaml without wiring it now FAILS generation
/// (see the assertion in `mutations_rs`) instead of degrading quietly; declaring it here is the
/// explicit, reviewable way to say "deliberately not yet".
pub(crate) const UNWIRED_MUTATIONS: &[&str] = &[];

pub(crate) fn wired_mutation_dispatch(name: &str) -> Option<String> {
    // verifyPhone resolves through the wrapped auth ACL (ADR-0015): its VerifyPhoneOutcome
    // (customerId/created) is no longer returned — the client reads `me` once the operation
    // SUCCEEDs.
    if name == "verifyPhone" {
        return Some(
            "application::commands::verify_phone(store.as_ref(), auth.as_ref(), customers.as_ref(), sessions.as_ref(), cmd, &actor).await.map(|_| ())".into(),
        );
    }
    // placeOrder needs the CatalogReadRepository (server-side line pricing from the live catalog —
    // the only price authority, rules.yaml#/ServerPriceAuthority) + the generated PaymentService
    // port (services.yaml `payment.request`, issue #26) + PaymentProcessStateStore (the
    // payment_process_manager row it opens and single-flights
    // on, ADR-20260719-193500). Its created intent is no longer returned — the checkout reads
    // queries/paymentStatus (+ paymentStatusChanged) off the run row. The saga's event legs
    // (PaymentCaptured/PaymentFailed) run in the infrastructure ProcessManagerRunner, not here.
    if name == "placeOrder" {
        return Some(
            // env.session_id: the anonymous session scope for the paymentStatus read survives an
            // app restart (#12); the router's prelude binds `env` from the mailbox row. The RSO-1
            // pair rides the same prelude: the evaluation instant is read ONCE here at the
            // delivery seam, and the enforcement gate (`enforce_service_hours_guard`) comes off
            // `CommandDeps` — resolved at the composition root, never a global read in the
            // handler.
            "application::commands::place_order(store.as_ref(), catalogs.as_ref(), payments.as_ref(), pm_state.as_ref(), cmd, env.session_id.map(domain::generated::scalars::SessionId), &actor, chrono::Utc::now(), enforce_service_hours_guard).await.map(|_| ())".into(),
        );
    }
    // The refund DECISION legs run on the RefundProcess orchestrator (application::process_managers::
    // refund), not an aggregate command handler: they need the RefundProcessStateStore (the pending
    // refund_process_manager row they decide on) and — for the approval — the generated
    // PaymentService port (services.yaml `payment.refund`) that requests the Stripe refund
    // (fail closed).
    if name == "approveRefund" {
        return Some(
            "application::process_managers::refund::approve_refund(store.as_ref(), refund_state.as_ref(), payments.as_ref(), cmd, &actor).await.map(|_| ())".into(),
        );
    }
    if name == "denyRefund" {
        return Some(
            "application::process_managers::refund::deny_refund(store.as_ref(), refund_state.as_ref(), cmd, &actor).await.map(|_| ())".into(),
        );
    }
    // (domain command, application::commands handler, extra port beyond the EventStore).
    enum Extra {
        None,
        /// `RestaurantReadRepository` — backs the SlugAlreadyTaken/RestaurantNotFound and
        /// CurrencyMismatch (default currency) checks.
        Restaurants,
        /// `SlugReservationRepository` -- the write-side arbiter of storefront-slug uniqueness
        /// (ADR-20260728-011344 D3). Deliberately NOT a read repository.
        SlugReservations,
        /// `GoogleOwnershipVerifier` — GBP ownership proof (ADR-0019).
        Ownership,
        /// `GbpOrderLinkProbe` — GBP 'Order online' link ping (ADR-0021).
        Probe,
        /// `ProspectionReadRepository` — backs the ProspectContactedTooRecently check (ADR-0020).
        Prospection,
        /// `CatalogReadRepository` — the offer-level live-catalog lookups behind the Cart line
        /// invariants (OfferNotFound / OfferUnavailable / InsufficientStock / InvalidOptionSelection).
        Catalogs,
        /// `AuthProviderGateway` — the wrapped Supabase Auth ACL boundary (ADR-0015).
        Auth,
        /// `AuthProviderGateway` + `CustomerReadRepository` — identity flows that also need the
        /// phone/email uniqueness-and-resolution lookups.
        AuthCustomers,
        /// `MailboxRequeue` — the write port that consults/flips the poisoned inbound_messages
        /// row itself (#315). Deliberately NOT a read repository: the flip is the arbiter.
        MailboxRequeue,
    }
    let (command, handler, extra) = match name {
        "registerRestaurant" => ("RegisterRestaurant", "register_restaurant", Extra::Restaurants),
        "configureRestaurantSlug" => {
            ("ConfigureRestaurantSlug", "configure_restaurant_slug", Extra::SlugReservations)
        }
        "activateRestaurant" => ("ActivateRestaurant", "activate_restaurant", Extra::None),
        "updateRestaurant" => ("UpdateRestaurant", "update_restaurant", Extra::None),
        "deactivateRestaurant" => ("DeactivateRestaurant", "deactivate_restaurant", Extra::None),
        "removeRestaurant" => ("RemoveRestaurant", "remove_restaurant", Extra::None),
        "changeOrderAcceptanceMode" => {
            ("ChangeOrderAcceptanceMode", "change_order_acceptance_mode", Extra::None)
        }
        "updateRestaurantGoogleBusinessProfile" => (
            "UpdateRestaurantGoogleBusinessProfile",
            "update_restaurant_google_business_profile",
            Extra::None,
        ),
        "markRestaurantClosed" => ("MarkRestaurantClosed", "mark_restaurant_closed", Extra::None),
        "claimRestaurantListing" => {
            ("ClaimRestaurantListing", "claim_restaurant_listing", Extra::Ownership)
        }
        "optOutRestaurantListing" => {
            ("OptOutRestaurantListing", "opt_out_restaurant_listing", Extra::Ownership)
        }
        "changeRestaurantListingStatus" => {
            ("ChangeRestaurantListingStatus", "change_restaurant_listing_status", Extra::None)
        }
        "configureGbpOrderLink" => {
            ("ConfigureGoogleBusinessProfileOrderLink", "configure_gbp_order_link", Extra::None)
        }
        "verifyGbpOrderLink" => {
            ("VerifyGoogleBusinessProfileOrderLink", "verify_gbp_order_link", Extra::Probe)
        }
        // Cart aggregate (ADR-0046 round 2: the checkout→order→delivery flow). Line-level commands
        // validate against the LIVE catalog (offer-level read port over the projected tree).
        "addCartLine" => ("AddCartLine", "add_cart_line", Extra::Catalogs),
        "removeCartLine" => ("RemoveCartLine", "remove_cart_line", Extra::None),
        "changeCartLineQuantity" => {
            ("ChangeCartLineQuantity", "change_cart_line_quantity", Extra::Catalogs)
        }
        // Order aggregate.
        "acceptOrder" => ("AcceptOrder", "accept_order", Extra::None),
        "rejectOrder" => ("RejectOrder", "reject_order", Extra::None),
        "startPreparation" => ("StartPreparation", "start_preparation", Extra::None),
        "markOrderReady" => ("MarkOrderReady", "mark_order_ready", Extra::None),
        "markOrderDelivered" => ("MarkOrderDelivered", "mark_order_delivered", Extra::None),
        "cancelOrderByCustomer" => ("CancelOrderByCustomer", "cancel_order_by_customer", Extra::None),
        "cancelOrderByRestaurant" => {
            ("CancelOrderByRestaurant", "cancel_order_by_restaurant", Extra::None)
        }
        "rateOrder" => ("RateOrder", "rate_order", Extra::None),
        "rateRestaurant" => ("RateRestaurant", "rate_restaurant", Extra::None),
        "tipOrder" => ("TipOrder", "tip_order", Extra::None),
        "requestRefund" => ("RequestRefund", "request_refund", Extra::None),
        // DeliveryJob aggregate (independent-rider fulfilment, ADR-0031).
        "acceptDelivery" => ("AcceptDelivery", "accept_delivery", Extra::None),
        "confirmPickup" => ("ConfirmPickup", "confirm_pickup", Extra::None),
        "completeDelivery" => ("CompleteDelivery", "complete_delivery", Extra::None),
        "cancelDelivery" => ("CancelDelivery", "cancel_delivery", Extra::None),
        // DeliveryPartnerRegistration aggregate (self-registration, #61).
        "registerDeliveryPartnerAvailability" => {
            ("RegisterDeliveryPartnerAvailability", "register_delivery_partner_availability", Extra::None)
        }
        "approveDeliveryPartnerAvailability" => {
            ("ApproveDeliveryPartnerAvailability", "approve_delivery_partner_availability", Extra::None)
        }
        "revokeDeliveryPartnerAvailability" => {
            ("RevokeDeliveryPartnerAvailability", "revoke_delivery_partner_availability", Extra::None)
        }
        // placeOrder is handled by the bespoke body above (Stripe-assigned payload fields).
        // RestaurantAccount aggregate.
        "registerRestaurantAccount" => {
            ("RegisterRestaurantAccount", "register_restaurant_account", Extra::None)
        }
        "updateRestaurantAccount" => {
            ("UpdateRestaurantAccount", "update_restaurant_account", Extra::None)
        }
        "deleteRestaurantAccount" => {
            ("DeleteRestaurantAccount", "delete_restaurant_account", Extra::None)
        }
        // Prospect aggregate (ADR-0020).
        "recordProspectContact" => {
            ("RecordProspectContact", "record_prospect_contact", Extra::Prospection)
        }
        "markProspectCold" => ("MarkProspectCold", "mark_prospect_cold", Extra::None),
        "recordProspectReply" => ("RecordProspectReply", "record_prospect_reply", Extra::None),
        // Catalog aggregate.
        "createCatalog" => ("CreateCatalog", "create_catalog", Extra::Restaurants),
        // The per-restaurant slug uniqueness (CatalogSlugAlreadyTaken) is a read-model check, not a
        // Postgres reservation: a catalog slug is a path inside one storefront, not a global host.
        "configureCatalogSlug" => ("ConfigureCatalogSlug", "configure_catalog_slug", Extra::Catalogs),
        "addProduct" => ("AddProduct", "add_product", Extra::Restaurants),
        "updateProduct" => ("UpdateProduct", "update_product", Extra::Restaurants),
        "removeProduct" => ("RemoveProduct", "remove_product", Extra::None),
        "addCatalogCategory" => ("AddCatalogCategory", "add_catalog_category", Extra::None),
        "updateCatalogCategory" => ("UpdateCatalogCategory", "update_catalog_category", Extra::None),
        "removeCatalogCategory" => ("RemoveCatalogCategory", "remove_catalog_category", Extra::None),
        "addOptionList" => ("AddOptionList", "add_option_list", Extra::None),
        "updateOptionList" => ("UpdateOptionList", "update_option_list", Extra::None),
        "removeOptionList" => ("RemoveOptionList", "remove_option_list", Extra::None),
        "updateOfferStock" => ("UpdateOfferStock", "update_offer_stock", Extra::None),
        "importCatalog" => ("ImportCatalog", "import_catalog", Extra::None),
        // Customer aggregate (wrapped Supabase Auth, ADR-0015).
        "requestPhoneVerification" => {
            ("RequestPhoneVerification", "request_phone_verification", Extra::Auth)
        }
        "requestEmailVerification" => {
            ("RequestEmailVerification", "request_email_verification", Extra::AuthCustomers)
        }
        "confirmEmailVerification" => {
            ("ConfirmEmailVerification", "confirm_email_verification", Extra::Auth)
        }
        "requestPhoneChange" => ("RequestPhoneChange", "request_phone_change", Extra::AuthCustomers),
        "confirmPhoneChange" => ("ConfirmPhoneChange", "confirm_phone_change", Extra::AuthCustomers),
        "changeLanguage" => ("ChangeLanguage", "change_language", Extra::None),
        "changeRiderStatus" => ("ChangeRiderStatus", "change_rider_status", Extra::None),
        "markRestaurantAsFavorite" => {
            ("MarkRestaurantAsFavorite", "mark_restaurant_as_favorite", Extra::Restaurants)
        }
        "unmarkRestaurantAsFavorite" => {
            ("UnmarkRestaurantAsFavorite", "unmark_restaurant_as_favorite", Extra::None)
        }
        "updateCustomerInfo" => ("UpdateCustomerInfo", "update_customer_info", Extra::None),
        "setCustomerPreferences" => ("SetCustomerPreferences", "set_customer_preferences", Extra::None),
        "setCustomerAddress" => ("SetCustomerAddress", "set_customer_address", Extra::None),
        "removeCustomerAddress" => ("RemoveCustomerAddress", "remove_customer_address", Extra::None),
        "setCustomerPaymentMethod" => {
            ("SetCustomerPaymentMethod", "set_customer_payment_method", Extra::None)
        }
        // Order conversations / messaging (#129) — the Conversation aggregate handlers
        // (crates/application/src/commands.rs); all plain (&dyn EventStore, cmd, &Actor) → Extra::None.
        "openConversation" => ("OpenConversation", "open_conversation", Extra::None),
        "postMessage" => ("PostMessage", "post_message", Extra::None),
        "recordMessageTranslation" => {
            ("RecordMessageTranslation", "record_message_translation", Extra::None)
        }
        "escalateToAdmin" => ("EscalateToAdmin", "escalate_to_admin", Extra::None),
        "muteParticipant" => ("MuteParticipant", "mute_participant", Extra::None),
        "unmuteParticipant" => ("UnmuteParticipant", "unmute_participant", Extra::None),
        // Reclamations / customer claims (#151) — the Reclamation aggregate handlers
        // (crates/application/src/commands.rs); all plain (&dyn EventStore, cmd, &Actor) → Extra::None.
        "openReclamation" => ("OpenReclamation", "open_reclamation", Extra::None),
        "resolveReclamation" => ("ResolveReclamation", "resolve_reclamation", Extra::None),
        "rejectReclamation" => ("RejectReclamation", "reject_reclamation", Extra::None),
        "reopenReclamation" => ("ReopenReclamation", "reopen_reclamation", Extra::None),
        "attachReclamationEvidence" => ("AttachReclamationEvidence", "attach_reclamation_evidence", Extra::None),
        // Mailbox supervision (#315): the ADMIN requeue of a cap-poisoned row — the handler
        // consults/flips the target row through the MailboxRequeue port and records the
        // intervention on the MailboxSupervision stream.
        "requeueMailboxMessage" => {
            ("RequeueMailboxMessage", "requeue_mailbox_message", Extra::MailboxRequeue)
        }
        // Both handlers existed in application::commands all along -- these two were missing from
        // THIS table only, which silently degraded them to `Err("not implemented")` resolver bodies
        // with no router arm, while `make validate` stayed green. See UNWIRED_MUTATIONS below.
        "recordDeliverySatisfaction" => {
            ("RecordDeliverySatisfaction", "record_delivery_satisfaction", Extra::None)
        }
        "escalateDelivery" => ("EscalateDelivery", "escalate_delivery", Extra::None),
        _ => return None,
    };
    // The extra argument beyond the EventStore — the router's prelude binds each of these local
    // names out of `CommandDeps`, so a name here IS a field there (one routing truth).
    let extra_arg = match extra {
        Extra::None => "",
        Extra::Restaurants => ", restaurants.as_ref()",
        Extra::SlugReservations => ", slugs.as_ref()",
        Extra::Ownership => ", ownership.as_ref()",
        Extra::Probe => ", probe.as_ref()",
        Extra::Prospection => ", prospection.as_ref()",
        Extra::Catalogs => ", catalogs.as_ref()",
        Extra::Auth => ", auth.as_ref()",
        Extra::AuthCustomers => ", auth.as_ref(), customers.as_ref()",
        Extra::MailboxRequeue => ", mailbox_requeue.as_ref()",
    };
    Some(format!(
        "application::commands::{handler}(store.as_ref(){extra_arg}, cmd, &actor).await.map(|_| ())"
    ))
}

/// Emit `crates/infrastructure/src/generated/command_router.rs` — the mailbox worker's
/// command-type → handler dispatch (#242 Runtime C, PROP-20260728-152752 §3.5). ONE routing truth:
/// the arms are derived from the SAME [`wired_mutation_dispatch`] table the GraphQL resolvers are
/// generated from, so a command wired there is automatically routable here and the two can never
/// disagree on ports or handler. The handler-call snippets are reused VERBATIM — the router binds
/// the same local names (`store`, `catalogs`, `env`, …) out of its `CommandDeps` bundle that the
/// resolver template binds out of `ctx.data`.
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

pub(crate) fn emit_infra_command_router(model: &Model) -> String {
    let api = parse_api(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/api.yaml + the wired-mutation dispatch table\n// — do not edit by hand. The mailbox worker's command router (#242 Runtime C): message_type →\n// application command handler, over the same ports the GraphQL resolvers inject. `None` = a command\n// type this build cannot route (unknown, or its handler is not wired yet) — the caller records the\n// row FAILED rather than crashing the lane.\n#![allow(unused_variables)]\n#![allow(clippy::redundant_clone)]\n\nuse std::sync::Arc;\n\nuse domain::shared::errors::DomainError;\n\n/// Every port any wired command handler needs — the worker-side counterpart of the resolvers'\n/// ctx.data injections, bundled once at the composition root.\n#[derive(Clone)]\npub struct CommandDeps {\n    pub store: Arc<dyn application::ports::EventStore>,\n    pub restaurants: Arc<dyn application::queries::RestaurantReadRepository>,\n    pub slugs: Arc<dyn application::queries::SlugReservationRepository>,\n    pub ownership: Arc<dyn application::ports::GoogleOwnershipVerifier>,\n    pub probe: Arc<dyn application::ports::GbpOrderLinkProbe>,\n    pub prospection: Arc<dyn application::queries::ProspectionReadRepository>,\n    pub catalogs: Arc<dyn application::queries::CatalogReadRepository>,\n    pub auth: Arc<dyn application::generated::services::IdentityService>,\n    pub customers: Arc<dyn application::queries::CustomerReadRepository>,\n    pub sessions: Arc<dyn application::auth_sessions::AuthSessionStore>,\n    pub payments: Arc<dyn application::generated::services::PaymentService>,\n    pub pm_state: Arc<dyn application::pm_state::PaymentProcessStateStore>,\n    pub refund_state: Arc<dyn application::pm_state::RefundProcessStateStore>,\n    pub mailbox_requeue: Arc<dyn application::queries::MailboxRequeue>,\n    /// RSO-1 (`configuration.yaml#/ENFORCE_SERVICE_HOURS_GUARD`): the PlaceOrder service-hours\n    /// enforcement gate, resolved ONCE at the composition root -- the handler takes it as a\n    /// parameter (the `when_at` style), never reads config/env itself.\n    pub enforce_service_hours_guard: bool,\n    /// #167 (`configuration.yaml#/ENFORCE_ACCEPTANCE_TIMEOUT`): the acceptance-timeout ACTION\n    /// gate, read at DELIVERY time by the kind-MESSAGE OrderAcceptanceTimedOut route -- same\n    /// composition-root resolution as `enforce_service_hours_guard`. OFF (the default) is SHADOW\n    /// MODE: the full still-PLACED guard runs, only the append is inert.\n    pub enforce_acceptance_timeout: bool,\n    /// #588 (`configuration.yaml#/ROUTE_ORDER_BIRTH_THROUGH_LANE`, ADR-20260816-040239): route\n    /// the saga's `deliver: OrderPlaced to: Order` through the Order's own mailbox lane instead of\n    /// appending to its stream from the saga. Read at DELIVERY time by the PM-fact route, which\n    /// hands the saga a lane sink only when this is ON -- OFF (the default) leaves the legacy\n    /// foreign-stream append untouched, so rollback is a config flip, never a redeploy.\n    pub route_order_birth_through_lane: bool,\n}\n\n/// The slice of the request envelope some handler calls read (placeOrder's session scope).\nstruct RouterEnv {\n    session_id: Option<uuid::Uuid>,\n}\n\n/// The `command.validate` handler-boundary wrapper (specs/observability.yaml `place-order`,\n/// RSO-1): c4-l3.yaml marks `command-handlers` `instrumented: false`, so the span lives HERE at\n/// the dispatch seam, never in the aggregate -- it instruments the handler call and records\n/// `business.validation_status` from the ACTUAL outcome. A Repository error is transient\n/// infrastructure (the delivery aborts and retries), so it records NO validation verdict --\n/// classifying it `rejected` would count a DB blip as a business refusal.\nasync fn validated<F>(handler: F) -> Result<(), DomainError>\nwhere\n    F: std::future::Future<Output = Result<(), DomainError>>,\n{\n    use tracing::Instrument as _;\n    let span = telemetry::spans::command_validate();\n    let result = handler.instrument(span.clone()).await;\n    match &result {\n        Ok(()) => telemetry::spans::record_validation_status(&span, \"accepted\"),\n        Err(DomainError::Repository(_)) => {}\n        Err(_) => telemetry::spans::record_validation_status(&span, \"rejected\"),\n    }\n    result\n}\n\n/// Route one mailbox COMMAND row to its handler. `None` = unroutable command type.\npub async fn dispatch_command(\n    deps: &CommandDeps,\n    command_type: &str,\n    payload: &serde_json::Value,\n    actor_ref: &application::ports::Actor,\n    session_id: Option<uuid::Uuid>,\n) -> Option<Result<(), DomainError>> {\n    let store = deps.store.clone();\n    let restaurants = deps.restaurants.clone();\n    let slugs = deps.slugs.clone();\n    let ownership = deps.ownership.clone();\n    let probe = deps.probe.clone();\n    let prospection = deps.prospection.clone();\n    let catalogs = deps.catalogs.clone();\n    let auth = deps.auth.clone();\n    let customers = deps.customers.clone();\n    let sessions = deps.sessions.clone();\n    let payments = deps.payments.clone();\n    let pm_state = deps.pm_state.clone();\n    let refund_state = deps.refund_state.clone();\n    let mailbox_requeue = deps.mailbox_requeue.clone();\n    let enforce_service_hours_guard = deps.enforce_service_hours_guard;\n    let env = RouterEnv { session_id };\n    let actor = actor_ref.clone();\n    match command_type {\n",
    );
    let mut seen: HashSet<String> = HashSet::new();
    for m in &api.mutations {
        let Some(handler_call) = wired_mutation_dispatch(&m.name) else { continue };
        if !seen.insert(m.command.clone()) {
            continue;
        }
        // Every arm runs through the `command.validate` wrapper: the span is constructed at the
        // dispatch seam and `validation_status` recorded from the outcome, for the WHOLE command
        // surface — the aggregate handlers themselves stay SDK-free (c4-l3 `instrumented: false`).
        out.push_str(&format!(
            "        \"{command}\" => {{\n            let cmd: domain::generated::commands::{command} = match serde_json::from_value(payload.clone()) {{\n                Ok(c) => c,\n                Err(e) => return Some(Err(DomainError::Repository(format!(\"{command} payload: {{e}}\")))),\n            }};\n            Some(validated(async {{ {call} }}).await)\n        }}\n",
            command = m.command,
            call = handler_call
        ));
    }
    out.push_str("        _ => None,\n    }\n}\n");
    // The runtime addressing surface (ADR-20260731-122500 "the mailbox is the only door") moved
    // to the actor_client boundary crate with #290 phase 1 (PROP-20260802-130500 D1) — the entry
    // constructors that consume it live there now. Re-exported here so every worker-side consumer
    // (composition root, standalone workers) keeps one import path and there is exactly ONE
    // definition of the frozen addressing contract.
    out.push_str(
        "\n// The frozen command-addressing tables (mailbox_address, ACTOR_MAILBOXES) live in the\n// actor_client boundary crate since #290 phase 1 — re-exported for the worker-side consumers.\npub use actor_client::generated::addresses::{mailbox_address, ACTOR_MAILBOXES};\n",
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
        let acl = acl_field_attr(model, &s.roles);
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
            ctx.data_opt::<crate::graphql::acl::RequestRole>(),
            Some(crate::graphql::acl::RequestRole::Admin)
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
                yield Ok(super::mutation::operation_from_mailbox(&row));
                if terminal {
                    return;
                }
            }
            loop {
                match watch.next().await {
                    Some(actor_client::OperationWatchEvent::Update(update)) => {
                        let terminal = !matches!(update.status, M::RECEIVED | M::SCHEDULED);
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
                            yield Ok(super::mutation::operation_from_mailbox(&row));
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
            ctx.data_opt::<crate::graphql::acl::RequestRole>(),
            Some(crate::graphql::acl::RequestRole::Admin)
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
