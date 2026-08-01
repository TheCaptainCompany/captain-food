use crate::*;

// ================================================================================================
// Service-catalog emitters (issue #26, ADR-20260719-214500 / codegen-roadmap item 4): trait +
// http client + binding wiring + expose-gated /services routes from specs/services.yaml.
// ================================================================================================

/// One operation of a service: the typed `input`/`output` property maps (services.yaml field style)
/// and the anticipated `errors.yaml` rejection names.
pub(crate) struct SvcOp {
    pub(crate) name: String,
    pub(crate) desc: String,
    pub(crate) input: Option<Value>,
    pub(crate) output: Option<Value>,
    pub(crate) errors: Vec<String>,
}

/// One service of services.yaml with its derived Rust names and SPEC-OWNED topology.
pub(crate) struct Svc {
    pub(crate) name: String,
    /// PascalCase base of the generated names (`payment` → trait `PaymentService`,
    /// structs `Payment<Op>Input`/`…Output`, client `HttpPaymentService`).
    pub(crate) base: String,
    pub(crate) desc: String,
    pub(crate) binding: String,
    pub(crate) expose: bool,
    pub(crate) ops: Vec<SvcOp>,
}

/// PascalCase of a snake_case name (`offer_job` → `OfferJob`).
pub(crate) fn pascal_snake(s: &str) -> String {
    s.split('_').map(pascal).collect()
}

/// The DERIVED HTTP path of a service operation: `/services/<service>/<op>`, snake_case →
/// kebab-case (ADR-20260719-214500 — derived, never hand-picked).
pub(crate) fn svc_http_path(service: &str, op: &str) -> String {
    format!("/services/{}/{}", service.replace('_', "-"), op.replace('_', "-"))
}

/// The address-book variable of an http-bound service: `SERVICE_<NAME>_URL`.
pub(crate) fn svc_url_var(service: &str) -> String {
    format!("SERVICE_{}_URL", service.to_uppercase())
}

/// Parse services.yaml (file order; the §2d `svc-*` rules already validated the shape).
pub(crate) fn parse_services(model: &Model) -> Vec<Svc> {
    let mut out = Vec::new();
    let Some(Value::Mapping(m)) = model.defs.get("services.yaml") else {
        return out;
    };
    for (k, node) in m {
        let Some(name) = k.as_str() else { continue };
        let ops = node
            .get("operations")
            .and_then(|x| x.as_mapping())
            .map(|ops| {
                ops.iter()
                    .filter_map(|(ok, op)| {
                        let oname = ok.as_str()?;
                        Some(SvcOp {
                            name: oname.to_string(),
                            desc: op.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string(),
                            input: op.get("input").cloned(),
                            output: op.get("output").cloned(),
                            errors: ref_strings(op.get("errors")).iter().filter_map(|r| ref_name(r)).collect(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(Svc {
            name: name.to_string(),
            base: pascal_snake(name),
            desc: node.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string(),
            binding: node.get("binding").and_then(|b| b.as_str()).unwrap_or("local").to_string(),
            expose: node.get("expose").and_then(|b| b.as_bool()).unwrap_or(false),
            ops,
        });
    }
    out
}

/// Emit one serde `camelCase` struct for a service operation's `input`/`output` property map.
/// Unlike events/commands (which carry `required:` lists), service payload fields are REQUIRED
/// unless marked `nullable: true` — the catalog declares the exact call surface.
pub(crate) fn push_service_struct(out: &mut String, name: &str, doc: &str, props: &Value) {
    out.push('\n');
    if !doc.trim().is_empty() {
        out.push_str(&format!("/// {}\n", ws1(doc).trim()));
    }
    out.push_str("#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]\n");
    out.push_str("#[serde(rename_all = \"camelCase\")]\n");
    out.push_str(&format!("pub struct {} {{\n", name));
    if let Some(map) = props.as_mapping() {
        for (pk, pnode) in map {
            let Some(prop) = pk.as_str() else { continue };
            let field = snake_field(prop);
            // PROVE serde's camelCase rename restores the exact spec property name on the wire.
            assert_eq!(
                serde_camel(&field),
                prop,
                "services.yaml#/{}/{}: field '{}' does not round-trip through serde rename_all",
                name,
                prop,
                field
            );
            let ident = if RUST_FIELD_KEYWORDS.contains(&field.as_str()) { format!("r#{}", field) } else { field };
            let ty = struct_field_type("services.yaml", name, prop, pnode);
            if pnode.get("nullable").and_then(|x| x.as_bool()) == Some(true) {
                out.push_str(&format!("    pub {}: Option<{}>,\n", ident, ty));
            } else {
                out.push_str(&format!("    pub {}: {},\n", ident, ty));
            }
        }
    }
    out.push_str("}\n");
}

/// The generated trait-method signature of a service operation: `(&self, input: <In>, meta:
/// &ServiceCallMeta) -> Result<<Out> | (), DomainError>`; an input-less operation drops the
/// `input` parameter.
pub(crate) fn svc_op_signature(svc: &Svc, op: &SvcOp) -> String {
    let input = if op.input.is_some() {
        format!("input: {}{}Input, ", svc.base, pascal_snake(&op.name))
    } else {
        String::new()
    };
    let output = if op.output.is_some() {
        format!("{}{}Output", svc.base, pascal_snake(&op.name))
    } else {
        "()".to_string()
    };
    format!("async fn {}(&self, {}meta: &ServiceCallMeta) -> Result<{}, DomainError>", op.name, input, output)
}

/// Emit `crates/application/src/generated/services.rs` — the SERVICE PORT traits (issue #26,
/// ADR-20260719-214500): per service one `<Base>Service` trait with one method per operation, plus
/// the typed `<Base><Op>Input`/`…Output` structs (serde `camelCase`, so the http binding's wire
/// shape IS the spec shape). Every call also carries the [`ServiceCallMeta`] ENVELOPE — the
/// correlation metadata (ADR-0041 spirit) that is never part of the spec-declared business input.
pub(crate) fn emit_services_application(model: &Model) -> String {
    let services = parse_services(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/services.yaml — do not edit by hand.\n\
         // SERVICE PORT traits (ADR-20260719-214500, issue #26): the abstract capabilities the domain\n\
         // calls, one trait per service, consumed by command handlers and process managers. Provider\n\
         // vocabulary never appears here — the implementation ACL (adapter crates) translates names and\n\
         // payloads. The spec-declared `input`/`output` are the BUSINESS payload; correlation metadata\n\
         // travels on the [`ServiceCallMeta`] envelope (like the event envelope, ADR-0041), so business\n\
         // ids an INBOUND adapter needs to map provider facts back onto our aggregates (e.g. the Stripe\n\
         // PaymentIntent `metadata` the webhook ACL reads back) are declared at the call site and copied\n\
         // verbatim by the provider ACL — never smuggled into the operation input.\n\n\
         use async_trait::async_trait;\n\
         use domain::generated::entities::*;\n\
         use domain::generated::events::*;\n\
         use domain::generated::scalars::*;\n\
         use domain::shared::errors::DomainError;\n\
         use serde::{Deserialize, Serialize};\n",
    );
    out.push_str(
        r#"
/// The service-call ENVELOPE: infrastructure/correlation metadata that travels WITH every
/// operation call but is never part of the spec-declared business input (ADR-0041 spirit).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceCallMeta {
    /// The triggering command/event's correlation id (ADR-0041), propagated across every binding.
    pub correlation_id: uuid::Uuid,
    /// Business correlation references the INBOUND adapter needs to map provider facts back onto
    /// our aggregates (e.g. Stripe intent metadata `orderId`/`restaurantId`/`cartId`) — copied
    /// verbatim onto the provider call, never business input.
    #[serde(default)]
    pub refs: std::collections::BTreeMap<String, String>,
}

impl ServiceCallMeta {
    /// Envelope for a call correlated to `correlation_id`, with no business refs.
    pub fn new(correlation_id: uuid::Uuid) -> Self {
        Self { correlation_id, refs: std::collections::BTreeMap::new() }
    }
    /// Attach one business correlation reference (builder style).
    pub fn with_ref(mut self, key: &str, value: impl Into<String>) -> Self {
        self.refs.insert(key.to_string(), value.into());
        self
    }
}
"#,
    );
    for svc in &services {
        out.push_str(&format!(
            "\n// ---------------------------------------------------------------------------------------------------\n// service `{}` — binding: {}, expose: {}\n// ---------------------------------------------------------------------------------------------------\n",
            svc.name, svc.binding, svc.expose
        ));
        for op in &svc.ops {
            if let Some(input) = &op.input {
                push_service_struct(
                    &mut out,
                    &format!("{}{}Input", svc.base, pascal_snake(&op.name)),
                    &format!("Input of `{}.{}`.", svc.name, op.name),
                    input,
                );
            }
            if let Some(output) = &op.output {
                push_service_struct(
                    &mut out,
                    &format!("{}{}Output", svc.base, pascal_snake(&op.name)),
                    &format!("Output of `{}.{}`.", svc.name, op.name),
                    output,
                );
            }
        }
        out.push('\n');
        if !svc.desc.trim().is_empty() {
            out.push_str(&format!("/// {}\n", ws1(&svc.desc).trim()));
        }
        out.push_str(&format!("#[async_trait]\npub trait {}Service: Send + Sync {{\n", svc.base));
        for op in &svc.ops {
            if !op.desc.trim().is_empty() {
                out.push_str(&format!("    /// {}\n", ws1(&op.desc).trim()));
            }
            if op.errors.is_empty() {
                out.push_str("    /// Anticipated rejections: none declared.\n");
            } else {
                out.push_str(&format!(
                    "    /// Anticipated rejections: {}.\n",
                    op.errors.iter().map(|e| format!("`errors.yaml#/{}`", e)).collect::<Vec<_>>().join(", ")
                ));
            }
            out.push_str(&format!("    {};\n", svc_op_signature(svc, op)));
        }
        out.push_str("}\n");
    }
    out
}

/// Emit `crates/infrastructure/src/generated/service_clients.rs` — the HTTP clients for the DERIVED
/// `/services/<service>/<op>` surface plus the shared wire envelopes. One `Http<Base>Service` per
/// service (compiled for every service so the wire path stays covered; the composition root only
/// CONSTRUCTS a client for a service whose spec binding is `http`, see `service_bindings.rs`).
pub(crate) fn emit_services_http_clients(model: &Model) -> String {
    let services = parse_services(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/services.yaml — do not edit by hand.\n\
         // HTTP clients for the DERIVED `/services/<service>/<op>` surface (ADR-20260719-214500,\n\
         // issue #26): every operation is a POST with the JSON call envelope `{ input, meta }`;\n\
         // a 2xx answers `{ output }` (null for output-less operations) and an error answers the\n\
         // kind-tagged `{ error }` envelope, so a remote failure rehydrates into the SAME\n\
         // `DomainError` a local call would return (rejections keep their errors.yaml code +\n\
         // context). Addresses come from `SERVICE_<NAME>_URL` — an address book, never a decision.\n\n\
         use application::generated::services::*;\n\
         use async_trait::async_trait;\n\
         use domain::shared::errors::DomainError;\n",
    );
    out.push_str(
        r#"
/// Wire envelope of one service-operation POST: the spec-declared input + the call envelope.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WireCall<I> {
    pub input: I,
    pub meta: ServiceCallMeta,
}

/// Wire envelope of a 2xx response (`output` is `null` for output-less operations).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WireOutput<O> {
    pub output: O,
}

/// Wire envelope of a non-2xx response.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WireErrorEnvelope {
    pub error: WireError,
}

/// A `DomainError` on the wire, kind-tagged so the caller-side error is indistinguishable
/// from a local call's.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireError {
    Rejected { code: String, context: serde_json::Value },
    Invariant { message: String },
    Repository { message: String },
}

impl From<&DomainError> for WireError {
    fn from(e: &DomainError) -> Self {
        match e {
            DomainError::Rejected { code, context } => {
                WireError::Rejected { code: code.clone(), context: context.clone() }
            }
            DomainError::Invariant(m) => WireError::Invariant { message: m.clone() },
            DomainError::Repository(m) => WireError::Repository { message: m.clone() },
        }
    }
}

impl From<WireError> for DomainError {
    fn from(e: WireError) -> Self {
        match e {
            WireError::Rejected { code, context } => DomainError::Rejected { code, context },
            WireError::Invariant { message } => DomainError::Invariant(message),
            WireError::Repository { message } => DomainError::Repository(message),
        }
    }
}

/// HTTP status of a service error: anticipated rejections are 422, invariants 409,
/// dependency failures 502.
pub fn wire_error_status(e: &WireError) -> u16 {
    match e {
        WireError::Rejected { .. } => 422,
        WireError::Invariant { .. } => 409,
        WireError::Repository { .. } => 502,
    }
}

/// POST one service-operation call and decode the response envelopes back into the port's
/// `Result`. Transport failures and undecodable bodies are `DomainError::Repository`.
async fn post_call<I: serde::Serialize, O: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    base_url: &str,
    path: &str,
    input: I,
    meta: &ServiceCallMeta,
) -> Result<O, DomainError> {
    let response = http
        .post(format!("{base_url}{path}"))
        .json(&WireCall { input, meta: meta.clone() })
        .send()
        .await
        .map_err(|e| DomainError::Repository(format!("service call: transport error on {path}: {e}")))?;
    let status = response.status().as_u16();
    let body = response
        .text()
        .await
        .map_err(|e| DomainError::Repository(format!("service call: body read error on {path}: {e}")))?;
    if (200..300).contains(&status) {
        let decoded: WireOutput<O> = serde_json::from_str(&body)
            .map_err(|e| DomainError::Repository(format!("service call: undecodable output on {path}: {e}")))?;
        return Ok(decoded.output);
    }
    match serde_json::from_str::<WireErrorEnvelope>(&body) {
        Ok(envelope) => Err(envelope.error.into()),
        Err(_) => Err(DomainError::Repository(format!(
            "service call: HTTP {status} on {path}: {}",
            &body[..body.len().min(200)]
        ))),
    }
}
"#,
    );
    for svc in &services {
        out.push_str(&format!(
            "\n/// HTTP [`{base}Service`] over `POST {{base}}{example}` (address: `{var}`).\npub struct Http{base}Service {{\n    http: reqwest::Client,\n    base_url: String,\n}}\n\nimpl Http{base}Service {{\n    pub fn new(base_url: impl Into<String>) -> Self {{\n        Self {{ http: reqwest::Client::new(), base_url: base_url.into().trim_end_matches('/').to_string() }}\n    }}\n}}\n\n#[async_trait]\nimpl {base}Service for Http{base}Service {{\n",
            base = svc.base,
            example = svc_http_path(&svc.name, "<op>"),
            var = svc_url_var(&svc.name),
        ));
        for op in &svc.ops {
            let arg = if op.input.is_some() { "input" } else { "()" };
            out.push_str(&format!(
                "    {} {{\n        post_call(&self.http, &self.base_url, \"{}\", {}, meta).await\n    }}\n",
                svc_op_signature(svc, op),
                svc_http_path(&svc.name, &op.name),
                arg
            ));
        }
        out.push_str("}\n");
    }
    out
}

/// Emit `crates/infrastructure/src/generated/service_bindings.rs` — the composition-root topology
/// resolvers (SPEC-OWNED binding, ADR-20260719-214500): one function per service. `binding: local`
/// invokes the supplied in-process constructor; `binding: http` ignores it and constructs the
/// generated client from `SERVICE_<NAME>_URL` (a missing address is a startup error). Flipping a
/// service's binding is a reviewed spec change that regenerates ONLY this wiring.
pub(crate) fn emit_service_bindings(model: &Model) -> String {
    let services = parse_services(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/services.yaml — do not edit by hand.\n\
         // Composition-root topology bindings (ADR-20260719-214500, issue #26): the deployment\n\
         // topology is DECIDED IN THE SPEC (`binding: local | http` per service) — environment\n\
         // configuration carries only addresses (`SERVICE_<NAME>_URL`), never decisions. The\n\
         // composition root supplies the in-process constructor for every service; whether it is\n\
         // used (local) or replaced by the generated HTTP client (http) is this module's call.\n\n\
         use application::generated::services::*;\n\
         use std::sync::Arc;\n",
    );
    for svc in &services {
        if svc.binding == "http" {
            out.push_str(&format!(
                "\n/// `{name}` — binding: http (services.yaml): the generated client over `{var}`;\n/// the in-process constructor is NOT wired. A missing address is a startup error.\npub fn {name}_service(\n    local: impl FnOnce() -> Arc<dyn {base}Service>,\n) -> Result<Arc<dyn {base}Service>, String> {{\n    let _ = local; // http binding — the in-process implementation is not used\n    let url = std::env::var(\"{var}\")\n        .map_err(|_| \"{var} is required: service '{name}' is bound http (services.yaml)\".to_string())?;\n    Ok(Arc::new(crate::generated::service_clients::Http{base}Service::new(url)))\n}}\n",
                name = svc.name,
                base = svc.base,
                var = svc_url_var(&svc.name),
            ));
        } else {
            out.push_str(&format!(
                "\n/// `{name}` — binding: local (services.yaml): the in-process adapter the composition\n/// root supplies; zero HTTP inside the deployable.\npub fn {name}_service(\n    local: impl FnOnce() -> Arc<dyn {base}Service>,\n) -> Result<Arc<dyn {base}Service>, String> {{\n    Ok(local())\n}}\n",
                name = svc.name,
                base = svc.base,
            ));
        }
    }
    out
}

/// Emit `specs/generated/render-config-sync.json` — the manifest the CI sync step consumes
/// (PROP-20260729-014500, D1–D4).
///
/// Only the keys CI must push to the Render service appear here: the secrets, plus the non-secrets a
/// key chose to source from a repo secret. Baked values are deliberately absent — they travel in the
/// image (D5), so pushing them to the service as well would recreate the mutable state the baking was
/// meant to remove, and the two copies would be free to disagree.
pub(crate) fn emit_render_sync_manifest(model: &Model) -> String {
    let keys = parse_config_keys(model);
    let mut out = String::from(
        "{\n  \"_generated\": \"by the Captain.Food codegen from specs/configuration.yaml — do not edit by hand\",\n\
         \x20 \"_contract\": \"env_key <- the named GitHub repo secret, per profile. Upsert only (D1): this manifest never expresses a deletion.\",\n\
         \x20 \"profiles\": {\n",
    );
    let profiles: Vec<&str> = CONFIG_PROFILES.to_vec();
    for (pi, profile) in profiles.iter().enumerate() {
        out.push_str(&format!("    \"{profile}\": [\n"));
        let mut rows: Vec<String> = Vec::new();
        for k in &keys {
            if k.consumer != "server" {
                continue;
            }
            if let Some(secret_name) = k.from_secret.get(*profile) {
                rows.push(format!(
                    "      {{ \"env_key\": \"{}\", \"from_secret\": \"{}\", \"secret\": {} }}",
                    k.name, secret_name, k.secret
                ));
            }
        }
        out.push_str(&rows.join(",\n"));
        if !rows.is_empty() {
            out.push('\n');
        }
        out.push_str(if pi + 1 == profiles.len() { "    ]\n" } else { "    ],\n" });
    }
    out.push_str("  }\n}\n");
    out
}

/// Emit `crates/server/src/generated/services_routes.rs` — the DERIVED `/services/<service>/<op>`
/// axum routes, emitted ONLY for services declaring `expose: true` (ADR-20260719-214500). With no
/// exposed service (the V0 default) the router is empty and state-generic; exposing one is a
/// reviewed spec change that regenerates the typed POST handlers + `ServicesRouterState`.
pub(crate) fn emit_services_routes(model: &Model) -> String {
    let services = parse_services(model);
    let exposed: Vec<&Svc> = services.iter().filter(|s| s.expose).collect();
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/services.yaml — do not edit by hand.\n\
         // The DERIVED `/services/<service>/<op>` surface (ADR-20260719-214500, issue #26): emitted\n\
         // ONLY for services declaring `expose: true`. All service operations are POSTs carrying the\n\
         // JSON call envelope `{ input, meta }`; responses are `{ output }` or the kind-tagged\n\
         // `{ error }` envelope (see infrastructure::generated::service_clients).\n",
    );
    if exposed.is_empty() {
        out.push_str(
            "\n/// No service declares `expose: true` (V0: one deployable, zero internal HTTP) — an empty\n/// router, mergeable into any state. Exposing a service is a reviewed spec change.\npub fn services_router<S: Clone + Send + Sync + 'static>() -> axum::Router<S> {\n    axum::Router::new()\n}\n",
        );
        return out;
    }
    out.push_str(
        "\nuse application::generated::services::*;\nuse axum::extract::State;\nuse axum::response::IntoResponse;\nuse axum::routing::post;\nuse axum::Json;\nuse infrastructure::generated::service_clients::{wire_error_status, WireCall, WireError, WireErrorEnvelope, WireOutput};\nuse std::sync::Arc;\n\n/// The local ports behind the exposed `/services/*` routes.\n#[derive(Clone)]\npub struct ServicesRouterState {\n",
    );
    for svc in &exposed {
        out.push_str(&format!("    pub {}: Arc<dyn {}Service>,\n", svc.name, svc.base));
    }
    out.push_str("}\n\n/// The exposed `/services/*` routes.\npub fn services_router(state: ServicesRouterState) -> axum::Router {\n    axum::Router::new()\n");
    for svc in &exposed {
        for op in &svc.ops {
            out.push_str(&format!(
                "        .route(\"{}\", post({}_{}))\n",
                svc_http_path(&svc.name, &op.name),
                svc.name,
                op.name
            ));
        }
    }
    out.push_str("        .with_state(state)\n}\n");
    for svc in &exposed {
        for op in &svc.ops {
            let input_ty = if op.input.is_some() {
                format!("{}{}Input", svc.base, pascal_snake(&op.name))
            } else {
                "serde_json::Value".to_string()
            };
            let call_args = if op.input.is_some() { "call.input, &call.meta" } else { "&call.meta" };
            out.push_str(&format!(
                "\nasync fn {name}_{op}(\n    State(state): State<ServicesRouterState>,\n    Json(call): Json<WireCall<{input_ty}>>,\n) -> axum::response::Response {{\n    match state.{name}.{op}({call_args}).await {{\n        Ok(output) => Json(WireOutput {{ output }}).into_response(),\n        Err(e) => {{\n            let error = WireError::from(&e);\n            let status = axum::http::StatusCode::from_u16(wire_error_status(&error))\n                .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);\n            (status, Json(WireErrorEnvelope {{ error }})).into_response()\n        }}\n    }}\n}}\n",
                name = svc.name,
                op = op.name,
                input_ty = input_ty,
                call_args = call_args,
            ));
        }
    }
    out
}

