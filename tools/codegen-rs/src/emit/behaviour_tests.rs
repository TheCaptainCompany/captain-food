use crate::*;

// ════════════════════════════════════════════════════════════════════════════════════════════════
// crates/application/src/generated/behaviour_tests.rs — the GENERATED behaviour-test suite
// (issue #24, codegen-roadmap item 2): one #[tokio::test] per tests.yaml Given/When/Then case, so
// the spec IS the executable suite. GIVEN seeds each fact onto its aggregate's stream (the
// TestBed mirrors read-model/PM-run effects), WHEN dispatches the command/event through the real
// write path (the same handlers/legs production uses), THEN asserts the appended facts equal the
// spec payloads (strict per-stream diff; `then: []` asserts a strict no-op) and `thrown` asserts
// the typed rejection code. The runtime the suite runs on is the hand-written
// `application::behaviour_support` (playbook: a failing behaviour test means fixing that runtime
// or this emitter — never the spec).
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// Aggregate-actor metadata the specs do not carry as data: the stream category (= the actor
/// name), the payload property that keys the aggregate's stream, and whether that id scalar is a
/// UUID (mapped through `support::uid`) or a plain string (used verbatim in the stream key).
/// Mirrors the domain `Aggregate` impls; a NEW aggregate actor must be added here — generation
/// panics otherwise, so the gap cannot pass silently.
pub(crate) const BT_AGGREGATES: &[(&str, &str, bool)] = &[
    ("RestaurantAccount", "restaurantAccountId", true),
    ("Restaurant", "restaurantId", true),
    ("Prospect", "restaurantId", true),
    ("Catalog", "catalogId", true),
    ("Customer", "customerId", true),
    ("Cart", "cartId", true),
    ("Order", "orderId", true),
    ("Payment", "paymentIntentId", false),
    ("DeliveryJob", "deliveryJobId", true),
    ("Rider", "riderId", true),
    ("DeliveryPartnerRegistration", "registrationId", true),
    ("Conversation", "orderId", true),   // id = orderId (a conversation's identity IS its order; #129)
    ("Reclamation", "reclamationId", true),   // id = reclamationId (its own identity; MULTIPLE claims per order; #151)
    ("CustomerCredit", "customerId", true),   // id = customerId (a per-customer store-credit ledger; #158)
    ("MailboxSupervision", "targetMessageId", true),   // id = the SUPERVISED row's messageId (#315)
    ("RestaurantMembership", "membershipId", true),   // #639 part C step 6-i (ADR-20260905-101349)
    ("RestaurantInvitation", "invitationId", true),   // #639 part C step 6-iv (ADR-20260905-101349 §2/§3)
    ("PlatformMembership", "platformMembershipId", true),   // #639 part C step 6-v (ADR-20260905-223957 §1)
];

pub(crate) fn bt_agg(actor: &str) -> Option<(&'static str, &'static str, bool)> {
    BT_AGGREGATES.iter().copied().find(|(a, _, _)| *a == actor)
}

/// event name → owning AGGREGATE actor (the stream its recorded fact lives on), built from
/// actors.yaml: an aggregate owns every event it emits (and every event it receives as an inbound
/// fact). Ambiguity (two aggregates claiming one event) is a generation error.
pub(crate) fn bt_event_owners(model: &Model) -> BTreeMap<String, &'static str> {
    let mut owners: BTreeMap<String, &'static str> = BTreeMap::new();
    for (agg, _, _) in BT_AGGREGATES {
        let def = model
            .defs
            .get("actors.yaml")
            .and_then(|m| m.get(*agg))
            .unwrap_or_else(|| panic!("behaviour-tests: actors.yaml#/{} missing", agg));
        let receives = def.get("receives").and_then(|r| r.as_sequence()).cloned().unwrap_or_default();
        let mut claim = |event: String| {
            if let Some(prev) = owners.get(&event) {
                assert_eq!(
                    prev, agg,
                    "behaviour-tests: event {} claimed by two aggregates ({} and {})",
                    event, prev, agg
                );
            }
            owners.insert(event, agg);
        };
        for entry in &receives {
            if let Some(msg) = entry.get("message").and_then(|m| m.get("$ref")).and_then(|x| x.as_str()) {
                if msg.starts_with("events.yaml#/") {
                    if let Some(name) = ref_name(msg) {
                        claim(name);
                    }
                }
            }
            for e in entry.get("emits").and_then(|e| e.as_sequence()).cloned().unwrap_or_default() {
                if let Some(name) = e.get("$ref").and_then(|x| x.as_str()).and_then(ref_name) {
                    claim(name);
                }
            }
        }
    }
    owners
}

/// Is `name` a scalars.yaml def (vs an entities.yaml value object)?
pub(crate) fn bt_is_scalar(model: &Model, name: &str) -> bool {
    model.defs.get("scalars.yaml").map(|m| m.get(name).is_some()).unwrap_or(false)
}

/// Render one yaml float/int as a Rust f64 literal (always with a decimal point).
pub(crate) fn bt_f64_lit(v: &Value) -> String {
    if let Some(i) = v.as_i64() {
        return format!("{}.0", i);
    }
    let f = v.as_f64().expect("behaviour-tests: numeric literal expected");
    let s = format!("{}", f);
    if s.contains('.') || s.contains('e') {
        s
    } else {
        format!("{}.0", s)
    }
}

/// Render a scalars.yaml-typed sample value as its Rust expression (`sc::` qualified).
pub(crate) fn bt_scalar_expr(model: &Model, name: &str, val: &Value, path: &str) -> String {
    let def = model
        .defs
        .get("scalars.yaml")
        .and_then(|m| m.get(name))
        .unwrap_or_else(|| panic!("behaviour-tests: scalars.yaml#/{} missing ({})", name, path));
    if def.get("enum").is_some() {
        let v = val.as_str().unwrap_or_else(|| panic!("behaviour-tests: {}: enum value must be a string", path));
        return format!("sc::{}::{}", name, v);
    }
    let ty = def.get("type").and_then(|t| t.as_str()).unwrap_or("string");
    if def.get("format").and_then(|f| f.as_str()) == Some("uuid") {
        let s = val.as_str().unwrap_or_else(|| panic!("behaviour-tests: {}: uuid sample must be a string", path));
        return format!("sc::{}(support::uid(\"{}\"))", name, rust_string_lit(s));
    }
    match ty {
        "integer" => format!("sc::{}({})", name, val.as_i64().unwrap_or_else(|| panic!("behaviour-tests: {}: integer expected", path))),
        "number" => format!("sc::{}({})", name, bt_f64_lit(val)),
        _ => {
            let s = val.as_str().unwrap_or_else(|| panic!("behaviour-tests: {}: string expected", path));
            format!("sc::{}(\"{}\".into())", name, rust_string_lit(s))
        }
    }
}

/// Render one property VALUE (no optionality wrapping) as a Rust expression. `ctx` is the spec
/// file the surrounding schema came from (file-relative `$ref`s resolve against it).
pub(crate) fn bt_value_expr(model: &Model, ctx: &str, node: &Value, val: &Value, path: &str) -> String {
    if let Some(rf) = node.get("$ref").and_then(|x| x.as_str()) {
        let name = ref_name(rf).unwrap_or_else(|| panic!("behaviour-tests: {}: malformed $ref", path));
        if bt_is_scalar(model, &name) {
            return bt_scalar_expr(model, &name, val, path);
        }
        let def = resolve_ref(model, rf, ctx)
            .unwrap_or_else(|| panic!("behaviour-tests: {}: unresolvable $ref {}", path, rf));
        let next_ctx = match rf.split_once("#/") {
            Some((f, _)) if !f.is_empty() => f.to_string(),
            _ => ctx.to_string(),
        };
        let module = match next_ctx.as_str() {
            "entities.yaml" => "ent",
            "commands.yaml" => "cmds",
            "events.yaml" => "evs",
            other => panic!("behaviour-tests: {}: struct $ref into unsupported file {}", path, other),
        };
        return bt_struct_expr(model, &next_ctx, &format!("{}::{}", module, name), def, val, path);
    }
    match node.get("type").and_then(|t| t.as_str()) {
        Some("array") => {
            let items = node.get("items").unwrap_or_else(|| panic!("behaviour-tests: {}: array without items", path));
            let seq = val.as_sequence().unwrap_or_else(|| panic!("behaviour-tests: {}: sequence expected", path));
            let parts: Vec<String> = seq
                .iter()
                .enumerate()
                .map(|(i, item)| bt_value_expr(model, ctx, items, item, &format!("{}[{}]", path, i)))
                .collect();
            format!("vec![{}]", parts.join(", "))
        }
        Some("string") => format!("\"{}\".to_string()", rust_string_lit(val.as_str().unwrap_or_else(|| panic!("behaviour-tests: {}: string expected", path)))),
        Some("integer") => format!("{}", val.as_i64().unwrap_or_else(|| panic!("behaviour-tests: {}: integer expected", path))),
        Some("boolean") => format!("{}", val.as_bool().unwrap_or_else(|| panic!("behaviour-tests: {}: boolean expected", path))),
        Some("number") => bt_f64_lit(val),
        other => panic!("behaviour-tests: {}: unsupported inline type {:?}", path, other),
    }
}

/// Render a sample `data` object as a Rust struct literal for a spec node with
/// `properties`/`required` — properties in spec order, absent optionals `None`, absent arrays
/// `Vec::new()` (the same optionality rules the struct emitters use).
pub(crate) fn bt_struct_expr(model: &Model, ctx: &str, qualified: &str, def: &Value, val: &Value, path: &str) -> String {
    let props = def
        .get("properties")
        .and_then(|p| p.as_mapping())
        .unwrap_or_else(|| panic!("behaviour-tests: {}: schema has no properties", path));
    let required: HashSet<&str> = def
        .get("required")
        .and_then(|r| r.as_sequence())
        .map(|s| s.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let obj = val.as_mapping();
    let mut fields = Vec::new();
    for (k, pnode) in props {
        let prop = k.as_str().expect("property key");
        let field = rust_ident(&snake_field(prop));
        let is_array = pnode.get("type").and_then(|t| t.as_str()) == Some("array");
        let optional = !required.contains(prop) || pnode.get("nullable").and_then(|n| n.as_bool()).unwrap_or(false);
        let sample = obj.and_then(|o| o.get(Value::String(prop.to_string())));
        let expr = match sample {
            Some(v) if !v.is_null() => {
                let inner = bt_value_expr(model, ctx, pnode, v, &format!("{}.{}", path, prop));
                if optional && !is_array {
                    format!("Some({})", inner)
                } else {
                    inner
                }
            }
            _ => {
                if is_array {
                    "Vec::new()".to_string()
                } else if optional {
                    "None".to_string()
                } else {
                    panic!("behaviour-tests: {}.{}: required property missing from sample data", path, prop)
                }
            }
        };
        fields.push(format!("{}: {}", field, expr));
    }
    format!("{} {{ {} }}", qualified, fields.join(", "))
}

/// snake_case test/fixture identifier from a PascalCase key.
pub(crate) fn bt_fn_name(key: &str) -> String {
    snake_field(key).trim_start_matches('_').to_string()
}

/// The stream EXPRESSION (Rust) for an aggregate + spec string id.
pub(crate) fn bt_stream_expr(agg: &str, uuid_keyed: bool, id: &str) -> String {
    if uuid_keyed {
        format!("format!(\"{}-{{}}\", support::uid(\"{}\"))", agg, rust_string_lit(id))
    } else {
        format!("\"{}-{}\".to_string()", agg, rust_string_lit(id))
    }
}

/// Resolve the stream of one event instance: owner aggregate + id (from the payload's id property,
/// else the test's running context for that aggregate, else the FIXTURE POOL's unique id). Updates
/// the context.
pub(crate) fn bt_event_stream(
    owners: &BTreeMap<String, &'static str>,
    pool: &BTreeMap<&'static str, BTreeSet<String>>,
    ctx: &mut BTreeMap<&'static str, String>,
    event: &str,
    data: Option<&Value>,
    where_: &str,
) -> (&'static str, String) {
    let agg = owners
        .get(event)
        .copied()
        .unwrap_or_else(|| panic!("behaviour-tests: {}: no aggregate owns event {}", where_, event));
    let (_, id_prop, _) = bt_agg(agg).expect("aggregate meta");
    let id = data
        .and_then(|d| d.get(id_prop))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| ctx.get(agg).cloned())
        .or_else(|| {
            let ids = pool.get(agg)?;
            if ids.len() == 1 {
                ids.iter().next().cloned()
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("behaviour-tests: {}: cannot key the {} stream for {}", where_, agg, event));
    ctx.insert(agg, id.clone());
    (agg, id)
}

/// Commands whose application handler takes the EVALUATION INSTANT as a parameter (RSO-1,
/// DECISIONS §43; "now is a parameter" — the `sms_guard.rs` precedent: a handler reading the
/// system clock internally cannot have both edges of a time boundary asserted). A command listed
/// here gets a `when_at` binding emitted before its call, and its `bt_command_call` arm threads
/// it; `when.at` on any OTHER command is an emitter panic, never a silent no-op (the #413
/// "silently invisible" defect class).
pub(crate) const BT_CLOCK_CONSUMING: &[&str] = &["PlaceOrder", "RestrictRider"];

/// The FIXED default `when.at` (a Tuesday noon, UTC): every clock-consuming test is deterministic
/// even when it declares no instant. Documented in specs/tests.yaml's header — keep the two in
/// step, and keep fixtures that pin `evaluatedAt` (e.g. `paymentIntentCreated`) on this value.
pub(crate) const BT_DEFAULT_WHEN_AT: &str = "2026-01-06T12:00:00Z";

/// Commands whose application handler takes a boolean CONFIGURATION GATE as a parameter (RSO-1
/// Phase 4, `when.gates` — beck's ruling: no global/env read inside a handler, so the in-process
/// suite stays order-independent): `(command, configuration key, the Rust binding the call arm
/// reads)`. The binding is ALWAYS emitted for a listed command — `true` when the test's
/// `when.gates` names the key, else the key's spec default — and a gate named on any OTHER
/// command (or an unknown key) is an emitter panic, never a silent no-op (the #413 "silently
/// invisible" defect class, the same posture as `when.at`).
pub(crate) const BT_GATE_CONSUMING: &[(&str, &str, &str)] = &[
    ("PlaceOrder", "ENFORCE_SERVICE_HOURS_GUARD", "enforce_service_hours_guard"),
    // ADR-20260906-192007 D-F/D-G: the signed-quote write door's own interlocked pair -- both
    // must be named in the SAME test's `when.gates` to open the guard (D-B's interlock), so a
    // scenario names both or neither. `TestPlaceOrderRejectsQuoteVerificationFailed` is the one
    // scenario that opens both; every other PlaceOrder test gets `false` for both (the spec
    // defaults), exactly `quote_guard_closed()`'s own posture.
    ("PlaceOrder", "RUN_QUOTE_REQUIRED_ON_PLACE_ORDER", "run_quote_required_on_place_order"),
    ("PlaceOrder", "RUN_FOLD_PRICED_CART_READ", "run_fold_priced_cart_read_for_quote_guard"),
    // #639 part C step 4-iii-A (ADR-20260904-152807 §7): the restrict door's release gate, read
    // at the WRITE door only — `reinstateRider` never consumes it (ReinstateRider is absent here
    // on purpose).
    ("RestrictRider", "RUN_RIDER_RESTRICTION_DOOR", "run_rider_restriction_door"),
    // #639 part C step 6-i (ADR-20260905-101349 §6): the staff access grant door's release gate,
    // read at the WRITE door only — `revokeRestaurantAccess` never consumes it (absent here on
    // purpose, the `RestrictRider`/`ReinstateRider` asymmetry).
    ("GrantRestaurantAccess", "RUN_MEMBER_ACCESS_GRANT", "run_member_access_grant"),
    // Round 2 (ADR-20260905-101349 §2 amendment): the invitation-accept grant leg is its OWN
    // command now, gated by the SAME door as the ADMIN grant above.
    ("GrantRestaurantAccessByInvitation", "RUN_MEMBER_ACCESS_GRANT", "run_member_access_grant"),
    // #639 part C step 6-ii (ADR-20260905-101349 §6): the member sign-in door's release gate,
    // read at BOTH mutation handlers (unlike the grant door's asymmetry) -- OFF refuses before
    // the identity provider is touched at all.
    ("RequestMemberSignInLink", "RUN_MEMBER_SIGN_IN_DOOR", "run_member_sign_in_door"),
    ("ConfirmMemberSignIn", "RUN_MEMBER_SIGN_IN_DOOR", "run_member_sign_in_door"),
    // #639 part C step 6-iv (ADR-20260905-101349 §2/§3): the invitation door's release gate, read
    // at the WRITE door only -- `revokeRestaurantInvitation` never consumes it (the
    // RestrictRider/ReinstateRider asymmetry).
    ("InviteRestaurantMember", "RUN_RESTAURANT_INVITATION", "run_restaurant_invitation"),
    // #639 part C step 6-v (ADR-20260905-223957 §5): the platform grant door's release gate, read
    // at the write door only -- no revoke command exists yet to be asymmetric with.
    ("GrantPlatformAccess", "RUN_PLATFORM_ACCESS_GRANT", "run_platform_access_grant"),
    // #639 part C step 6-iii (ADR-20260906-023825): the ADMIN sign-in door's release gate, read at
    // BOTH mutation handlers (the RUN_MEMBER_SIGN_IN_DOOR shape) -- OFF refuses before the identity
    // provider is touched at all.
    ("RequestAdminSignInLink", "RUN_ADMIN_SIGN_IN_DOOR", "run_admin_sign_in_door"),
    ("ConfirmAdminSignIn", "RUN_ADMIN_SIGN_IN_DOOR", "run_admin_sign_in_door"),
];

/// EVENT receives whose application recorder takes a boolean CONFIGURATION GATE as a parameter
/// (#167 — the acceptance-timeout ACTION gate, read at delivery time): same contract as
/// [`BT_GATE_CONSUMING`] — the binding is ALWAYS emitted for a listed event (`true` when the
/// test's `when.gates` names the key, else the key's spec default), and a gate named on any
/// other event is an emitter panic, never a silent no-op (the #413 defect class).
pub(crate) const BT_GATE_CONSUMING_EVENTS: &[(&str, &str, &str)] =
    &[("OrderAcceptanceTimedOut", "ENFORCE_ACCEPTANCE_TIMEOUT", "enforce_acceptance_timeout")];

/// The dispatch expression for a WHEN command (a `cmd` binding is in scope; for
/// [`BT_CLOCK_CONSUMING`] commands a `when_at: chrono::DateTime<chrono::Utc>` binding too).
pub(crate) fn bt_command_call(cmd: &str) -> String {
    let snake = match cmd {
        "ConfigureGoogleBusinessProfileOrderLink" => "configure_gbp_order_link".to_string(),
        "VerifyGoogleBusinessProfileOrderLink" => "verify_gbp_order_link".to_string(),
        _ => bt_fn_name(cmd),
    };
    match cmd {
        "PlaceOrder" => "crate::commands::place_order(&bed.store, &bed.catalogs, &bed.payments, &bed.payment_pm, cmd, None, &support::actor(), when_at, enforce_service_hours_guard, &support::quote_guard_for(run_quote_required_on_place_order, run_fold_priced_cart_read_for_quote_guard)).await".to_string(),
        // The Art. 11 log's decidedAt/effectiveAt are BOTH server-set (ADR-20260904-081527 §5) —
        // "now is a parameter" (RSO-1), never a system-clock read inside the handler.
        "RestrictRider" => "crate::commands::restrict_rider(&bed.store, cmd, &support::actor(), when_at, run_rider_restriction_door).await".to_string(),
        "ApproveRefund" => "crate::process_managers::refund::approve_refund(&bed.store, &bed.refund_pm, &bed.payments, cmd, &support::actor()).await".to_string(),
        "DenyRefund" => "crate::process_managers::refund::deny_refund(&bed.store, &bed.refund_pm, cmd, &support::actor()).await".to_string(),
        "RegisterRestaurant" | "CreateCatalog" | "AddProduct" | "UpdateProduct" | "MarkRestaurantAsFavorite" => {
            format!("crate::commands::{}(&bed.store, &bed.restaurants, cmd, &support::actor()).await", snake)
        }
        // Slug uniqueness is arbitrated by a write-side reservation (ADR-20260728-011344 D3), never by
        // the eventually-consistent Restaurant projection -- so the handler takes that port, not a
        // read repo.
        "ConfigureRestaurantSlug" => {
            format!("crate::commands::{}(&bed.store, &bed.slugs, cmd, &support::actor()).await", snake)
        }
        // A rider's login credential is bound ONCE, population-wide, by the write-side
        // `auth_subject_reservations` table (#639 part C step 2a, #794) -- the same arbitration as
        // the storefront slug, so the handler takes that port; the TestBed fake's "already-bound"
        // sentinel drives the rejection case.
        "RegisterRider" => {
            format!("crate::commands::{}(&bed.store, &bed.auth_subjects, cmd, &support::actor()).await", snake)
        }
        // #639 part C step 6-i: the SAME 2a reservation table, gated at the write door
        // (RUN_MEMBER_ACCESS_GRANT, checked FIRST, before the store is even read).
        "GrantRestaurantAccess" => {
            format!("crate::commands::{}(&bed.store, &bed.auth_subjects, cmd, &support::actor(), run_member_access_grant).await", snake)
        }
        // The requeue consults/flips the inbound_messages row through the MailboxRequeue port (#315);
        // the TestBed fake's sentinels drive the poisoned / settled / unknown cases.
        "RequeueMailboxMessage" => {
            format!("crate::commands::{}(&bed.store, &bed.mailbox_requeue, cmd, &support::actor()).await", snake)
        }
        "ClaimRestaurantListing" | "OptOutRestaurantListing" => {
            format!("crate::commands::{}(&bed.store, &bed.ownership, cmd, &support::actor()).await", snake)
        }
        "VerifyGoogleBusinessProfileOrderLink" => {
            format!("crate::commands::{}(&bed.store, &bed.probe, cmd, &support::actor()).await", snake)
        }
        // A catalog slug is a PATH inside one storefront, not a global host, so its per-restaurant
        // uniqueness is a read-model check against the sibling catalogs -- not the write-side
        // reservation the restaurant HOST needs.
        "AddCartLine" | "ChangeCartLineQuantity" | "ConfigureCatalogSlug" => {
            format!("crate::commands::{}(&bed.store, &bed.catalogs, cmd, &support::actor()).await", snake)
        }
        "RecordProspectContact" => {
            format!("crate::commands::{}(&bed.store, &bed.prospection, cmd, &support::actor()).await", snake)
        }
        "RequestPhoneVerification" | "ConfirmEmailVerification" => {
            format!("crate::commands::{}(&bed.store, &bed.identity, cmd, &support::actor()).await", snake)
        }
        // VerifyPhone additionally parks the provider session for cookie pickup (#112).
        "VerifyPhone" => {
            format!("crate::commands::{}(&bed.store, &bed.identity, &bed.customers, &bed.auth_sessions, cmd, &support::actor()).await", snake)
        }
        "RequestEmailVerification" | "RequestPhoneChange" | "ConfirmPhoneChange" => {
            format!("crate::commands::{}(&bed.store, &bed.identity, &bed.customers, cmd, &support::actor()).await", snake)
        }
        // The rider sign-in door (#639 part C step 2c-i): the code request is the customer's send
        // leg verbatim (it never consults the rider read model); the confirmation identifies
        // through the bed's SpecRiders bridge, parks the post-stamp session, and names the support
        // route the bed carries (SpecSupportContact). The `None` here is the X-SESSION-ID slot
        // (envelope data, not payload -- the place_order shape); `emit_behaviour_tests` fills it
        // per case, since a confirm without a session is REFUSED (#852 review, B1).
        "RequestRiderSignInCode" => {
            format!("crate::commands::{}(&bed.store, &bed.identity, cmd, &support::actor()).await", snake)
        }
        "ConfirmRiderSignIn" => {
            format!("crate::commands::{}(&bed.store, &bed.identity, &bed.riders, &bed.auth_sessions, bed.support_contact.0.as_ref(), cmd, None, &support::actor()).await", snake)
        }
        // The member sign-in door (#639 part C step 6-ii): the email transposition of the rider
        // pair, plus the door gate both handlers consume (unlike the grant door's asymmetry).
        "RequestMemberSignInLink" => {
            format!("crate::commands::{}(&bed.store, &bed.identity, cmd, &support::actor(), run_member_sign_in_door).await", snake)
        }
        "ConfirmMemberSignIn" => {
            format!("crate::commands::{}(&bed.store, &bed.identity, &bed.members, &bed.auth_sessions, bed.support_contact.0.as_ref(), cmd, None, &support::actor(), run_member_sign_in_door).await", snake)
        }
        // The roster and the invitation (#639 part C step 6-iv): InviteRestaurantMember takes the
        // door gate; RevokeRestaurantInvitation falls through to the default `(store, cmd, actor)`
        // arm below (never gated); AcceptRestaurantInvitation verifies the magic-link token
        // through the SAME identity port ConfirmMemberSignIn uses.
        "InviteRestaurantMember" => {
            format!("crate::commands::{}(&bed.store, cmd, &support::actor(), run_restaurant_invitation).await", snake)
        }
        "AcceptRestaurantInvitation" => {
            format!("crate::commands::{}(&bed.store, &bed.identity, cmd, &support::actor()).await", snake)
        }
        // Round 2 (ADR-20260905-101349 §2 amendment): the SECOND command of the two-lane accept,
        // its own PUBLIC door -- verifies its OWN token through the SAME identity port, then reads
        // across to the RestaurantInvitation stream and the 2a reservation table.
        "GrantRestaurantAccessByInvitation" => {
            format!("crate::commands::{}(&bed.store, &bed.identity, &bed.auth_subjects, cmd, &support::actor(), run_member_access_grant).await", snake)
        }
        // The platform grant and the ADMIN seam binding (#639 part C step 6-v,
        // ADR-20260905-223957 §1): the handler checks the `PlatformMember` bridge (the arbiter,
        // never the 2a reservation table -- ADMIN is not a PrincipalKind) before appending.
        "GrantPlatformAccess" => {
            format!("crate::commands::{}(&bed.store, &bed.platform_members, cmd, &support::actor(), run_platform_access_grant).await", snake)
        }
        // The ADMIN sign-in door (#639 part C step 6-iii, ADR-20260906-023825): the
        // ConfirmMemberSignIn shape, transposed -- the platform grant bridge instead of the
        // Member bridge, both gated at BOTH handlers (the RestaurantMembership sign-in pair's
        // own asymmetry-free shape).
        "RequestAdminSignInLink" => {
            format!("crate::commands::{}(&bed.store, &bed.identity, cmd, &support::actor(), run_admin_sign_in_door).await", snake)
        }
        "ConfirmAdminSignIn" => {
            format!("crate::commands::{}(&bed.store, &bed.identity, &bed.platform_members, &bed.auth_sessions, bed.support_contact.0.as_ref(), cmd, None, &support::actor(), run_admin_sign_in_door).await", snake)
        }
        _ => format!("crate::commands::{}(&bed.store, cmd, &support::actor()).await", snake),
    }
}

/// The dispatch expression for a WHEN event on a PROCESS MANAGER (an `ev` binding is in scope).
pub(crate) fn bt_pm_event_call(pm: &str, event: &str) -> String {
    match (pm, event) {
        ("PlaceOrderProcess", "PaymentAuthorized") => "crate::process_managers::place_order::on_payment_authorized(&bed.store, &bed.payment_pm, &ev, &support::envelope()).await".into(),
        ("PlaceOrderProcess", "PaymentFailed") => "crate::process_managers::place_order::on_payment_failed(&bed.payment_pm, &ev, &support::envelope()).await".into(),
        ("PaymentSettlementProcess", "OrderDelivered") => "crate::process_managers::payment_settlement::on_order_delivered(&bed.store, &bed.orders, &bed.payments, &ev, &support::envelope()).await".into(),
        ("PaymentSettlementProcess", "OrderRejectedByRestaurant") => "crate::process_managers::payment_settlement::on_order_rejected(&bed.orders, &bed.payments, &ev, &support::envelope()).await".into(),
        ("PaymentSettlementProcess", "OrderCancelledByCustomer") => "crate::process_managers::payment_settlement::on_order_cancelled_by_customer(&bed.orders, &bed.payments, &ev, &support::envelope()).await".into(),
        ("PaymentSettlementProcess", "OrderCancelledByRestaurant") => "crate::process_managers::payment_settlement::on_order_cancelled_by_restaurant(&bed.orders, &bed.payments, &ev, &support::envelope()).await".into(),
        ("RefundProcess", "OrderRejectedByRestaurant") => "crate::process_managers::refund::on_order_rejected(&bed.store, &bed.refund_pm, &bed.orders, &ev, &support::envelope()).await".into(),
        ("RefundProcess", "OrderCancelledByCustomer") => "crate::process_managers::refund::on_order_cancelled_by_customer(&bed.store, &bed.refund_pm, &bed.orders, &ev, &support::envelope()).await".into(),
        ("RefundProcess", "OrderCancelledByRestaurant") => "crate::process_managers::refund::on_order_cancelled_by_restaurant(&bed.store, &bed.refund_pm, &bed.orders, &ev, &support::envelope()).await".into(),
        ("RefundProcess", "RefundRequested") => "crate::process_managers::refund::on_refund_requested(&bed.store, &bed.refund_pm, &bed.orders, &ev, &support::envelope()).await".into(),
        ("RefundProcess", "PaymentRefunded") => "crate::process_managers::refund::on_payment_refunded(&bed.refund_pm, &ev).await".into(),
        ("CartBindingProcess", "CustomerIdentified") => "crate::process_managers::cart_binding::on_customer_identified(&bed.store, &bed.cart_pm, &bed.carts, &ev, &support::envelope()).await".into(),
        ("DeliveryDispatchProcess", "OrderMarkedReady") => "crate::process_managers::delivery_dispatch::on_order_marked_ready(&bed.store, &bed.dispatch_pm, &bed.orders, &bed.delivery, &bed.dispatch_config, &ev, &support::envelope()).await".into(),
        ("DeliveryDispatchProcess", "DeliveryAcceptedByPartner") => "crate::process_managers::delivery_dispatch::on_delivery_accepted_by_partner(&bed.dispatch_pm, &ev).await".into(),
        ("DeliveryDispatchProcess", "DeliveryRejectedByPartner") => "crate::process_managers::delivery_dispatch::on_delivery_rejected_by_partner(&bed.store, &bed.dispatch_pm, &bed.delivery, &bed.dispatch_config, &ev, &support::envelope()).await".into(),
        ("DeliveryDispatchProcess", "DeliveryEscalationRequested") => "crate::process_managers::delivery_dispatch::on_delivery_escalation_requested(&bed.store, &bed.dispatch_pm, &bed.delivery, &bed.dispatch_config, &ev, &support::envelope()).await".into(),
        ("DeliveryDispatchProcess", "DeliveryOfferTimedOut") => "crate::process_managers::delivery_dispatch::on_delivery_offer_timed_out(&bed.store, &bed.dispatch_pm, &bed.delivery, &bed.dispatch_config, &ev, &support::envelope()).await".into(),
        ("DeliveryDispatchProcess", "DeliveryStatusUpdated") => "crate::process_managers::delivery_dispatch::on_delivery_status_updated(&bed.store, &bed.dispatch_pm, &ev, &support::envelope()).await".into(),
        ("DeliveryDispatchProcess", "DeliveryCompleted") => "crate::process_managers::delivery_dispatch::on_delivery_completed(&bed.store, &bed.dispatch_pm, &ev, &support::envelope()).await".into(),
        ("ReclamationProcess", "ReclamationResolved") => "crate::process_managers::reclamation::on_reclamation_resolved(&bed.store, &bed.refund_pm, &bed.orders, &bed.payments, &ev, &support::envelope()).await".into(),
        _ => panic!("behaviour-tests: no dispatch entry for process-manager {} ← event {} — extend bt_pm_event_call", pm, event),
    }
}

/// Emit `crates/application/src/generated/behaviour_tests.rs`.
pub(crate) fn emit_behaviour_tests(model: &Model) -> String {
    let owners = bt_event_owners(model);
    // (actor, inbox message) → reminder names its receive `schedules:` (ADR-20260731-214500 §2):
    // every curated test whose WHEN hits such a receive also asserts the scheduling effect —
    // schedule at +window, then re-declare under the reminder's DECLARED policy (`in-place`
    // postpones the row, ADR-20260731-150500; `keep` leaves the first scheduled_at, #167) — as
    // generated code.
    let reminder_policy: BTreeMap<(String, String), String> = parse_reminders(model)
        .into_iter()
        .map(|r| {
            let policy = r.reschedule.clone().unwrap_or_else(|| "in-place".to_string());
            ((r.actor, r.name), policy)
        })
        .collect();
    let mut schedules_of: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for a in &parse_actors(model) {
        for e in &a.receives {
            if e.schedules.is_empty() {
                continue;
            }
            let msg = match reminder_ref_parts(&e.message_ref) {
                Some((_, rname)) => {
                    reminder_payload_event(model, &e.message_ref).unwrap_or(rname)
                }
                None => match ref_name(&e.message_ref) {
                    Some(m) => m,
                    None => continue,
                },
            };
            let names: Vec<String> = e
                .schedules
                .iter()
                .filter_map(|r| reminder_ref_parts(r))
                .map(|(_, n)| n)
                .collect();
            schedules_of.entry((a.name.clone(), msg)).or_default().extend(names);
        }
    }
    let tests_doc = model.defs.get("tests.yaml").expect("tests.yaml");
    let fixtures = tests_doc.get("fixtures").and_then(|f| f.as_mapping()).cloned().unwrap_or_default();
    let tests = tests_doc.get("tests").and_then(|t| t.as_mapping()).cloned().unwrap_or_default();

    // The fixture pool's ids per aggregate — the fallback when an event payload does not carry its
    // aggregate's id (e.g. RefundApproved on the Payment stream).
    let mut pool: BTreeMap<&'static str, BTreeSet<String>> = BTreeMap::new();
    for (_, fx) in &fixtures {
        let event = match fx.get("type").and_then(|t| t.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name) {
            Some(e) => e,
            None => continue,
        };
        if let Some(agg) = owners.get(&event) {
            let (_, id_prop, _) = bt_agg(agg).expect("aggregate meta");
            if let Some(id) = fx.get("data").and_then(|d| d.get(id_prop)).and_then(|v| v.as_str()) {
                pool.entry(agg).or_default().insert(id.to_string());
            }
        }
    }

    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/tests.yaml — do not edit by hand.\n\
         // The behaviour suite (issue #24): one #[tokio::test] per Given/When/Then case — the spec IS\n\
         // the test suite. Runs on the hand-written `application::behaviour_support` runtime; when a\n\
         // test fails, fix that runtime or the emitter (tools/codegen-rs), never this file or the spec.\n\
         #![allow(dead_code)]\n\n\
         use domain::generated::commands as cmds;\n\
         use domain::generated::entities as ent;\n\
         use domain::generated::events as evs;\n\
         use domain::generated::events::DomainEvent;\n\
         use domain::generated::scalars as sc;\n\n\
         use crate::behaviour_support::{self as support, TestBed};\n\n",
    );

    // ── fixture constructors ──────────────────────────────────────────────────────────────────
    for (k, fx) in &fixtures {
        let name = k.as_str().expect("fixture key");
        let event = fx
            .get("type")
            .and_then(|t| t.get("$ref"))
            .and_then(|x| x.as_str())
            .and_then(ref_name)
            .unwrap_or_else(|| panic!("behaviour-tests: fixtures/{}: malformed type", name));
        let def = resolve_ref(model, &format!("events.yaml#/{}", event), "tests.yaml")
            .unwrap_or_else(|| panic!("behaviour-tests: events.yaml#/{} missing", event));
        let data = fx.get("data").cloned().unwrap_or(Value::Null);
        let literal = bt_struct_expr(model, "events.yaml", &format!("evs::{}", event), def, &data, &format!("fixtures/{}", name));
        out.push_str(&format!(
            "/// tests.yaml#/fixtures/{} — events.yaml#/{}\nfn fx_{}() -> DomainEvent {{\n    DomainEvent::{}({})\n}}\n\n",
            name, event, bt_fn_name(name), event, literal
        ));
    }

    // ── the spec read-model baseline (fixture pool → canned rows the sagas/pricing read) ──────
    out.push_str(
        "/// Read-model baseline canned from the fixture pool: the catalog offers pricing reads and\n\
         /// the canonical OrderTracking rows the saga legs read (`read_order`) — state the spec's\n\
         /// GIVEN (an event list) cannot express but its cases assume.\n\
         async fn spec_baseline(bed: &TestBed) {\n",
    );
    for (k, fx) in &fixtures {
        let name = k.as_str().expect("fixture key");
        let event = fx.get("type").and_then(|t| t.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name).unwrap_or_default();
        if event == "OrderPlaced" {
            out.push_str(&format!(
                "    if let DomainEvent::OrderPlaced(op) = fx_{}() {{\n        bed.orders.upsert(support::tracking_row_from_order_placed(&op));\n    }}\n",
                bt_fn_name(name)
            ));
        }
        if event == "ProductAdded" || event == "CatalogImported" {
            out.push_str(&format!(
                "    support::install_catalog_offers(bed, &fx_{}());\n",
                bt_fn_name(name)
            ));
        }
    }
    out.push_str("}\n\n");

    // ── one test per case ─────────────────────────────────────────────────────────────────────
    for (k, t) in &tests {
        let key = k.as_str().expect("test key");
        let title = t.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let rules: Vec<String> = t
            .get("rules")
            .and_then(|r| r.as_sequence())
            .map(|s| s.iter().filter_map(|v| v.get("$ref").and_then(|x| x.as_str()).and_then(ref_name)).collect())
            .unwrap_or_default();
        let actor_ref = t
            .get("actor")
            .and_then(|a| a.get("$ref"))
            .and_then(|x| x.as_str())
            .unwrap_or_else(|| panic!("behaviour-tests: {}: missing actor", key));
        let actor = ref_name(actor_ref).unwrap_or_else(|| panic!("behaviour-tests: {}: malformed actor ref", key));
        let is_pm = actor_ref.starts_with("processmanager.yaml#/");
        let mut ctx: BTreeMap<&'static str, String> = BTreeMap::new();

        out.push_str(&format!("/// tests.yaml#/tests/{} — \"{}\"\n", key, rust_string_lit(title)));
        if !rules.is_empty() {
            out.push_str(&format!("/// rules: {}\n", rules.join(", ")));
        }
        out.push_str("#[tokio::test]\n");
        out.push_str(&format!("async fn {}() {{\n", bt_fn_name(key)));
        out.push_str("    let bed = TestBed::new();\n    spec_baseline(&bed).await;\n");

        // GIVEN — group consecutive fixtures of the same stream into one seed call.
        let given: Vec<String> = t
            .get("given")
            .and_then(|g| g.as_sequence())
            .map(|s| {
                s.iter()
                    .filter_map(|v| v.get("$ref").and_then(|x| x.as_str()))
                    .map(|r| r.trim_start_matches("#/fixtures/").to_string())
                    .collect()
            })
            .unwrap_or_default();
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for fx_name in &given {
            let fx = fixtures
                .get(Value::String(fx_name.clone()))
                .unwrap_or_else(|| panic!("behaviour-tests: {}: unknown fixture {}", key, fx_name));
            let event = fx.get("type").and_then(|t| t.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name).unwrap();
            let (agg, id) = bt_event_stream(&owners, &pool, &mut ctx, &event, fx.get("data"), &format!("{}/given", key));
            let (_, _, uuid_keyed) = bt_agg(agg).expect("aggregate meta");
            let stream = bt_stream_expr(agg, uuid_keyed, &id);
            let call = format!("fx_{}()", bt_fn_name(fx_name));
            match groups.last_mut() {
                Some((s, evs_)) if *s == stream => evs_.push(call),
                _ => groups.push((stream, vec![call])),
            }
        }
        for (stream, evs_) in &groups {
            out.push_str(&format!("    bed.seed(&{}, vec![{}]).await;\n", stream, evs_.join(", ")));
        }
        out.push_str("    let before = bed.snapshot();\n");

        // WHEN
        let when = t.get("when").unwrap_or_else(|| panic!("behaviour-tests: {}: missing when", key));
        let wref = when
            .get("type")
            .and_then(|ty| ty.get("$ref"))
            .and_then(|x| x.as_str())
            .unwrap_or_else(|| panic!("behaviour-tests: {}: malformed when", key));
        let msg = ref_name(wref).unwrap();
        let wdata = when.get("data").cloned().unwrap_or(Value::Null);
        if wref.starts_with("commands.yaml#/") {
            let def = resolve_ref(model, wref, "tests.yaml").unwrap();
            let literal = bt_struct_expr(model, "commands.yaml", &format!("cmds::{}", msg), def, &wdata, &format!("{}/when", key));
            out.push_str(&format!("    let cmd = {};\n", literal));
            // `when.at` — the evaluation instant (RSO-1, DECISIONS §43). Only meaningful on a
            // clock-consuming command; anywhere else it would be silently ignored, so refuse.
            let when_at = when.get("at").and_then(|v| v.as_str());
            let clock_consuming = BT_CLOCK_CONSUMING.contains(&msg.as_str());
            if when_at.is_some() && !clock_consuming {
                panic!(
                    "behaviour-tests: {}: `when.at` is set but '{}' is not clock-consuming ({:?}) — the value would assert nothing; extend BT_CLOCK_CONSUMING and its bt_command_call arm when the handler takes the instant",
                    key, msg, BT_CLOCK_CONSUMING
                );
            }
            if clock_consuming {
                out.push_str(&format!(
                    "    let when_at: chrono::DateTime<chrono::Utc> = \"{}\".parse().expect(\"when.at: RFC3339 instant\");\n",
                    when_at.unwrap_or(BT_DEFAULT_WHEN_AT)
                ));
            }
            // `when.gates` — boolean configuration gates switched ON for this one dispatch (RSO-1
            // Phase 4). Only meaningful on a gate-consuming command; anywhere else the value
            // would be silently dropped, so refuse (the `when.at` posture).
            let gate_keys: Vec<String> = when
                .get("gates")
                .and_then(|g| g.as_sequence())
                .map(|s| {
                    s.iter()
                        .filter_map(|e| e.get("$ref").and_then(|r| r.as_str()))
                        .filter_map(|r| r.strip_prefix("configuration.yaml#/keys/"))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let consumed: Vec<&(&str, &str, &str)> =
                BT_GATE_CONSUMING.iter().filter(|(c, _, _)| *c == msg).collect();
            for gate in &gate_keys {
                if !consumed.iter().any(|(_, gk, _)| gk == gate) {
                    panic!(
                        "behaviour-tests: {}: `when.gates` names '{}' but '{}' does not consume it ({:?}) — the value would assert nothing; extend BT_GATE_CONSUMING and its bt_command_call arm when the handler takes the gate",
                        key, gate, msg, BT_GATE_CONSUMING
                    );
                }
            }
            for (_, gate_key, binding) in consumed {
                // Absent from `when.gates` = the key's SPEC DEFAULT (tests.yaml header), so the
                // suite exercises the production posture unless the test says otherwise.
                let value = if gate_keys.iter().any(|g| g == gate_key) {
                    true
                } else {
                    model
                        .defs
                        .get("configuration.yaml")
                        .and_then(|c| c.get("keys"))
                        .and_then(|k| k.get(*gate_key))
                        .and_then(|d| d.get("default"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                };
                out.push_str(&format!("    let {}: bool = {};\n", binding, value));
            }
            let mut call = bt_command_call(&msg);
            // `when.principal` (PROP-20260728-135632 §2.2, #235): the ACTING principal for tests
            // exercising `requires` — userType + optional domainId handle. Absent = the fixed
            // ADMIN harness actor (acting `any`; claims then need explicit principals).
            if let Some(p) = when.get("principal") {
                let ut = p.get("userType").and_then(|x| x.as_str()).unwrap_or("ADMIN");
                let expr = match p.get("domainId").and_then(|x| x.as_str()) {
                    Some(h) => format!("support::actor_principal(\"{}\", Some(support::uid(\"{}\")))", ut, h),
                    None => format!("support::actor_principal(\"{}\", None)", ut),
                };
                call = call.replace("&support::actor()", &format!("&{}", expr));
            }
            // The rider confirm's session is ENVELOPE data (X-SESSION-ID, ADR-0041), so no spec
            // case can carry it as payload. The bed presents a session by default -- the production
            // shape, the SDUI client always sends the header -- and withholds it ONLY for a case
            // whose declared outcome IS the missing-session refusal: that refusal's sole
            // precondition is the absent envelope, so the `thrown` set names it and no `when.session`
            // key is needed (#852 review, B1).
            if msg == "ConfirmRiderSignIn" {
                let refuses_without_session = t
                    .get("thrown")
                    .and_then(|x| x.as_sequence())
                    .map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.get("$ref").and_then(|x| x.as_str()))
                            .filter_map(ref_name)
                            .any(|name| name == "RiderSignInRequiresSession")
                    })
                    .unwrap_or(false);
                if !refuses_without_session {
                    call = call.replace(
                        ", None, &support::actor()",
                        ", Some(sc::SessionId(support::uid(\"session-1\"))), &support::actor()",
                    );
                }
            }
            // The member confirm's session is ENVELOPE data too (#639 part C step 6-ii) -- the
            // SAME shape as ConfirmRiderSignIn, withheld only for its own missing-session case.
            if msg == "ConfirmMemberSignIn" {
                let refuses_without_session = t
                    .get("thrown")
                    .and_then(|x| x.as_sequence())
                    .map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.get("$ref").and_then(|x| x.as_str()))
                            .filter_map(ref_name)
                            .any(|name| name == "MemberSignInRequiresSession")
                    })
                    .unwrap_or(false);
                if !refuses_without_session {
                    call = call.replace(
                        ", None, &support::actor(), run_member_sign_in_door)",
                        ", Some(sc::SessionId(support::uid(\"session-1\"))), &support::actor(), run_member_sign_in_door)",
                    );
                }
            }
            // The ADMIN confirm's session is ENVELOPE data too (#639 part C step 6-iii) -- the
            // SAME shape as ConfirmMemberSignIn, withheld only for its own missing-session case.
            if msg == "ConfirmAdminSignIn" {
                let refuses_without_session = t
                    .get("thrown")
                    .and_then(|x| x.as_sequence())
                    .map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.get("$ref").and_then(|x| x.as_str()))
                            .filter_map(ref_name)
                            .any(|name| name == "AdminSignInRequiresSession")
                    })
                    .unwrap_or(false);
                if !refuses_without_session {
                    call = call.replace(
                        ", None, &support::actor(), run_admin_sign_in_door)",
                        ", Some(sc::SessionId(support::uid(\"session-1\"))), &support::actor(), run_admin_sign_in_door)",
                    );
                }
            }
            // TipOrder derives `tippedBy` from the acting persona (ADR-0041): dispatch as the
            // RESTAURANT user type when the asserted fact says the restaurant tipped.
            if msg == "TipOrder" {
                let restaurant_tips = t
                    .get("then")
                    .and_then(|x| x.as_sequence())
                    .map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.get("$ref").and_then(|x| x.as_str()))
                            .filter_map(|r| fixtures.get(Value::String(r.trim_start_matches("#/fixtures/").to_string())))
                            .any(|fx| {
                                fx.get("data").and_then(|d| d.get("tippedBy")).and_then(|v| v.as_str())
                                    == Some("RESTAURANT")
                            })
                    })
                    .unwrap_or(false);
                if restaurant_tips {
                    call = call.replace("support::actor()", "support::actor_as(\"RESTAURANT\")");
                }
            }
            out.push_str(&format!("    let result = {};\n", call));
        } else {
            // No event reaction consumes `when.at` today — refuse rather than silently drop it.
            if when.get("at").is_some() {
                panic!(
                    "behaviour-tests: {}: `when.at` is set on event '{}' but no PM reaction consumes an instant — the value would assert nothing",
                    key, msg
                );
            }
            // `when.gates` on an EVENT dispatch (#167): only the [`BT_GATE_CONSUMING_EVENTS`]
            // recorders take a configuration gate — anywhere else the value would be silently
            // dropped, so refuse (the `when.at` posture, the #413 defect class).
            let gate_keys: Vec<String> = when
                .get("gates")
                .and_then(|g| g.as_sequence())
                .map(|s| {
                    s.iter()
                        .filter_map(|e| e.get("$ref").and_then(|r| r.as_str()))
                        .filter_map(|r| r.strip_prefix("configuration.yaml#/keys/"))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let consumed: Vec<&(&str, &str, &str)> =
                BT_GATE_CONSUMING_EVENTS.iter().filter(|(e, _, _)| *e == msg).collect();
            for gate in &gate_keys {
                if !consumed.iter().any(|(_, gk, _)| gk == gate) {
                    panic!(
                        "behaviour-tests: {}: `when.gates` names '{}' but event '{}' does not consume it ({:?}) — the value would assert nothing; extend BT_GATE_CONSUMING_EVENTS and its dispatch arm when the recorder takes the gate",
                        key, gate, msg, BT_GATE_CONSUMING_EVENTS
                    );
                }
            }
            for (_, gate_key, binding) in consumed {
                // Absent from `when.gates` = the key's SPEC DEFAULT (tests.yaml header), so the
                // suite exercises the production posture unless the test says otherwise.
                let value = if gate_keys.iter().any(|g| g == gate_key) {
                    true
                } else {
                    model
                        .defs
                        .get("configuration.yaml")
                        .and_then(|c| c.get("keys"))
                        .and_then(|k| k.get(*gate_key))
                        .and_then(|d| d.get("default"))
                        .and_then(|d| d.as_bool())
                        .unwrap_or_else(|| {
                            panic!(
                                "behaviour-tests: {}: gate key {} has no boolean spec default",
                                key, gate_key
                            )
                        })
                };
                out.push_str(&format!("    let {}: bool = {};\n", binding, value));
            }
            let def = resolve_ref(model, wref, "tests.yaml").unwrap();
            let literal = bt_struct_expr(model, "events.yaml", &format!("evs::{}", msg), def, &wdata, &format!("{}/when", key));
            out.push_str(&format!("    let ev = {};\n", literal));
            if is_pm {
                out.push_str(&format!("    let result = {};\n", bt_pm_event_call(&actor, &msg)));
            } else {
                // Aggregate ← delivered/inbound fact: record it on its stream through the write
                // path (Stripe payment facts go through the real inbound recording function).
                if matches!(msg.as_str(), "PaymentAuthorized" | "PaymentCaptured" | "PaymentReleased" | "PaymentFailed" | "PaymentRefunded") {
                    out.push_str(&format!(
                        "    let result = crate::payments::record_inbound_payment_event(&bed.store, DomainEvent::{}(ev), &support::actor()).await;\n",
                        msg
                    ));
                } else if msg == "OrderAcceptanceTimedOut" {
                    // #167: the acceptance-deadline recorder — record semantics iff still
                    // PLACED, with the ENFORCE_ACCEPTANCE_TIMEOUT gate read at the append (its
                    // binding was emitted above; the guard runs identically in shadow).
                    out.push_str(
                        "    let result = crate::commands::record_order_acceptance_timeout(&bed.store, DomainEvent::OrderAcceptanceTimedOut(ev), enforce_acceptance_timeout, &support::actor()).await;\n",
                    );
                } else if msg == "RestaurantRegistered" {
                    // The registry (SIRENE) inbound path (ADR-20260728-011344 D4). Unlike every other
                    // inbound fact, this one is NOT recorded verbatim: the aggregate folds its own stream
                    // and may emit a DIFFERENT event (RestaurantUpdated) or none at all. `record_fact`
                    // would append the report as-is and silently assert the wrong thing.
                    out.push_str(
                        "    let result = crate::commands::record_inbound_restaurant_registration(&bed.store, DomainEvent::RestaurantRegistered(ev), &support::actor()).await;\n",
                    );
                } else {
                    let (agg, id) = bt_event_stream(&owners, &pool, &mut ctx, &msg, Some(&wdata), &format!("{}/when", key));
                    let (_, _, uuid_keyed) = bt_agg(agg).expect("aggregate meta");
                    out.push_str(&format!(
                        "    let result = bed.record_fact(&{}, DomainEvent::{}(ev)).await;\n",
                        bt_stream_expr(agg, uuid_keyed, &id),
                        msg
                    ));
                }
            }
        }

        // THEN / THROWN
        if let Some(thrown) = t.get("thrown").and_then(|x| x.as_sequence()) {
            let codes: Vec<String> = thrown
                .iter()
                .filter_map(|e| e.get("$ref").and_then(|x| x.as_str()).and_then(ref_name))
                .map(|c| format!("\"{}\"", c))
                .collect();
            out.push_str(&format!(
                "    let err = result.expect_err(\"{}: the spec expects a typed rejection\");\n",
                key
            ));
            out.push_str(&format!("    support::assert_thrown(\"{}\", &err, &[{}]);\n", key, codes.join(", ")));
            out.push_str(&format!("    bed.assert_appended(\"{}\", &before, &[]);\n", key));
        } else {
            out.push_str(&format!("    let _ = result.expect(\"{}: the spec expects acceptance\");\n", key));
            let then: Vec<String> = t
                .get("then")
                .and_then(|x| x.as_sequence())
                .map(|s| {
                    s.iter()
                        .filter_map(|v| v.get("$ref").and_then(|x| x.as_str()))
                        .map(|r| r.trim_start_matches("#/fixtures/").to_string())
                        .collect()
                })
                .unwrap_or_default();
            let mut expected = Vec::new();
            for fx_name in &then {
                let fx = fixtures
                    .get(Value::String(fx_name.clone()))
                    .unwrap_or_else(|| panic!("behaviour-tests: {}: unknown fixture {}", key, fx_name));
                let event = fx.get("type").and_then(|t| t.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name).unwrap();
                let (agg, id) = bt_event_stream(&owners, &pool, &mut ctx, &event, fx.get("data"), &format!("{}/then", key));
                let (_, _, uuid_keyed) = bt_agg(agg).expect("aggregate meta");
                expected.push(format!("({}, fx_{}())", bt_stream_expr(agg, uuid_keyed, &id), bt_fn_name(fx_name)));
            }
            if expected.is_empty() {
                out.push_str(&format!("    bed.assert_appended(\"{}\", &before, &[]);\n", key));
            } else {
                out.push_str(&format!(
                    "    bed.assert_appended(\"{}\", &before, &[\n        {},\n    ]);\n",
                    key,
                    expected.join(",\n        ")
                ));
            }
            // The handler's third observable effect (ADR-20260731-214500 §2): a receive declaring
            // `schedules:` also asserts, in the SAME accepted-case test, that the reminder lands
            // SCHEDULED at +window and that re-declaring postpones the ONE pending row in place.
            if let Some(names) = schedules_of.get(&(actor.clone(), msg.clone())) {
                let (agg, id_prop, uuid_keyed) = bt_agg(&actor).unwrap_or_else(|| {
                    panic!("behaviour-tests: {}: schedules on non-aggregate actor {}", key, actor)
                });
                assert!(
                    uuid_keyed,
                    "behaviour-tests: {}: scheduling assertions only support uuid-keyed aggregates (got {}) — extend when a use case lands",
                    key, agg
                );
                let id = wdata
                    .get(id_prop)
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| {
                        panic!("behaviour-tests: {}: when.data lacks '{}' — the scheduling assertion needs the instance id", key, id_prop)
                    });
                for rname in names {
                    let policy = reminder_policy
                        .get(&(actor.clone(), rname.clone()))
                        .map(String::as_str)
                        .unwrap_or("in-place");
                    // Shared preamble: declare once, assert the fresh SCHEDULED row at +window.
                    out.push_str(&format!(
                        "    // schedules: {rname} — the third observable effect (ADR-20260731-214500 §2; reschedule: {policy})\n    {{\n        use actor_client::MailboxScheduleOutcome;\n        let mailbox = actor_client::mailbox::mem::MemMailbox::default();\n        let actor_id = support::uid(\"{id}\");\n        let spec = actor_client::reminders::reminder_schedules_for(\"{actor}\", \"{msg}\")\n            .find(|s| s.reminder == \"{rname}\")\n            .expect(\"{key}: schedule declared in actors.yaml\");\n        let t1 = chrono::Utc::now() + chrono::Duration::from_std(spec.after_default).expect(\"{key}: window fits chrono\");\n        let first = actor_client::reminders::declare(&mailbox, spec, actor_id, 0, t1, support::actor().correlation_id)\n            .await\n            .expect(\"{key}: declare\");\n        assert!(matches!(first, MailboxScheduleOutcome::Scheduled), \"{key}: expected a fresh SCHEDULED row, got {{first:?}}\");\n        let row = actor_client::reminder_message_id(actor_id, spec.reminder);\n        assert_eq!(mailbox.scheduled_at(row), Some(t1), \"{key}: due at +window\");\n        let t2 = t1 + chrono::Duration::days(1);\n        let again = actor_client::reminders::declare(&mailbox, spec, actor_id, 0, t2, support::actor().correlation_id)\n            .await\n            .expect(\"{key}: redeclare\");\n",
                        rname = rname,
                        policy = policy,
                        id = id,
                        actor = actor,
                        msg = msg,
                        key = key,
                    ));
                    match policy {
                        "keep" => out.push_str(&format!(
                            "        assert!(matches!(again, MailboxScheduleOutcome::Kept), \"{key}: re-declaring must KEEP the first occurrence (reschedule: keep, #167), got {{again:?}}\");\n        assert_eq!(mailbox.scheduled_at(row), Some(t1), \"{key}: the first scheduled_at wins — a re-declaration never extends the deadline\");\n        assert_eq!(mailbox.entries().len(), 1, \"{key}: one pending occurrence per (actor, purpose)\");\n    }}\n",
                            key = key,
                        )),
                        _ => out.push_str(&format!(
                            "        assert!(matches!(again, MailboxScheduleOutcome::Rescheduled), \"{key}: re-declaring must postpone the SAME row (ADR-20260731-150500), got {{again:?}}\");\n        assert_eq!(mailbox.scheduled_at(row), Some(t2), \"{key}: the pending occurrence moved in place\");\n        assert_eq!(mailbox.entries().len(), 1, \"{key}: one pending occurrence per (actor, purpose)\");\n    }}\n",
                            key = key,
                        )),
                    }
                }
            }
        }
        out.push_str("}\n\n");
    }
    out
}

