use crate::*;

// ─── schema.generated.graphql (port of emit/schema.ts) ──────────────────────────────────────────

pub(crate) struct ApiField {
    pub(crate) name: String,
    pub(crate) ty: String,
    pub(crate) is_ref: bool,
    pub(crate) required: bool,
    pub(crate) nullable: bool,
    pub(crate) array: bool,
    pub(crate) format: Option<String>,
    pub(crate) description: Option<String>,
}
pub(crate) struct ApiType {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) reads: Vec<String>,
    /// OPTIONAL `readsInfrastructure:` — the write-path table(s) a TRANSIENT type is served from
    /// (ADR-20260812-214500). Present ⇔ the type has no read model behind it and says so; the
    /// validator's `transient-type-undeclared-infrastructure` makes the declaration mandatory rather
    /// than inferring transience from a missing `reads:`.
    pub(crate) reads_infrastructure: Vec<String>,
    pub(crate) properties: Vec<ApiField>,
    /// OPTIONAL per-type `navRoles:` — FK-derived navigation edge → LITERAL roles list (#22,
    /// ADR-20260720-230000). Omitted edge = open (inherits the parent type's reachability).
    pub(crate) nav_roles: Vec<(String, Vec<String>)>,
}
/// A query's DSL-declared `argsExactlyOneOf` (#749): exactly ONE of the named optional args must
/// be provided. Unspellable in GraphQL's argument type system (`@oneOf` covers input objects
/// only), so the declaration drives a GENERATED resolver check (emit/server_graphql) plus an SDL
/// description stating the contract — never ad-hoc resolver code.
pub(crate) struct ExactlyOneOf {
    /// The arg names (last segment of the declaration's `of:` $refs, which must point back at
    /// this query's own args — validator-enforced).
    pub(crate) args: Vec<String>,
    /// The errors.yaml error zero-of/two-of reject with (last segment of the `throws:` $ref).
    pub(crate) throws: String,
}

impl ExactlyOneOf {
    /// The ONE human sentence stating the contract — shared by the SDL emitter and the server
    /// input-type emitter so the two descriptions cannot drift.
    pub(crate) fn sentence(&self) -> String {
        format!(
            "Exactly one of `{}` must be provided; zero or both reject with `{}`.",
            self.args.join("`, `"),
            self.throws
        )
    }
}

pub(crate) struct ApiQuery {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) args: Vec<ApiField>,
    pub(crate) returns_type: String,
    pub(crate) returns_list: bool,
    pub(crate) returns_nullable: bool,
    pub(crate) reads: Vec<String>,
    pub(crate) roles: Vec<String>,
    pub(crate) slice: String,
    /// `argsExactlyOneOf:` — `None` for the (vast) majority of queries.
    pub(crate) exactly_one_of: Option<ExactlyOneOf>,
}
pub(crate) struct ApiMutation {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) command: String,
    pub(crate) roles: Vec<String>,
    pub(crate) slice: String,
    pub(crate) payload: Vec<ApiField>,
}
pub(crate) struct Api {
    pub(crate) types: Vec<ApiType>,
    pub(crate) queries: Vec<ApiQuery>,
    pub(crate) mutations: Vec<ApiMutation>,
    pub(crate) subscriptions: Vec<ApiQuery>,
    /// api.yaml `inputs:` — generator-injected input types that are not command payloads
    /// (MetadataInput, ADR-20260720-015500). (name, fields) pairs, emission order = declaration.
    pub(crate) inputs: Vec<(String, Vec<ApiField>)>,
}

pub(crate) const DIRECTIVES: &str = "directive @auth(requires: [UserType!]!) on FIELD_DEFINITION\ndirective @public on FIELD_DEFINITION\ndirective @command(name: String!) on FIELD_DEFINITION\ndirective @reads(views: [String!]!) on FIELD_DEFINITION";

pub(crate) fn pascal(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
pub(crate) fn camel(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// refOrName: the LAST `/`-segment of a `$ref` (object or string) or a bare type string.
pub(crate) fn ref_or_name(v: &Value) -> String {
    if let Some(r) = v.get("$ref").and_then(|x| x.as_str()) {
        return r.rsplit('/').next().unwrap_or("").to_string();
    }
    if let Some(s) = v.as_str() {
        return s.rsplit('/').next().unwrap_or("").to_string();
    }
    String::new()
}
pub(crate) fn name_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_sequence())
        .map(|s| s.iter().map(ref_or_name).filter(|r| !r.is_empty()).collect())
        .unwrap_or_default()
}
pub(crate) fn string_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_sequence())
        .map(|s| s.iter().filter_map(|i| i.as_str().map(|x| x.to_string())).collect())
        .unwrap_or_default()
}

pub(crate) fn parse_field(name: &str, n: &Value) -> ApiField {
    let is_ref = n.get("$ref").and_then(|x| x.as_str()).is_some();
    let ty = if is_ref {
        ref_or_name(n)
    } else {
        n.get("type").and_then(|x| x.as_str()).unwrap_or("").to_string()
    };
    let flag = |k: &str| n.get(k).and_then(|x| x.as_bool()) == Some(true);
    ApiField {
        name: name.to_string(),
        ty,
        is_ref,
        required: flag("required"),
        nullable: flag("nullable"),
        array: flag("array"),
        format: n.get("format").and_then(|x| x.as_str()).map(|s| s.to_string()),
        description: n.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()),
    }
}
pub(crate) fn field_map(v: Option<&Value>) -> Vec<ApiField> {
    match v.and_then(|x| x.as_mapping()) {
        Some(m) => m.iter().filter_map(|(k, node)| k.as_str().map(|name| parse_field(name, node))).collect(),
        None => vec![],
    }
}

/// api.yaml `types.<T>.navRoles` — field name → literal roles list for FK-derived nav edges (#22).
pub(crate) fn nav_roles_map(v: Option<&Value>) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    if let Some(Value::Mapping(m)) = v {
        for (k, r) in m {
            if let (Some(field), Some(seq)) = (k.as_str(), r.as_sequence()) {
                out.push((
                    field.to_string(),
                    seq.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
                ));
            }
        }
    }
    out
}

pub(crate) fn parse_api(model: &Model) -> Api {
    let sect = |k: &str| model.defs.get("api.yaml").and_then(|v| v.get(k)).and_then(|v| v.as_mapping());
    let mut types = Vec::new();
    if let Some(m) = sect("types") {
        for (k, t) in m {
            if let Some(name) = k.as_str() {
                types.push(ApiType { name: name.into(), description: t.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()), reads: name_list(t.get("reads")), reads_infrastructure: name_list(t.get("readsInfrastructure")), properties: field_map(t.get("properties")), nav_roles: nav_roles_map(t.get("navRoles")) });
            }
        }
    }
    let reads_by_type: HashMap<String, Vec<String>> = types.iter().map(|t| (t.name.clone(), t.reads.clone())).collect();
    let parse_query = |name: &str, q: &Value, with_reads: bool| -> ApiQuery {
        let returns = q.get("returns");
        let rt = returns.and_then(|r| r.get("$ref")).or_else(|| returns.and_then(|r| r.get("type")));
        let returns_type = rt.map(ref_or_name).unwrap_or_default();
        let reads = if with_reads {
            reads_by_type.get(&returns_type).cloned().unwrap_or_default()
        } else {
            vec![]
        };
        ApiQuery {
            name: name.into(),
            description: q.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()),
            args: field_map(q.get("args")),
            returns_type,
            returns_list: returns.and_then(|r| r.get("array")).and_then(|x| x.as_bool()) == Some(true),
            returns_nullable: returns.and_then(|r| r.get("nullable")).and_then(|x| x.as_bool()) == Some(true),
            reads,
            roles: string_list(q.get("roles")),
            slice: q.get("slice").and_then(|x| x.as_str()).unwrap_or("V0").to_string(),
            exactly_one_of: q.get("argsExactlyOneOf").map(|d| ExactlyOneOf {
                args: name_list(d.get("of")),
                throws: d.get("throws").map(ref_or_name).unwrap_or_default(),
            }),
        }
    };
    let mut queries = Vec::new();
    if let Some(m) = sect("queries") {
        for (k, q) in m {
            if let Some(n) = k.as_str() {
                queries.push(parse_query(n, q, true));
            }
        }
    }
    let mut subscriptions = Vec::new();
    if let Some(m) = sect("subscriptions") {
        for (k, q) in m {
            if let Some(n) = k.as_str() {
                subscriptions.push(parse_query(n, q, false));
            }
        }
    }
    let mut mutations = Vec::new();
    if let Some(m) = sect("mutations") {
        for (k, mu) in m {
            if let Some(n) = k.as_str() {
                mutations.push(ApiMutation {
                    name: n.into(),
                    description: mu.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()),
                    command: mu.get("command").map(ref_or_name).unwrap_or_default(),
                    roles: string_list(mu.get("roles")),
                    slice: mu.get("slice").and_then(|x| x.as_str()).unwrap_or("V0").to_string(),
                    payload: field_map(mu.get("payload")),
                });
            }
        }
    }
    let mut inputs = Vec::new();
    if let Some(m) = sect("inputs") {
        for (k, def) in m {
            if let Some(n) = k.as_str() {
                inputs.push((n.to_string(), field_map(def.get("properties"))));
            }
        }
    }
    Api { types, queries, mutations, subscriptions, inputs }
}

pub(crate) fn inline_primitive(t: &str, format: Option<&str>) -> String {
    match t {
        "integer" => "Int".into(),
        "boolean" => "Boolean".into(),
        "string" => if format == Some("date-time") { "DateTime".into() } else { "String".into() },
        _ => "String".into(),
    }
}

pub(crate) fn ref_target_file(r: &str, ctx: &str) -> Option<String> {
    let pr = parse_ref(r)?;
    let file = if pr.file.is_empty() { ctx.to_string() } else { pr.file };
    if is_source_file(&file) { Some(file) } else { None }
}

pub(crate) fn base_type(model: &Model, node: &Value, ctx: &str, input: bool) -> String {
    if let Some(rf) = node.get("$ref").and_then(|x| x.as_str()) {
        let file = ref_target_file(rf, ctx);
        let name = parse_ref(rf).and_then(|p| p.path.into_iter().next()).unwrap_or_else(|| "String".into());
        if file.as_deref() == Some("scalars.yaml") {
            return name;
        }
        return if input { format!("{}Input", name) } else { name };
    }
    if node.get("type").and_then(|x| x.as_str()) == Some("array") {
        if let Some(items) = node.get("items") {
            return format!("[{}!]", base_type(model, items, ctx, input));
        }
    }
    inline_primitive(
        node.get("type").and_then(|x| x.as_str()).unwrap_or("string"),
        node.get("format").and_then(|x| x.as_str()),
    )
}

pub(crate) fn object_fields(model: &Model, def: &Value, ctx: &str, input: bool) -> Vec<String> {
    let props = match def.get("properties").and_then(|p| p.as_mapping()) {
        Some(m) => m,
        None => return vec![],
    };
    let required: HashSet<&str> = def
        .get("required")
        .and_then(|r| r.as_sequence())
        .map(|s| s.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    let mut out = Vec::new();
    for (k, p) in props {
        let name = match k.as_str() {
            Some(s) => s,
            None => continue,
        };
        if input && p.get("readOnly").and_then(|x| x.as_bool()) == Some(true) {
            continue;
        }
        let base = base_type(model, p, ctx, input);
        let non_null = if input {
            required.contains(name)
        } else {
            p.get("nullable").and_then(|x| x.as_bool()) != Some(true)
        };
        out.push(format!("  {}: {}{}", name, base, if non_null { "!" } else { "" }));
    }
    out
}

pub(crate) fn scalar_names(model: &Model) -> HashSet<String> {
    model
        .defs
        .get("scalars.yaml")
        .and_then(|v| v.as_mapping())
        .map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

pub(crate) fn api_field_type(model: &Model, f: &ApiField, input: bool) -> String {
    let mut base = if f.is_ref {
        if input && !scalar_names(model).contains(&f.ty) {
            format!("{}Input", f.ty)
        } else {
            f.ty.clone()
        }
    } else {
        inline_primitive(&f.ty, f.format.as_deref())
    };
    if f.array {
        base = format!("[{}!]", base);
    }
    let non_null = if input { f.required } else { !f.nullable };
    format!("{}{}", base, if non_null { "!" } else { "" })
}

pub(crate) fn scalars_block(model: &Model) -> String {
    let mut lines = vec!["scalar DateTime".to_string()];
    if let Some(m) = model.defs.get("scalars.yaml").and_then(|v| v.as_mapping()) {
        for (k, def) in m {
            if let Some(name) = k.as_str() {
                if !def.get("enum").map(|e| e.is_sequence()).unwrap_or(false) {
                    lines.push(format!("scalar {}", name));
                }
            }
        }
    }
    lines.join("\n")
}

pub(crate) fn enums_block(model: &Model) -> String {
    let mut blocks = Vec::new();
    if let Some(m) = model.defs.get("scalars.yaml").and_then(|v| v.as_mapping()) {
        for (k, def) in m {
            if let (Some(name), Some(vals)) = (k.as_str(), def.get("enum").and_then(|e| e.as_sequence())) {
                let body: Vec<String> = vals.iter().map(|v| format!("  {}", v.as_str().unwrap_or(""))).collect();
                blocks.push(format!("enum {} {{\n{}\n}}", name, body.join("\n")));
            }
        }
    }
    blocks.join("\n\n")
}

/// One FK-derived navigation field on an output type (shared by the SDL emitter and the server
/// async-graphql emitter, so the two can never drift).
pub(crate) struct NavField {
    pub(crate) field: String,
    pub(crate) target: String,
    pub(crate) list: bool,
    pub(crate) nullable: bool,
}

pub(crate) fn nav_add(
    entity: &str,
    nf: NavField,
    entity_names: &HashSet<String>,
    seen: &mut HashMap<String, HashSet<String>>,
    out: &mut HashMap<String, Vec<NavField>>,
) {
    // Both ends must be registered API types: a navigation field TO an unregistered aggregate (e.g.
    // Payment, whose View_PendingRefunds fk only documents read lineage) would emit an SDL/Rust
    // reference to a type that does not exist.
    if !entity_names.contains(entity) || !entity_names.contains(&nf.target) {
        return;
    }
    let s = seen.entry(entity.to_string()).or_default();
    if s.contains(&nf.field) {
        return;
    }
    s.insert(nf.field.clone());
    out.entry(entity.to_string()).or_default().push(nf);
}

/// FK-derived navigation fields per entity, structured (views.yaml foreign keys → `src.tgt` single
/// navigation + `tgt.srcs` reverse collection).
pub(crate) fn nav_fields(views: &[SqlView], entity_names: &HashSet<String>) -> HashMap<String, Vec<NavField>> {
    let view_agg: HashMap<String, String> = views.iter().map(|v| (v.name.clone(), v.aggregate.clone())).collect();
    let mut seen: HashMap<String, HashSet<String>> = HashMap::new();
    let mut out: HashMap<String, Vec<NavField>> = HashMap::new();
    for v in views {
        for col in &v.columns {
            let fk = match &col.fk {
                Some(f) => f,
                None => continue,
            };
            let target_view = fk.split('.').next().unwrap_or("");
            let tgt = match view_agg.get(target_view) {
                Some(t) if !t.is_empty() => t.clone(),
                _ => continue,
            };
            let src = v.aggregate.clone();
            if entity_names.contains(&tgt) {
                nav_add(&src, NavField { field: camel(&tgt), target: tgt.clone(), list: false, nullable: col.nullable }, entity_names, &mut seen, &mut out);
                nav_add(&tgt, NavField { field: format!("{}s", camel(&src)), target: src.clone(), list: true, nullable: false }, entity_names, &mut seen, &mut out);
            }
        }
    }
    out
}

pub(crate) fn nav_by_entity(
    views: &[SqlView],
    entity_names: &HashSet<String>,
    nav_roles: &HashMap<String, HashMap<String, Vec<String>>>,
) -> HashMap<String, Vec<String>> {
    nav_fields(views, entity_names)
        .into_iter()
        .map(|(entity, nfs)| {
            let lines = nfs
                .into_iter()
                .map(|n| {
                    // Guarded edge (#22): same @auth directive as operations; omitted = bare/open.
                    let auth = nav_roles
                        .get(&entity)
                        .and_then(|m| m.get(&n.field))
                        .map(|roles| format!(" {}", auth_directive(roles)))
                        .unwrap_or_default();
                    if n.list {
                        format!("  {}: [{}!]!{}", n.field, n.target, auth)
                    } else {
                        format!("  {}: {}{}{}", n.field, n.target, if n.nullable { "" } else { "!" }, auth)
                    }
                })
                .collect();
            (entity, lines)
        })
        .collect()
}

pub(crate) fn output_types_block(model: &Model, views: &[SqlView], api: &Api) -> String {
    let registered: HashSet<String> = api.types.iter().map(|t| t.name.clone()).collect();
    let nav_roles: HashMap<String, HashMap<String, Vec<String>>> = api
        .types
        .iter()
        .map(|t| (t.name.clone(), t.nav_roles.iter().cloned().collect()))
        .collect();
    let nav = nav_by_entity(views, &registered, &nav_roles);
    let mut blocks = Vec::new();
    if let Some(m) = model.defs.get("entities.yaml").and_then(|v| v.as_mapping()) {
        for (k, def) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            if registered.contains(name) {
                continue;
            }
            let mut fields = object_fields(model, def, "entities.yaml", false);
            if let Some(nf) = nav.get(name) {
                fields.extend(nf.clone());
            }
            blocks.push(format!("type {} {{\n{}\n}}", name, fields.join("\n")));
        }
    }
    for t in &api.types {
        let mut fields: Vec<String> = t.properties.iter().map(|f| format!("  {}: {}", f.name, api_field_type(model, f, false))).collect();
        if let Some(nf) = nav.get(&t.name) {
            fields.extend(nf.clone());
        }
        blocks.push(format!("type {} {{\n{}\n}}", t.name, fields.join("\n")));
    }
    blocks.join("\n\n")
}

pub(crate) fn visit_inputs(model: &Model, name: &str, file: &str, needed: &mut Vec<(String, String)>, visited: &mut HashSet<String>) {
    let key = format!("{}#{}", file, name);
    if visited.contains(&key) {
        return;
    }
    visited.insert(key);
    let def = match model.defs.get(file).and_then(|d| d.get(name)) {
        Some(d) => d,
        None => return,
    };
    let mut refs = Vec::new();
    collect_refs(def, file, &mut refs);
    for (_loc, r) in refs {
        if let Some(tf) = ref_target_file(&r, file) {
            let rn = parse_ref(&r).and_then(|p| p.path.into_iter().next());
            if tf != "scalars.yaml" {
                if let Some(rn) = rn {
                    needed.push((rn.clone(), tf.clone()));
                    visit_inputs(model, &rn, &tf, needed, visited);
                }
            }
        }
    }
}

pub(crate) fn input_types_block(model: &Model, api: &Api) -> String {
    let mut needed: Vec<(String, String)> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();

    let mut command_inputs = Vec::new();
    for m in &api.mutations {
        if let Some(def) = model.defs.get("commands.yaml").and_then(|d| d.get(&m.command)) {
            command_inputs.push(format!("input {}Input {{\n{}\n}}", m.command, object_fields(model, def, "commands.yaml", true).join("\n")));
            visit_inputs(model, &m.command, "commands.yaml", &mut needed, &mut visited);
        }
    }

    let scalars = scalar_names(model);
    let mut query_inputs = Vec::new();
    for q in &api.queries {
        if q.args.is_empty() {
            continue;
        }
        let fields: Vec<String> = q.args.iter().map(|a| format!("  {}: {}", a.name, api_field_type(model, a, true))).collect();
        // `argsExactlyOneOf` (#749): the one-of contract is unspellable in the type system, so the
        // SDL DESCRIPTION states it — generated from the same declaration the resolver check is.
        let one_of_doc = q
            .exactly_one_of
            .as_ref()
            .map(|x| format!("\"\"\"\n{}\n\"\"\"\n", x.sentence()))
            .unwrap_or_default();
        query_inputs.push(format!("{}input {}QueryInput {{\n{}\n}}", one_of_doc, pascal(&q.name), fields.join("\n")));
        for a in &q.args {
            if a.is_ref && !scalars.contains(&a.ty) {
                visit_inputs(model, &a.ty, "entities.yaml", &mut needed, &mut visited);
            }
        }
    }

    let mut subscription_inputs = Vec::new();
    for s in &api.subscriptions {
        if s.args.is_empty() {
            continue;
        }
        let fields: Vec<String> = s.args.iter().map(|a| format!("  {}: {}", a.name, api_field_type(model, a, true))).collect();
        subscription_inputs.push(format!("input {}SubscriptionInput {{\n{}\n}}", pascal(&s.name), fields.join("\n")));
        for a in &s.args {
            if a.is_ref && !scalars.contains(&a.ty) {
                visit_inputs(model, &a.ty, "entities.yaml", &mut needed, &mut visited);
            }
        }
    }

    let mut emitted: HashSet<String> = HashSet::new();
    let mut object_inputs = Vec::new();
    for (name, file) in &needed {
        if emitted.contains(name) {
            continue;
        }
        emitted.insert(name.clone());
        if let Some(def) = model.defs.get(file).and_then(|d| d.get(name)) {
            object_inputs.push(format!("input {}Input {{\n{}\n}}", name, object_fields(model, def, file, true).join("\n")));
        }
    }

    // Generator-injected inputs (api.yaml `inputs:` — MetadataInput): declared fields, all optional
    // unless marked required (the technical envelope is always client-optional).
    let mut declared_inputs = Vec::new();
    for (name, fields) in &api.inputs {
        let lines: Vec<String> = fields
            .iter()
            .map(|f| format!("  {}: {}", f.name, api_field_type(model, f, true)))
            .collect();
        declared_inputs.push(format!("input {} {{\n{}\n}}", name, lines.join("\n")));
    }

    let mut all = command_inputs;
    all.extend(query_inputs);
    all.extend(subscription_inputs);
    all.extend(object_inputs);
    all.extend(declared_inputs);
    all.join("\n\n")
}

pub(crate) fn auth_directive(roles: &[String]) -> String {
    // Literal roles (ADR-20260720-191500): omitted = open to every role path (@public); present =
    // exactly the listed paths (@auth) — PUBLIC inside `requires` is the anonymous path.
    if roles.is_empty() {
        "@public".to_string()
    } else {
        format!("@auth(requires: [{}])", roles.join(", "))
    }
}

pub(crate) fn query_block(api: &Api) -> String {
    let fields: Vec<String> = api
        .queries
        .iter()
        .map(|q| {
            let arg_str = if q.args.is_empty() {
                String::new()
            } else {
                format!("(input: {}QueryInput{})", pascal(&q.name), if q.args.iter().any(|a| a.required) { "!" } else { "" })
            };
            let inner = if q.returns_list { format!("[{}!]", q.returns_type) } else { q.returns_type.clone() };
            let ret = format!("{}{}", inner, if q.returns_nullable { "" } else { "!" });
            let reads = if q.reads.is_empty() {
                String::new()
            } else {
                format!(" @reads(views: [{}])", q.reads.iter().map(|v| format!("\"{}\"", v)).collect::<Vec<_>>().join(", "))
            };
            format!("  {}{}: {} {}{}", q.name, arg_str, ret, auth_directive(&q.roles), reads)
        })
        .collect();
    format!("type Query {{\n{}\n}}", fields.join("\n"))
}

pub(crate) fn mutation_block(api: &Api) -> String {
    // Acceptance-first (ADR-20260720-015500): every mutation takes the optional technical envelope
    // and returns the ONE shared MutationAcceptance — business outcomes are reads.
    let fields: Vec<String> = api
        .mutations
        .iter()
        .map(|m| {
            format!(
                "  {}(input: {}Input!, metadata: MetadataInput): MutationAcceptance! {} @command(name: \"{}\")",
                m.name, m.command, auth_directive(&m.roles), m.command
            )
        })
        .collect();
    format!("type Mutation {{\n{}\n}}", fields.join("\n"))
}

pub(crate) fn subscription_block(api: &Api) -> String {
    let fields: Vec<String> = api
        .subscriptions
        .iter()
        .map(|s| {
            let arg_str = if s.args.is_empty() {
                String::new()
            } else {
                format!("(input: {}SubscriptionInput{})", pascal(&s.name), if s.args.iter().any(|a| a.required) { "!" } else { "" })
            };
            let inner = if s.returns_list { format!("[{}!]", s.returns_type) } else { s.returns_type.clone() };
            let ret = format!("{}{}", inner, if s.returns_nullable { "" } else { "!" });
            format!("  {}{}: {} {}", s.name, arg_str, ret, auth_directive(&s.roles))
        })
        .collect();
    format!("type Subscription {{\n{}\n}}", fields.join("\n"))
}

pub(crate) fn header(title: &str) -> String {
    let bar = "=".repeat(78);
    format!("# {}\n# {}\n# {}", bar, title, bar)
}

pub(crate) fn emit_schema(model: &Model) -> String {
    let api = parse_api(model);
    let views = parse_views(model);
    let mut s = String::new();
    s.push_str("# GENERATED by tools/codegen from specs/api.yaml (+ scalars/entities/commands/views) — do not edit by hand.\n");
    s.push_str("# Strong typing: one scalars.yaml type = one GraphQL scalar/enum. Navigation fields on output types\n");
    s.push_str("# are derived from views.yaml foreign keys. Mutations are ACCEPTANCE-FIRST (ADR-20260720-015500):\n");
    s.push_str("# every mutation takes an optional `metadata: MetadataInput` and returns the shared MutationAcceptance\n");
    s.push_str("# (effective envelope + operationStatus); business outcomes are reads (operationStatus/paymentStatus).\n\n");
    s.push_str(&header("Custom scalars"));
    s.push('\n');
    s.push_str(&scalars_block(model));
    s.push_str("\n\n");
    s.push_str(&header("Enums"));
    s.push('\n');
    s.push_str(&enums_block(model));
    s.push_str("\n\n");
    s.push_str(&header("Directives — ACL (@auth/@public) + declared links (@command/@reads)"));
    s.push('\n');
    s.push_str(DIRECTIVES);
    s.push_str("\n\n");
    s.push_str(&header("Output types (entities.yaml + FK-derived navigation + projections)"));
    s.push('\n');
    s.push_str(&output_types_block(model, &views, &api));
    s.push_str("\n\n");
    s.push_str(&header("Input types (mutation command payloads + query args)"));
    s.push('\n');
    s.push_str(&input_types_block(model, &api));
    s.push_str("\n\n");
    s.push_str(&header("Queries — read side"));
    s.push('\n');
    s.push_str(&query_block(&api));
    s.push_str("\n\n");
    s.push_str(&header("Mutations — write side"));
    s.push('\n');
    s.push_str(&mutation_block(&api));
    if !api.subscriptions.is_empty() {
        s.push_str("\n\n"); // template line break + the conditional's leading newline
        s.push_str(&header("Subscriptions — streams"));
        s.push('\n');
        s.push_str(&subscription_block(&api));
        s.push('\n');
    }
    s
}

