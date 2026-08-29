// Wired into `validate()` full-strength by #717 (no skip list, no warning-only): the run against
// the real corpus that had kept it un-wired (21 dead bindings across 5 screens — the estimate was
// "13") was paid down in the same PR, corpus first, wiring last, every commit green.

use crate::*;

// ─── §25 — screen `{{ … }}` template bindings resolve against the bound api type (#468, #529 model) ─
//
// beck's finding that motivated this rule: NOTHING in the validator resolved a screen's `{{ root.path }}`
// text binding against the api type `root` actually names — every existing screens check proves an
// action/resolver `$ref` resolves (`resolver-not-a-query`, `action-not-a-mutation`, …), never a content
// interpolation. A screen could therefore bind six non-existent `Cart` fields (`cart.subtotal`,
// `cart.deliveryFee`, `cart.serviceFee`, `cart.total`, `cart.discount`, `cart.minimumOrderMet` — none of
// which the `Cart` api type carries) and `make validate` stayed at 0 errors while the rendered cart
// showed `undefined` (#468).
//
// HONEST SCOPE (never guess-match names, per the dispatch): a screen never declares "the type of
// `cart`" directly. What it declares is `data_requirements: [cart.current, …]`, each name a RESOLVER
// binding whose `query.$ref` names an `api.yaml` query with a `returns: { $ref: '#/types/T' }`. The
// RUNTIME then stores that query's result under BOTH the resolver name's first dotted segment and,
// when the second segment is a plain lowercase/underscore word, the reversed `second_first` alias —
// `crates/web/src/renderer.rs#RenderContext::insert_resolved` is the ONE place that decides this, so
// `screen_binding_roots` below mirrors it exactly rather than inventing a second naming scheme. A
// screen's OTHER `{{ }}` roots (loop variables from `item_template`, plain UI/form state like
// `selected_delivery_mode` or `promo_field.value`) are NOT data-requirement-derived and are simply not
// in the roots map — those paths are left UNCHECKED, not flagged, which is the declared boundary this
// rule's docstring on `check_screen_bindings` states again at the call site.
//
// Path resolution walks `properties` on the root's api type, following `$ref` into `entities.yaml` /
// `scalars.yaml` / another `api.yaml` type exactly as those files actually nest (api.yaml types live
// under `types:`; entities/scalars are flat top-level maps) — never a hand-rolled parallel schema. A
// property with `array: true` stops the walk (a further segment past an array is a per-item field the
// item's own loop variable would carry, not this root, and guessing `.length` semantics is exactly the
// kind of name-matching this rule refuses to do); a property with no `$ref` (a plain scalar) likewise
// stops the walk. Only a segment that names NO property at all on the current type is an error.

/// Every root name a screen's `data_requirements` make available to `{{ }}` bindings, mapped to
/// `(api type, api.yaml query name)` — the type the walk resolves against, and the query whose
/// generated client selection must FETCH what the walk approves (the emitter's consumer; #717
/// round 1: validating a nav path the client never selects renders empty with the gate green).
/// Mirrors `RenderContext::insert_resolved`. `gap` resolvers and resolvers whose query cannot be
/// resolved contribute no root — they are legitimately outside the api's typed surface, not a
/// defect this rule reports on.
pub(crate) fn screen_binding_roots(
    model: &Model,
    resolvers: Option<&serde_yaml::Mapping>,
    data_requirements: &[String],
) -> BTreeMap<String, (String, String)> {
    let mut roots = BTreeMap::new();
    let Some(resolvers) = resolvers else { return roots };
    for dr in data_requirements {
        let Some(r) = resolvers.get(Value::String(dr.clone())) else { continue };
        if r.get("gap").map(|v| !v.is_null()).unwrap_or(false) {
            continue;
        }
        let Some(rf) = r.get("query").and_then(|q| q.get("$ref")).and_then(|x| x.as_str()) else { continue };
        let query_name = rf.rsplit('/').next().unwrap_or("");
        let Some(query_def) =
            model.defs.get("api.yaml").and_then(|v| v.get("queries")).and_then(|v| v.get(query_name))
        else {
            continue;
        };
        let Some(returns_ref) = query_def.get("returns").and_then(|r| r.get("$ref")).and_then(|x| x.as_str())
        else {
            continue;
        };
        let type_name = returns_ref.rsplit('/').next().unwrap_or("").to_string();
        if type_name.is_empty() {
            continue;
        }
        let mut parts = dr.splitn(2, '.');
        let first = parts.next().unwrap_or(dr.as_str()).to_string();
        if let Some(second) = parts.next() {
            if !second.is_empty() && second.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                roots.insert(format!("{second}_{first}"), (type_name.clone(), query_name.to_string()));
            }
        }
        roots.insert(first, (type_name, query_name.to_string()));
    }
    roots
}

/// The api type definition's own `Value` node ("`.properties`" carrier), keyed by the file its
/// definition actually lives in: `api.yaml` types nest under `types:`; every other source file
/// (`entities.yaml`, `scalars.yaml`, a scope's own entities fragment merged into the same logical key)
/// is a flat top-level map, matching how `specs/**` is authored.
fn type_def<'a>(model: &'a Model, ctx_file: &str, type_name: &str) -> Option<&'a Value> {
    if ctx_file == "api.yaml" {
        model.defs.get("api.yaml")?.get("types")?.get(type_name)
    } else {
        model.defs.get(ctx_file)?.get(type_name)
    }
}

/// One step of the path walk: does `type_val` (defined in `ctx_file`) declare a property `field`?
/// `Ok(None)` = the field exists but is a leaf for this walk's purposes (no `$ref`, an `array: true`
/// collection, or a scalar with no further properties) — a trailing segment past it is left unchecked
/// (the honest boundary, not a false-negative dodge: see the module doc). `Ok(Some(..))` = the field
/// exists and names another object to keep walking (value, defining file, type name). `Err(())` = no
/// property of that name exists at all.
fn step<'a>(model: &'a Model, type_val: &'a Value, ctx_file: &str, field: &str) -> Result<Option<(&'a Value, String, String)>, ()> {
    let props = type_val.get("properties").and_then(|p| p.as_mapping()).ok_or(())?;
    let prop = props.get(Value::String(field.to_string())).ok_or(())?;
    if prop.get("array").and_then(|a| a.as_bool()) == Some(true) {
        return Ok(None);
    }
    let Some(rf) = prop.get("$ref").and_then(|x| x.as_str()) else {
        return Ok(None);
    };
    let next_name = ref_or_name(prop);
    let next_ctx = ref_target_file(rf, ctx_file).unwrap_or_else(|| ctx_file.to_string());
    if next_ctx == "scalars.yaml" {
        return Ok(None); // a scalar's own shape is opaque here — nothing further to walk
    }
    match type_def(model, &next_ctx, &next_name) {
        Some(v) => Ok(Some((v, next_ctx, next_name))),
        None => Ok(None), // resolves to something this walk cannot see into — stop, don't guess
    }
}

/// Resolve a dotted path's segments (AFTER the root) against `type_name`/`ctx_file`. `Some(field)` is
/// the first segment that names no property anywhere along the walk; `None` = the whole path resolved
/// (or the walk hit a declared leaf/unresolvable boundary before running out of segments — the honest
/// "checks what it can" stop, not a pass on a guess).
///
/// `nav` is the FK-derived navigation-field map (`api::nav_fields`, the SAME derivation the SDL and
/// server emitters consume, so this walk can never invent an edge the schema does not declare): a
/// segment that names no declared property may still be a real generated field of the composed
/// schema — `Cart.restaurant: Restaurant!`, `DeliveryJob.restaurant: Restaurant!` — and the walk
/// follows a single-target edge into its api type (#717). A collection edge (`Restaurant.carts`)
/// stops the walk like any `array: true` property.
pub(crate) fn first_unknown_segment(
    model: &Model,
    nav: &HashMap<String, Vec<NavField>>,
    type_name: &str,
    ctx_file: &str,
    segments: &[&str],
) -> Option<(String, String)> {
    let mut cur = type_def(model, ctx_file, type_name)?;
    let mut cur_ctx = ctx_file.to_string();
    let mut cur_name = type_name.to_string();
    for seg in segments {
        match step(model, cur, &cur_ctx, seg) {
            Err(()) => {
                let nav_field = if cur_ctx == "api.yaml" {
                    nav.get(cur_name.as_str()).and_then(|nfs| nfs.iter().find(|n| n.field == *seg))
                } else {
                    None
                };
                match nav_field {
                    None => return Some(((*seg).to_string(), cur_name)),
                    Some(n) if n.list => return None, // per-item fields belong to a loop variable, not this path
                    Some(n) => match type_def(model, "api.yaml", &n.target) {
                        Some(v) => {
                            cur = v;
                            cur_ctx = "api.yaml".to_string();
                            cur_name = n.target.clone();
                        }
                        None => return None, // target outside the walkable surface — stop, don't guess
                    },
                }
            }
            Ok(None) => return None, // hit a leaf/unresolvable boundary — nothing left to check
            Ok(Some((next, next_ctx, next_name))) => {
                cur = next;
                cur_ctx = next_ctx;
                cur_name = next_name;
            }
        }
    }
    None
}

/// A `^[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)+$` dotted path — the ONLY shape this rule
/// touches. Anything else inside `{{ }}` (a ternary, an arithmetic expression, a bare root with no
/// dot, a `| filter` already stripped by the caller) is not a simple field access and is left alone —
/// guessing at expression semantics is exactly the false-positive risk the dispatch ruled out.
pub(crate) fn simple_path_regex() -> regex::Regex {
    regex::Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)+$").unwrap()
}

/// The wired rule (§25): every simple dotted `{{ root.path }}` binding in one screen's subtree,
/// whose root a `data_requirements` resolver actually feeds, must resolve on the api type that
/// resolver's query returns. Anything outside that shape (loop variables, UI/form state, bare
/// roots, expressions) is left unchecked — the declared honest boundary in the module doc.
pub(crate) fn check_screen_bindings(
    model: &Model,
    issues: &mut Vec<Issue>,
    sfkey: &str,
    sid: &str,
    screen: &Value,
    resolvers: Option<&serde_yaml::Mapping>,
    nav: &HashMap<String, Vec<NavField>>,
) {
    let data_requirements: Vec<String> = screen
        .get("data_requirements")
        .and_then(|x| x.as_sequence())
        .map(|s| s.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let roots = screen_binding_roots(model, resolvers, &data_requirements);
    if roots.is_empty() {
        return;
    }
    let mustache = regex::Regex::new(r"\{\{([^{}]+)\}\}").unwrap();
    let simple = simple_path_regex();
    let mut bindings: Vec<(String, String)> = Vec::new();
    collect_template_bindings(screen, "", &mustache, &mut bindings);
    for (loc, expr) in bindings {
        let path = expr.split('|').next().unwrap_or("").trim();
        if !simple.is_match(path) {
            continue;
        }
        let mut segs = path.split('.');
        let root = segs.next().unwrap_or("");
        let Some((type_name, _query)) = roots.get(root) else { continue };
        let rest: Vec<&str> = segs.collect();
        if let Some((unknown, at_type)) = first_unknown_segment(model, nav, type_name, "api.yaml", &rest) {
            issues.push(err(
                "screen-binding-unknown-field",
                format!("{}/screens/{}{}", sfkey, sid, loc),
                format!(
                    "binding '{{{{ {} }}}}' walks '{}', which type '{}' (root '{}': {}) declares neither \
                     as a property nor as an FK-derived navigation field — the widget renders empty \
                     while the spec reads as though it were bound (#468).",
                    path, unknown, at_type, root, type_name
                ),
            ));
        }
    }
}

/// Every `{{ … }}` occurrence in a screen's component tree, with a location string built from map
/// keys / sequence indices. Mirrors `collect_on_success_types`'s whole-subtree walk style.
pub(crate) fn collect_template_bindings(node: &Value, loc: &str, mustache: &regex::Regex, out: &mut Vec<(String, String)>) {
    match node {
        Value::Sequence(seq) => {
            for (i, n) in seq.iter().enumerate() {
                collect_template_bindings(n, &format!("{loc}[{i}]"), mustache, out);
            }
        }
        Value::Mapping(map) => {
            for (k, v) in map {
                let key = k.as_str().unwrap_or("?");
                collect_template_bindings(v, &format!("{loc}.{key}"), mustache, out);
            }
        }
        Value::String(s) => {
            for cap in mustache.captures_iter(s) {
                out.push((loc.to_string(), cap[1].trim().to_string()));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cart_type(props: &[(&str, Value)]) -> Value {
        let mut properties = serde_yaml::Mapping::new();
        for (k, v) in props {
            properties.insert(Value::from(*k), v.clone());
        }
        let mut ty = serde_yaml::Mapping::new();
        ty.insert(Value::from("properties"), Value::Mapping(properties));
        Value::Mapping(ty)
    }

    fn money_ref() -> Value {
        let mut m = serde_yaml::Mapping::new();
        m.insert(Value::from("$ref"), Value::from("entities.yaml#/Money"));
        Value::Mapping(m)
    }

    fn breakdown_ref() -> Value {
        let mut m = serde_yaml::Mapping::new();
        m.insert(Value::from("$ref"), Value::from("entities.yaml#/PaymentBreakdown"));
        Value::Mapping(m)
    }

    fn model_with(cart_props: &[(&str, Value)], breakdown_props: &[(&str, Value)]) -> Model {
        let mut defs: BTreeMap<String, Value> = BTreeMap::new();
        let mut api_types = serde_yaml::Mapping::new();
        api_types.insert(Value::from("Cart"), cart_type(cart_props));
        let mut api = serde_yaml::Mapping::new();
        api.insert(Value::from("types"), Value::Mapping(api_types));
        defs.insert("api.yaml".into(), Value::Mapping(api));

        let mut entities = serde_yaml::Mapping::new();
        entities.insert(Value::from("PaymentBreakdown"), cart_type(breakdown_props));
        defs.insert("entities.yaml".into(), Value::Mapping(entities));

        Model { defs, ..Default::default() }
    }

    #[test]
    fn resolves_a_nested_ref_path_that_exists() {
        let model = model_with(
            &[("breakdown", breakdown_ref())],
            &[("total", money_ref())],
        );
        assert_eq!(first_unknown_segment(&model, &HashMap::new(), "Cart", "api.yaml", &["breakdown", "total"]), None);
    }

    #[test]
    fn flags_the_first_segment_that_names_no_property() {
        let model = model_with(&[("breakdown", breakdown_ref())], &[("total", money_ref())]);
        assert_eq!(
            first_unknown_segment(&model, &HashMap::new(), "Cart", "api.yaml", &["totalAmoun"]),
            Some(("totalAmoun".to_string(), "Cart".to_string()))
        );
    }

    #[test]
    fn flags_a_field_two_levels_deep() {
        let model = model_with(&[("breakdown", breakdown_ref())], &[("total", money_ref())]);
        assert_eq!(
            first_unknown_segment(&model, &HashMap::new(), "Cart", "api.yaml", &["breakdown", "discount"]),
            Some(("discount".to_string(), "PaymentBreakdown".to_string()))
        );
    }

    #[test]
    fn a_scalar_leaf_stops_the_walk_without_erroring_on_a_trailing_segment() {
        // `restaurantId` is a plain scalar $ref (no properties of its own) — the honest boundary
        // means a further segment past it is left unchecked, not guessed at.
        let mut restaurant_id = serde_yaml::Mapping::new();
        restaurant_id.insert(Value::from("$ref"), Value::from("scalars.yaml#/RestaurantId"));
        let model = model_with(&[("restaurantId", Value::Mapping(restaurant_id))], &[]);
        assert_eq!(first_unknown_segment(&model, &HashMap::new(), "Cart", "api.yaml", &["restaurantId", "anything"]), None);
    }

    #[test]
    fn an_array_field_stops_the_walk_rather_than_guessing_per_item_semantics() {
        let mut lines = serde_yaml::Mapping::new();
        lines.insert(Value::from("$ref"), Value::from("entities.yaml#/OrderLineItem"));
        lines.insert(Value::from("array"), Value::from(true));
        let model = model_with(&[("lines", Value::Mapping(lines))], &[]);
        assert_eq!(first_unknown_segment(&model, &HashMap::new(), "Cart", "api.yaml", &["lines", "length"]), None);
    }

    #[test]
    fn a_collection_nav_edge_stops_the_walk_like_an_array_property() {
        // The `n.list => return None` branch (checkpoint item, beck): a reverse-FK collection edge
        // (`Cart.orders: [Order!]!`) stops the walk — per-item fields belong to a loop variable,
        // not this root — exactly as a declared `array: true` property does.
        let model = model_with(&[("breakdown", breakdown_ref())], &[("total", money_ref())]);
        let mut nav: HashMap<String, Vec<NavField>> = HashMap::new();
        nav.insert(
            "Cart".into(),
            vec![NavField { field: "orders".into(), target: "Order".into(), list: true, nullable: false }],
        );
        assert_eq!(first_unknown_segment(&model, &nav, "Cart", "api.yaml", &["orders", "anything"]), None);
        // …and the collection edge is matched by NAME, not by mood: a typo'd segment on the SAME
        // type, resolved against the SAME nav map, still fires rather than riding the edge's stop.
        assert_eq!(
            first_unknown_segment(&model, &nav, "Cart", "api.yaml", &["ordersz"]),
            Some(("ordersz".to_string(), "Cart".to_string()))
        );
    }

    #[test]
    fn simple_path_regex_accepts_dotted_identifiers_only() {
        let re = simple_path_regex();
        assert!(re.is_match("cart.breakdown.total"));
        assert!(!re.is_match("cart")); // no dot — nothing to check structurally
        assert!(!re.is_match("restaurant.deliveryFee == 0 ? '' : restaurant.deliveryFee"));
        assert!(!re.is_match("restaurant.deliveryTime + 10"));
    }

    #[test]
    fn screen_binding_roots_mirrors_the_renderer_aliasing() {
        let mut defs: BTreeMap<String, Value> = BTreeMap::new();
        let mut queries = serde_yaml::Mapping::new();
        let mut current = serde_yaml::Mapping::new();
        let mut returns = serde_yaml::Mapping::new();
        returns.insert(Value::from("$ref"), Value::from("#/types/Cart"));
        current.insert(Value::from("returns"), Value::Mapping(returns));
        queries.insert(Value::from("current"), Value::Mapping(current));
        let mut api = serde_yaml::Mapping::new();
        api.insert(Value::from("queries"), Value::Mapping(queries));
        defs.insert("api.yaml".into(), Value::Mapping(api));
        let model = Model { defs, ..Default::default() };

        let mut resolvers = serde_yaml::Mapping::new();
        let mut cart_current = serde_yaml::Mapping::new();
        let mut query = serde_yaml::Mapping::new();
        query.insert(Value::from("$ref"), Value::from("api.yaml#/queries/current"));
        cart_current.insert(Value::from("query"), Value::Mapping(query));
        resolvers.insert(Value::from("cart.current"), Value::Mapping(cart_current));

        let roots = screen_binding_roots(&model, Some(&resolvers), &["cart.current".to_string()]);
        assert_eq!(roots.get("cart"), Some(&("Cart".to_string(), "current".to_string())));
        // second segment "current" is lowercase-only, so the reversed alias is also registered.
        assert_eq!(roots.get("current_cart"), Some(&("Cart".to_string(), "current".to_string())));
    }
}
