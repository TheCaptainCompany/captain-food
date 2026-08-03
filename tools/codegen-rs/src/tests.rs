    use super::*;

    #[test]
    fn dotted_keys_become_pascal_variants() {
        assert_eq!(dotted_variant("restaurants.featured"), "RestaurantsFeatured");
        assert_eq!(dotted_variant("operationStatus.byMessage"), "OperationStatusByMessage");
        assert_eq!(dotted_variant("add_to_cart"), "AddToCart");
    }

    #[test]
    fn ref_op_name_takes_the_last_pointer_segment() {
        let node: serde_yaml::Value =
            serde_yaml::from_str("$ref: 'api.yaml#/mutations/addCartLine'").expect("parses");
        assert_eq!(ref_op_name(&node).as_deref(), Some("addCartLine"));
        let plain: serde_yaml::Value = serde_yaml::from_str("gap: 'nothing binds this'").expect("parses");
        assert_eq!(ref_op_name(&plain), None);
    }

    // ─── data-layer selection sets (#80) ────────────────────────────────────────────────────────

    /// A minimal api.yaml/scalars.yaml/entities.yaml model exercising every selection shape:
    /// inline primitives, scalar refs, entity value objects (with entity-LOCAL refs and the
    /// `type: array` + `items` spelling), a SELF-recursive type, and a deep acyclic chain.
    fn selection_fixture() -> Model {
        let mut defs = BTreeMap::new();
        let y = |s: &str| serde_yaml::from_str::<Value>(s).expect("valid yaml");
        defs.insert(
            "scalars.yaml".into(),
            y("NodeId: { type: string }\nMoneyCents: { type: integer }\nCurrencyCode: { type: string }\n"),
        );
        defs.insert(
            "entities.yaml".into(),
            y(r#"
Money:
  type: object
  properties:
    amountCents: { $ref: 'scalars.yaml#/MoneyCents' }
    currency: { $ref: 'scalars.yaml#/CurrencyCode' }
Line:
  type: object
  properties:
    unitPrice: { $ref: '#/Money' }
    options:
      type: array
      items: { $ref: '#/Money' }
"#),
        );
        // `nodes` recurses (Node → next → Node); `cart` mixes shapes; `deep` is a 10-level
        // acyclic chain (deeper than SELECTION_MAX_DEPTH = 8) whose tail must be truncated.
        let mut api = String::from(
            r#"
types:
  Node:
    properties:
      id: { $ref: 'scalars.yaml#/NodeId' }
      next: { $ref: '#/types/Node', nullable: true }
  Loop:
    properties:
      inner: { $ref: '#/types/Loop', nullable: true }
  Cart:
    properties:
      id: { $ref: 'scalars.yaml#/NodeId' }
      open: { type: boolean }
      lines: { $ref: 'entities.yaml#/Line', array: true }
      dead: { $ref: '#/types/Loop', nullable: true }
"#,
        );
        for i in 1..=10 {
            api.push_str(&format!(
                "  D{i}:\n    properties:\n      tag{i}: {{ type: string }}\n{}",
                if i < 10 { format!("      child: {{ $ref: '#/types/D{}' }}\n", i + 1) } else { String::new() }
            ));
        }
        api.push_str(
            r#"queries:
  node:
    returns: { $ref: '#/types/Node', nullable: true }
  cart:
    returns: { $ref: '#/types/Cart', nullable: true }
  deep:
    returns: { $ref: '#/types/D1' }
  tag:
    returns: { $ref: 'scalars.yaml#/NodeId' }
"#,
        );
        defs.insert("api.yaml".into(), y(&api));
        Model { defs }
    }

    #[test]
    fn selection_terminates_on_a_type_cycle_and_omits_the_recursive_field() {
        let m = selection_fixture();
        // Node → next → Node is a cycle: the descent stops and `next` is OMITTED (a bare object
        // field would be invalid GraphQL), leaving only the scalar.
        assert_eq!(query_selection(&m, "node").as_deref(), Some("{ id }"));
    }

    #[test]
    fn selection_expands_entities_arrays_and_drops_a_field_with_nothing_selectable() {
        let m = selection_fixture();
        // `lines` expands through entities.yaml (entity-LOCAL `#/Money` refs + the
        // `type: array` + `items` spelling); `dead` (a pure self-cycle with no leaves) collapses
        // to an EMPTY selection and is omitted entirely — the omission bubbles up.
        assert_eq!(
            query_selection(&m, "cart").as_deref(),
            Some(
                "{ id open lines { unitPrice { amountCents currency } options { amountCents currency } } }"
            )
        );
    }

    #[test]
    fn selection_truncated_at_the_depth_bound_omits_the_object_field_not_emits_it_bare() {
        let m = selection_fixture();
        let sel = query_selection(&m, "deep").expect("object-typed query expands");
        // The chain is 10 levels; the bound is SELECTION_MAX_DEPTH = 8. Level 8's `child` would
        // start level 9, which the bound refuses — so D8 keeps its scalar and OMITS `child`.
        assert!(sel.contains("tag8"), "level at the bound keeps its leaves: {sel}");
        assert!(!sel.contains("tag9"), "level past the bound must be truncated: {sel}");
        // The truncated object field is dropped, never emitted bare/empty (invalid GraphQL).
        assert!(!sel.contains("child }"), "no bare object field: {sel}");
        assert!(!sel.contains("{ }") && !sel.contains("{}"), "no empty selection set: {sel}");
    }

    #[test]
    fn scalar_returning_query_needs_no_selection_set() {
        let m = selection_fixture();
        assert_eq!(query_selection(&m, "tag"), None);
    }

    #[test]
    fn parse_ref_splits_file_and_pointer() {
        let p = parse_ref("api.yaml#/queries/restaurants").expect("parses");
        assert_eq!(p.file, "api.yaml");
        assert_eq!(p.path, vec!["queries".to_string(), "restaurants".to_string()]);
    }

    #[test]
    fn parse_ref_keeps_dotted_translation_key_as_one_segment() {
        let p = parse_ref("translations.yaml#/home.craving").expect("parses");
        assert_eq!(p.file, "translations.yaml");
        assert_eq!(p.path, vec!["home.craving".to_string()]);
    }

    #[test]
    fn parse_ref_local_has_empty_file() {
        let p = parse_ref("#/fixtures/orderPlaced").expect("parses");
        assert_eq!(p.file, "");
        assert_eq!(p.path, vec!["fixtures".to_string(), "orderPlaced".to_string()]);
    }

    #[test]
    fn parse_ref_rejects_non_pointer() {
        assert!(parse_ref("api.yaml").is_none());
    }

    // ─── §1b ref-kind contract ──────────────────────────────────────────────────────────────────

    #[test]
    fn glob_star_stops_at_a_dot_but_doublestar_does_not() {
        assert!(glob("*.receives[*].message", "Cart.receives[3].message"));
        assert!(!glob("*.message", "Cart.receives[3].message"));
        assert!(glob("*.properties.**", "AddCartLine.properties.line.items"));
        assert!(glob("screens/*.yaml", "screens/captain_frontoffice.yaml"));
        assert!(glob("**.subscription", "screens[3].subscription"));
        assert!(!glob("resolvers.**", "actions.checkout.mutation"));
    }

    #[test]
    fn normalize_site_wildcards_names_and_indices_but_keeps_field_names() {
        assert_eq!(normalize_site("Cart.receives[12].message"), "*.receives[*].message");
        assert_eq!(
            normalize_site("PlaceOrderProcess.receives[0].steps[3].read.where.cart_id.from"),
            "*.receives[*].steps[*].read.where.*.from"
        );
        assert_eq!(normalize_site("types.Cart.properties.status"), "types.*.properties.status");
    }

    /// The model behind the kind checks below: two tables of DIFFERENT kinds that a naive
    /// "starts_with database/tables/" test cannot tell apart, plus a command and a payload object.
    fn kind_fixture() -> Model {
        let mut defs = BTreeMap::new();
        let y = |s: &str| serde_yaml::from_str::<Value>(s).expect("valid yaml");
        defs.insert(
            "database/tables/process_managers.yaml".into(),
            y("payment_process_manager:\n  columns:\n    cart_id: { type: text }\n"),
        );
        defs.insert("database/tables/referential.yaml".into(), y("ref_currency:\n  columns:\n    code: { type: text }\n"));
        defs.insert("commands.yaml".into(), y("PlaceOrder:\n  type: object\nCartLine:\n  type: object\n"));
        defs.insert("scalars.yaml".into(), y("OrderId:\n  type: string\nOrderStatus:\n  enum: [NEW, PAID]\n"));
        Model { defs }
    }

    #[test]
    fn classify_separates_kinds_that_share_a_file_or_a_directory() {
        let m = kind_fixture();
        let handled: BTreeSet<String> = ["PlaceOrder".to_string()].into_iter().collect();
        let k = |r: &str| {
            let p = parse_ref(r).expect("parses");
            classify(&p.file, &p.path, resolve_ref(&m, r, "x.yaml").expect("resolves"), &handled)
        };
        // Same directory, different kinds.
        assert_eq!(k("database/tables/process_managers.yaml#/payment_process_manager"), Some(Kind::PmStateTable));
        assert_eq!(k("database/tables/referential.yaml#/ref_currency"), Some(Kind::ReferentialTable));
        assert_eq!(k("database/tables/process_managers.yaml#/payment_process_manager/columns/cart_id"), Some(Kind::TableColumn));
        // Same file, different kinds: a handled command vs a shared payload sub-object.
        assert_eq!(k("commands.yaml#/PlaceOrder"), Some(Kind::Command));
        assert_eq!(k("commands.yaml#/CartLine"), Some(Kind::PayloadObject));
        // A scalar with an `enum` is an enum scalar (what a lifecycle `status` requires).
        assert_eq!(k("scalars.yaml#/OrderId"), Some(Kind::Scalar));
        assert_eq!(k("scalars.yaml#/OrderStatus"), Some(Kind::EnumScalar));
    }

    #[test]
    fn ref_kind_rejects_a_state_table_that_is_not_a_state_table() {
        let mut m = kind_fixture();
        m.defs.insert(
            "processmanager.yaml".into(),
            serde_yaml::from_str(
                "RefundProcess:\n  state_table: { $ref: 'database/tables/referential.yaml#/ref_currency' }\n",
            )
            .expect("valid yaml"),
        );
        let mut issues = Vec::new();
        validate_ref_kinds(&m, &mut issues);
        let hit = issues.iter().find(|i| i.rule == "ref-kind").expect("kind violation reported");
        assert!(hit.message.contains("referential table"), "{}", hit.message);
        assert!(hit.message.contains("process-manager state table"), "{}", hit.message);
    }

    #[test]
    fn ref_kind_accepts_the_right_state_table() {
        let mut m = kind_fixture();
        m.defs.insert(
            "processmanager.yaml".into(),
            serde_yaml::from_str(
                "RefundProcess:\n  state_table: { $ref: 'database/tables/process_managers.yaml#/payment_process_manager' }\n",
            )
            .expect("valid yaml"),
        );
        let mut issues = Vec::new();
        validate_ref_kinds(&m, &mut issues);
        assert!(issues.is_empty(), "expected no issues, got {:?}", issues.iter().map(|i| &i.message).collect::<Vec<_>>());
    }

    #[test]
    fn ref_site_undeclared_is_fail_closed() {
        let mut m = kind_fixture();
        // A brand-new ref-carrying field nobody declared a contract for.
        m.defs.insert(
            "processmanager.yaml".into(),
            serde_yaml::from_str("RefundProcess:\n  brand_new_field: { $ref: 'commands.yaml#/PlaceOrder' }\n")
                .expect("valid yaml"),
        );
        let mut issues = Vec::new();
        validate_ref_kinds(&m, &mut issues);
        let hit = issues.iter().find(|i| i.rule == "ref-site-undeclared").expect("undeclared site reported");
        assert!(hit.message.contains("'RefundProcess.brand_new_field'"), "{}", hit.message);
    }

    // ─── pinned resolver args (#82) ─────────────────────────────────────────────────────────────

    /// An api.yaml whose `restaurants` query declares `list` (an enum-typed arg) and `city` — the
    /// shape #82 tripped over: the screens surfaces pinned `listKey`, which does not exist.
    fn resolver_args_fixture() -> Model {
        inline_model(&[
            (
                "scalars.yaml",
                "RestaurantListKey:\n  type: string\n  enum: [ORDER_AGAIN, RECOMMENDED, TOP_DEALS]\nCityName: { type: string }\n",
            ),
            (
                "api.yaml",
                "queries:\n  restaurants:\n    args:\n      city: { $ref: 'scalars.yaml#/CityName' }\n      list: { $ref: 'scalars.yaml#/RestaurantListKey' }\n    returns: { $ref: '#/types/Restaurant', array: true }\n  me:\n    returns: { $ref: '#/types/Customer', nullable: true }\n",
            ),
        ])
    }

    /// The #82 regression: a pinned key the bound query does not declare is an ERROR, not silence.
    #[test]
    fn resolver_arg_not_declared_by_the_bound_query_is_an_error() {
        let m = resolver_args_fixture();
        let args: Value = serde_yaml::from_str("listKey: RECOMMENDED").expect("valid yaml");
        let mut issues = Vec::new();
        validate_resolver_args(&m, &mut issues, "screens/x.yaml/resolvers/r/args", "restaurants", &args);
        assert_eq!(issues.len(), 1, "{:?}", issues.iter().map(|i| &i.message).collect::<Vec<_>>());
        assert_eq!(issues[0].rule, "resolver-unknown-arg");
        // The message names the real args, so the fix is obvious from the error alone.
        assert!(issues[0].message.contains("declared: city|list"), "{}", issues[0].message);
    }

    /// A query that declares NO args at all still rejects a pin (rather than skipping the check).
    #[test]
    fn resolver_arg_on_an_argless_query_is_an_error() {
        let m = resolver_args_fixture();
        let args: Value = serde_yaml::from_str("anything: X").expect("valid yaml");
        let mut issues = Vec::new();
        validate_resolver_args(&m, &mut issues, "screens/x.yaml/resolvers/r/args", "me", &args);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].rule, "resolver-unknown-arg");
        assert!(issues[0].message.contains("it declares none"), "{}", issues[0].message);
    }

    /// The key exists but the pinned literal is outside the arg's enum.
    #[test]
    fn resolver_arg_value_outside_the_enum_is_an_error() {
        let m = resolver_args_fixture();
        let args: Value = serde_yaml::from_str("list: NEARBY").expect("valid yaml");
        let mut issues = Vec::new();
        validate_resolver_args(&m, &mut issues, "screens/x.yaml/resolvers/r/args", "restaurants", &args);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].rule, "resolver-invalid-arg-value");
        assert!(issues[0].message.contains("ORDER_AGAIN|RECOMMENDED|TOP_DEALS"), "{}", issues[0].message);
    }

    /// The corrected pin — and a non-enum arg, which the value check leaves alone.
    #[test]
    fn correctly_pinned_resolver_args_are_clean() {
        let m = resolver_args_fixture();
        let args: Value = serde_yaml::from_str("list: RECOMMENDED\ncity: Tours").expect("valid yaml");
        let mut issues = Vec::new();
        validate_resolver_args(&m, &mut issues, "screens/x.yaml/resolvers/r/args", "restaurants", &args);
        assert!(issues.is_empty(), "{:?}", issues.iter().map(|i| &i.message).collect::<Vec<_>>());
    }

    // ─── data-layer input-type names (#97) ──────────────────────────────────────────────────────

    /// The divergence #97 exists for: a mutation whose COMMAND name is not `Pascal(mutation)` —
    /// the SDL names the input after the command, so a convention-deriving client would send a
    /// type the schema does not have.
    #[test]
    fn action_input_types_come_from_the_command_not_the_mutation_name() {
        let m = inline_model(&[
            (
                "screens/restaurant_frontoffice.yaml",
                "resolvers:\n  gbp.link: { query: { $ref: 'api.yaml#/queries/gbpLink' } }\nactions:\n  configure_gbp: { kind: mutation, mutation: { $ref: 'api.yaml#/mutations/configureGbpOrderLink' } }\n",
            ),
            (
                "api.yaml",
                "queries:\n  gbpLink:\n    args:\n      restaurantId: { type: string, required: true }\n    returns: { type: string }\n  me:\n    returns: { type: string }\nmutations:\n  configureGbpOrderLink:\n    command: { $ref: 'commands.yaml#/ConfigureGoogleBusinessProfileOrderLink' }\n",
            ),
        ]);
        let out = emit_web_data_layer(&m);
        // The mutation input is the COMMAND's name — the convention (`ConfigureGbpOrderLinkInput`)
        // would be wrong and must appear nowhere.
        assert!(
            out.contains("Some(\"ConfigureGoogleBusinessProfileOrderLinkInput\")"),
            "{out}"
        );
        assert!(!out.contains("ConfigureGbpOrderLinkInput"), "{out}");
        // A query with args gets its SDL input-type name; the naming lives in ONE place (the emitter).
        assert!(out.contains("Some(\"GbpLinkQueryInput\")"), "{out}");
    }

    // ─── design tokens → CSS (#115) ─────────────────────────────────────────────────────────────

    #[test]
    fn design_tokens_emit_css_custom_properties() {
        let m = inline_model(&[(
            "screens/restaurant_frontoffice.yaml",
            "design_tokens:\n  colors:\n    primary: \"#F97316\"\n    surface_muted: \"#F9FAFB\"\n  typography:\n    font_family: \"Inter, sans-serif\"\n    scale: { base: \"1rem\", \"2xl\": \"1.5rem\" }\n  radius: { full: \"9999px\" }\n",
        )]);
        let css = emit_web_tokens_css(&m);
        assert!(css.contains(":root {"));
        // Colors → --color-*, snake→kebab.
        assert!(css.contains("--color-primary: #F97316;"), "{css}");
        assert!(css.contains("--color-surface-muted: #F9FAFB;"), "{css}");
        // typography.font_family is special-cased; the scale map → --text-*.
        assert!(css.contains("--font-family: Inter, sans-serif;"), "{css}");
        assert!(css.contains("--text-base: 1rem;"), "{css}");
        assert!(css.contains("--text-2xl: 1.5rem;"), "{css}");
        // radius group.
        assert!(css.contains("--radius-full: 9999px;"), "{css}");
    }

    // ─── screen-tree emitter (#87) ──────────────────────────────────────────────────────────────

    /// A minimal surface exercising: global-chrome expansion, prop flattening (i18n ref, binding,
    /// literal, nested config with a non-component `type`), child recursion, and `sdui: false`.
    fn screens_fixture() -> Model {
        inline_model(&[
            (
                "screens/restaurant_frontoffice.yaml",
                r#"
component_registry:
  layout: [section, tab_bar]
  chrome: [sticky_header, page_header]
  content: [text]
  order: [order_list]
  inputs: [button, logo]
resolvers:
  orders.mine: { query: { $ref: 'api.yaml#/queries/orders' } }
global_components:
  topbar:
    type: sticky_header
    slots:
      left: [{ type: logo, asset: "/assets/logo.svg" }]
screens:
  - id: queue
    roles: [RESTAURANT]
    route: "/"
    sdui: true
    requires_auth: true
    data_requirements: [orders.mine]
    components:
      - { component: topbar }
      - { type: page_header, title: { $ref: 'x.translations.yaml#/q.title' } }
      - type: tab_bar
        filters:
          - { id: sort, type: dropdown }
      - type: order_list
        items: "{{ orders }}"
        empty_state: { title: { $ref: 'x.translations.yaml#/q.empty' } }
      - type: section
        content:
          - { type: button, label: { $ref: 'x.translations.yaml#/q.go' }, variant: primary }
  - id: pay
    roles: [CUSTOMER]
    route: "/pay"
    sdui: false
    components:
      - { type: text, value: "never emitted" }
"#,
            ),
            ("api.yaml", "queries:\n  orders:\n    returns: { type: string }\n"),
        ])
    }

    #[test]
    fn screen_trees_expand_chrome_flatten_props_and_bind_resolvers() {
        let out = emit_web_screens(&screens_fixture());
        // The surface module + both screens.
        assert!(out.contains("pub mod restaurant_frontoffice"), "{out}");
        assert!(out.contains("id: \"queue\""));
        // { component: topbar } expanded to its sticky_header definition, slots flattened to children.
        assert!(out.contains("ComponentKind::StickyHeader"));
        assert!(out.contains("ComponentKind::Logo"));
        // Prop kinds: i18n ref (dotted key kept whole), binding, literal, and the flattened
        // nested empty_state config.
        assert!(out.contains("PropValue::I18n(\"q.title\")"));
        assert!(out.contains("PropValue::Binding(\"orders\")"));
        assert!(out.contains("(\"variant\", PropValue::Text(\"primary\"))"));
        assert!(out.contains("(\"empty_state.title\", PropValue::I18n(\"q.empty\"))"));
        // `filters[].type: dropdown` is CONFIG, not a child component — flattened, not dispatched.
        assert!(out.contains("(\"filters.0.type\", PropValue::Text(\"dropdown\"))"), "{out}");
        // data_requirements bound through ResolverKey.
        assert!(out.contains("data_requirements: &[ResolverKey::OrdersMine]"));
        // sdui:false → empty tree but the route still registers.
        assert!(out.contains("route: \"/pay\""));
        let pay = out.split("id: \"pay\"").nth(1).expect("pay screen emitted");
        assert!(pay.contains("tree: &[]"), "non-SDUI screens carry no tree");
    }

    #[test]
    fn bottom_sheets_emit_into_the_surface_tables() {
        let mut m = screens_fixture();
        // Add a sheets section to the fixture surface (bottom_sheet is registered in the fixture? —
        // no: extend the registry first, then the sheet).
        m.defs.insert(
            "screens/restaurant_frontoffice.yaml".into(),
            serde_yaml::from_str(
                r#"
component_registry:
  layout: [section]
  chrome: [bottom_sheet]
  inputs: [button, otp_input]
screens: []
bottom_sheets:
  auth_sheet:
    id: auth_sheet
    type: bottom_sheet
    title: { $ref: 'x.translations.yaml#/auth.title' }
    sections:
      - { type: otp_input, id: otp_field, length: 6 }
      - { type: button, label: { $ref: 'x.translations.yaml#/auth.go' }, action: { type: verify_otp, code: "{{ otp_field.value }}" } }
"#,
            )
            .expect("valid yaml"),
        );
        let out = emit_web_screens(&m);
        assert!(out.contains("pub const SHEETS: &[Sheet]"), "{out}");
        assert!(out.contains("Sheet { id: \"auth_sheet\""), "{out}");
        // `sections` is a child key: the otp input + button are CHILD NODES of the sheet.
        assert!(out.contains("ComponentKind::OtpInput"), "{out}");
        // The bare-style action prop and its form-field binding flatten as props (#94 executor).
        assert!(out.contains("(\"action.code\", PropValue::Binding(\"otp_field.value\"))"), "{out}");
    }

    #[test]
    #[should_panic(expected = "not in the shared component_registry")]
    fn unregistered_component_type_aborts_the_emitter() {
        let mut m = screens_fixture();
        m.defs.insert(
            "screens/rogue.yaml".into(),
            serde_yaml::from_str(
                "screens:\n  - id: r\n    route: \"/r\"\n    components:\n      - { type: not_registered }\n",
            )
            .expect("valid yaml"),
        );
        emit_web_screens(&m);
    }

    #[test]
    fn source_file_membership() {
        assert!(is_source_file("api.yaml"));
        assert!(is_source_file("architecture/c4-l2.yaml"));
        assert!(is_source_file("services.yaml"));
        assert!(is_source_file("screens/captain_frontoffice.yaml"));
        assert!(is_source_file("restaurant_frontoffice.translations.yaml"));
        assert!(!is_source_file("nope.yaml"));
    }

    #[test]
    fn snake_type_is_module_case() {
        assert_eq!(snake_type("Order"), "order");
        assert_eq!(snake_type("DeliveryJob"), "delivery_job");
        assert_eq!(snake_type("RestaurantAccount"), "restaurant_account");
    }

    #[test]
    fn svc_op_name_is_snake_case_domain_verb() {
        assert!(svc_op_name_ok("request"));
        assert!(svc_op_name_ok("offer_job"));
        assert!(svc_op_name_ok("verify_phone_otp"));
        assert!(!svc_op_name_ok("Request"));
        assert!(!svc_op_name_ok("offer-job"));
        assert!(!svc_op_name_ok("_request"));
        assert!(!svc_op_name_ok("1request"));
        assert!(!svc_op_name_ok(""));
    }

    #[test]
    fn pm_base_name_strips_suffix_and_keeps_process_for_single_words() {
        assert_eq!(pm_base_name("payment_process_manager"), "PaymentProcess");
        assert_eq!(pm_base_name("refund_process_manager"), "RefundProcess");
        assert_eq!(pm_base_name("cart_binding_process_manager"), "CartBinding");
        assert_eq!(pm_base_name("delivery_dispatch_process_manager"), "DeliveryDispatch");
    }

    #[test]
    fn pm_lookup_method_is_by_column_minus_id() {
        assert_eq!(pm_lookup_method("cart_id"), "by_cart");
        assert_eq!(pm_lookup_method("payment_intent_id"), "by_payment_intent");
        assert_eq!(pm_lookup_method("delivery_job_id"), "by_delivery_job");
        assert_eq!(pm_lookup_method("session_id"), "by_session");
    }

    /// A Model from inline YAML sources (path → content), for emitter tests that need spec shapes
    /// the committed catalog does not exercise (http binding, expose: true).
    fn inline_model(files: &[(&str, &str)]) -> Model {
        let mut defs = BTreeMap::new();
        for (path, content) in files {
            let parsed: Value = serde_yaml::from_str(content).expect("test yaml parses");
            defs.insert(path.to_string(), strip_meta(parsed));
        }
        Model { defs }
    }

    // #110 — translation hygiene gates. All run `validate_translations` on a minimal fixture.
    fn translation_rules(files: &[(&str, &str)]) -> Vec<Issue> {
        let mut issues = Vec::new();
        validate_translations(&inline_model(files), &mut issues);
        issues
    }

    #[test]
    fn translation_locale_missing_flags_a_key_without_all_locales() {
        // A key with only `en` (no `fr`) is a hard error — full coverage across SUPPORTED_LOCALES.
        let issues = translation_rules(&[(
            "translations.yaml",
            "common.hi: { messages: { en: \"Hi\" } }",
        )]);
        let f = issues.iter().find(|i| i.rule == "translation-locale-missing").expect("locale-missing");
        assert!(f.message.contains("fr"), "names the missing locale: {}", f.message);
    }

    #[test]
    fn translation_key_unused_flags_a_key_no_screen_or_code_ref_references() {
        // `common.orphan` is referenced by no screen and no code_refs → unused (must be deleted).
        let issues = translation_rules(&[(
            "translations.yaml",
            "common.orphan: { messages: { en: \"x\", fr: \"x\" } }",
        )]);
        assert!(
            issues.iter().any(|i| i.rule == "translation-key-unused" && i.location.ends_with("common.orphan")),
            "orphan key should be flagged unused"
        );
    }

    #[test]
    fn translation_key_used_by_a_screen_ref_is_not_flagged_unused() {
        // Same key, now referenced by a screen `$ref` → not unused.
        let issues = translation_rules(&[
            ("translations.yaml", "common.hi: { messages: { en: \"Hi\", fr: \"Salut\" } }"),
            (
                "screens/x.yaml",
                "screens:\n  - id: s\n    title: { $ref: 'translations.yaml#/common.hi' }\n",
            ),
        ]);
        assert!(
            !issues.iter().any(|i| i.rule == "translation-key-unused"),
            "screen-referenced key must not be unused: {:?}",
            issues.iter().map(|i| &i.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn translation_key_covered_by_a_code_ref_wildcard_is_not_flagged_unused() {
        // A `prefix.*` code_ref marks matching keys used (the hand-written-Rust escape hatch).
        let issues = translation_rules(&[
            (
                "translations.yaml",
                "order.status.placed.title: { messages: { en: \"Placed\", fr: \"Reçue\" } }",
            ),
            ("translations.code_refs.yaml", "code_refs:\n  - key: order.status.*\n"),
        ]);
        assert!(!issues.iter().any(|i| i.rule == "translation-key-unused"), "code_ref-covered key not unused");
        assert!(!issues.iter().any(|i| i.rule == "translation-code-ref-unknown"), "wildcard matches a key");
    }

    #[test]
    fn translation_code_ref_unknown_flags_a_stale_manifest_entry() {
        // A code_refs entry matching no catalog key is stale.
        let issues = translation_rules(&[
            ("translations.yaml", "common.hi: { messages: { en: \"Hi\", fr: \"Salut\" } }"),
            ("translations.code_refs.yaml", "code_refs:\n  - key: order.ghost\n"),
        ]);
        assert!(
            issues.iter().any(|i| i.rule == "translation-code-ref-unknown" && i.location.ends_with("order.ghost")),
            "stale code_ref should be flagged"
        );
    }

    #[test]
    fn code_refs_manifest_has_no_stale_entries_against_the_real_crates() {
        // Companion to `translation-code-ref-unknown`: every code_refs entry must actually appear in
        // hand-written Rust, else the manifest is lying. Grep crates/**/*.rs for each entry's literal
        // (the `prefix` for a wildcard). Runs from tools/codegen-rs, so specs/crates are two levels up.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let manifest = std::fs::read_to_string(root.join("specs/translations.code_refs.yaml"))
            .expect("read translations.code_refs.yaml");
        let doc: Value = serde_yaml::from_str(&manifest).expect("parse manifest");
        let entries = doc.get("code_refs").and_then(|v| v.as_sequence()).cloned().unwrap_or_default();
        assert!(!entries.is_empty(), "manifest should declare at least the tracking.rs keys");

        // Collect all Rust source under crates/ once.
        fn rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        rs_files(&p, out);
                    } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                        out.push(p);
                    }
                }
            }
        }
        let mut files = Vec::new();
        rs_files(&root.join("crates"), &mut files);
        let haystack: String = files
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .collect::<Vec<_>>()
            .join("\n");

        for entry in &entries {
            let pat = entry.get("key").and_then(|v| v.as_str()).expect("code_ref key");
            let needle = pat.strip_suffix(".*").map(|p| format!("{p}.")).unwrap_or_else(|| pat.to_string());
            assert!(
                haystack.contains(&needle),
                "code_refs entry '{pat}' matches no literal in crates/**/*.rs — stale manifest entry (remove it)."
            );
        }
    }

    const SVC_HTTP_EXPOSED: &str = r#"
geocoding:
  description: "Test service."
  operations:
    resolve_address:
      description: "Resolve one address."
      input:
        query: { type: string }
      output:
        latitude: { type: number }
      errors: []
    warm_cache:
      description: "No input, no output."
      errors: []
  binding: http
  expose: true
  implementations:
    nominatim:
      routes:
        resolve_address: 'POST /adapters/nominatim/search'
        warm_cache: 'POST /adapters/nominatim/warm'
"#;

    #[test]
    fn services_trait_signatures_follow_the_catalog() {
        let model = inline_model(&[("services.yaml", SVC_HTTP_EXPOSED)]);
        let out = emit_services_application(&model);
        assert!(out.contains("pub trait GeocodingService: Send + Sync {"), "{out}");
        assert!(
            out.contains("async fn resolve_address(&self, input: GeocodingResolveAddressInput, meta: &ServiceCallMeta) -> Result<GeocodingResolveAddressOutput, DomainError>;"),
            "{out}"
        );
        // Input-less + output-less operation: no input parameter, unit result.
        assert!(
            out.contains("async fn warm_cache(&self, meta: &ServiceCallMeta) -> Result<(), DomainError>;"),
            "{out}"
        );
        assert!(out.contains("pub struct GeocodingResolveAddressInput {\n    pub query: String,\n}"), "{out}");
    }

    #[test]
    fn services_http_client_derives_paths_and_kebab_case() {
        let model = inline_model(&[("services.yaml", SVC_HTTP_EXPOSED)]);
        let out = emit_services_http_clients(&model);
        assert!(out.contains("pub struct HttpGeocodingService"), "{out}");
        assert!(out.contains("\"/services/geocoding/resolve-address\""), "{out}");
        assert!(out.contains("post_call(&self.http, &self.base_url, \"/services/geocoding/warm-cache\", (), meta).await"), "{out}");
    }

    #[test]
    fn service_bindings_honor_the_spec_topology() {
        let http = inline_model(&[("services.yaml", SVC_HTTP_EXPOSED)]);
        let out = emit_service_bindings(&http);
        assert!(out.contains("SERVICE_GEOCODING_URL"), "{out}");
        assert!(out.contains("HttpGeocodingService::new(url)"), "{out}");
        let local = inline_model(&[(
            "services.yaml",
            "payment:\n  operations:\n    request:\n      errors: []\n  binding: local\n  expose: false\n",
        )]);
        let out = emit_service_bindings(&local);
        assert!(out.contains("pub fn payment_service("), "{out}");
        assert!(out.contains("Ok(local())"), "{out}");
        assert!(!out.contains("SERVICE_PAYMENT_URL"), "{out}");
    }

    #[test]
    fn services_routes_are_expose_gated() {
        let none = inline_model(&[(
            "services.yaml",
            "payment:\n  operations:\n    request:\n      errors: []\n  binding: local\n  expose: false\n",
        )]);
        let out = emit_services_routes(&none);
        assert!(out.contains("pub fn services_router<S: Clone + Send + Sync + 'static>() -> axum::Router<S> {"), "{out}");
        assert!(!out.contains("ServicesRouterState"), "{out}");
        let exposed = inline_model(&[("services.yaml", SVC_HTTP_EXPOSED)]);
        let out = emit_services_routes(&exposed);
        assert!(out.contains("pub struct ServicesRouterState {\n    pub geocoding: Arc<dyn GeocodingService>,\n}"), "{out}");
        assert!(out.contains(".route(\"/services/geocoding/resolve-address\", post(geocoding_resolve_address))"), "{out}");
        assert!(out.contains("Json(call): Json<WireCall<GeocodingResolveAddressInput>>"), "{out}");
    }

    #[test]
    fn svc_names_derive_mechanically() {
        assert_eq!(pascal_snake("payment"), "Payment");
        assert_eq!(pascal_snake("offer_job"), "OfferJob");
        assert_eq!(pascal_snake("verify_phone_otp"), "VerifyPhoneOtp");
        assert_eq!(svc_http_path("payment", "request"), "/services/payment/request");
        assert_eq!(svc_http_path("delivery", "offer_job"), "/services/delivery/offer-job");
        assert_eq!(svc_url_var("payment"), "SERVICE_PAYMENT_URL");
        assert_eq!(svc_url_var("catalog_sync"), "SERVICE_CATALOG_SYNC_URL");
    }

    #[test]
    fn svc_adapter_route_is_post_under_adapters() {
        assert!(svc_adapter_route_ok("POST /adapters/stripe/payment-intents"));
        assert!(svc_adapter_route_ok("POST /adapters/avelo37/deliveries"));
        assert!(!svc_adapter_route_ok("GET /adapters/stripe/refunds"));
        assert!(!svc_adapter_route_ok("POST /adapters/stripe")); // provider alone — needs ≥1 path segment
        assert!(!svc_adapter_route_ok("POST /services/payment/request")); // the DERIVED surface is never declared
        assert!(!svc_adapter_route_ok("POST /adapters/Stripe/refunds"));
        assert!(!svc_adapter_route_ok("POST /adapters/stripe/refunds/"));
    }

    // ─── Makefile portability — recipe lines must be pure ASCII ─────────────────────────────────

    /// Native Windows GNU Make hands recipe lines to Cygwin's `sh` with broken quoting as soon as
    /// the line contains a byte > 127: `sh` receives the entire recipe as ONE word and dies with
    /// `$'...': command not found`, so the target fails for a reason unrelated to what it does —
    /// an em dash in the `check-drift` message once made `make rust` fail with zero actual drift.
    /// Only tab-indented RECIPE text reaches `sh`; comments, variable assignments and
    /// `$(shell ...)` lines are interpreted by make itself and may keep non-ASCII.
    ///
    /// Detection is deliberately the simple over-approximation "any line starting with a TAB":
    /// this Makefile does not set `.RECIPEPREFIX`, and treating a stray tab-indented non-recipe
    /// line as a recipe only tightens the guard, never loosens it.
    /// `required` and `default` are MUTUALLY EXCLUSIVE — you must choose (product-owner directive,
    /// 2026-07-29). A required key carrying a default can never be reported missing, because the
    /// default always satisfies it: the requirement is silently inert, which is worse than not
    /// declaring it, since the spec then states a guarantee the runtime does not make.
    #[test]
    fn a_required_key_may_not_also_declare_a_default() {
        let spec = r#"
keys:
  BOTH:
    type: string
    required: [production]
    default: "fallback"
    gates: "Declares both, which cannot be honoured."
"#;
        let model = Model {
            defs: BTreeMap::from([(
                "configuration.yaml".to_string(),
                serde_yaml::from_str::<Value>(spec).expect("parses"),
            )]),
        };
        let mut issues = Vec::new();
        validate_configuration(&model, &mut issues);
        let hit = issues.iter().find(|i| i.rule == "config-required-with-default");
        assert!(
            hit.is_some(),
            "a key declaring BOTH `required` and `default` must be rejected; got {:?}",
            issues.iter().map(|i| i.rule).collect::<Vec<_>>()
        );
    }

    /// The complement: each on its own is fine. A guard that also rejects the legal shapes would push
    /// people to stop declaring defaults at all.
    #[test]
    fn required_alone_and_default_alone_are_both_accepted() {
        for body in [
            "    required: [production]\n",
            "    default: \"fallback\"\n",
        ] {
            let spec = format!(
                "keys:\n  ONE:\n    type: string\n{body}    gates: \"Fine on its own.\"\n"
            );
            let model = Model {
                defs: BTreeMap::from([(
                    "configuration.yaml".to_string(),
                    serde_yaml::from_str::<Value>(&spec).expect("parses"),
                )]),
            };
            let mut issues = Vec::new();
            validate_configuration(&model, &mut issues);
            assert!(
                !issues.iter().any(|i| i.rule == "config-required-with-default"),
                "{body:?} is a legal declaration"
            );
        }
    }

    /// A numeric key with NO declared default must resolve to `None`, never to a typed zero.
    ///
    /// The emitter used to substitute `0` for a defaultless `int`, so
    /// `DELIVERY_OFFER_MAX_TTL_SECONDS` — whose real fallback (900s) lives in
    /// `DeliveryOfferTimeoutWorker::new`, which has no Config to read — would have been printed in the
    /// boot report as a delivery-offer ceiling of ZERO SECONDS. An operator reading that would conclude
    /// every offer times out instantly. A report that states a number the process never applies is
    /// worse than one that says `unset`, because it is trusted.
    #[test]
    fn a_numeric_key_without_a_default_resolves_to_absent_not_to_zero() {
        let spec = r#"
keys:
  TTL_SECONDS:
    type: int
    gates: "No default -- the fallback lives at the call site."
  WITH_DEFAULT_SECONDS:
    type: int
    default: 30
    gates: "Declares its own."
"#;
        let model = Model {
            defs: BTreeMap::from([(
                "configuration.yaml".to_string(),
                serde_yaml::from_str::<Value>(spec).expect("parses"),
            )]),
        };
        let emitted = emit_config(&model);
        assert!(
            emitted.contains("pub ttl_seconds: Option<i64>"),
            "a defaultless int must be Option<i64>:\n{emitted}"
        );
        assert!(
            !emitted.contains("raw(\"TTL_SECONDS\").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0)"),
            "a defaultless int must not fall back to 0:\n{emitted}"
        );
        assert!(
            emitted.contains("self.ttl_seconds.map_or_else"),
            "the boot report must print `unset`, not a fabricated number:\n{emitted}"
        );
        // The complement: a declared default is still applied, and still reported as a plain value.
        assert!(
            emitted.contains("pub with_default_seconds: i64")
                && emitted.contains(".unwrap_or(30)"),
            "a declared default must survive this change:\n{emitted}"
        );
    }

    /// The Render sync must resolve repo secrets from the manifest ALONE — never from a second list of
    /// key names maintained by hand in the workflow.
    ///
    /// It did, and the list drifted on its first real run: `HONEYCOMB_API_KEY` and the four `OVH_*`
    /// credentials were declared in `specs/configuration.yaml` AND configured as repo secrets, but
    /// missing from the workflow's `env:` block, so the sync reported "repo secret is not set". That is
    /// the most expensive shape of wrong: it tells an operator who configured the secret correctly that
    /// they did not, and points them at the wrong file. Two lists of the same names is precisely the
    /// drift configuration.yaml exists to abolish.
    #[test]
    fn the_render_sync_takes_its_secret_names_only_from_the_manifest() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let wf = std::fs::read_to_string(root.join(".github/workflows/render-config-sync.yml"))
            .expect("the render-config-sync workflow must exist");
        assert!(
            wf.contains("toJSON(secrets)"),
            "the workflow must source every repo secret as one object, so the manifest is the only \
             list of names"
        );
        // RENDER_API_KEY is the one legitimate direct reference: it is the workflow's own credential
        // for reaching Render, not a value the manifest ever names.
        //
        // Comment lines are skipped, because the comment above this very block quotes the
        // `secrets.NAME` shape it warns against — a rule that cannot survive being explained is a rule
        // people work around by deleting the explanation.
        let direct: Vec<&str> = wf
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .flat_map(|l| {
                l.match_indices("secrets.").map(move |(at, _)| {
                    let tail = &l[at + "secrets.".len()..];
                    let end = tail.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'));
                    &tail[..end.unwrap_or(tail.len())]
                })
            })
            .filter(|n| !n.is_empty())
            .collect();
        assert_eq!(
            direct,
            ["RENDER_API_KEY"],
            "only RENDER_API_KEY may be referenced by name; every other secret is looked up from the \
             manifest. Naming one here recreates the list that drifted."
        );
    }

    /// A key with a DECLARED DEFAULT must be consumed through the generated `Config`, never re-read
    /// from the environment at the call site.
    ///
    /// This is the gap the product owner caught: `WEB_ASSETS_DIR` declared `default: /app/web-assets`,
    /// the generated reader resolved it correctly — and the composition root ignored it, doing its own
    /// `env::var(..).unwrap_or_else(|_| "/app/web-assets")`. Two copies of one default, and the spec's
    /// copy was the inert one. Declaring a default is only meaningful if the declaration is what runs.
    #[test]
    fn a_declared_default_is_not_re_implemented_at_the_call_site() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let spec = std::fs::read_to_string(root.join("specs/configuration.yaml"))
            .expect("specs/configuration.yaml must exist");
        let model = Model {
            defs: BTreeMap::from([(
                "configuration.yaml".to_string(),
                serde_yaml::from_str::<Value>(&spec).expect("configuration.yaml parses"),
            )]),
        };
        let defaulted: BTreeSet<String> = parse_config_keys(&model)
            .into_iter()
            .filter(|k| k.default.is_some() && k.consumer == "server")
            .map(|k| k.name)
            .collect();
        assert!(!defaulted.is_empty(), "no defaulted keys parsed");

        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name != "target" && name != "tests" {
                        walk(&p, out);
                    }
                } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        walk(&root.join("crates/server"), &mut files);
        files.sort();

        let mut offenders = Vec::new();
        for f in &files {
            if f.to_string_lossy().ends_with("generated/config.rs") {
                continue; // the generated reader IS where the default is applied
            }
            let Ok(src) = std::fs::read_to_string(f) else { continue };
            for (idx, line) in src.lines().enumerate() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for key in &defaulted {
                    if line.contains(&format!("env::var(\"{key}\"")) 
                        || line.contains(&format!("env_flag(\"{key}\""))
                    {
                        offenders.push(format!(
                            "  {}:{}: {key}",
                            f.strip_prefix(&root).unwrap_or(f).display(),
                            idx + 1
                        ));
                    }
                }
            }
        }
        offenders.sort();
        assert!(
            offenders.is_empty(),
            "these keys declare a DEFAULT in specs/configuration.yaml but are re-read from the \
             environment at the call site:\n{}\n\n\
             Fix: read them from the resolved `Config` (`config.<field>`) instead. Why: a default \
             declared in the spec and re-typed at the call site is TWO sources of truth, and the \
             spec's copy is the one that turns out to be inert — the declaration then documents a \
             behaviour nothing implements.",
            offenders.join("\n")
        );
    }

    /// Every environment variable the crates READ must be DECLARED in `specs/configuration.yaml`
    /// (PROP-20260729-004500, issue #246).
    ///
    /// This is the rule that stops the inventory rotting again, and it exists because the rot was
    /// measured rather than imagined: `render.yaml` documented 9 of ~21 variables, `RUN_SIRENE_WORKER`
    /// gated a paused pipeline while being written down NOWHERE (6,649 rows sat PENDING for four hours
    /// because of it), and `API_SECRET` sat configured on the production service, read by nothing.
    /// Any hand-maintained list drifts; only a gate keeps one honest.
    ///
    /// Scope: non-test Rust under `crates/**`. Test files legitimately set throwaway variables
    /// (`DATABASE_URL` overrides, `DB_TESTS_REQUIRED`), and the GENERATED reader is itself the thing
    /// being checked, so both are excluded.
    ///
    /// The scan is deliberately NOT a plain search for `env::var("NAME")`. That is how the first
    /// version of this gate was written, and six `OVH_*` credentials stayed invisible to it for weeks:
    /// `OvhSmsClient::from_env` reads them through a closure (`let var = |k: &str| env::var(k)`), so no
    /// key name is ever adjacent to `env::var`. Widening the harvest surfaced 17 more. The three
    /// shapes actually used in this repo are all recognised, and a fourth — a read whose key the scan
    /// cannot attribute at all — is rejected outright rather than passing silently. See
    /// [`env_reads_in`].
    #[test]
    fn every_env_var_read_by_the_crates_is_declared() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let spec = std::fs::read_to_string(root.join("specs/configuration.yaml"))
            .expect("specs/configuration.yaml must exist — it is the configuration source of truth");
        let model = Model {
            defs: BTreeMap::from([(
                "configuration.yaml".to_string(),
                serde_yaml::from_str::<Value>(&spec).expect("configuration.yaml parses"),
            )]),
        };
        let declared: BTreeSet<String> =
            parse_config_keys(&model).into_iter().map(|k| k.name).collect();
        assert!(!declared.is_empty(), "no keys parsed from configuration.yaml");

        // Platform-injected or tooling-only names the APP never reads through its own config.
        let exempt: BTreeSet<&str> = ["DB_TESTS_REQUIRED", "CARGO_MANIFEST_DIR", "OUT_DIR"]
            .into_iter()
            .collect();

        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name != "target" && name != "tests" {
                        walk(&p, out);
                    }
                } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        walk(&root.join("crates"), &mut files);
        files.sort();
        assert!(!files.is_empty(), "found no crate sources to scan");

        let scanned: Vec<&std::path::PathBuf> = files
            .iter()
            // The generated reader legitimately names every key; it IS the declaration's output.
            .filter(|f| !f.to_string_lossy().ends_with("generated/config.rs"))
            .collect();

        // The `*_ENV` const table is built across the WHOLE tree before anything is scanned, because
        // the const and the read routinely live in different files: `stripe::acl` declares
        // `STRIPE_WEBHOOK_SECRET_ENV` and `stripe::http` is what passes it to `env::var`. A per-file
        // table reports four such reads as unresolvable, which is a false alarm, and a gate that cries
        // wolf gets weakened rather than obeyed.
        let mut consts: BTreeMap<String, String> = BTreeMap::new();
        for f in &scanned {
            if let Ok(src) = std::fs::read_to_string(f) {
                consts.extend(env_name_consts_in(&src));
            }
        }

        let mut offenders: Vec<String> = Vec::new();
        let mut blind: Vec<String> = Vec::new();
        for f in &scanned {
            let Ok(src) = std::fs::read_to_string(f) else { continue };
            let rel = f.strip_prefix(&root).unwrap_or(f).display().to_string();
            let scan = env_reads_in(&src, &consts);
            for (line, name) in scan.keys {
                if !declared.contains(&name) && !exempt.contains(name.as_str()) {
                    offenders.push(format!("  {rel}:{line}: {name}"));
                }
            }
            for (line, expr) in scan.blind {
                blind.push(format!("  {rel}:{line}: env::var({expr})"));
            }
        }
        offenders.sort();
        offenders.dedup();
        blind.sort();
        blind.dedup();

        // Reported FIRST: a read the scan cannot attribute makes the offender list above unreliable,
        // so an unattributable read is a defect in its own right rather than a gap to be tolerated.
        assert!(
            blind.is_empty(),
            "these environment reads name their key in a way this gate cannot resolve, so the key \
             could be undeclared and the gate would not notice:\n{}\n\n\
             Fix: read it by string literal (`env::var(\"MY_KEY\")`), or name it with a \
             `const MY_KEY_ENV: &str = \"MY_KEY\";` and pass that. Both forms let the gate see the key; \
             a computed or forwarded name does not.",
            blind.join("\n")
        );
        assert!(
            offenders.is_empty(),
            "these environment variables are READ by the crates but NOT declared in \
             specs/configuration.yaml:\n{}\n\n\
             Fix: declare each one (type, required-per-profile, default, secret, and `gates` — what \
             breaks without it), then `make generate`. Why: an undeclared variable is invisible to the \
             startup validation, to the boot report and to every derived manifest, which is exactly how \
             RUN_SIRENE_WORKER came to gate a production pipeline while being written down nowhere.",
            offenders.join("\n")
        );
    }

    /// Collect `const SOME_NAME: &str = "SOME_KEY";` bindings whose VALUE looks like an environment key,
    /// as `ident -> key`. Gathered tree-wide before scanning, because the const and the `env::var` that
    /// consumes it are usually in different modules of the same crate.
    ///
    /// The value has to look like a key, rather than the identifier having to end in `_ENV`: the naming
    /// convention is how the code documents intent, and making the gate depend on it would mean a const
    /// named anything else silently escapes.
    fn env_name_consts_in(src: &str) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for line in src.lines() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            let Some(at) = line.find("const ") else { continue };
            let tail = &line[at + 6..];
            let Some((ident, rhs)) = tail.split_once(':') else { continue };
            let Some((_, value)) = rhs.split_once('=') else { continue };
            let value = value.trim().trim_end_matches(';').trim();
            let Some(lit) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else { continue };
            let ident = ident.trim();
            if !lit.is_empty()
                && lit.starts_with(|c: char| c.is_ascii_uppercase())
                && lit.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                && ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                out.insert(ident.to_string(), lit.to_string());
            }
        }
        out
    }

    /// What [`env_reads_in`] found in one source file: keys it could attribute, and reads it could not.
    struct EnvScan {
        /// `(1-based line, KEY)` for every environment key the file reads.
        keys: Vec<(usize, String)>,
        /// `(1-based line, argument expression)` for reads whose key could not be attributed.
        blind: Vec<(usize, String)>,
    }

    /// Find every environment key a Rust source reads, across the three shapes this repo uses.
    ///
    /// 1. **Literal** — `env::var("PORT")`, `env_flag("RUN_PROJECTOR", …)`. The obvious one.
    /// 2. **Named const** — `const AVELO37_API_KEY_ENV: &str = "AVELO37_API_KEY";` passed as
    ///    `env::var(AVELO37_API_KEY_ENV)`. The adapters all use this; the const's VALUE is the key.
    /// 3. **Wrapper** — a fn or closure that reads whatever key it is handed
    ///    (`let var = |k: &str| env::var(k).ok()`), called with literals: `var("OVH_ENDPOINT")`. This
    ///    is the shape that hid the OVH credentials, and the reason the scan resolves wrappers by
    ///    name instead of only looking next to `env::var`.
    ///
    /// Anything else — a key assembled at runtime, or forwarded from a caller the scan cannot see —
    /// lands in `blind`. That is not pedantry: an unresolvable read is precisely a read that could name
    /// an undeclared key while the gate reports all clear.
    ///
    /// A line-oriented scan, not a parse. It is checking a convention the repo already follows, and the
    /// convention is what keeps it honest — the alternative is a syn dependency in the codegen to read
    /// six call sites.
    fn env_reads_in(src: &str, known_consts: &BTreeMap<String, String>) -> EnvScan {
        fn is_key(s: &str) -> bool {
            !s.is_empty()
                && s.starts_with(|c: char| c.is_ascii_uppercase())
                && s.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        }
        // `foo("BAR")` / `foo(BAR_ENV)` — the first argument of a call to `name`, verbatim.
        fn first_args<'a>(line: &'a str, name: &str) -> Vec<&'a str> {
            let mut out = Vec::new();
            let mut rest = line;
            while let Some(at) = rest.find(name) {
                let before = rest[..at].chars().next_back();
                let tail = &rest[at + name.len()..];
                rest = tail;
                // Reject an identifier this is merely the tail of (`my_env_flag(`), and the macro form
                // (`env!("CARGO_PKG_VERSION")` is a compile-time read, not a runtime configuration key).
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') || !tail.starts_with('(') {
                    continue;
                }
                let inner = &tail[1..];
                let end = inner.find([',', ')']).unwrap_or(inner.len());
                out.push(inner[..end].trim());
            }
            out
        }

        let lines: Vec<&str> = src.lines().collect();
        let code: Vec<(usize, &str)> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| !l.trim_start().starts_with("//"))
            .map(|(i, l)| (i + 1, *l))
            .collect();

        // Shape 2: this file's own consts, on top of the tree-wide table the caller supplies.
        let mut consts = known_consts.clone();
        consts.extend(env_name_consts_in(src));

        // Shape 3: wrappers — a binding whose body reads the environment, so its own callers carry the
        // key. `env_flag` is the crate-wide one; a local `let var = |k: &str| env::var(k)` is the same
        // thing with a shorter life. Also collect the parameter names such a wrapper forwards, since
        // `env::var(k)` inside the wrapper is a resolved read, not a blind one.
        let mut wrappers: BTreeSet<&str> = BTreeSet::from(["env_flag"]);
        let mut forwarded: BTreeSet<&str> = BTreeSet::new();
        for (i, (_, line)) in code.iter().enumerate() {
            if !line.contains("env::var(") {
                continue;
            }
            // The binding or fn this read belongs to, looking back a few lines for a multi-line
            // signature (`pub fn env_flag(name: &str, …)` reads on the line below its own header).
            for (_, prev) in code[i.saturating_sub(3)..=i].iter() {
                if let Some(rest) = prev.split_once("let ").map(|(_, r)| r) {
                    if let Some((bind, _)) = rest.split_once('=') {
                        let bind = bind.trim().trim_start_matches("mut ").trim();
                        if !bind.is_empty() && bind.chars().all(|c| c.is_alphanumeric() || c == '_') {
                            wrappers.insert(bind);
                        }
                    }
                }
                if let Some(rest) = prev.split_once("fn ").map(|(_, r)| r) {
                    if let Some((f, _)) = rest.split_once('(') {
                        wrappers.insert(f.trim());
                    }
                }
                // `|k: &str|` or `(name: &str,` — the parameter the wrapper forwards to `env::var`.
                let mut hay = *prev;
                while let Some(at) = hay.find(": &str") {
                    let head = &hay[..at];
                    let start = head
                        .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .map(|p| p + 1)
                        .unwrap_or(0);
                    let ident = &head[start..];
                    if !ident.is_empty() {
                        forwarded.insert(ident);
                    }
                    hay = &hay[at + 6..];
                }
            }
        }

        let mut scan = EnvScan { keys: Vec::new(), blind: Vec::new() };
        for (no, line) in &code {
            for wrapper in wrappers.iter().chain(std::iter::once(&"env::var")) {
                for arg in first_args(line, wrapper) {
                    if let Some(lit) = arg.strip_prefix('"').and_then(|a| a.strip_suffix('"')) {
                        if is_key(lit) {
                            scan.keys.push((*no, lit.to_string()));
                        }
                    // A qualified path (`acl::STRIPE_WEBHOOK_SECRET_ENV`) names the same const as the
                    // bare ident an import would give it, so resolve on the last segment.
                    } else if let Some(key) = consts.get(arg.rsplit("::").next().unwrap_or(arg)) {
                        scan.keys.push((*no, key.clone()));
                    } else if *wrapper == "env::var" && !forwarded.contains(arg) {
                        scan.blind.push((*no, arg.to_string()));
                    }
                }
            }
        }
        scan
    }

    /// The gate must see a key read through a closure, a `*_ENV` const, and a plain literal alike —
    /// and must refuse a read it cannot attribute.
    ///
    /// This exists because the closure case is not hypothetical: it is how
    /// `OvhSmsClient::from_env` reads its four credentials, and the previous gate scanned for
    /// `env::var("` so it reported all clear on six undeclared keys. Testing the scanner directly
    /// (rather than only through the repo sweep) is what keeps that from regressing quietly once the
    /// tree happens to be clean.
    #[test]
    fn the_drift_gate_sees_every_shape_of_environment_read() {
        let none = BTreeMap::new();
        let literal = env_reads_in(r#"let p = std::env::var("PORT").ok();"#, &none);
        assert_eq!(literal.keys.iter().map(|(_, k)| k.as_str()).collect::<Vec<_>>(), ["PORT"]);
        assert!(literal.blind.is_empty());

        // Shape 3 — the OVH shape, verbatim. A gate anchored on `env::var("` sees nothing here.
        let closure = env_reads_in(
            r#"
            pub fn from_env() -> Option<Self> {
                let var = |k: &str| std::env::var(k).ok().filter(|s| !s.is_empty());
                Some(Self { key: var("OVH_APPLICATION_KEY")?, sender: var("OVH_SMS_SENDER") })
            }
            "#,
            &none,
        );
        let found: Vec<&str> = closure.keys.iter().map(|(_, k)| k.as_str()).collect();
        assert!(
            found.contains(&"OVH_APPLICATION_KEY") && found.contains(&"OVH_SMS_SENDER"),
            "closure-wrapped reads must be attributed, got {found:?}"
        );
        assert!(
            closure.blind.is_empty(),
            "`env::var(k)` inside the wrapper forwards a parameter -- not a blind read: {:?}",
            closure.blind
        );

        // Shape 2 — the adapter shape: the const's VALUE is the key, not its identifier.
        let named = env_reads_in(
            r#"
            pub const AVELO37_API_KEY_ENV: &str = "AVELO37_API_KEY";
            let key = std::env::var(AVELO37_API_KEY_ENV).ok()?;
            "#,
            &none,
        );
        assert_eq!(named.keys.iter().map(|(_, k)| k.as_str()).collect::<Vec<_>>(), ["AVELO37_API_KEY"]);
        assert!(named.blind.is_empty());

        // The same const declared in ANOTHER file — `stripe::acl` declares the name, `stripe::http`
        // does the reading. Resolving this is why the const table is built tree-wide first.
        let cross = BTreeMap::from([(
            "STRIPE_WEBHOOK_SECRET_ENV".to_string(),
            "STRIPE_WEBHOOK_SECRET".to_string(),
        )]);
        let elsewhere = env_reads_in(
            r#"let secret = match std::env::var(STRIPE_WEBHOOK_SECRET_ENV) { Ok(s) => s, _ => return };"#,
            &cross,
        );
        assert_eq!(
            elsewhere.keys.iter().map(|(_, k)| k.as_str()).collect::<Vec<_>>(),
            ["STRIPE_WEBHOOK_SECRET"]
        );
        assert!(elsewhere.blind.is_empty(), "a cross-file const is resolvable: {:?}", elsewhere.blind);

        // The fourth shape: unresolvable. Reported rather than passed over in silence.
        let computed =
            env_reads_in(r#"let v = std::env::var(format!("PREFIX_{suffix}")).ok();"#, &none);
        assert!(computed.keys.is_empty());
        assert_eq!(computed.blind.len(), 1, "a computed key must be reported: {:?}", computed.blind);

        // `env!` is a compile-time build fact, not runtime configuration — it must not be harvested.
        let macro_read = env_reads_in(r#"let v = env!("CARGO_PKG_VERSION");"#, &none);
        assert!(
            macro_read.keys.is_empty() && macro_read.blind.is_empty(),
            "env! is not a configuration read: {:?} {:?}",
            macro_read.keys,
            macro_read.blind
        );
    }

    /// The `domain` and `application` layers must never reach the telemetry SDK (issue #191's
    /// Definition of Done: "No business/domain crate depends on the telemetry SDK").
    ///
    /// Two different rules, because the layers are not equivalent:
    ///
    /// - `domain` gets NEITHER the SDK nor the `tracing` facade. It is pure DDD; an aggregate that can
    ///   log is an aggregate whose decisions start being shaped by what is convenient to trace.
    /// - `application` may have the `tracing` FACADE (so a saga leg's diagnostics are structured and
    ///   correlated) but never `opentelemetry*` and never `crates/telemetry`. **It may say things; only
    ///   boundaries may measure them.** `c4-l3.yaml` marks `command-handlers` `instrumented: false`, and
    ///   this is what makes that flag true rather than aspirational.
    ///
    /// Enforced as a dependency test rather than left to review, because the failure is silent: adding
    /// `telemetry` to `application` compiles, passes every other test, and quietly moves instrumentation
    /// into the layer the whole architecture exists to keep clean.
    #[test]
    fn domain_and_application_never_depend_on_the_telemetry_sdk() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        // A guard that cannot find its target must FAIL, never silently pass.
        let read = |crate_name: &str| -> String {
            let path = root.join("crates").join(crate_name).join("Cargo.toml");
            std::fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!(
                    "cannot read {} ({e}) -- if the crate moved, fix this guard; do NOT let it pass",
                    path.display()
                )
            })
        };

        // Dependency lines only: a `#`-comment or a doc sentence mentioning opentelemetry is prose,
        // not an edge in the graph, and failing on it would train people to reword comments.
        let dep_names = |manifest: &str| -> Vec<String> {
            manifest
                .lines()
                .map(str::trim)
                .filter(|l| !l.starts_with('#') && l.contains('='))
                .filter_map(|l| l.split('=').next())
                .map(|n| n.trim().trim_matches('"').to_string())
                .filter(|n| !n.is_empty())
                .collect()
        };

        let forbidden_everywhere = ["telemetry", "opentelemetry", "opentelemetry_sdk", "opentelemetry-otlp", "tracing-opentelemetry", "tracing-subscriber"];

        for (crate_name, facade_allowed) in [("domain", false), ("application", true)] {
            let manifest = read(crate_name);
            let deps = dep_names(&manifest);
            for bad in forbidden_everywhere {
                assert!(
                    !deps.iter().any(|d| d == bad),
                    "crates/{crate_name}/Cargo.toml depends on `{bad}`.\n\
                     Fix: move the instrumentation to a FRAMEWORK boundary -- the command bus, event \
                     store, publisher, projectors, saga runner, GraphQL gateway or middleware (the \
                     components marked `instrumented: true` in specs/architecture/c4-l3.yaml).\n\
                     Why: docs/claude/observability.md and issue #191's Definition of Done both require \
                     the business layers to stay free of the telemetry SDK. Beyond architecture, an \
                     aggregate that needs a subscriber to run is an aggregate that cannot be unit-tested."
                );
            }
            let has_facade = deps.iter().any(|d| d == "tracing");
            if !facade_allowed {
                assert!(
                    !has_facade,
                    "crates/{crate_name}/Cargo.toml depends on `tracing`.\n\
                     Fix: remove it. `domain` is pure DDD and logs nothing -- not even through a facade.\n\
                     Why: the domain must be reasonable about entirely on its own terms. `application` \
                     is the innermost layer permitted the facade, and only for events, never spans."
                );
            }
        }
    }

    /// Every span and attribute the `command-acceptance` and `place-order` contracts declare REQUIRED
    /// must actually be constructed somewhere in `crates/telemetry/src/spans.rs`, and every metric they
    /// name must exist in `contract.rs`.
    ///
    /// This is the test that makes issue #191's Definition of Done checkable rather than a claim. The
    /// failure it exists to catch is silent in both directions: a contract can gain a required span that
    /// nothing emits (the observability-agent then reports a violation that looks like broken
    /// instrumentation), and a span name can be typo'd at the call site (the span is still emitted, it
    /// just no longer satisfies the contract naming it). Neither shows up in a compile or a normal test.
    ///
    /// Scoped to those two contracts deliberately: they are the ones the DoD names. The other nine
    /// contracts are not yet emitted, and asserting them here would fail for work this issue does not
    /// claim to have done.
    #[test]
    fn the_required_observability_contracts_are_actually_emitted() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let read = |rel: &str| -> String {
            let path = root.join(rel);
            std::fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!("cannot read {} ({e}) -- fix this guard rather than letting it pass", path.display())
            })
        };

        let obs: serde_yaml::Value = serde_yaml::from_str(&read("specs/observability.yaml"))
            .expect("specs/observability.yaml parses");
        let contract_rs = read("crates/telemetry/src/contract.rs");

        // Only the PRODUCTION half of spans.rs counts. Cutting the test module is not tidiness: the
        // first version of this guard searched the whole file and passed when `command.journal` was
        // renamed to `command.journalx`, because a `#[cfg(test)]` assertion still contained the old
        // literal. A guard satisfied by a test asserting the thing it is meant to verify is worse than
        // no guard at all.
        let spans_all = read("crates/telemetry/src/spans.rs");
        let spans_rs = spans_all.split("#[cfg(test)]").next().unwrap_or(&spans_all).to_string();

        // The span names ACTUALLY CONSTRUCTED: the first string literal of each `info_span!` call.
        // Matching construction sites rather than "the name appears somewhere in the file" is what makes
        // a typo'd or renamed span fail here instead of silently violating its contract at runtime.
        let constructed: std::collections::BTreeSet<String> = {
            let mut out = std::collections::BTreeSet::new();
            let mut rest = spans_rs.as_str();
            while let Some(at) = rest.find("info_span!(") {
                let tail = &rest[at + "info_span!(".len()..];
                if let Some(open) = tail.find('"') {
                    let after = &tail[open + 1..];
                    if let Some(close) = after.find('"') {
                        out.insert(after[..close].to_string());
                    }
                }
                rest = tail;
            }
            out
        };
        assert!(
            !constructed.is_empty(),
            "parsed no info_span! call sites out of crates/telemetry/src/spans.rs -- the guard is \
             broken, not the code. Fix the parser rather than deleting the test."
        );

        let mut missing: Vec<String> = Vec::new();
        for feature in ["command-acceptance", "place-order"] {
            let node = obs.get(feature).unwrap_or_else(|| {
                panic!("specs/observability.yaml no longer declares the '{feature}' contract")
            });

            // Required spans: the span NAME must be built in spans.rs, and each of its required
            // attribute KEYS must appear there too (as a declared field or a `record` target).
            let spans = node.get("spans").and_then(|s| s.as_sequence()).unwrap_or_else(|| {
                panic!("'{feature}' declares no spans")
            });
            for sp in spans {
                let required = sp.get("required").and_then(|r| r.as_bool()).unwrap_or(false);
                if !required {
                    continue;
                }
                let name = sp.get("name").and_then(|n| n.as_str()).unwrap_or_default();
                if !constructed.contains(name) {
                    missing.push(format!("  {feature}: span '{name}' is required but never constructed"));
                    continue;
                }
                for at in sp.get("attributes").and_then(|a| a.as_sequence()).map(|s| s.as_slice()).unwrap_or(&[]) {
                    if !at.get("required").and_then(|r| r.as_bool()).unwrap_or(false) {
                        continue;
                    }
                    let key = at.get("key").and_then(|k| k.as_str()).unwrap_or_default();
                    // Match a real tracing FIELD ASSIGNMENT (`business.foo = ...`) or an exactly-quoted
                    // constant in contract.rs. Both need the delimiter: a bare `contains(key)` is
                    // satisfied by any longer name that merely starts with it, so renaming
                    // `business.dispatch_outcome` to `business.dispatch_outcomeX` slipped past the first
                    // version of this check. Prefix matching is not name matching.
                    let as_field = format!("{key} = ");
                    if !spans_rs.contains(&as_field) && !contract_rs.contains(&format!("\"{key}\"")) {
                        missing.push(format!(
                            "  {feature}: span '{name}' requires attribute '{key}', which is set nowhere"
                        ));
                    }
                }
            }

            // Metrics AND business_metrics: both blocks are part of the contract, and the split between
            // them is itself required (technical vs BAM), so neither may be skipped.
            for block in ["metrics", "business_metrics"] {
                for m in node.get(block).and_then(|m| m.as_sequence()).map(|s| s.as_slice()).unwrap_or(&[]) {
                    let name = m.get("name").and_then(|n| n.as_str()).unwrap_or_default();
                    if !contract_rs.contains(&format!("\"{name}\"")) {
                        missing.push(format!(
                            "  {feature}: {block} '{name}' has no constant in contract.rs"
                        ));
                    }
                }
            }
        }

        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "specs/observability.yaml requires telemetry that the code does not emit:\n{}\n\n\
             Fix: add the span/attribute/metric in crates/telemetry (spans.rs + contract.rs) and emit it \
             from the FRAMEWORK boundary that owns it.\n\
             Why: an unemitted required span makes the observability-agent report a contract violation \
             that reads as broken instrumentation, and a contract nothing satisfies is the state issue \
             #191 was filed to end -- 898 lines of guarantees, none of them true.",
            missing.join("\n")
        );
    }

    /// The generated config reader must enforce the pattern the SPEC declares -- byte for byte.
    ///
    /// It did not. The emitter escaped each pattern for a normal Rust string literal
    /// (`\\` -> `\\\\`) and then wrote it into a RAW one (`r"..."`), where escapes are not processed.
    /// So `^(0(\\.[0-9]+)?|1(\\.0+)?)$` reached the regex engine with a LITERAL backslash in it and could
    /// never match `1.0` -- the app reported its own baked, valid default as INVALID. On the
    /// `development` profile that is a warning and the boot continues; on **production and staging the
    /// boot is REFUSED**, so this was a latent production-boot blocker that only stayed hidden because
    /// production was running the development profile.
    ///
    /// `make validate` cannot catch it: the validator compiles the DECODED pattern from the spec and is
    /// perfectly happy. The defect exists only in the emitted text, which is exactly what this asserts.
    #[test]
    fn generated_config_patterns_match_the_spec_byte_for_byte() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let generated = std::fs::read_to_string(root.join("crates/server/src/generated/config.rs"))
            .expect("generated config reader must exist -- run `make generate`");
        let scalars = std::fs::read_to_string(root.join("specs/scalars.yaml"))
            .expect("specs/scalars.yaml must exist");
        let scalars: serde_yaml::Value =
            serde_yaml::from_str(&scalars).expect("scalars.yaml parses");

        // The raw-literal form is the bug itself: escapes written for a normal literal, emitted where
        // they are taken verbatim. Pinned directly so a revert fails loudly rather than subtly.
        assert!(
            !generated.contains("matches_pattern(r\"") && !generated.contains("pattern: r\""),
            "config patterns must be emitted as NORMAL string literals -- a raw literal takes the \
             emitter's escaping verbatim and doubles every backslash in the regex"
        );

        // `scalar: "NAME", pattern: "LITERAL"` -- LITERAL may contain escaped quotes.
        let re = regex::Regex::new(r#"scalar: "([A-Za-z0-9_]+)", pattern: "((?:[^"\\]|\\.)*)""#)
            .expect("extractor compiles");
        let found: Vec<_> = re.captures_iter(&generated).collect();
        assert!(
            !found.is_empty(),
            "no pattern literals found in the generated reader -- the extractor or the emitted shape \
             changed, and a silently-empty guard is worse than none"
        );

        for c in found {
            let name = &c[1];
            // Undo Rust's normal-string escaping to recover what the regex engine actually receives.
            let mut actual = String::new();
            let mut chars = c[2].chars();
            while let Some(ch) = chars.next() {
                if ch == '\\' {
                    match chars.next() {
                        Some(next) => actual.push(next), // \\ -> \ , \" -> "
                        None => actual.push(ch),
                    }
                } else {
                    actual.push(ch);
                }
            }
            let expected = scalars
                .get(name)
                .and_then(|s| s.get("pattern"))
                .and_then(|p| p.as_str())
                .unwrap_or_else(|| panic!("scalar {name} has no pattern in scalars.yaml"));
            assert_eq!(
                actual, expected,
                "scalar {name}: the generated reader enforces a DIFFERENT regex than the spec declares"
            );
            regex::Regex::new(&actual)
                .unwrap_or_else(|e| panic!("scalar {name}: emitted pattern does not compile: {e}"));
        }
    }

    /// The `/health` readiness gate moves in the SAME commit as any migration the binary depends
    /// on. Prose failed three times in one week (#279 nine migrations stale; then twice on
    /// 2026-08-02/03, each within hours of a fix whose doc says exactly this) — so the rule is now
    /// executable: the constant must equal the NEWEST migration timestamp, always. Deploy runs
    /// BEFORE db-migrate (ADR-20260730-051500), and this gate holding the new binary at 503 is the
    /// only thing covering that window; stale = inert for precisely the failure it exists to catch.
    #[test]
    fn required_schema_version_matches_the_latest_migration() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let migrations_dir = root.join("migrations");
        let mut latest: i64 = 0;
        let mut counted = 0usize;
        for entry in std::fs::read_dir(&migrations_dir).unwrap_or_else(|e| {
            panic!(
                "cannot read {} ({e}) — if the migrations moved, fix this path; do NOT let this \
                 guard silently pass",
                migrations_dir.display()
            )
        }) {
            let name = entry.expect("dirent").file_name().to_string_lossy().into_owned();
            let Some((prefix, _)) = name.split_once('_') else { continue };
            if let Ok(ts) = prefix.parse::<i64>() {
                latest = latest.max(ts);
                counted += 1;
            }
        }
        assert!(
            counted > 0,
            "no timestamped migrations found under {} — the guard would be vacuous",
            migrations_dir.display()
        );
        let lib_path = root.join("crates/server/src/lib.rs");
        let lib = std::fs::read_to_string(&lib_path)
            .unwrap_or_else(|e| panic!("cannot read {} ({e})", lib_path.display()));
        let declared: i64 = lib
            .lines()
            .find_map(|l| {
                l.trim()
                    .strip_prefix("pub const REQUIRED_SCHEMA_VERSION: i64 = ")
                    .and_then(|rest| rest.trim_end_matches(';').trim().parse().ok())
            })
            .expect(
                "REQUIRED_SCHEMA_VERSION not found in crates/server/src/lib.rs — if the constant \
                 was renamed or moved, update this guard in the same commit",
            );
        assert_eq!(
            declared, latest,
            "REQUIRED_SCHEMA_VERSION ({declared}) != newest migration ({latest}): the /health \
             readiness gate is STALE. It must be bumped in the SAME commit as the migration — a \
             new binary that needs the migration would otherwise report ok and take traffic \
             through the deploy-then-migrate window."
        );
    }

    // NOTE (#284 slice 3): this fn had LOST its `#[test]` attribute — a stray duplicate sat on
    // `generated_config_patterns_match_the_spec_byte_for_byte` above, so the guard silently never
    // ran (rustc accepts a duplicate `#[test]`, and dead test-module fns only warn). Restored.
    #[test]
    fn makefile_recipe_lines_are_ascii() {
        // CARGO_MANIFEST_DIR (= tools/codegen-rs) is the one anchor that holds both locally and
        // in CI; the repo Makefile is two levels up. A guard that silently no-ops when it cannot
        // find its target is worse than no guard, so a missing Makefile FAILS, never skips.
        let makefile = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Makefile");
        let bytes = std::fs::read(&makefile).unwrap_or_else(|e| {
            panic!(
                "cannot read the repo Makefile at {} ({e}) — if the crate moved, fix this path; \
                 do NOT let this guard silently pass",
                makefile.display()
            )
        });
        let text = String::from_utf8_lossy(&bytes);
        let offenders: Vec<String> = text
            .lines()
            .enumerate()
            .filter(|(_, line)| line.starts_with('\t') && !line.is_ascii())
            .map(|(idx, line)| format!("  Makefile:{}: {}", idx + 1, line.trim_end()))
            .collect();
        assert!(
            offenders.is_empty(),
            "Makefile RECIPE lines (tab-indented) must be pure ASCII, but these are not:\n{}\n\
             Fix: replace typographic characters with ASCII equivalents — `--` for `—` (em dash), \
             `->` for `→`, `|` for `·`.\n\
             Why: native Windows GNU Make passes each recipe line to Cygwin's `sh` with broken \
             quoting once the line contains a byte > 127 — `sh` receives the WHOLE recipe as one \
             word and fails with `$'...': command not found`, making the target fail for a reason \
             unrelated to what it does (an em dash in the `check-drift` message once broke \
             `make rust` while there was zero drift). Non-recipe lines (comments, variable \
             assignments) may keep non-ASCII; only tab-indented command text is affected.",
            offenders.join("\n")
        );
    }

    /// The mailbox door stays CLOSED (#284 slice 3, PROP-20260728-152752 §2.1; #290 phase 1,
    /// PROP-20260802-130500 D1): a `MailboxEntry` may be assembled only inside the actor_client
    /// boundary crate — the shared constructors in `actor_client::enqueue`, the reminders
    /// constructor (`actor_client::reminders::scheduled_entry`), the type's own module
    /// (definition + mem double + the D5 fixtures), or tests going through the fixture door. Any
    /// other construction site is a NEW door the typed clients cannot guard — and since #290
    /// phase 1 it also fails to COMPILE (pub(crate) fields), so this scan is belt-and-braces on
    /// the boundary crate itself, where an in-crate shortcut around the shared constructors would
    /// still build.
    ///
    /// Style of `makefile_recipe_lines_are_ascii`: executable, loud, never skips — every
    /// allowlisted path is asserted to exist AND to still contain the construction it excuses, so
    /// the guard fails loudly if its targets move instead of silently no-oping. The scan matches
    /// Whitespace-tolerant detector for `MailboxEntry {` — `MailboxEntry{`, a line break before the
    /// brace, or extra spaces must not slip past the guard (the #292 review's evasion NIT). A `use
    /// … as` alias still would; the compiler enforcement above is what closes that for good.
    fn mentions_entry_construction(text: &str) -> bool {
        let mut rest = text;
        while let Some(i) = rest.find("MailboxEntry") {
            let after = &rest[i + "MailboxEntry".len()..];
            if after.trim_start().starts_with('{') {
                return true;
            }
            rest = after;
        }
        false
    }

    /// `MailboxEntry {` (construction OR destructuring); a destructuring in a new src file trips
    /// it too, on purpose — naming the file in the allowlist is a conscious decision, not noise.
    #[test]
    fn mailbox_entry_is_constructed_only_behind_the_typed_doors() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = root.canonicalize().expect("repo root resolves");

        // The sanctioned constructor sites, relative to the repo root. Since #290 phase 1
        // (PROP-20260802-130500 D1) ALL of them live inside the actor_client boundary crate —
        // `MailboxEntry` fields are pub(crate) there, so the COMPILER now enforces what this scan
        // tripwires: an entry construction anywhere else no longer merely fails this test, it
        // fails to build. The scan stays as belt-and-braces on the boundary crate itself (an
        // in-crate shortcut around the shared constructors would still compile).
        const ALLOWED: &[(&str, &str)] = &[
            // The type itself + the mem double's seeding + the D5 test-fixtures conversions.
            ("crates/actor_client/src/mailbox.rs", "the MailboxEntry definition + mem double + fixtures"),
            // `scheduled_entry`: the generated-reminders row constructor the in-tx `schedules:`
            // upsert (`infrastructure::mailbox::apply_schedules_in_tx`) binds from (ADR-20260731-214500).
            ("crates/actor_client/src/reminders.rs", "the reminders scheduled_entry constructor"),
            // The shared crate-internal constructors every door (typed client or bulk path) delegates to.
            ("crates/actor_client/src/enqueue.rs", "the shared enqueue constructors"),
        ];
        for (rel, why) in ALLOWED {
            let path = root.join(rel);
            let src = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!(
                    "allowlisted constructor file {rel} ({why}) cannot be read ({e}) — if it \
                     moved, move this allowlist WITH it; do NOT let this guard silently no-op"
                )
            });
            assert!(
                mentions_entry_construction(&src),
                "allowlisted file {rel} ({why}) no longer constructs MailboxEntry — the \
                 constructor moved; move this allowlist entry with it so the guard stays real"
            );
        }

        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    if p.file_name().and_then(|n| n.to_str()) != Some("target") {
                        walk(&p, out);
                    }
                } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        walk(&root.join("crates"), &mut files);
        files.sort();
        assert!(!files.is_empty(), "found no crate sources to scan");

        let mut offenders: Vec<String> = Vec::new();
        let mut hits = 0usize;
        for f in &files {
            let rel = f.strip_prefix(&root).unwrap_or(f);
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            let Ok(src) = std::fs::read_to_string(f) else {
                panic!("cannot read {rel_str} — a partially-scanned tree is a silent no-op");
            };
            for (idx, line) in src.lines().enumerate() {
                if !mentions_entry_construction(line) {
                    continue;
                }
                hits += 1;
                let allowed = ALLOWED.iter().any(|(a, _)| rel_str == *a)
                    // Integration tests seed rows directly (they ARE the thing behind the door).
                    || rel.components().any(|c| c.as_os_str() == "tests");
                if !allowed {
                    offenders.push(format!("  {rel_str}:{}: {}", idx + 1, line.trim()));
                }
            }
        }
        assert!(
            hits >= ALLOWED.len(),
            "the scan found fewer `MailboxEntry {{` sites ({hits}) than the allowlist excuses — \
             the pattern or the type was renamed and this guard went blind; fix the scan"
        );
        assert!(
            offenders.is_empty(),
            "`MailboxEntry {{` is constructed outside the sanctioned doors:\n{}\n\n\
             Fix: go through a generated typed actor client \
             (actor_client::generated::actor_clients — send/record/schedule), or, for \
             actor_client-internal machinery, the shared constructors in \
             actor_client::enqueue. If this is genuinely a new sanctioned constructor, \
             add it to this test's allowlist WITH its justification. Why: a hand-assembled row \
             bypasses the one derivation every door shares (lane, partition, principal, channel, \
             deterministic identity) — the exact drift #284's typed clients exist to prevent.",
            offenders.join("\n")
        );
    }

    /// Does this syntax node mention `name` anywhere in its token stream? Path-spelling agnostic,
    /// so `MailboxAccess`, `mailbox::MailboxAccess` and `crate::mailbox::MailboxAccess` all match.
    fn mentions(node: &impl quote::ToTokens, name: &str) -> bool {
        node.to_token_stream().to_string().contains(name)
    }

    /// Is this type EXACTLY the witness — not a wrapper around it?
    ///
    /// Used ONLY for the port trait's parameters, where a wrapper weakens the demand:
    /// `access: Option<MailboxAccess>` *mentions* the witness, so a substring check accepts it,
    /// and then the caller writes `mb.cancel_scheduled(id, None)` and needs no witness at all.
    /// Review pass 4 defeated the guard's own primary assertion that way.
    fn is_exact_witness(ty: &syn::Type) -> bool {
        let syn::Type::Path(p) = ty else { return false };
        p.qself.is_none()
            && p.path.segments.last().is_some_and(|s| {
                s.ident == WITNESS && matches!(s.arguments, syn::PathArguments::None)
            })
    }

    /// Is this `#[cfg(..)]` satisfied ONLY in a test build — `test`, the `test-fixtures` feature,
    /// or both? Such an item is never compiled for a dependent crate, so it cannot leak.
    ///
    /// An INVERTED cfg is not a gate and must not be read as one: `#[cfg(not(feature =
    /// "test-fixtures"))]` names the feature while compiling in exactly the builds where it is
    /// OFF — every release build. Pass 4 used that to make the guard's own excuse mechanism grant
    /// the mint.
    fn is_test_only_cfg(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|a| {
            if !a.path().is_ident("cfg") {
                return false;
            }
            let m = &a.meta;
            let toks = quote::quote!(#m).to_string();
            // WHOLE tokens: a substring match honours `#[cfg(feature = "fastest")]` as a test
            // gate and stops scanning everything under it, in release.
            cfg_tokens(&toks).iter().any(|t| *t == "test" || *t == "test-fixtures")
                && !toks.contains("not (")
        })
    }

    /// A cfg attribute's token stream split into whole words (keeping `-`/`_` so feature names
    /// stay intact).
    fn cfg_tokens(toks: &str) -> Vec<&str> {
        toks.split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'))
            .filter(|t| !t.is_empty())
            .collect()
    }

    /// Specifically a POSITIVE `test-fixtures` gate — what makes the one public mint legitimate.
    fn is_fixtures_gate(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|a| {
            a.path().is_ident("cfg") && {
                let m = &a.meta;
                let t = quote::quote!(#m).to_string();
                cfg_tokens(&t).contains(&"test-fixtures") && !t.contains("not (")
            }
        })
    }

    /// Everything the #304 scan needs to say about one `actor_client` source.
    #[derive(Default)]
    struct WitnessScan {
        leaks: Vec<String>,
        /// Methods of the `Mailbox` port trait: (name, takes the witness EXACTLY).
        port_methods: Vec<(String, bool)>,
        saw_port_trait: bool,
    }

    const WITNESS: &str = "MailboxAccess";

    impl WitnessScan {
        /// THE RULE, and it is a CLOSED one: the witness may not appear in ANY release-reachable
        /// public signature, anywhere in the crate, except on the closed exemption list (the
        /// `Mailbox` trait's own items, `impl Mailbox for _` blocks, and the cfg-gated
        /// `MailboxAccess::for_tests`).
        ///
        /// Five review passes killed every version of this guard that asked WHERE the witness
        /// appears. Text forms, then item kinds, then output-and-field positions — each framing
        /// left a slot uninspected, and the last one left two: a generic BOUND
        /// (`pub fn mint<T: From<MailboxAccess>>() -> T`) and a PARAMETER on a non-port item
        /// (`pub fn with_access(f: impl FnOnce(MailboxAccess) -> R) -> R`, a scoped-capability
        /// helper written `pub` by accident — the most plausible real mistake of the whole set).
        /// So this asks nothing about position: the whole signature — generics, where-clause,
        /// inputs, output, field and variant types — is one token stream, and any mention fails.
        fn check_sig(&mut self, rel: &str, what: &str, public: bool, sig: &impl quote::ToTokens) {
            if public && mentions(sig, WITNESS) {
                self.leaks.push(format!("  {rel}: {what} names the witness in a public signature"));
            }
        }
    }

    fn is_pub(vis: &syn::Visibility) -> bool {
        matches!(vis, syn::Visibility::Public(_))
    }

    /// Walk one module's items. `test_only` tracks whether an ancestor cfg makes everything here
    /// invisible to dependent crates (so it cannot leak); `is_port` whether this is the port
    /// module, the one place the witness's own declaration and impls may live.
    fn scan_items(items: &[syn::Item], rel: &str, test_only: bool, is_port: bool, out: &mut WitnessScan) {
        use syn::Item;
        for item in items {
            let item_test_only = test_only || is_test_only_cfg(item_attrs(item));
            match item {
                Item::Mod(m) => {
                    let pathy = m.attrs.iter().any(|a| {
                        a.path().is_ident("path") || {
                            // `#[cfg_attr(<cond>, path = "..")]` is a `#[path]` in disguise, and
                            // the condition can be arranged to fire in release. The escape hatch
                            // needs the same scrutiny as the rule it guards.
                            let mm = &a.meta;
                            a.path().is_ident("cfg_attr")
                                && quote::quote!(#mm).to_string().contains("path =")
                        }
                    });
                    if pathy {
                        out.leaks.push(format!(
                            "  {rel}: `mod {}` carries `#[path]` — this guard walks the directory, \
                             so a file outside `crates/actor_client/src` is invisible to it",
                            m.ident
                        ));
                    }
                    if let Some((_, inner)) = &m.content {
                        scan_items(inner, rel, item_test_only, is_port, out);
                    }
                }

                // THE PORT TRAIT — every method must take the witness, EXACTLY (not wrapped).
                Item::Trait(t) if t.ident == "Mailbox" => {
                    out.saw_port_trait = true;
                    for ti in &t.items {
                        match ti {
                            syn::TraitItem::Fn(f) => out.port_methods.push((
                                f.sig.ident.to_string(),
                                f.sig.inputs.iter().any(|i| match i {
                                    syn::FnArg::Typed(pt) => is_exact_witness(&pt.ty),
                                    syn::FnArg::Receiver(_) => false,
                                }),
                            )),
                            // Associated items on the port trait. Not exploitable today only
                            // because `Mailbox` is used as `dyn` and such a const makes it
                            // dyn-incompatible (E0038) — safety borrowed from a usage pattern
                            // elsewhere, so it is owned here instead.
                            syn::TraitItem::Const(c) => {
                                out.check_sig(rel, &format!("`Mailbox::{}`", c.ident), true, &c.ty)
                            }
                            syn::TraitItem::Type(ty) => {
                                let b = &ty.bounds;
                                out.check_sig(
                                    rel,
                                    &format!("`Mailbox::{}` bounds", ty.ident),
                                    true,
                                    &quote::quote!(#b),
                                )
                            }
                            // A macro INVOCATION expands to items this walk can never see.
                            syn::TraitItem::Macro(m) => out.leaks.push(format!(
                                "  {rel}: `Mailbox` contains a macro invocation (`{}!`) — trait \
                                 items must be written out, or this guard cannot see them",
                                m.mac.path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default()
                            )),
                            _ => {}
                        }
                    }
                }

                // Every OTHER public trait: `Mailbox` as a supertrait (bounds OR where-clause) is
                // an ungated door for any port holder; and each item's WHOLE signature is checked.
                Item::Trait(t) if is_pub(&t.vis) && !item_test_only => {
                    let st = &t.supertraits;
                    let supers = quote::quote!(#st).to_string();
                    let wheres = t
                        .generics
                        .where_clause
                        .as_ref()
                        .map(|w| quote::quote!(#w).to_string())
                        .unwrap_or_default();
                    if supers.contains("Mailbox") || wheres.contains("Mailbox") {
                        out.leaks.push(format!(
                            "  {rel}: `pub trait {}` has `Mailbox` as a supertrait (bound or \
                             `where` clause) — a provided method on it reaches \
                             `cancel_scheduled`/`by_message` with a witness it mints itself, for \
                             any port holder",
                            t.ident
                        ));
                    }
                    for ti in &t.items {
                        match ti {
                            syn::TraitItem::Fn(f) => out.check_sig(
                                rel,
                                &format!("`{}::{}`", t.ident, f.sig.ident),
                                true,
                                &f.sig,
                            ),
                            syn::TraitItem::Const(c) => out.check_sig(
                                rel,
                                &format!("`{}::{}`", t.ident, c.ident),
                                true,
                                &c.ty,
                            ),
                            // An associated TYPE's bounds are a signature slot too:
                            // `type Out: From<MailboxAccess>` hands one over.
                            syn::TraitItem::Type(ty) => {
                                let b = &ty.bounds;
                                out.check_sig(
                                    rel,
                                    &format!("`{}::{}` bounds", t.ident, ty.ident),
                                    true,
                                    &quote::quote!(#b),
                                )
                            }
                            _ => {}
                        }
                    }
                }

                Item::Impl(i) => {
                    let on_witness = mentions(&i.self_ty, WITNESS);
                    let trait_impl = i.trait_.is_some();
                    // EXEMPTION: an `impl Mailbox for _` legitimately names the witness in every
                    // method — that IS the port contract.
                    let is_port_impl = i
                        .trait_
                        .as_ref()
                        .and_then(|(_, p, _)| p.segments.last())
                        .is_some_and(|s| s.ident == "Mailbox");
                    if on_witness && trait_impl {
                        out.leaks.push(format!(
                            "  {rel}: a trait impl on the witness (`{}`) — `Default`, `From`, \
                             `FromStr` and friends are all public mints",
                            i.trait_.as_ref().map(|(_, p, _)| quote::quote!(#p).to_string()).unwrap_or_default()
                        ));
                    } else if on_witness && !is_port {
                        out.leaks.push(format!(
                            "  {rel}: an inherent impl on the witness outside the port module. \
                             (Keep them in crates/actor_client/src/mailbox.rs — this guard does \
                             not resolve out-of-line modules, so moving them costs the gate.)"
                        ));
                    }
                    // The supertrait rule's TWIN SPELLING: `impl<T: Mailbox> Ext for T` is at
                    // least as idiomatic as `trait Ext: Mailbox`, and reaches the port exactly the
                    // same way. Blocking one and waving the other through is not a rule.
                    if !is_port_impl {
                        // BOTH slots, mirroring the trait arm: `ToTokens for Generics` emits the
                        // `<..>` params WITHOUT the where-clause, so reading `i.generics` alone
                        // blocks `impl<T: Mailbox>` and waves `impl<T> .. where T: Mailbox`
                        // through — the very spelling-vs-class mistake this arm exists to fix.
                        let g = &i.generics;
                        let w = i
                            .generics
                            .where_clause
                            .as_ref()
                            .map(|w| quote::quote!(#w).to_string())
                            .unwrap_or_default();
                        if quote::quote!(#g).to_string().contains("Mailbox") || w.contains("Mailbox") {
                            out.leaks.push(format!(
                                "  {rel}: a blanket `impl<T: Mailbox>` — a method on it reaches \
                                 `cancel_scheduled`/`by_message` with a witness it mints itself, \
                                 for any port holder (same door as a `Mailbox` supertrait)"
                            ));
                        }
                    }
                    if is_port_impl {
                        continue;
                    }
                    for ii in &i.items {
                        let ii_test_only = item_test_only || is_test_only_cfg(impl_item_attrs(ii));
                        match ii {
                            syn::ImplItem::Fn(f) => {
                                let public = trait_impl || is_pub(&f.vis);
                                if on_witness && public {
                                    // The ONE sanctioned public mint: `for_tests`, under a
                                    // POSITIVE test-fixtures gate.
                                    let sanctioned = is_port
                                        && f.sig.ident == "for_tests"
                                        && (is_fixtures_gate(&f.attrs) || fixtures_gated(rel, item_test_only));
                                    if !sanctioned {
                                        out.leaks.push(format!(
                                            "  {rel}: `MailboxAccess::{}` is public outside the \
                                             cfg-gated fixtures module",
                                            f.sig.ident
                                        ));
                                    }
                                } else if !ii_test_only {
                                    out.check_sig(
                                        rel,
                                        &format!("associated fn `{}`", f.sig.ident),
                                        public,
                                        &f.sig,
                                    );
                                }
                            }
                            syn::ImplItem::Const(c) if !ii_test_only => out.check_sig(
                                rel,
                                &format!("associated const `{}`", c.ident),
                                trait_impl || is_pub(&c.vis),
                                &c.ty,
                            ),
                            syn::ImplItem::Type(t) if !ii_test_only => out.check_sig(
                                rel,
                                &format!("associated type `{}`", t.ident),
                                trait_impl || is_pub(&t.vis),
                                &t.ty,
                            ),
                            _ => {}
                        }
                    }
                }

                // Free items — the WHOLE signature, so a generic bound or a callback parameter is
                // as visible as a return type.
                Item::Fn(f) if !item_test_only => {
                    out.check_sig(rel, &format!("`pub fn {}`", f.sig.ident), is_pub(&f.vis), &f.sig)
                }
                Item::Const(c) if !item_test_only => {
                    out.check_sig(rel, &format!("`const {}`", c.ident), is_pub(&c.vis), &c.ty)
                }
                Item::Static(s) if !item_test_only => {
                    out.check_sig(rel, &format!("`static {}`", s.ident), is_pub(&s.vis), &s.ty)
                }
                Item::Type(t) if mentions(&t.ty, WITNESS) => {
                    out.leaks.push(format!(
                        "  {rel}: `type {} = MailboxAccess` — an alias lets a later public item \
                         yield the witness without ever naming it",
                        t.ident
                    ));
                }

                Item::Struct(s) => {
                    for (n, f) in s.fields.iter().enumerate() {
                        if s.ident == WITNESS {
                            if is_pub(&f.vis) {
                                out.leaks.push(format!(
                                    "  {rel}: the witness's field is `pub` — `MailboxAccess(())` \
                                     now compiles in every crate and the entire port surface \
                                     re-opens"
                                ));
                            }
                        } else if !item_test_only {
                            let name = f.ident.as_ref().map(|i| i.to_string()).unwrap_or_else(|| n.to_string());
                            out.check_sig(
                                rel,
                                &format!("field `{}.{name}`", s.ident),
                                is_pub(&s.vis) && is_pub(&f.vis),
                                &f.ty,
                            );
                        }
                    }
                }
                Item::Enum(e) if is_pub(&e.vis) && !item_test_only => {
                    for v in &e.variants {
                        for (n, f) in v.fields.iter().enumerate() {
                            let name = f.ident.as_ref().map(|i| i.to_string()).unwrap_or_else(|| n.to_string());
                            out.check_sig(
                                rel,
                                &format!("variant field `{}::{}.{name}`", e.ident, v.ident),
                                true,
                                &f.ty,
                            );
                        }
                    }
                }
                Item::Union(u) if is_pub(&u.vis) && !item_test_only => {
                    for f in &u.fields.named {
                        out.check_sig(
                            rel,
                            &format!("union field `{}`", u.ident),
                            is_pub(&f.vis),
                            &f.ty,
                        );
                    }
                }
                Item::ForeignMod(fm) if !item_test_only => out.check_sig(
                    rel,
                    "an `extern` block",
                    true,
                    &quote::quote!(#fm),
                ),

                // EXPANSION this walk cannot follow. Matched on the path's LAST SEGMENT and on
                // the invocation as well as the definition: `std::include!` walked past an
                // `is_ident("include")` check, and `forge!(crate::mailbox::MailboxAccess)` forged
                // a public mint while only the `macro_rules!` DEFINITION was being inspected.
                Item::Macro(m) => {
                    let name = m
                        .mac
                        .path
                        .segments
                        .last()
                        .map(|s| s.ident.to_string())
                        .unwrap_or_default();
                    if name == "include" {
                        out.leaks.push(format!(
                            "  {rel}: `include!` splices a file this guard never walks — keep \
                             every module a real file under `crates/actor_client/src`"
                        ));
                    } else if mentions(&m.mac.tokens, WITNESS) {
                        out.leaks.push(format!(
                            "  {rel}: an item-position macro (`{name}!`) carries the witness as a \
                             token — expansion is invisible to this guard, so write the item out"
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    /// `mailbox.rs`'s `fixtures` module is the sanctioned home of the public mint; an item is
    /// inside it when an ancestor carried the positive gate.
    fn fixtures_gated(rel: &str, ancestor_test_only: bool) -> bool {
        rel.ends_with("mailbox.rs") && ancestor_test_only
    }

    fn item_attrs(i: &syn::Item) -> &[syn::Attribute] {
        use syn::Item::*;
        match i {
            Const(x) => &x.attrs, Enum(x) => &x.attrs, ExternCrate(x) => &x.attrs, Fn(x) => &x.attrs,
            ForeignMod(x) => &x.attrs, Impl(x) => &x.attrs, Macro(x) => &x.attrs, Mod(x) => &x.attrs,
            Static(x) => &x.attrs, Struct(x) => &x.attrs, Trait(x) => &x.attrs, TraitAlias(x) => &x.attrs,
            Type(x) => &x.attrs, Union(x) => &x.attrs, Use(x) => &x.attrs, _ => &[],
        }
    }

    fn impl_item_attrs(i: &syn::ImplItem) -> &[syn::Attribute] {
        use syn::ImplItem::*;
        match i {
            Const(x) => &x.attrs, Fn(x) => &x.attrs, Type(x) => &x.attrs, Macro(x) => &x.attrs, _ => &[],
        }
    }

    /// The `Mailbox` PORT SURFACE (#304, PROP-20260802-130500 §5 directive): every method of the
    /// port demands a `MailboxAccess` witness, so holding an `Arc<dyn Mailbox>` is not holding the
    /// door — only `actor_client` can mint one, and the compiler refuses every call from anywhere
    /// else.
    ///
    /// WHAT THE COMPILER DOES, AND WHAT THIS DOES. The compiler makes the rule unbreakable from
    /// OUTSIDE the boundary crate: no out-of-crate caller can mint a witness, so no port method is
    /// callable. Every remaining way to reopen the door is an edit INSIDE `actor_client`, and this
    /// is what catches those.
    ///
    /// IT PARSES THE AST, and that is the whole point. Three review passes each defeated a
    /// string-matching version of this guard, by shapes that were not clever — `pub  fn` with two
    /// spaces, a split signature, `impl Default for MailboxAccess`, `pub const KEY: MailboxAccess`,
    /// a type alias, an extension trait one file over. Every one of those is the SAME rule at the
    /// AST level ("a public item that yields the witness"), so parsing replaces a patch-per-shape
    /// treadmill with four structural rules. What it still cannot see is macro EXPANSION — so a
    /// `macro_rules!` mentioning the witness, and a macro invocation inside the port trait, are
    /// refused outright rather than waved through.
    #[test]
    fn every_mailbox_port_method_demands_the_access_witness() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = root.canonicalize().expect("repo root resolves");
        let port_rel = "crates/actor_client/src/mailbox.rs";
        let src_root = root.join("crates/actor_client/src");

        let mut scan = WitnessScan::default();
        let mut files = Vec::new();
        let mut stack = vec![src_root.clone()];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).expect("actor_client sources are readable").flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                    files.push(p);
                }
            }
        }
        files.sort();
        assert!(files.len() >= 5, "found only {} actor_client sources — the walk went blind", files.len());

        for p in &files {
            let rel = p.strip_prefix(&root).unwrap_or(p).to_string_lossy().replace('\\', "/");
            let text = std::fs::read_to_string(p).expect("a partially-scanned crate is a silent no-op");
            // The GENERATED clients must reach the port only through `crate::enqueue`; naming the
            // witness there is the line that forces PROP-20260802-130500 phase 2 to widen the mint.
            // The one check that stays textual, because the concern is an EXPRESSION in a method
            // body rather than an item signature — comment lines are dropped first so a generated
            // doc comment explaining this very rule does not trip it.
            if rel.contains("/generated/")
                && text
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("//"))
                    .any(|l| l.contains(WITNESS))
            {
                scan.leaks.push(format!(
                    "  {rel}: GENERATED code must never name the witness — delegate through \
                     crate::enqueue (insert_mapped / schedule_mapped / cancel_scheduled_mapped)"
                ));
            }
            // `#[path]` and `include!` are caught in the AST walk below, not by text: a doc
            // comment mentioning either must not fail the build.
            let file = syn::parse_file(&text)
                .unwrap_or_else(|e| panic!("{rel} does not parse ({e}) — this guard reads the AST"));
            let gated = is_test_only_cfg(&file.attrs);
            scan_items(&file.items, &rel, gated, rel == port_rel, &mut scan);
        }

        assert!(scan.saw_port_trait, "no `trait Mailbox` found — renamed? move this guard with it");

        // The in-crate mint stays `pub(crate)`. Widening it is how the boundary would slide from
        // level 4 (compiler) to level 3 (manifest allowlist) — see the note below.
        let port = std::fs::read_to_string(root.join(port_rel)).expect("the port module");
        assert!(
            port.contains("pub(crate) fn granted() -> Self"),
            "the in-crate mint `MailboxAccess::granted()` is gone or widened — it must stay \
             `pub(crate)`.\n\nNOTE for PROP-20260802-130500 phase 2 (per-actor client crates): \
             widening this mint is how the port boundary silently slides from compiler-enforced to \
             allowlist-enforced. If phase 2 needs it, that is a DECISION to record, not a refactor."
        );

        assert_eq!(
            scan.port_methods.len(),
            5,
            "the Mailbox port has {} methods, this guard expects 5 ({:?}). If you ADDED one, \
             confirm it takes the witness and bump this number — the bump is the act of having \
             looked. If it DROPPED, fix the scan rather than deleting it.",
            scan.port_methods.len(),
            scan.port_methods.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
        );
        let naked: Vec<&str> = scan
            .port_methods
            .iter()
            .filter(|(_, ok)| !ok)
            .map(|(n, _)| n.as_str())
            .collect();
        assert!(
            naked.is_empty(),
            "these `Mailbox` port methods take no `MailboxAccess` witness: {naked:?}\n\n\
             Fix: add a `MailboxAccess` parameter. Why: without it, any holder of an \
             `Arc<dyn Mailbox>` can call the method directly and bypass the generated typed \
             clients (write) and `ActorClient` (read) — the #304 hole. A method keyed only by \
             primitives (`by_message`, `cancel_scheduled`) is the dangerous shape: the entry's \
             private fields do NOT incidentally close it."
        );
        assert!(
            scan.leaks.is_empty(),
            "the MailboxAccess witness leaks:\n{}\n\n\
             Fix: keep every impl and construction of the witness in `{port_rel}`, keep the only \
             public mint `for_tests()` under the cfg-gated `fixtures` module, and reach the port \
             through the shared delegates in `crate::enqueue`. Why: ONE public route to a witness \
             reopens every method of the port for every crate in the workspace.",
            scan.leaks.join("\n")
        );
    }

    /// What one function BODY does, read from the AST rather than its text.
    ///
    /// The first version of this scan matched two hand-picked spellings (`MailboxAccess(())`,
    /// `MailboxAccess::granted()`) and resolved calls with `body.contains("{name}(")`. Review found
    /// both unsound in ways that are ordinary rather than adversarial: `MailboxAccess { 0: () }` is
    /// the same construction of a tuple struct, and `let f = MailboxAccess::granted;` — or any
    /// `.map(insert_mapped)` / `unwrap_or_else(Self::helper)` — passes a function as a VALUE, so the
    /// ident is never followed by `(`. Both are read correctly here.
    #[derive(Default)]
    struct BodyScan {
        /// Constructs the witness directly (`MailboxAccess(..)` or `MailboxAccess { .. }`).
        mints: bool,
        /// Constructs `Self(..)`/`Self { .. }` — a mint when the enclosing impl is on the witness.
        self_ctor: bool,
        /// A macro invocation whose opaque tokens mention the witness. Expansion is invisible, so
        /// this is treated as a mint conservatively — an EXPRESSION-position macro was the one
        /// macro shape the #304 item-position refusal never covered.
        opaque_macro: bool,
        /// Every ident referenced anywhere in the body: call targets, method names, and bare paths
        /// in value position.
        refs: std::collections::HashSet<String>,
    }

    /// Every IDENT in a token stream, recursing into groups and skipping literals — a macro's
    /// arguments are opaque, but its identifiers are still call edges worth following.
    fn ident_tokens(ts: proc_macro2::TokenStream) -> Vec<String> {
        let mut out = Vec::new();
        for t in ts {
            match t {
                proc_macro2::TokenTree::Ident(i) => out.push(i.to_string()),
                proc_macro2::TokenTree::Group(g) => out.extend(ident_tokens(g.stream())),
                _ => {}
            }
        }
        out
    }

    fn last_seg(p: &syn::Path) -> String {
        p.segments.last().map(|s| s.ident.to_string()).unwrap_or_default()
    }

    impl<'ast> syn::visit::Visit<'ast> for BodyScan {
        fn visit_expr_call(&mut self, n: &'ast syn::ExprCall) {
            if let syn::Expr::Path(p) = &*n.func {
                match last_seg(&p.path).as_str() {
                    WITNESS => self.mints = true,
                    "Self" => self.self_ctor = true,
                    _ => {}
                }
            }
            syn::visit::visit_expr_call(self, n);
        }
        fn visit_expr_struct(&mut self, n: &'ast syn::ExprStruct) {
            match last_seg(&n.path).as_str() {
                WITNESS => self.mints = true,
                "Self" => self.self_ctor = true,
                _ => {}
            }
            syn::visit::visit_expr_struct(self, n);
        }
        fn visit_path(&mut self, n: &'ast syn::Path) {
            // EVERY segment, so a bare `MailboxAccess::granted` in value position is seen exactly
            // like a call to it.
            for seg in &n.segments {
                self.refs.insert(seg.ident.to_string());
            }
            syn::visit::visit_path(self, n);
        }
        fn visit_expr_method_call(&mut self, n: &'ast syn::ExprMethodCall) {
            self.refs.insert(n.method.to_string());
            syn::visit::visit_expr_method_call(self, n);
        }
        fn visit_macro(&mut self, n: &'ast syn::Macro) {
            // LITERALS ARE NOT CODE. Harvesting a macro's whole token text made
            // `println!("access granted for the caller")` taint its enclosing public fn, and
            // `println!("MailboxAccess")` read as a mint — with a failure message advising
            // `pub(crate)`, when the real fix was rewording a log line. Same principle as excluding
            // doc attributes: prose naming the mint is prose.
            let idents = ident_tokens(n.tokens.clone());
            if idents.iter().any(|t| t == WITNESS) {
                self.opaque_macro = true;
            }
            self.refs.extend(idents);
            syn::visit::visit_macro(self, n);
        }
    }

    /// Scan a function's body, with its ATTRIBUTES excluded — a doc comment naming the mint is
    /// documentation, not a door, and the text-based version reported it as one with advice
    /// ("make it `pub(crate)`") whose only real remedy was deleting the docs.
    fn scan_body(block: Option<&syn::Block>, on_witness: bool) -> (bool, std::collections::HashSet<String>) {
        use syn::visit::Visit;
        let mut s = BodyScan::default();
        if let Some(b) = block {
            s.visit_block(b);
        }
        (s.mints || s.opaque_macro || (on_witness && s.self_ctor), s.refs)
    }

    /// One function-like item seen by the door scan.
    struct FnNode {
        rel: String,
        name: String,
        /// `pub` at its own site. Deliberately NOT resolved through module privacy: treating a
        /// `pub fn` in a private module as public over-approximates, which is the safe direction
        /// and forces every re-exported door onto the allowlist by name.
        public: bool,
        /// Under a `#[cfg(test)]` / `test-fixtures` ancestor — never compiled for a dependent.
        test_only: bool,
        /// Constructs a witness in its body (AST-derived: any construction of the type, or of
        /// `Self` inside an impl on the witness, or an opaque macro mentioning it).
        mints: bool,
        /// Every ident the body references — call targets, method names, bare paths in value
        /// position.
        refs: std::collections::HashSet<String>,
    }

    /// Collect every fn in a module tree with its body, for the call-graph scan.
    fn collect_fns(
        items: &[syn::Item],
        rel: &str,
        test_only: bool,
        out: &mut Vec<FnNode>,
    ) {
        use syn::Item;
        for item in items {
            let t = test_only || is_test_only_cfg(item_attrs(item));
            match item {
                Item::Mod(m) => {
                    if let Some((_, inner)) = &m.content {
                        collect_fns(inner, rel, t, out);
                    }
                }
                Item::Fn(f) => {
                    let (mints, refs) = scan_body(Some(&f.block), false);
                    out.push(FnNode {
                        rel: rel.into(),
                        name: f.sig.ident.to_string(),
                        public: is_pub(&f.vis),
                        test_only: t,
                        mints,
                        refs,
                    })
                }
                Item::Impl(i) => {
                    let trait_impl = i.trait_.is_some();
                    let on_witness = mentions(&i.self_ty, WITNESS);
                    for ii in &i.items {
                        if let syn::ImplItem::Fn(f) = ii {
                            let (mints, refs) = scan_body(Some(&f.block), on_witness);
                            out.push(FnNode {
                                rel: rel.into(),
                                name: f.sig.ident.to_string(),
                                // A trait impl's methods are as public as the trait.
                                public: trait_impl || is_pub(&f.vis),
                                test_only: t || is_test_only_cfg(&f.attrs),
                                mints,
                                refs,
                            });
                        }
                    }
                }
                Item::Trait(tr) => {
                    for ti in &tr.items {
                        if let syn::TraitItem::Fn(f) = ti {
                            // Only PROVIDED methods have a body that could mint.
                            if let Some(block) = &f.default {
                                let (mints, refs) = scan_body(Some(block), false);
                                out.push(FnNode {
                                    rel: rel.into(),
                                    name: f.sig.ident.to_string(),
                                    public: is_pub(&tr.vis),
                                    test_only: t,
                                    mints,
                                    refs,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// EVERY PUBLIC MAILBOX DOOR IS DECLARED (#329, NARROWING the #304 residual class).
    ///
    /// The witness guard asks what a SIGNATURE says. That leaves one class it cannot see, named
    /// openly in ADR-20260803-172654: a public in-crate item that mints internally and hands the
    /// capability out through a signature that never mentions the witness —
    /// `pub fn cancel_any(&self, id: Uuid) -> Result<bool>` over a held `Arc<dyn Mailbox>`.
    /// Seven review passes on #304 established that no amount of signature analysis closes it.
    ///
    /// REACHABILITY narrows it. The provenance argument is sound: calling a port method requires a
    /// witness, and a witness comes from (a) a construction or (b) a parameter; case (b) names the
    /// witness in a signature and is caught by `every_mailbox_port_method_demands_the_access_witness`,
    /// so seeding on CONSTRUCTIONS and propagating through the call graph covers the other half.
    /// (A field, a const or a static all reduce to (a) or (b): something had to mint or receive the
    /// witness to put it there.)
    ///
    /// But this scan is a SYNTACTIC approximation of that call graph — it resolves calls by ident,
    /// with no type information — so it does NOT discharge the semantic argument, and saying it did
    /// was the review-corrected overclaim of ADR-20260803-203455. Scope: sound for constructions the
    /// AST recognises as constructions of the witness, and for call edges resolvable by ident.
    /// A complete rule needs type resolution (a rustc lint, or HIR/MIR reachability) — see #331.
    ///
    /// The payoff is not just the narrowing: the set of publicly-reachable minting functions IS the
    /// door list. Every entry below is a door someone deliberately opened, and adding one is an
    /// edit to this allowlist — which is exactly the ADR-20260802-170059 posture ("the declaration
    /// is the permission") applied to the crate's own surface rather than to the spec.
    #[test]
    fn every_public_mailbox_door_is_declared() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = root.canonicalize().expect("repo root resolves");

        // THE DOOR LIST. A publicly-reachable function that can reach a mint must be named here,
        // with why it is a door. Anything else is a new door nobody declared.
        // Keyed by (file, name), never by name alone: `send` is both a generated client's write
        // door and `broadcast::Sender::send`, and a bare-name allowlist would pre-authorise any
        // future `pub fn send` anywhere in the crate.
        // (file, name, gated-by-a-cargo-feature, why). `gated` matters for taint: a wrapper does
        // NOT inherit the feature that contains the door it calls.
        const DOORS: &[(&str, &str, bool, &str)] = &[
            // The generated per-actor clients — the write door (#284 slice 2).
            ("crates/actor_client/src/generated/actor_clients.rs", "send", false, "typed command door"),
            ("crates/actor_client/src/generated/actor_clients.rs", "record", false, "typed inbound-fact door"),
            ("crates/actor_client/src/generated/actor_clients.rs", "schedule", false, "reminder door (reminders-declaring actors)"),
            ("crates/actor_client/src/generated/actor_clients.rs", "cancel_scheduling", false, "reminder withdrawal (ADR-20260731-150500 §3)"),
            // The one generic read door (PROP-20260802-130500 D4).
            ("crates/actor_client/src/client.rs", "get_operation_status", false, "ActorClient: the ONLY read path over inbound_messages status"),
            // The reminder constructor the in-transaction `schedules:` upsert binds from.
            ("crates/actor_client/src/reminders.rs", "declare", false, "the pool-backed reminder declaration (ADR-20260731-214500)"),
            // The D8 bulk fact door — additionally gated by the `bulk-door` feature, which
            // `bulk_door_feature_is_granted_only_to_infrastructure` allows only infrastructure to enable.
            ("crates/actor_client/src/enqueue.rs", "enqueue_inbound_facts", true, "the UNTYPED bulk fact door, `bulk-door` feature (#290 review BLOCKING-1a)"),
            // Test-only reference implementations (never in a release graph).
            ("crates/actor_client/src/enqueue.rs", "cancel_reminder", true, "test-only reference impl behind `test-fixtures`"),
            ("crates/actor_client/src/enqueue.rs", "schedule_reminder", true, "test-only reference impl behind `test-fixtures`"),
            ("crates/actor_client/src/mailbox.rs", "for_tests", true, "the D5 test-only witness mint, cfg-gated"),
        ];
        let is_door = |f: &FnNode| DOORS.iter().any(|(p, n, _, _)| *p == f.rel && *n == f.name);
        let is_ungated_door =
            |f: &FnNode| DOORS.iter().any(|(p, n, g, _)| *p == f.rel && *n == f.name && !*g);

        let mut files = Vec::new();
        let mut stack = vec![root.join("crates/actor_client/src")];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).expect("actor_client sources are readable").flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                    files.push(p);
                }
            }
        }
        files.sort();
        assert!(files.len() >= 5, "found only {} sources — the walk went blind", files.len());

        let mut fns: Vec<FnNode> = Vec::new();
        for p in &files {
            let rel = p.strip_prefix(&root).unwrap_or(p).to_string_lossy().replace('\\', "/");
            let text = std::fs::read_to_string(p).expect("a partial scan is a silent no-op");
            let file = syn::parse_file(&text)
                .unwrap_or_else(|e| panic!("{rel} does not parse ({e}) — this guard reads the AST"));
            collect_fns(&file.items, &rel, is_test_only_cfg(&file.attrs), &mut fns);
        }

        // SEED: a body that CONSTRUCTS a witness, read from the AST.
        let mut tainted: std::collections::HashSet<usize> =
            fns.iter().enumerate().filter(|(_, f)| f.mints).map(|(i, _)| i).collect();
        // Anti-blindness, named specifically: `!tainted.is_empty()` alone is satisfied by the
        // test-only `for_tests`, so the PRODUCTION mint could go dark unnoticed.
        assert!(
            fns.iter().any(|f| f.mints && f.name == "granted"),
            "no function named `granted` constructs a MailboxAccess any more. If the mint was \
             RENAMED that is fine — update this assertion to the new name (the AST seed follows \
             renames, so the guard is still live). If it was REMOVED or its construction moved \
             behind something the AST scan cannot see, the seed has gone blind: fix the seed, do \
             not delete the test."
        );

        // PROPAGATE to a fixpoint: a function that calls a tainted one is tainted. Matched by
        // ident, which over-approximates across same-named methods — the safe direction.
        loop {
            // Taint flows out of MINTS and internal helpers, and STOPS at an UNGATED declared
            // door: a function calling `RestaurantClient::send` is using the sanctioned public
            // API, which every crate has anyway. It does NOT stop at a gated door — that door's
            // containment is a cargo feature on its `pub use`, and an in-crate wrapper does not
            // inherit the feature, so wrapping `enqueue_inbound_facts` would have re-exposed the
            // untyped bulk door to crates the `bulk-door` manifest guard exists to exclude.
            let names: Vec<String> = tainted
                .iter()
                .filter(|i| !is_ungated_door(&fns[**i]))
                .map(|i| fns[*i].name.clone())
                .collect();
            let mut grew = false;
            for (i, f) in fns.iter().enumerate() {
                if tainted.contains(&i) {
                    continue;
                }
                // NO same-ident exclusion. `names` holds only TAINTED functions, so `n == f.name`
                // cannot mean self-recursion — it means a DIFFERENT tainted function shares this
                // one's name, which is exactly the edge "public `Facade::new` calls crate-internal
                // minting `Held::new`". Excluding it dropped a real, ident-resolvable edge and
                // reopened the class this guard exists to narrow (`new` is the commonest ident in
                // Rust). Including it costs nothing: the clean tree stays green.
                if names.iter().any(|n| f.refs.contains(n)) {
                    grew = true;
                    tainted.insert(i);
                }
            }
            if !grew {
                break;
            }
        }

        let undeclared: Vec<String> = tainted
            .iter()
            .map(|i| &fns[*i])
            .filter(|f| f.public && !f.test_only && !is_door(f))
            .map(|f| format!("  {}: `{}`", f.rel, f.name))
            .collect();
        assert!(
            undeclared.is_empty(),
            "these PUBLIC functions can reach a MailboxAccess mint but are not declared doors:\n{}\n\n\
             Fix: make it `pub(crate)`; or UPDATE THE PATH of an existing DOORS entry if the \
             function merely moved (a file rename or module split is the usual cause); or — if it \
             really is a new door — add it to DOORS WITH the reason it exists. Why: this is the \
             class `every_mailbox_port_method_demands_the_access_witness` cannot see, because such \
             a function never names the witness in its signature (`pub fn cancel_any(&self, id)` \
             over a held `Arc<dyn Mailbox>`). NOTE this scan resolves calls by IDENT, so a public \
             fn can also be flagged for merely mentioning a tainted name — check the body before \
             assuming it is a door.",
            undeclared.join("\n")
        );

        // Both directions, like the entry-construction guard: a stale door entry is an excuse
        // nobody is using, and it would silently permit a future function of that name.
        let stale: Vec<String> = DOORS
            .iter()
            .filter(|(p, n, _, _)| !tainted.iter().any(|i| &fns[*i].name == n && &fns[*i].rel == p))
            .map(|(p, n, _, _)| format!("{p}::{n}"))
            .collect();
        assert!(
            stale.is_empty(),
            "these declared doors no longer reach the mailbox: {stale:?}\n\n\
             Fix: update the PATH if the function merely moved (a module split or file rename is the \
             usual cause), or remove the entry if the door is genuinely gone. A stale entry \
             pre-authorises any future function that happens to take that name in that file."
        );
    }

    /// The Cargo.toml CAPABILITY ALLOWLIST (#290 phase 1, PROP-20260802-130500 D3): `sqlx` (talk
    /// to the database) and `reqwest` (reach the network) may appear in a crate's RELEASE
    /// dependency sections only when that crate is explicitly allowlisted here WITH its reason.
    /// This is the side door the typed mailbox clients cannot see — "add sqlx to some crate and
    /// just query the table" — turned into a red test on the very `Cargo.toml` diff that grants
    /// the capability. cargo-deny was considered and skipped: it is not present in the dev/CI
    /// images and `[bans]` cannot express per-crate grants of a workspace-wide dependency; this
    /// test is executable everywhere `cargo test` runs (CI's `codegen` job included), in the
    /// house style of the Makefile and mailbox-door guards.
    ///
    /// BOTH directions are asserted, like the door guard: a non-allowlisted holder fails, and an
    /// allowlisted crate that no longer holds the capability fails too — a stale excuse is an
    /// open door someone will eventually use. Dev-dependencies are out of scope on purpose: a
    /// test may talk SQL; the release graph may not grow a capability silently.
    #[test]
    fn capability_dependencies_are_allowlisted() {
        // (manifest path, capability, WHY the crate holds it)
        const ALLOWED: &[(&str, &str, &str)] = &[
            // ── sqlx — who may talk to Postgres at all ──
            ("crates/infrastructure/Cargo.toml", "sqlx",
             "THE adapter layer: event store, View_* read repos, and the SQL side of the mailbox boundary (PgMailbox)"),
            ("crates/actor_runtime/Cargo.toml", "sqlx",
             "the durable mailbox runtime is SQL by design (leases, fencing, head-of-line drain); its extraction floor is 'sqlx + tokio + serde'"),
            ("crates/sirene_ingest/Cargo.toml", "sqlx",
             "raw SIRENE ingestion into its OWN staging tables (ADR-0045)"),
            ("crates/adapters/stripe/Cargo.toml", "sqlx",
             "the adapter owns its webhook staging/dedupe tables (ADR-0045 posture)"),
            ("crates/adapters/hubrise/Cargo.toml", "sqlx",
             "the adapter owns its connection/staging tables (ADR-0045 posture)"),
            ("crates/adapters/avelo37/Cargo.toml", "sqlx",
             "the adapter owns its webhook staging tables (ADR-0045 posture)"),
            ("crates/adapters/coopcycle/Cargo.toml", "sqlx",
             "the adapter owns its webhook staging tables (ADR-0045 posture)"),
            ("crates/adapters/uber_direct/Cargo.toml", "sqlx",
             "the adapter owns its webhook staging tables (ADR-0045 posture)"),
            ("crates/server/Cargo.toml", "sqlx",
             "composition root: constructs the PgPool it injects and runs the /health _sqlx_migrations schema probe (ADR-0042/0043) — moving pool construction behind a port still leaks sqlx types through every wiring signature, so the exception stays until that refactor is designed"),
            // ── reqwest — who may reach the network ──
            ("crates/infrastructure/Cargo.toml", "reqwest",
             "the generated /services/* HTTP clients (ADR-20260719-214500) + OVH SMS outbound"),
            ("crates/sirene_ingest/Cargo.toml", "reqwest", "the SIRENE API client"),
            ("crates/telemetry/Cargo.toml", "reqwest", "OTLP-over-HTTP export to Honeycomb EU"),
            ("crates/web/Cargo.toml", "reqwest", "the SSR data layer resolves screens over HTTP"),
            ("crates/adapters/stripe/Cargo.toml", "reqwest", "partner outbound API"),
            ("crates/adapters/hubrise/Cargo.toml", "reqwest", "partner outbound API"),
            ("crates/adapters/avelo37/Cargo.toml", "reqwest", "partner outbound API"),
            ("crates/adapters/coopcycle/Cargo.toml", "reqwest", "partner outbound API"),
            ("crates/adapters/uber_direct/Cargo.toml", "reqwest", "partner outbound API"),
            ("crates/server/Cargo.toml", "reqwest",
             "the ADR-0047 auth verifier fetches the Supabase JWKS over HTTPS (identity wrapper lives in server today; measured holder kept with this WHY)"),
        ];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root resolves");

        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    if p.file_name().and_then(|n| n.to_str()) != Some("target") {
                        walk(&p, out);
                    }
                } else if p.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml") {
                    out.push(p);
                }
            }
        }
        let mut manifests = Vec::new();
        walk(&root.join("crates"), &mut manifests);
        walk(&root.join("tools"), &mut manifests);
        manifests.sort();
        assert!(!manifests.is_empty(), "found no member manifests to scan");

        /// The capabilities a manifest GRANTS in its release graph: dependency names found in any
        /// `[...dependencies]` section that is not a dev-dependencies section. Line-based on
        /// purpose (matches the workspace's one-line dependency style); a multi-line dep TABLE
        /// (`[dependencies.sqlx]`) is caught by the section header match.
        fn release_grants(src: &str, dep: &str) -> bool {
            let mut in_release_deps = false;
            for line in src.lines() {
                let t = line.trim();
                if t.starts_with('[') {
                    let header = t.trim_start_matches('[').trim_end_matches(']');
                    // `[dependencies.sqlx]`-style table headers grant directly.
                    if header.ends_with(&format!("dependencies.{dep}"))
                        && !header.contains("dev-dependencies")
                    {
                        return true;
                    }
                    in_release_deps =
                        header.ends_with("dependencies") && !header.contains("dev-dependencies");
                    continue;
                }
                if in_release_deps
                    && (t.starts_with(&format!("{dep} ")) || t.starts_with(&format!("{dep}=")))
                {
                    return true;
                }
            }
            false
        }

        let mut offenders: Vec<String> = Vec::new();
        for m in &manifests {
            let rel = m.strip_prefix(&root).unwrap_or(m).to_string_lossy().replace('\\', "/");
            let src = std::fs::read_to_string(m).unwrap_or_else(|e| {
                panic!("cannot read {rel} ({e}) — a partially-scanned workspace is a silent no-op")
            });
            for cap in ["sqlx", "reqwest"] {
                let holds = release_grants(&src, cap);
                let excused = ALLOWED.iter().any(|(p, c, _)| *p == rel && *c == cap);
                if holds && !excused {
                    offenders.push(format!("  {rel} grants `{cap}` without an allowlist entry"));
                }
                if !holds && excused {
                    offenders.push(format!(
                        "  {rel} is allowlisted for `{cap}` but no longer holds it — remove the stale entry"
                    ));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "the Cargo.toml capability allowlist (PROP-20260802-130500 D3) is violated:\n{}\n\n\
             Fix: either the crate should not hold this capability (talk through a port /\n\
             the actor_client doors instead), or holding it is a DELIBERATE architectural\n\
             decision — then add it to this test's allowlist WITH the reason. Why: any crate\n\
             holding sqlx can bypass every domain rule with one query, and any crate holding\n\
             reqwest can exfiltrate or call side effects review never sees; the allowlist makes\n\
             the grant a loud, reviewable diff instead of a silent Cargo.toml line.",
            offenders.join("\n")
        );
    }

    /// The D6 LINT-FLOOR guard (#302, PROP-20260802-130500 D6): every workspace member inherits
    /// the workspace `[lints]` baseline (`unsafe_code = forbid`), boundary crates additionally
    /// deny `unreachable_pub` in their own `[lints]` tables (Cargo inheritance is all-or-nothing,
    /// so they restate the floor), and the cargo-machete step stays wired in CI. Without this
    /// test, the floor holds only for the crates that existed when #302 landed — a NEW member
    /// crate ships with no `[lints]` at all and the whole floor silently stops being
    /// workspace-wide. Style of the D3 capability allowlist: both directions asserted, an
    /// FFI-crate opt-out must be allowlisted here WITH its reason (none exists today).
    #[test]
    fn lint_floor_covers_every_member() {
        // (manifest path, WHY the crate may opt out of `unsafe_code = forbid`) — empty on
        // purpose: no crate writes unsafe today. The day UniFFI/Tauri needs one, the entry
        // lands here with its justification, a loud reviewable diff.
        const FFI_EXEMPT: &[(&str, &str)] = &[];

        // Boundary crates: their own [lints.rust] must restate the floor AND deny
        // `unreachable_pub` — on a boundary, a `pub` item nobody outside uses is an open door
        // someone WILL use (ADR-20260802-170059, mechanically). `server` is NOT here: its 207
        // findings live mostly in the generated GraphQL layer — widening the floor to it is
        // emitter work, tracked as follow-up, not silently claimed by this guard.
        const BOUNDARY: &[&str] = &[
            "crates/actor_client/Cargo.toml",
            "crates/infrastructure/Cargo.toml",
            "crates/telemetry/Cargo.toml",
            "crates/adapters/stripe/Cargo.toml",
            "crates/adapters/hubrise/Cargo.toml",
            "crates/adapters/avelo37/Cargo.toml",
            "crates/adapters/coopcycle/Cargo.toml",
            "crates/adapters/uber_direct/Cargo.toml",
        ];

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root resolves");

        // The workspace baseline itself must hold — a floor nobody defines covers nothing.
        let ws = std::fs::read_to_string(root.join("Cargo.toml"))
            .expect("workspace Cargo.toml readable");
        assert!(
            ws.contains("[workspace.lints.rust]") && ws.contains("unsafe_code = \"forbid\""),
            "the workspace [lints] baseline (unsafe_code = forbid) is gone from Cargo.toml — \
             the D6 lint floor no longer exists"
        );

        // The third leg of D6: the unused-dependency gate must stay wired in CI — an unused
        // dependency is an unheld capability someone can silently start using.
        let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml"))
            .expect(".github/workflows/ci.yml readable");
        assert!(
            ci.contains("cargo machete"),
            "ci.yml no longer runs `cargo machete` — the D6 unused-dependency gate is unwired"
        );

        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    if p.file_name().and_then(|n| n.to_str()) != Some("target") {
                        walk(&p, out);
                    }
                } else if p.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml") {
                    out.push(p);
                }
            }
        }
        let mut manifests = Vec::new();
        walk(&root.join("crates"), &mut manifests);
        walk(&root.join("tools"), &mut manifests);
        manifests.sort();
        assert!(!manifests.is_empty(), "found no member manifests to scan");

        /// Line-based section scan (workspace house style): does this manifest carry the given
        /// `[lints...]` header with the given `key = value` line inside that section?
        fn section_has(src: &str, header: &str, entry: &str) -> bool {
            let mut in_section = false;
            for line in src.lines() {
                let t = line.trim();
                if t.starts_with('[') {
                    in_section = t == header;
                    continue;
                }
                if in_section && t.starts_with(entry) {
                    return true;
                }
            }
            false
        }

        let mut offenders: Vec<String> = Vec::new();
        for m in &manifests {
            let rel = m.strip_prefix(&root).unwrap_or(m).to_string_lossy().replace('\\', "/");
            let src = std::fs::read_to_string(m).unwrap_or_else(|e| {
                panic!("cannot read {rel} ({e}) — a partially-scanned workspace is a silent no-op")
            });
            let inherits = section_has(&src, "[lints]", "workspace = true");
            let own_forbid = section_has(&src, "[lints.rust]", "unsafe_code = \"forbid\"");
            let own_deny = section_has(&src, "[lints.rust]", "unreachable_pub = \"deny\"");
            let exempt = FFI_EXEMPT.iter().any(|(p, _)| *p == rel);
            if BOUNDARY.contains(&rel.as_str()) {
                if !(own_forbid && own_deny) {
                    offenders.push(format!(
                        "  {rel} is a BOUNDARY crate but its [lints.rust] no longer carries \
                         unsafe_code = forbid AND unreachable_pub = deny"
                    ));
                }
            } else if !inherits && !own_forbid && !exempt {
                offenders.push(format!(
                    "  {rel} carries no lint floor: add `[lints] workspace = true` (or, for a \
                     genuine FFI crate, an FFI_EXEMPT entry here WITH its reason)"
                ));
            }
        }
        assert!(
            offenders.is_empty(),
            "the D6 lint floor (PROP-20260802-130500, #302) has holes:\n{}\n\n\
             Why: `unsafe_code = forbid` keeps AI-authored code inside safe Rust workspace-wide, \
             and `unreachable_pub = deny` on boundary crates makes a dead `pub` item a compile \
             error instead of an open door; a member without the floor re-opens both silently.",
            offenders.join("\n")
        );
    }

    /// The D5 escape-hatch guard (#290 phase 1, PROP-20260802-130500 D5): the `test-fixtures`
    /// feature on `actor_client` (mem mailbox double, EntryFixture conversions, drift-guard
    /// reference impls) may be enabled ONLY from `[dev-dependencies]` — a release artifact that
    /// turns it on would ship a public constructor for the very type the boundary crate exists to
    /// seal. The check is part of the D5 decision, not optional: a feature is opt-in-able by
    /// mistake, so the mistake must be CI-red.
    #[test]
    fn test_fixtures_feature_never_reaches_a_release_artifact() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root resolves");
        // The feature must still exist under this exact name — if it is renamed, this guard must
        // move with it, never silently scan for nothing.
        let client_manifest = std::fs::read_to_string(root.join("crates/actor_client/Cargo.toml"))
            .expect("crates/actor_client/Cargo.toml readable");
        assert!(
            client_manifest.contains("test-fixtures = []"),
            "actor_client no longer declares the `test-fixtures` feature — move this guard with it"
        );

        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    if p.file_name().and_then(|n| n.to_str()) != Some("target") {
                        walk(&p, out);
                    }
                } else if p.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml") {
                    out.push(p);
                }
            }
        }
        let mut manifests = Vec::new();
        walk(&root.join("crates"), &mut manifests);
        walk(&root.join("tools"), &mut manifests);
        manifests.sort();

        let mut offenders: Vec<String> = Vec::new();
        let mut dev_grants = 0usize;
        for m in &manifests {
            let rel = m.strip_prefix(&root).unwrap_or(m).to_string_lossy().replace('\\', "/");
            let src = std::fs::read_to_string(m).unwrap_or_else(|e| {
                panic!("cannot read {rel} ({e}) — a partially-scanned workspace is a silent no-op")
            });
            if rel == "crates/actor_client/Cargo.toml" {
                continue; // the declaring crate itself
            }
            let mut section = String::new();
            for line in src.lines() {
                let t = line.trim();
                if t.starts_with('[') {
                    section = t.trim_start_matches('[').trim_end_matches(']').to_string();
                    continue;
                }
                if t.contains("test-fixtures") {
                    if section.contains("dev-dependencies") {
                        dev_grants += 1;
                    } else {
                        offenders.push(format!("  {rel} [{section}]: {t}"));
                    }
                }
            }
        }
        assert!(
            dev_grants > 0,
            "no [dev-dependencies] enables `test-fixtures` anywhere — either the feature was \
             renamed (move this guard with it) or the scan went blind; both must be loud"
        );
        assert!(
            offenders.is_empty(),
            "`test-fixtures` is enabled OUTSIDE [dev-dependencies] — a release artifact would \
             ship the sealed type's test constructors:\n{}\n\nFix: move the grant to that \
             crate's [dev-dependencies] (tests get it; the shipped lib/bin never does).",
            offenders.join("\n")
        );
    }

    /// The BULK-DOOR grant guard (#290 review BLOCKING-1a): the `bulk-door` feature on
    /// `actor_client` (the untyped `enqueue_inbound_facts` + `InboundFact` export) may be enabled
    /// by EXACTLY ONE manifest — `crates/infrastructure` (its SIRENE sweep is the D8-deferred
    /// bulk producer). Cargo features UNIFY across a build graph, so once infrastructure lights
    /// the feature a sibling crate could technically NAME the export — which is precisely why
    /// this guard fails the MANIFEST grant, the loud reviewable act, in any other crate (dev or
    /// release: a test wanting the bulk door is a scope decision, not a convenience). Both
    /// directions, like the capability allowlist: a second grant fails, and infrastructure
    /// dropping the grant while the feature still exists fails too (a stale gate is a gate
    /// nobody notices being reopened).
    #[test]
    fn bulk_door_feature_is_granted_only_to_infrastructure() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root resolves");
        // The feature must still exist under this exact name — renamed means this guard moves
        // with it, never silently scans for nothing.
        let client_manifest = std::fs::read_to_string(root.join("crates/actor_client/Cargo.toml"))
            .expect("crates/actor_client/Cargo.toml readable");
        assert!(
            client_manifest.contains("bulk-door = []"),
            "actor_client no longer declares the `bulk-door` feature — move this guard with it"
        );

        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    if p.file_name().and_then(|n| n.to_str()) != Some("target") {
                        walk(&p, out);
                    }
                } else if p.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml") {
                    out.push(p);
                }
            }
        }
        let mut manifests = Vec::new();
        walk(&root.join("crates"), &mut manifests);
        walk(&root.join("tools"), &mut manifests);
        manifests.sort();

        let mut offenders: Vec<String> = Vec::new();
        let mut infrastructure_grants = false;
        for m in &manifests {
            let rel = m.strip_prefix(&root).unwrap_or(m).to_string_lossy().replace('\\', "/");
            let src = std::fs::read_to_string(m).unwrap_or_else(|e| {
                panic!("cannot read {rel} ({e}) — a partially-scanned workspace is a silent no-op")
            });
            if rel == "crates/actor_client/Cargo.toml" {
                continue; // the declaring crate itself
            }
            for (idx, line) in src.lines().enumerate() {
                if line.contains("bulk-door") && !line.trim_start().starts_with('#') {
                    if rel == "crates/infrastructure/Cargo.toml" {
                        infrastructure_grants = true;
                    } else {
                        offenders.push(format!("  {rel}:{}: {}", idx + 1, line.trim()));
                    }
                }
            }
        }
        assert!(
            infrastructure_grants,
            "crates/infrastructure no longer enables `bulk-door` — either the SIRENE bulk path \
             moved (move this guard's allowlist with it) or the gate went stale; both must be loud"
        );
        assert!(
            offenders.is_empty(),
            "`bulk-door` is granted outside crates/infrastructure — a second crate would gain \
             the UNTYPED batched fact door the sealed {{Actor}}Fact traits exist to prevent:\n{}\n\n\
             Fix: record facts through the typed clients' `record`, or make the new bulk \
             producer a deliberate decision — then move it behind infrastructure or extend this \
             allowlist WITH the reason.",
            offenders.join("\n")
        );

        // THE NAMING SCAN — what the feature gate provably cannot close (#290 re-review):
        // cargo features UNIFY, so once infrastructure lights `bulk-door` the export resolves for
        // EVERY crate in the graph — a manifest-less scratch crate compiling
        // `pub use actor_client::{enqueue_inbound_facts, InboundFact}` was demonstrated. The
        // manifest grant above is the loud reviewable act; THIS scan is the enforcement: any
        // source reference to the bulk-door symbols outside `crates/infrastructure` (the one
        // sanctioned producer) and `crates/actor_client` (the definition) fails, door-guard
        // style. Allowlist-asserted-alive: infrastructure must still NAME both symbols, or the
        // producer moved and this scan went stale.
        fn walk_rs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    if p.file_name().and_then(|n| n.to_str()) != Some("target") {
                        walk_rs(&p, out);
                    }
                } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                    out.push(p);
                }
            }
        }
        let mut sources = Vec::new();
        walk_rs(&root.join("crates"), &mut sources);
        sources.sort();
        assert!(!sources.is_empty(), "found no crate sources to scan");

        const SYMBOLS: &[&str] = &["enqueue_inbound_facts", "InboundFact"];
        let mut named_offenders: Vec<String> = Vec::new();
        let mut infra_names: HashSet<&str> = HashSet::new();
        for f in &sources {
            let rel = f.strip_prefix(&root).unwrap_or(f).to_string_lossy().replace('\\', "/");
            let src = std::fs::read_to_string(f).unwrap_or_else(|e| {
                panic!("cannot read {rel} ({e}) — a partially-scanned tree is a silent no-op")
            });
            let inside_the_door = rel.starts_with("crates/actor_client/")
                || rel.starts_with("crates/infrastructure/");
            for (idx, line) in src.lines().enumerate() {
                for sym in SYMBOLS {
                    if !line.contains(sym) {
                        continue;
                    }
                    if rel.starts_with("crates/infrastructure/") {
                        infra_names.insert(sym);
                    }
                    if !inside_the_door {
                        named_offenders.push(format!("  {rel}:{}: {}", idx + 1, line.trim()));
                    }
                }
            }
        }
        assert_eq!(
            infra_names.len(),
            SYMBOLS.len(),
            "crates/infrastructure no longer names {SYMBOLS:?} — the SIRENE bulk producer moved; \
             move this scan's allowlist with it so the guard stays real"
        );
        assert!(
            named_offenders.is_empty(),
            "the bulk-door symbols are NAMED outside crates/infrastructure — feature \
             unification makes the export resolve graph-wide, so the reference itself is the \
             violation:\n{}\n\nFix: record facts through a typed client's `record`; a new bulk \
             producer is a scope decision recorded on the issue, then added to this scan's \
             allowlist WITH the reason.",
            named_offenders.join("\n")
        );
    }

    // ─── §2f — reminders + declarative deletion (ADR-20260731-214500) ───────────────────────────

    const RD_SCALARS: &str = "OrderId: { type: string }\nRestaurantId: { type: string }\nCatalogId: { type: string }\n";
    const RD_EVENTS: &str = r#"
OrderPlaced:
  type: object
  properties:
    orderId: { $ref: 'scalars.yaml#/OrderId' }
OrderExpired:
  type: object
  properties:
    orderId: { $ref: 'scalars.yaml#/OrderId' }
OrderDeleted:
  type: object
  properties:
    orderId: { $ref: 'scalars.yaml#/OrderId' }
RestaurantDeleted:
  type: object
  properties:
    restaurantId: { $ref: 'scalars.yaml#/RestaurantId' }
CatalogDeleted:
  type: object
  properties:
    catalogId: { $ref: 'scalars.yaml#/CatalogId' }
"#;
    const RD_CONFIG: &str = "keys:\n  ORDER_RETENTION_WINDOW_DAYS:\n    type: int\n    default: 365\n    gates: \"Retention window for terminal orders.\"\n";

    /// The pilot shapes of ADR-20260731-214500: a windowed reminder + windowed deletion trigger
    /// with an undo (`Order`), and a child-declared PROPAGATION trigger with a typed `match`
    /// (`Catalog` dies when the parent's receipt fact lands).
    const RD_ACTORS_VALID: &str = r#"
Order:
  type: aggregate
  identity: { $ref: '#/Order/state/orderId' }
  reminders:
    OrderExpired:
      payload: { $ref: 'events.yaml#/OrderExpired' }
      after: { $ref: 'configuration.yaml#/keys/ORDER_RETENTION_WINDOW_DAYS' }
      reschedule: in-place
  receives:
    - message: { $ref: 'events.yaml#/OrderPlaced' }
      emits: []
      schedules:
        - { $ref: '#/Order/reminders/OrderExpired' }
    - message: { $ref: '#/Order/reminders/OrderExpired' }
      emits: []
  deletion:
    triggers:
      - on: [{ $ref: 'events.yaml#/OrderExpired' }]
        after: { $ref: 'configuration.yaml#/keys/ORDER_RETENTION_WINDOW_DAYS' }
        cancelled_on: [{ $ref: 'events.yaml#/OrderPlaced' }]
    receipt: { $ref: 'events.yaml#/OrderDeleted' }
Catalog:
  type: aggregate
  state:
    restaurantId: {}
  deletion:
    triggers:
      - on: [{ $ref: 'events.yaml#/RestaurantDeleted' }]
        match:
          event: { $ref: 'events.yaml#/RestaurantDeleted/properties/restaurantId' }
          state: { $ref: '#/Catalog/state/restaurantId' }
    receipt: { $ref: 'events.yaml#/CatalogDeleted' }
"#;

    fn rd_model(actors_yaml: &str) -> Model {
        inline_model(&[
            ("scalars.yaml", RD_SCALARS),
            ("events.yaml", RD_EVENTS),
            ("commands.yaml", "PlaceOrder:\n  type: object\n"),
            ("configuration.yaml", RD_CONFIG),
            ("actors.yaml", actors_yaml),
        ])
    }

    fn rd_issues(actors_yaml: &str) -> Vec<Issue> {
        let mut issues = Vec::new();
        validate_reminders_and_deletion(&rd_model(actors_yaml), &mut issues);
        issues
    }

    fn rules_of(issues: &[Issue]) -> Vec<&str> {
        issues.iter().map(|i| i.rule).collect()
    }

    #[test]
    fn valid_reminders_and_deletion_are_clean_in_both_gates() {
        let m = rd_model(RD_ACTORS_VALID);
        // §2f: the dedicated semantic rules.
        let mut issues = Vec::new();
        validate_reminders_and_deletion(&m, &mut issues);
        assert!(issues.is_empty(), "{:?}", issues.iter().map(|i| (i.rule, &i.message)).collect::<Vec<_>>());
        // §1b: every new ref site is declared in REF_CONTRACT and classifies to the right kind
        // (payload→event, after→configuration key, schedules/message→reminder, match→property/state).
        let mut kinds = Vec::new();
        validate_ref_kinds(&m, &mut kinds);
        assert!(kinds.is_empty(), "{:?}", kinds.iter().map(|i| (i.rule, &i.message)).collect::<Vec<_>>());
    }

    #[test]
    fn reminder_without_a_same_actor_receive_is_an_error() {
        let issues = rd_issues(
            "Order:\n  type: aggregate\n  reminders:\n    OrderExpired:\n      payload: { $ref: 'events.yaml#/OrderExpired' }\n  receives:\n    - message: { $ref: 'events.yaml#/OrderPlaced' }\n      emits: []\n",
        );
        assert_eq!(rules_of(&issues), vec!["reminder-without-receive"], "{:?}", issues.iter().map(|i| &i.message).collect::<Vec<_>>());
        assert!(issues[0].location.ends_with("Order/reminders/OrderExpired"), "{}", issues[0].location);
    }

    #[test]
    fn receive_of_an_undeclared_reminder_is_an_error() {
        let issues = rd_issues(
            "Order:\n  type: aggregate\n  reminders:\n    OrderExpired:\n      payload: { $ref: 'events.yaml#/OrderExpired' }\n  receives:\n    - message: { $ref: '#/Order/reminders/OrderExpired' }\n      emits: []\n    - message: { $ref: '#/Order/reminders/Ghost' }\n      emits: []\n",
        );
        assert_eq!(rules_of(&issues), vec!["receive-without-reminder"], "{:?}", issues.iter().map(|i| &i.message).collect::<Vec<_>>());
    }

    #[test]
    fn a_receive_of_another_actors_reminder_is_an_error() {
        // A reminder is one actor talking to ITSELF — Catalog receiving Order's reminder both
        // fails the receive (wrong actor) and leaves the reminder itself uncovered.
        let issues = rd_issues(
            "Order:\n  type: aggregate\n  reminders:\n    OrderExpired:\n      payload: { $ref: 'events.yaml#/OrderExpired' }\n  receives: []\nCatalog:\n  type: aggregate\n  receives:\n    - message: { $ref: '#/Order/reminders/OrderExpired' }\n      emits: []\n",
        );
        let rules = rules_of(&issues);
        assert!(rules.contains(&"receive-without-reminder"), "{:?}", rules);
        assert!(rules.contains(&"reminder-without-receive"), "{:?}", rules);
    }

    #[test]
    fn schedules_must_resolve_to_a_same_actor_reminder() {
        let issues = rd_issues(
            "Order:\n  type: aggregate\n  reminders:\n    OrderExpired:\n      payload: { $ref: 'events.yaml#/OrderExpired' }\n  receives:\n    - message: { $ref: '#/Order/reminders/OrderExpired' }\n      emits: []\nCatalog:\n  type: aggregate\n  receives:\n    - message: { $ref: 'events.yaml#/OrderPlaced' }\n      emits: []\n      schedules:\n        - { $ref: '#/Order/reminders/OrderExpired' }\n",
        );
        assert_eq!(rules_of(&issues), vec!["schedules-unresolved"], "{:?}", issues.iter().map(|i| &i.message).collect::<Vec<_>>());
        assert!(issues[0].location.contains("Catalog.receives[0].schedules[0]"), "{}", issues[0].location);
    }

    #[test]
    fn reminder_payload_must_be_an_events_yaml_fact_and_reschedule_in_place() {
        // A command payload models the wrong thing: the deadline's passage cannot be refused
        // (ADR-20260731-153000 §1a); and `in-place` is the only reschedule semantics that exists.
        let issues = rd_issues(
            "Order:\n  type: aggregate\n  reminders:\n    OrderExpired:\n      payload: { $ref: 'commands.yaml#/PlaceOrder' }\n      reschedule: cancel-and-recreate\n  receives:\n    - message: { $ref: '#/Order/reminders/OrderExpired' }\n      emits: []\n",
        );
        let rules = rules_of(&issues);
        assert!(rules.contains(&"reminder-payload-not-event"), "{:?}", rules);
        assert!(rules.contains(&"reminder-reschedule-unknown"), "{:?}", rules);
        assert_eq!(issues.len(), 2, "{:?}", issues.iter().map(|i| (i.rule, &i.message)).collect::<Vec<_>>());
    }

    #[test]
    fn deletion_refs_must_resolve_events_window_and_receipt() {
        let issues = rd_issues(
            "Order:\n  type: aggregate\n  deletion:\n    triggers:\n      - on: [{ $ref: 'events.yaml#/Ghost' }]\n        after: { $ref: 'configuration.yaml#/keys/GHOST_WINDOW' }\n",
        );
        // Missing receipt + unresolved `on` event + unresolved `after` key — all deletion-ref-unresolved.
        assert_eq!(rules_of(&issues), vec!["deletion-ref-unresolved"; 3], "{:?}", issues.iter().map(|i| (&i.location, &i.message)).collect::<Vec<_>>());
    }

    #[test]
    fn deletion_match_is_required_for_propagation_and_strongly_typed() {
        let issues = rd_issues(
            "Catalog:\n  type: aggregate\n  state:\n    restaurantId: {}\n  deletion:\n    triggers:\n      - on: [{ $ref: 'events.yaml#/RestaurantDeleted' }]\n      - on: [{ $ref: 'events.yaml#/RestaurantDeleted' }]\n        match:\n          event: { $ref: 'events.yaml#/OrderExpired/properties/orderId' }\n          state: { $ref: '#/Catalog/state/ghostField' }\n    receipt: { $ref: 'events.yaml#/CatalogDeleted' }\n",
        );
        // triggers[0]: propagation without match; triggers[1]: event outside `on` + unknown state field.
        assert_eq!(rules_of(&issues), vec!["deletion-match-untyped"; 3], "{:?}", issues.iter().map(|i| (&i.location, &i.message)).collect::<Vec<_>>());
        assert!(issues[1].message.contains("not one of this trigger's `on` events"), "{}", issues[1].message);
        assert!(issues[2].message.contains("not a declared state field"), "{}", issues[2].message);
    }

    #[test]
    fn deletion_propagation_tree_cycles_are_reported() {
        // Order dies on CatalogDeleted, Catalog dies on OrderDeleted — each lists the other's
        // receipt as its trigger, so the emergent tree is a 2-cycle.
        let issues = rd_issues(
            "Order:\n  type: aggregate\n  deletion:\n    triggers:\n      - on: [{ $ref: 'events.yaml#/CatalogDeleted' }]\n        after: { $ref: 'configuration.yaml#/keys/ORDER_RETENTION_WINDOW_DAYS' }\n    receipt: { $ref: 'events.yaml#/OrderDeleted' }\nCatalog:\n  type: aggregate\n  deletion:\n    triggers:\n      - on: [{ $ref: 'events.yaml#/OrderDeleted' }]\n        after: { $ref: 'configuration.yaml#/keys/ORDER_RETENTION_WINDOW_DAYS' }\n    receipt: { $ref: 'events.yaml#/CatalogDeleted' }\n",
        );
        assert_eq!(rules_of(&issues), vec!["deletion-tree-cycle"], "{:?}", issues.iter().map(|i| (&i.location, &i.message)).collect::<Vec<_>>());
        assert!(issues[0].message.contains("Catalog") && issues[0].message.contains("Order"), "{}", issues[0].message);
    }

    #[test]
    fn deletion_policy_table_is_emitted_only_when_declared() {
        // No `deletion:` anywhere → no file at all (zero drift until the first spec delta lands).
        assert!(emit_infra_deletion_policy(&rd_model("Order:\n  type: aggregate\n  receives: []\n")).is_none());
        let table = emit_infra_deletion_policy(&rd_model(RD_ACTORS_VALID)).expect("declared → emitted");
        for needle in [
            "actor_type: \"Order\"",
            "on: &[\"OrderExpired\"]",
            "after_config_key: Some(\"ORDER_RETENTION_WINDOW_DAYS\")",
            "cancelled_on: &[\"OrderPlaced\"]",
            "receipt: \"OrderDeleted\"",
            "actor_type: \"Catalog\"",
            "after_config_key: None",
            "match_event_property: Some(\"restaurantId\")",
            "match_state_field: Some(\"restaurantId\")",
            "receipt: \"CatalogDeleted\"",
            "identity: \"orderId\"",
        ] {
            assert!(table.contains(needle), "missing `{}` in:\n{}", needle, table);
        }
    }

    #[test]
    fn deletion_self_match_may_bind_the_implicit_identity_field() {
        // The Order pilot's shape (#272 D2): the deletion trigger is the actor's own recorded
        // expiry fact, matched on the IDENTITY field — which the typed `identity` ref declares
        // implicitly (no explicit `state:` entry needed, same doctrine as
        // `is_implicit_identity_state_ref`).
        let issues = rd_issues(
            "Order:\n  type: aggregate\n  identity: { $ref: '#/Order/state/orderId' }\n  reminders:\n    OrderExpired:\n      payload: { $ref: 'events.yaml#/OrderExpired' }\n      after: { $ref: 'configuration.yaml#/keys/ORDER_RETENTION_WINDOW_DAYS' }\n  receives:\n    - message: { $ref: '#/Order/reminders/OrderExpired' }\n      emits: []\n  deletion:\n    triggers:\n      - on: [{ $ref: 'events.yaml#/OrderExpired' }]\n        match:\n          event: { $ref: 'events.yaml#/OrderExpired/properties/orderId' }\n          state: { $ref: '#/Order/state/orderId' }\n    receipt: { $ref: 'events.yaml#/OrderDeleted' }\n",
        );
        assert!(issues.is_empty(), "{:?}", issues.iter().map(|i| (i.rule, &i.message)).collect::<Vec<_>>());
    }

    #[test]
    fn reminder_schedule_table_renders_the_declared_effect() {
        let table = emit_app_reminders(&rd_model(RD_ACTORS_VALID));
        for needle in [
            "actor_type: \"Order\"",
            "on_message: \"OrderPlaced\"",
            "reminder: \"OrderExpired\"",
            "payload_event: \"OrderExpired\"",
            "identity_prop: \"orderId\"",
            "after_days_key: \"ORDER_RETENTION_WINDOW_DAYS\"",
            "after_default_days: 365",
        ] {
            assert!(table.contains(needle), "missing `{}` in:\n{}", needle, table);
        }
        // No declarations → an EMPTY table, but the module still renders (the hand-written
        // application::reminders runtime compiles against a stable module).
        let empty = emit_app_reminders(&rd_model("Order:\n  type: aggregate\n  receives: []\n"));
        assert!(empty.contains("REMINDER_SCHEDULES: &[ReminderSchedule] = &[\n];"), "{}", empty);
    }

    #[test]
    fn deletion_receipt_and_trigger_facts_are_not_orphans() {
        // Section 3's `event-orphan` walk: a fact that exists only in a `deletion:` block is
        // engine vocabulary — the receipt is RECORDED on the ledger, the triggers CONSUMED.
        let model = rd_model(RD_ACTORS_VALID);
        let Report { issues, .. } = validate(&model);
        assert!(
            !issues.iter().any(|i| i.rule == "event-orphan" && (i.location.contains("OrderDeleted") || i.location.contains("CatalogDeleted") || i.location.contains("RestaurantDeleted"))),
            "{:?}",
            issues.iter().filter(|i| i.rule == "event-orphan").map(|i| &i.location).collect::<Vec<_>>()
        );
    }

    #[test]
    fn documentation_renders_reminders_and_deletion_only_when_declared() {
        let md = emit_documentation(&rd_model(RD_ACTORS_VALID));
        for needle in [
            "Reminders (self-scheduled facts — ADR-20260731-214500):",
            "⏰ `OrderExpired`",
            "⏰ schedules `OrderExpired`",
            "Deletion (declarative, generic engine — ADR-20260731-214500):",
            "_immediate (propagation)_",
            "⚙️ `ORDER_RETENTION_WINDOW_DAYS`",
        ] {
            assert!(md.contains(needle), "missing `{}` in the actor docs", needle);
        }
        let bare = emit_documentation(&rd_model("Order:\n  type: aggregate\n  receives:\n    - message: { $ref: 'events.yaml#/OrderPlaced' }\n      emits: []\n"));
        assert!(!bare.contains("Reminders (self-scheduled facts"), "section must not render undeclared");
        assert!(!bare.contains("Deletion (declarative"), "section must not render undeclared");
    }

    #[test]
    fn reminder_identity_is_derived_never_declared() {
        // ADR-20260731-214500 consequences: a reminder's identity is UUIDv5(actor_id, name) —
        // the runtime's reminder_message_id computes it; declaring it is a hard error.
        let issues = rd_issues(
            "Order:\n  type: aggregate\n  reminders:\n    OrderExpired:\n      payload: { $ref: 'events.yaml#/OrderExpired' }\n      identity: orderId\n  receives:\n    - message: { $ref: '#/Order/reminders/OrderExpired' }\n      emits: []\n",
        );
        assert_eq!(rules_of(&issues), vec!["reminder-identity-declared"], "{:?}", issues.iter().map(|i| &i.message).collect::<Vec<_>>());
        assert!(issues[0].location.ends_with("Order/reminders/OrderExpired"), "{}", issues[0].location);
        assert!(issues[0].message.contains("UUIDv5"), "{}", issues[0].message);
    }

    // ─── typed identity / requires $refs (ADR-20260731-214500 consequences, #272 D2) ────────────

    fn mb_issues(actors_yaml: &str) -> Vec<Issue> {
        let mut issues = Vec::new();
        validate_mailbox_addressing(&rd_model(actors_yaml), &mut issues);
        issues
    }

    /// `mailbox.activations` shape rules (#272 D3): a knob that parses to nothing would silently
    /// run the global defaults — every malformed form must be a hard error, and the two legal
    /// forms must be clean.
    #[test]
    fn mailbox_activations_shapes_are_validated() {
        let base = |act: &str| {
            format!(
                "Order:\n  type: aggregate\n  identity: {{ $ref: '#/Order/state/orderId' }}\n  mailbox:\n    partitions: 100\n    activations: {act}\n  receives: []\n",
            )
        };
        for (bad, why) in [
            ("300", "a bare scalar is neither bool nor mapping"),
            ("{ enabled: \"yes\" }", "enabled must be a real bool"),
            ("{ idle_seconds: \"300\" }", "idle_seconds must be an integer"),
            ("{ idle_seconds: 0 }", "zero would disable rather than tune -- use `activations: false`"),
            ("{ idle_secs: 300 }", "an unknown key must not fall through to defaults"),
        ] {
            let issues = mb_issues(&base(bad));
            assert_eq!(
                rules_of(&issues),
                vec!["mb-activations-shape"],
                "{why}: {:?}",
                issues.iter().map(|i| &i.message).collect::<Vec<_>>()
            );
        }
        for ok_form in ["false", "true", "{ enabled: false }", "{ idle_seconds: 60 }", "{ enabled: true, idle_seconds: 60 }"] {
            let issues = mb_issues(&base(ok_form));
            assert!(
                issues.iter().all(|i| i.rule != "mb-activations-shape"),
                "legal form '{ok_form}' flagged: {:?}",
                issues.iter().map(|i| &i.message).collect::<Vec<_>>()
            );
        }
        // The block is legal on a PROCESS MANAGER too (any mailbox actor) — and still validated.
        let pm = "RefundProcess:\n  type: process-manager\n  mailbox:\n    partitions: 100\n    activations: { idle_secs: 1 }\n  receives: []\n";
        let issues = mb_issues(pm);
        assert!(
            issues.iter().any(|i| i.rule == "mb-activations-shape"),
            "PM activations must be validated: {:?}",
            issues.iter().map(|i| (i.rule, &i.message)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn actor_identity_bare_string_is_a_hard_error() {
        let issues = mb_issues(
            "Order:\n  type: aggregate\n  identity: orderId\n  receives: []\n",
        );
        assert_eq!(rules_of(&issues), vec!["identity-untyped"], "{:?}", issues.iter().map(|i| &i.message).collect::<Vec<_>>());
        assert!(
            issues[0].message.contains("#/Order/state/orderId"),
            "the error suggests the exact typed form: {}",
            issues[0].message
        );
    }

    #[test]
    fn actor_identity_ref_must_target_a_same_actor_state_field() {
        // Another actor's state field is NOT this actor's identity — the ref must be
        // `#/<SameActor>/state/<field>`.
        let wrong_actor = mb_issues(
            "Order:\n  type: aggregate\n  identity: { $ref: '#/Catalog/state/orderId' }\n  receives: []\n",
        );
        assert_eq!(rules_of(&wrong_actor), vec!["identity-state-field-missing"], "{:?}", wrong_actor.iter().map(|i| &i.message).collect::<Vec<_>>());
        let wrong_path = mb_issues(
            "Order:\n  type: aggregate\n  identity: { $ref: 'events.yaml#/OrderPlaced/properties/orderId' }\n  receives: []\n",
        );
        assert_eq!(rules_of(&wrong_path), vec!["identity-state-field-missing"], "{:?}", wrong_path.iter().map(|i| &i.message).collect::<Vec<_>>());
        // The well-formed typed ref is clean — the identity field is IMPLICITLY declared by the
        // ref itself (no explicit `state:` entry needed: the stream key exists before any fold).
        let ok = mb_issues(
            "Order:\n  type: aggregate\n  identity: { $ref: '#/Order/state/orderId' }\n  receives:\n    - message: { $ref: 'events.yaml#/OrderPlaced' }\n      emits: []\n",
        );
        assert!(ok.is_empty(), "{:?}", ok.iter().map(|i| (i.rule, &i.message)).collect::<Vec<_>>());
    }

    #[test]
    fn implicit_identity_state_ref_is_exempt_from_ref_dangling() {
        let m = rd_model(
            "Order:\n  type: aggregate\n  identity: { $ref: '#/Order/state/orderId' }\n  receives: []\n",
        );
        // The identity's own self-ref names the implicit stream-key field…
        assert!(is_implicit_identity_state_ref(&m, "#/Order/state/orderId", "actors.yaml"));
        assert!(is_implicit_identity_state_ref(&m, "actors.yaml#/Order/state/orderId", "tests.yaml"));
        // …but any OTHER field, actor or file stays a dangling ref.
        assert!(!is_implicit_identity_state_ref(&m, "#/Order/state/ghost", "actors.yaml"));
        assert!(!is_implicit_identity_state_ref(&m, "#/Catalog/state/orderId", "actors.yaml"));
        assert!(!is_implicit_identity_state_ref(&m, "events.yaml#/Order/state/orderId", "actors.yaml"));
    }

    #[test]
    fn missing_identity_property_warns_split_by_message_kind() {
        // PlaceOrder (a command with no properties) → identity-property-not-on-command;
        // CatalogDeleted (an event without orderId) → id-not-in-payload; the reminder
        // self-message is exempt (the reminder row itself carries the actor_id).
        let issues = mb_issues(
            "Order:\n  type: aggregate\n  identity: { $ref: '#/Order/state/orderId' }\n  reminders:\n    OrderExpired:\n      payload: { $ref: 'events.yaml#/OrderExpired' }\n  receives:\n    - message: { $ref: 'commands.yaml#/PlaceOrder' }\n      emits: []\n    - message: { $ref: 'events.yaml#/CatalogDeleted' }\n      emits: []\n    - message: { $ref: '#/Order/reminders/OrderExpired' }\n      emits: []\n",
        );
        assert_eq!(
            rules_of(&issues),
            vec!["identity-property-not-on-command", "id-not-in-payload"],
            "{:?}",
            issues.iter().map(|i| (i.rule, &i.message)).collect::<Vec<_>>()
        );
        assert!(issues.iter().all(|i| i.level == Level::Warning), "both stay WARN — the mailbox mints an addressing-only id (calibration, see §2d doc)");
    }

    /// THE DECLARATION IS THE PERMISSION (product-owner directive, 2026-08-02, generalized to the
    /// whole client surface): per actor, `send` + the sealed `{Actor}Command` trait exist IFF the
    /// actor's `receives` declares ≥1 COMMAND; `record` + `{Actor}Fact` IFF it declares ≥1
    /// inbound FACT; `schedule`/`cancel_scheduling` IFF it declares `reminders:`. An unjustified surface is
    /// ABSENT (a compile error at any call site), never uncallable-but-present. Bidirectional
    /// over the real catalog, with the per-actor declaration sets re-derived HERE from the model
    /// (an independent scan — the guard does not trust the emitter's own).
    #[test]
    fn client_surface_exists_only_with_a_spec_declaration() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let model = load_model(&root.join("specs")).expect("load real specs");
        let declaring: HashSet<String> =
            parse_reminders(&model).into_iter().map(|r| r.actor).collect();
        assert!(
            !declaring.is_empty(),
            "no actor declares reminders — if the Order pilot left the spec, this guard needs a \
             new positive case, not silence"
        );
        // Independent per-actor receives scan: ref path encodes the kind (§1b).
        let mut has_commands: HashSet<String> = HashSet::new();
        let mut has_facts: HashSet<String> = HashSet::new();
        if let Some(Value::Mapping(actors)) = model.defs.get("actors.yaml") {
            for (k, def) in actors {
                let Some(name) = k.as_str().filter(|s| *s != "principals") else { continue };
                let Some(receives) = def.get("receives").and_then(|r| r.as_sequence()) else {
                    continue;
                };
                for entry in receives {
                    let Some(r) =
                        entry.get("message").and_then(|m| m.get("$ref")).and_then(|r| r.as_str())
                    else {
                        continue;
                    };
                    if r.starts_with("commands.yaml#/") {
                        has_commands.insert(name.to_string());
                    } else if r.starts_with("events.yaml#/") {
                        has_facts.insert(name.to_string());
                    }
                }
            }
        }
        assert!(!has_commands.is_empty() && !has_facts.is_empty(), "the receives scan went blind");

        let clients = emit_actor_clients(&model);
        let mut seen_blocks = 0usize;
        for block in clients.split("\n// ─── ").skip(1) {
            let name = block.split(' ').next().expect("actor name heads the block");
            seen_blocks += 1;
            let surface = [
                ("pub async fn send", has_commands.contains(name), "receives >=1 COMMAND"),
                (
                    &format!("pub trait {name}Command") as &str,
                    has_commands.contains(name),
                    "receives >=1 COMMAND",
                ),
                ("pub async fn record", has_facts.contains(name), "receives >=1 inbound FACT"),
                (
                    &format!("pub trait {name}Fact") as &str,
                    has_facts.contains(name),
                    "receives >=1 inbound FACT",
                ),
                ("pub async fn schedule", declaring.contains(name), "declares reminders:"),
                ("pub async fn cancel_scheduling", declaring.contains(name), "declares reminders:"),
            ];
            for (needle, justified, why) in surface {
                assert_eq!(
                    block.contains(needle),
                    justified,
                    "{name}: `{needle}` must exist IFF the actor {why} in actors.yaml — the spec \
                     declaration is the permission (product-owner directive, 2026-08-02)"
                );
            }
        }
        assert!(seen_blocks > 1, "the per-actor block scan went blind — fix the separator parse");
    }

    /// The §2e fixture: a declared state field with clean lineage, a typed acting ref over it.
    const REQ_SCALARS: &str = "CustomerId: { type: string }\nOrderId: { type: string }\n";
    const REQ_EVENTS: &str = "ConversationOpened:\n  type: object\n  properties:\n    customerId: { $ref: 'scalars.yaml#/CustomerId' }\n";
    const REQ_ACTOR_HEAD: &str = "principals:\n  CUSTOMER: { id: { $ref: 'scalars.yaml#/CustomerId' } }\nConversation:\n  type: aggregate\n  identity: { $ref: '#/Conversation/state/orderId' }\n  state:\n    customerId:\n      type: { $ref: 'scalars.yaml#/CustomerId' }\n      from: [{ $ref: 'events.yaml#/ConversationOpened/properties/customerId' }]\n  receives:\n    - message: { $ref: 'commands.yaml#/PostMessage' }\n      emits: [{ $ref: 'events.yaml#/ConversationOpened' }]\n";

    fn req_issues(acting: &str) -> Vec<Issue> {
        let actors = format!("{}      requires:\n        acting:\n{}", REQ_ACTOR_HEAD, acting);
        let m = inline_model(&[
            ("scalars.yaml", REQ_SCALARS),
            ("events.yaml", REQ_EVENTS),
            ("commands.yaml", "PostMessage:\n  type: object\n"),
            ("actors.yaml", actors.as_str()),
        ]);
        let mut issues = Vec::new();
        validate_actor_state(&m, &mut issues);
        issues
    }

    #[test]
    fn requires_acting_typed_ref_and_any_keyword_are_clean() {
        let issues = req_issues("          CUSTOMER: { $ref: '#/Conversation/state/customerId' }\n          ADMIN: any\n");
        assert!(issues.is_empty(), "{:?}", issues.iter().map(|i| (i.rule, &i.message)).collect::<Vec<_>>());
    }

    #[test]
    fn requires_acting_bare_state_path_is_a_hard_error() {
        let issues = req_issues("          CUSTOMER: state.customerId\n");
        assert_eq!(rules_of(&issues), vec!["requires-acting-untyped"], "{:?}", issues.iter().map(|i| &i.message).collect::<Vec<_>>());
        assert!(
            issues[0].message.contains("#/Conversation/state/customerId"),
            "the error suggests the exact typed form: {}",
            issues[0].message
        );
    }

    #[test]
    fn requires_acting_ref_must_resolve_to_a_declared_same_actor_state_field() {
        // Wrong actor → shape error; right shape but undeclared field → unknown-field error
        // (acting refs are STRICT: unlike `identity`, they bind to explicitly declared fold
        // state — the folded value is what the authorization compares against).
        let wrong_actor = req_issues("          CUSTOMER: { $ref: '#/Order/state/customerId' }\n");
        assert_eq!(rules_of(&wrong_actor), vec!["req-state-unknown"], "{:?}", wrong_actor.iter().map(|i| &i.message).collect::<Vec<_>>());
        let ghost_field = req_issues("          CUSTOMER: { $ref: '#/Conversation/state/ghost' }\n");
        assert_eq!(rules_of(&ghost_field), vec!["req-state-unknown"], "{:?}", ghost_field.iter().map(|i| &i.message).collect::<Vec<_>>());
        assert!(ghost_field[0].message.contains("undeclared state field"), "{}", ghost_field[0].message);
    }

    #[test]
    fn typed_identity_migration_keeps_generated_runtime_byte_identical() {
        // The string-path → typed-$ref migration changes DECLARATION SYNTAX only: the identity
        // property NAMES are unchanged, so the frozen routing contract (command_router.rs) and
        // the declared-state folds (states.rs) must regenerate byte-identically against the
        // committed files — any diff here means the migration changed semantics, not syntax.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let model = load_model(&root.join("specs")).expect("load real specs");
        let router = emit_infra_command_router(&model);
        let committed_router = std::fs::read_to_string(root.join("crates/infrastructure/src/generated/command_router.rs")).expect("committed command_router.rs");
        assert_eq!(router, committed_router, "command_router.rs must stay byte-identical across the typed-identity migration");
        // The addressing half of the frozen routing contract lives in the actor_client crate
        // since #290 phase 1 — same byte-identity requirement.
        let addresses = emit_actor_addresses(&model);
        let committed_addresses = std::fs::read_to_string(root.join("crates/actor_client/src/generated/addresses.rs")).expect("committed addresses.rs");
        assert_eq!(addresses, committed_addresses, "addresses.rs must stay byte-identical across the typed-identity migration");
        let states = emit_domain_states(&model);
        let committed_states = std::fs::read_to_string(root.join("crates/domain/src/generated/states.rs")).expect("committed states.rs");
        assert_eq!(states, committed_states, "states.rs must stay byte-identical across the typed-requires migration");
        // 0 errors, and the CALIBRATION holds: exactly one command legitimately lacks its
        // identity property (RequestPhoneVerification — the server mints the customer id), which
        // is why identity-property-not-on-command is a WARN, not an error (§2d doc).
        let Report { issues, .. } = validate(&model);
        for i in &issues {
            assert!(i.level != Level::Error, "real specs must stay 0-error: {} at {}: {}", i.rule, i.location, i.message);
        }
        let cmd_warns: Vec<&Issue> = issues.iter().filter(|i| i.rule == "identity-property-not-on-command").collect();
        assert_eq!(cmd_warns.len(), 1, "{:?}", cmd_warns.iter().map(|i| (&i.location, &i.message)).collect::<Vec<_>>());
        assert_eq!(cmd_warns[0].location, "actors.yaml/Customer");
        assert!(cmd_warns[0].message.contains("RequestPhoneVerification"), "{}", cmd_warns[0].message);
    }

    #[test]
    fn real_specs_carry_the_order_retention_pilot_and_gain_no_issues() {
        // The Order pilot IS in the committed catalog (#272 D2): one reminder (OrderExpired,
        // windowed, rescheduled in place), one deletion block (self-trigger on the recorded
        // expiry, receipt OrderDeleted) — and the whole catalog keeps validating with zero
        // errors and none of the §2f rules tripping.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let model = load_model(&root.join("specs")).expect("load real specs");
        let reminders = parse_reminders(&model);
        assert_eq!(
            reminders.iter().map(|r| (r.actor.as_str(), r.name.as_str())).collect::<Vec<_>>(),
            vec![("Order", "OrderExpired")],
            "the Order pilot is the one declared reminder"
        );
        let deletions = parse_deletions(&model);
        assert_eq!(
            deletions.iter().map(|d| d.actor.as_str()).collect::<Vec<_>>(),
            vec!["Order"],
            "the Order pilot is the one declared deletion block"
        );
        let table = emit_infra_deletion_policy(&model).expect("Order declares deletion → table emitted");
        assert!(table.contains("receipt: \"OrderDeleted\""), "{}", table);
        let schedules = emit_app_reminders(&model);
        for on in ["MarkOrderDelivered", "RejectOrder", "CancelOrderByCustomer", "CancelOrderByRestaurant"] {
            assert!(
                schedules.contains(&format!("on_message: \"{}\"", on)),
                "terminal receive {} must schedule the expiry:\n{}",
                on,
                schedules
            );
        }
        let Report { issues, .. } = validate(&model);
        const NEW_RULES: [&str; 10] = [
            "reminder-identity-declared",
            "reminder-without-receive",
            "receive-without-reminder",
            "schedules-unresolved",
            "deletion-ref-unresolved",
            "deletion-match-untyped",
            "deletion-tree-cycle",
            "reminder-payload-not-event",
            "reminder-after-unresolved",
            "reminder-reschedule-unknown",
        ];
        for i in &issues {
            assert!(!NEW_RULES.contains(&i.rule), "unexpected {} at {}: {}", i.rule, i.location, i.message);
            assert!(i.level != Level::Error, "real specs must stay 0-error: {} at {}: {}", i.rule, i.location, i.message);
        }
        let md = emit_documentation(&model);
        assert!(md.contains("Reminders (self-scheduled facts"), "the pilot renders its doc section");
        assert!(md.contains("Deletion (declarative"), "the pilot renders its doc section");
    }

    // ─── §13 — proposal hygiene (#272 — CLAUDE.md "Named concerns" + docs/proposals/README.md) ──

    fn hygiene(files: &[(&str, &str)]) -> Vec<Issue> {
        let owned: Vec<(String, String)> =
            files.iter().map(|(p, c)| (p.to_string(), c.to_string())).collect();
        validate_proposal_hygiene(&owned)
    }

    fn hygiene_rules(issues: &[Issue]) -> Vec<&'static str> {
        issues.iter().map(|i| i.rule).collect()
    }

    const TRACK: &str =
        "- **Tracking issue**: [#9 \"t\"](https://github.com/TheCaptainCompany/captain-food/issues/9)\n";

    #[test]
    fn proposal_status_line_is_required() {
        let list_form = format!("# PROP-x — t\n- **Status**: Proposed\n{TRACK}");
        assert!(hygiene(&[("docs/proposals/PROP-a.md", &list_form)]).is_empty());
        let bare_form = format!("# PROP-x — t\n**Status**: Proposed\n{TRACK}");
        assert!(
            hygiene(&[("docs/proposals/PROP-b.md", &bare_form)]).is_empty(),
            "the bare `**Status**:` form used by existing files must be tolerated"
        );
        let missing = format!("# PROP-x — t\nno header block here\n{TRACK}");
        let issues = hygiene(&[("docs/proposals/PROP-c.md", &missing)]);
        assert_eq!(hygiene_rules(&issues), vec!["proposal-status-missing"]);
        assert!(issues[0].level == Level::Error, "status is a header REQUIREMENT — error");
    }

    #[test]
    fn proposal_tracking_issue_link_must_be_in_the_header() {
        let ok = format!("# PROP-x — t\n- **Status**: Proposed\n{TRACK}");
        assert!(hygiene(&[("docs/proposals/PROP-a.md", &ok)]).is_empty());
        // A bare `#NN` is a dead reference in repo markdown — only the full URL counts.
        let bare_number = "# PROP-x — t\n- **Status**: Proposed\n- **Tracking issue**: #272\n";
        let issues = hygiene(&[("docs/proposals/PROP-b.md", bare_number)]);
        assert_eq!(hygiene_rules(&issues), vec!["proposal-tracking-issue-missing"]);
        assert!(issues[0].level == Level::Error, "corpus-calibrated: every PROP-* passes — error");
        // A link buried past the first 40 lines is not IN THE HEADER.
        let buried = format!("# PROP-x — t\n- **Status**: Proposed\n{}{TRACK}", "\n".repeat(45));
        assert_eq!(
            hygiene_rules(&hygiene(&[("docs/proposals/PROP-c.md", &buried)])),
            vec!["proposal-tracking-issue-missing"]
        );
    }

    #[test]
    fn approved_proposal_blocks_on_unchecked_concern_scoped_to_the_concerns_block() {
        // Header-entry form: an unchecked named concern under `- **Concerns**:` blocks Approved.
        let unresolved = format!(
            "# P\n- **Status**: Approved — ADR-20260731-000000\n{TRACK}- **Concerns**:\n  - [ ] latency: unmeasured at peak\n"
        );
        let issues = hygiene(&[("docs/proposals/PROP-a.md", &unresolved)]);
        assert_eq!(hygiene_rules(&issues), vec!["proposal-approved-unresolved-concern"]);
        assert!(issues[0].level == Level::Error);
        // Resolving = CHECKING the item with a resolution — the checked form passes.
        let resolved = format!(
            "# P\n- **Status**: Approved — ADR-20260731-000000\n{TRACK}- **Concerns**:\n  - [x] latency: measured — P99 ok at peak\n"
        );
        assert!(hygiene(&[("docs/proposals/PROP-b.md", &resolved)]).is_empty());
        // `## Concerns` section form: blank lines inside the section do not end it — the next
        // heading does.
        let section = format!(
            "# P\n- **Status**: APPROVED (in-session) — ADR-20260731-000000\n{TRACK}\n## Concerns\n\n- [ ] naming: collides with the PM state table\n\n## Next steps\n"
        );
        assert_eq!(
            hygiene_rules(&hygiene(&[("docs/proposals/PROP-c.md", &section)])),
            vec!["proposal-approved-unresolved-concern"]
        );
        // SCOPED: unchecked checklists OUTSIDE the Concerns block (scope checklists, a checklist
        // after the sibling header field that ends the entry-form block) must NOT trip the rule.
        let scope_only = format!(
            "# P\n- **Status**: Approved — ADR-20260731-000000\n{TRACK}- **Concerns**:\n  - [x] a: done\n- **Realized by**: ADR-20260731-000000\n\n## Scope\n- [ ] later slice, deliberately deferred\n"
        );
        assert!(hygiene(&[("docs/proposals/PROP-d.md", &scope_only)]).is_empty());
        // An unchecked concern on a NON-approved proposal is fine — it is what blocks the flip.
        let proposed = format!(
            "# P\n- **Status**: Proposed\n{TRACK}- **Concerns**:\n  - [ ] latency: unmeasured at peak\n"
        );
        assert!(hygiene(&[("docs/proposals/PROP-e.md", &proposed)]).is_empty());
    }

    #[test]
    fn approved_proposal_must_reference_a_decision_record() {
        let no_adr = format!("# P\n- **Status**: Approved (product owner, in-session)\n{TRACK}body\n");
        let issues = hygiene(&[("docs/proposals/PROP-a.md", &no_adr)]);
        assert_eq!(hygiene_rules(&issues), vec!["proposal-approved-without-decision"]);
        assert!(issues[0].level == Level::Error, "corpus-calibrated: every Approved PROP-* names an ADR — error");
        let with_adr = format!(
            "# P\n- **Status**: Approved\n{TRACK}Recorded by ADR-20260731-061609.\n"
        );
        assert!(hygiene(&[("docs/proposals/PROP-b.md", &with_adr)]).is_empty());
        // Not Approved → no decision-record requirement.
        let proposed = format!("# P\n- **Status**: Proposed\n{TRACK}body\n");
        assert!(hygiene(&[("docs/proposals/PROP-c.md", &proposed)]).is_empty());
        // Lowercase "partially approved" prose inside a Proposed status is NOT an approval
        // (the PROP-20260730-032306 shape).
        let partial = format!("# P\n- **Status**: Proposed (partially approved — see §3)\n{TRACK}body\n");
        assert!(hygiene(&[("docs/proposals/PROP-d.md", &partial)]).is_empty());
    }

    #[test]
    fn real_proposals_satisfy_the_hygiene_rules() {
        // The committed corpus is the calibration baseline (2026-07-31): all four rules are ERRORS
        // because every PROP-* file passes them — this test keeps that true, so the gate stays
        // 0-error and no rule ever needs grandfathering down to a warning.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let files = load_proposal_files(&root);
        assert!(!files.is_empty(), "expected the committed docs/proposals/PROP-*.md corpus");
        let issues = validate_proposal_hygiene(&files);
        let errors: Vec<String> = issues
            .iter()
            .filter(|i| i.level == Level::Error)
            .map(|i| format!("{} at {}: {}", i.rule, i.location, i.message))
            .collect();
        assert!(errors.is_empty(), "proposal hygiene must be 0-error on the committed corpus:\n{}", errors.join("\n"));
    }

    #[test]
    fn actor_clients_cover_every_mailbox_actor() {
        // #284 slice 1 (PROP-20260728-152752 §2.1): the typed-client surface must span the SAME
        // actor set the composition root spawns workers for — one client + sealed Command/Fact
        // marker traits per ACTOR_MAILBOXES entry. The actor list is parsed out of the EMITTED
        // addresses table (not re-derived from the spec), so the two artifacts cannot diverge
        // silently — and the router must keep RE-EXPORTING that one definition (#290 phase 1).
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let model = load_model(&root.join("specs")).expect("load real specs");
        let clients = emit_actor_clients(&model);
        let addresses = emit_actor_addresses(&model);
        let router = emit_infra_command_router(&model);
        assert!(
            router.contains("pub use actor_client::generated::addresses::{mailbox_address, ACTOR_MAILBOXES};"),
            "the router must re-export the ONE addressing definition, never re-emit it"
        );
        let block = addresses
            .split("pub const ACTOR_MAILBOXES: &[(&str, u16)] = &[")
            .nth(1)
            .and_then(|rest| rest.split("];").next())
            .expect("ACTOR_MAILBOXES block in the emitted addresses table");
        let actors: Vec<&str> = block
            .lines()
            .filter_map(|l| l.trim().strip_prefix("(\""))
            .filter_map(|l| l.split('"').next())
            .collect();
        assert!(!actors.is_empty(), "expected at least one mailbox actor in the emitted router");
        for actor in &actors {
            // The CLIENT struct exists for every mailbox actor (it is the lane handle); the
            // marker traits and methods are SPEC-GATED per declaration — asserted bidirectionally
            // by `client_surface_exists_only_with_a_spec_declaration` below.
            let item = format!("pub struct {actor}Client");
            assert!(clients.contains(&item), "generated actor_clients.rs lacks `{item}`");
        }
        // The seal itself must be present — without the private supertrait module the whole
        // compile-time guarantee (no impls outside the generated file) evaporates.
        assert!(clients.contains("mod sealed {"), "the privacy seal module is missing");
    }
