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
        Model { defs, ..Default::default() }
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
        Model { defs, ..Default::default() }
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
        Model { defs, ..Default::default() }
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
            ..Default::default()
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
                ..Default::default()
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
            ..Default::default()
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
        // The configuration catalog is split across specs/{scope}/configuration.yaml fragments
        // (ADR-20260807-183024 D5) — load the merged logical model, not one file.
        let model = load_model(&root.join("specs")).expect("load real specs");
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
        // The configuration catalog is split across specs/{scope}/configuration.yaml fragments
        // (ADR-20260807-183024 D5) — load the merged logical model, not one file.
        let model = load_model(&root.join("specs")).expect("load real specs");
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
    /// Scoped to the EMITTED contracts deliberately (#191's two plus `cart-price` since #451): the
    /// remaining contracts are not yet emitted, and asserting them here would fail for work no issue
    /// claims to have done. When a contract's instrumentation lands, add its feature to the list.
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
        for feature in ["command-acceptance", "place-order", "cart-price"] {
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
        // The scalars catalog is split across specs/{scope}/scalars.yaml fragments
        // (ADR-20260807-183024 D1) — read the merged logical catalog from the loader.
        let model = load_model(&root.join("specs").to_path_buf()).expect("load real specs");
        let scalars = model.defs.get("scalars.yaml").expect("scalars catalog").clone();

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
             (the per-actor crates under crates/clients/* — send/record/schedule), or, for \
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
        attrs.iter().any(attr_is_test_only)
    }


    /// Every trait DERIVED by this attribute, following nested `cfg_attr` the way rustc expands it.
    ///
    /// A one-level parse bans two spellings rather than the class: rustc expands
    /// `#[cfg_attr(a, cfg_attr(b, derive(Default)))]` recursively, and the `#[path]` check in this
    /// same file already scans the whole token stream and so survives nesting. A cfg_attr whose
    /// condition is POSITIVELY test-only contributes nothing — that derive never reaches a
    /// dependent crate.
    fn derives_from_meta(m: &syn::Meta) -> Vec<String> {
        if m.path().is_ident("derive") {
            return ident_tokens(quote::quote!(#m))
                .into_iter()
                .filter(|t| t != "derive")
                .collect();
        }
        if m.path().is_ident("cfg_attr") {
            let syn::Meta::List(l) = m else { return Vec::new() };
            let Ok(nested) = l.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            ) else {
                return Vec::new();
            };
            let mut it = nested.iter();
            let Some(cond) = it.next() else { return Vec::new() };
            if cfg_is_test_only(cond) {
                return Vec::new();
            }
            return it.flat_map(derives_from_meta).collect();
        }
        Vec::new()
    }

    /// Is this cfg predicate satisfied ONLY in a test build — evaluated over the cfg tree rather
    /// than sniffed for the substring `test`?
    ///
    /// The distinction is a door. `any(test, feature = "serde")` MENTIONS `test` and fires in every
    /// release build with the feature on, so a mention-check reads it as a gate and stops scanning
    /// everything under it. Refusing `any(..)` outright is not the fix either: the real
    /// `#[cfg(any(test, feature = "test-fixtures"))]` on the fixtures module is a legitimate
    /// all-test-disjuncts `any` that must keep passing. So: `any` is test-only when EVERY disjunct
    /// is, `all` when ANY conjunct is, and `not` never (conservative).
    ///
    /// One predicate, actually shared — `is_test_only_cfg`, `is_fixtures_gate` and the
    /// `cfg_attr`-derive arm all route through it. Three inline near-copies of this logic existed
    /// before, they were not identical, and the weakest of them decided whether a whole item was
    /// scanned at all.
    fn cfg_is_test_only(m: &syn::Meta) -> bool {
        let path_is = |p: &syn::Path, n: &str| p.is_ident(n);
        match m {
            // `test`
            syn::Meta::Path(p) => path_is(p, "test"),
            // `feature = "test-fixtures"` — compared EXACTLY. A `contains` here accepts
            // `test-fixtures-v2`, `no-test-fixtures` and `serde-test-fixtures-shim`, each of which
            // would switch both guards off for everything under it in a release build. The
            // structure is evaluated; the leaf must be too, or the evaluator just moves the sniff
            // one level down.
            syn::Meta::NameValue(nv) => {
                path_is(&nv.path, "feature")
                    && matches!(
                        &nv.value,
                        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(v), .. })
                            if v.value() == "test-fixtures"
                    )
            }
            syn::Meta::List(l) => {
                let Ok(inner) = l.parse_args_with(
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                ) else {
                    return false;
                };
                if path_is(&l.path, "any") {
                    !inner.is_empty() && inner.iter().all(cfg_is_test_only)
                } else if path_is(&l.path, "all") {
                    inner.iter().any(cfg_is_test_only)
                } else {
                    // `not(..)` and anything unrecognised: never a gate.
                    false
                }
            }
        }
    }

    /// The same question asked of a whole `#[cfg(..)]` ATTRIBUTE.
    fn attr_is_test_only(a: &syn::Attribute) -> bool {
        if !a.path().is_ident("cfg") {
            return false;
        }
        a.parse_args::<syn::Meta>().map(|m| cfg_is_test_only(&m)).unwrap_or(false)
    }

    /// Specifically a POSITIVE `test-fixtures` gate — what makes the one public mint legitimate.
    fn is_fixtures_gate(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|a| {
            attr_is_test_only(a) && {
                let m = &a.meta;
                quote::quote!(#m).to_string().contains("test-fixtures")
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
                    if s.ident == WITNESS {
                        // A DERIVE is a trait impl spelled in one word, and the leak rule below
                        // only ever saw `Item::Impl`. `#[derive(Default)]` on the witness hands
                        // every crate in the workspace a public mint via `Default::default()` —
                        // proven from `server`, which holds only the port. Allowlist the derives
                        // that cannot construct one; refuse the rest as a class.
                        const HARMLESS: &[&str] =
                            &["Debug", "Clone", "Copy", "PartialEq", "Eq", "Hash", "PartialOrd", "Ord"];
                        let mut derived: Vec<String> = Vec::new();
                        for a in &s.attrs {
                            derived.extend(derives_from_meta(&a.meta));
                        }
                        for tok in derived {
                            if tok != "derive" && !HARMLESS.contains(&tok.as_str()) {
                                out.leaks.push(format!(
                                    "  {rel}: `derive({tok})` on the witness — a derive is a trait \
                                     impl in one word, and anything that can construct `Self` \
                                     (`Default`, `From`, `FromStr`, `Deserialize`, …) is a public \
                                     mint for every crate in the workspace"
                                ));
                            }
                        }
                    }
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

        // PHASE 2 (#306): the generated per-actor client crates are outside this crate, so the
        // compiler already makes the witness unmintable there. The scan still refuses to see it
        // NAMED — a client crate that mentions the witness is one whose emitter started reaching
        // for the port directly instead of delegating through `ActorDoor`, which is the shape that
        // would make widening the mint look like the obvious fix. Same textual form (and the same
        // comment-stripping) as the `/generated/` rule above, for the same reason.
        let clients_root = root.join("crates/clients");
        let mut client_files = Vec::new();
        let mut stack = vec![clients_root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                    client_files.push(p);
                }
            }
        }
        assert!(
            !client_files.is_empty(),
            "found no crates/clients sources — the per-actor client walk went blind"
        );
        for p in &client_files {
            let rel = p.strip_prefix(&root).unwrap_or(p).to_string_lossy().replace('\\', "/");
            let text = std::fs::read_to_string(p).expect("a partially-scanned tree is a silent no-op");
            if text.lines().filter(|l| !l.trim_start().starts_with("//")).any(|l| l.contains(WITNESS))
            {
                scan.leaks.push(format!(
                    "  {rel}: a generated client crate must never name the witness — enqueue \
                     through `actor_client::ActorDoor`"
                ));
            }
        }

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

    /// The same scan over a `const`/`static` INITIALIZER. Item initializers are constructions like
    /// any other, and skipping them was a traversal gap rather than a scope limit: hoisting the
    /// witness into `const HELD: MailboxAccess = MailboxAccess(());` — the ordinary way to stop
    /// calling the mint in three places — made a public `cancel_any` over a held `Arc<dyn Mailbox>`
    /// invisible to BOTH guards, which is verbatim the shape this test exists to catch.
    fn scan_init(expr: &syn::Expr, on_witness: bool) -> (bool, std::collections::HashSet<String>) {
        use syn::visit::Visit;
        let mut s = BodyScan::default();
        s.visit_expr(expr);
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

    /// Items declared inside a function BODY. An `impl` written in a fn is NOT scoped to it —
    /// rustc says so itself ("an `impl` is never scoped, even when it is nested inside an item") —
    /// so `fn setup() { impl Held { pub(crate) const H: W = W(()); } }`
    /// puts a mint in the crate while the walk sees only a private, never-called `setup`.
    fn nested_items(block: &syn::Block) -> Vec<syn::Item> {
        block
            .stmts
            .iter()
            .filter_map(|st| match st {
                syn::Stmt::Item(i) => Some(i.clone()),
                _ => None,
            })
            .collect()
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
                    });
                    collect_fns(&nested_items(&f.block), rel, t, out);
                }
                // A `const`/`static` that CONSTRUCTS a witness is a mint whose name then flows
                // through the existing fixpoint like any other callee.
                Item::Const(c) => {
                    let (mints, refs) = scan_init(&c.expr, false);
                    out.push(FnNode {
                        rel: rel.into(),
                        name: c.ident.to_string(),
                        public: is_pub(&c.vis),
                        test_only: t,
                        mints,
                        refs,
                    })
                }
                Item::Static(st) => {
                    let (mints, refs) = scan_init(&st.expr, false);
                    out.push(FnNode {
                        rel: rel.into(),
                        name: st.ident.to_string(),
                        public: is_pub(&st.vis),
                        test_only: t,
                        mints,
                        refs,
                    })
                }
                Item::Impl(i) => {
                    let trait_impl = i.trait_.is_some();
                    let on_witness = mentions(&i.self_ty, WITNESS);
                    for ii in &i.items {
                        if let syn::ImplItem::Const(c) = ii {
                            let (mints, refs) = scan_init(&c.expr, on_witness);
                            out.push(FnNode {
                                rel: rel.into(),
                                name: c.ident.to_string(),
                                public: trait_impl || is_pub(&c.vis),
                                test_only: t || is_test_only_cfg(&c.attrs),
                                mints,
                                refs,
                            });
                        }
                        if let syn::ImplItem::Fn(f) = ii {
                            collect_fns(&nested_items(&f.block), rel, t, out);
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
                        // A trait-declared associated const with a DEFAULT is the fourth const
                        // position, and the one the first pass at this missed. A PRIVATE trait
                        // carrying it is invisible to the signature guard too (that arm only
                        // inspects `pub trait`), so the two together left the class open.
                        if let syn::TraitItem::Const(c) = ti {
                            if let Some((_, expr)) = &c.default {
                                let (mints, refs) = scan_init(expr, false);
                                out.push(FnNode {
                                    rel: rel.into(),
                                    name: c.ident.to_string(),
                                    public: is_pub(&tr.vis),
                                    test_only: t,
                                    mints,
                                    refs,
                                });
                            }
                        }
                        if let syn::TraitItem::Fn(f) = ti {
                            // Only PROVIDED methods have a body that could mint.
                            if let Some(block) = &f.default {
                                collect_fns(&nested_items(block), rel, t, out);
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
            // The `ActorDoor` facade — what the typed write doors became in phase 2 (#306).
            // WHERE THE OLD FOUR ENTRIES WENT, and why the scan does not follow them: until
            // phase 2 the four typed doors (`send`/`record`/`schedule`/`cancel_scheduling`) sat
            // in ONE file in this crate, `src/generated/actor_clients.rs`, and were declared here
            // as four entries. #306 moved every `{Actor}Client` into its own `crates/clients/*`
            // crate, OUT of the tree this scan walks (`crates/actor_client/src`). That is a real
            // reduction in reach and is recorded rather than papered over — it is sound because
            // those crates are 100% EMITTED from actors.yaml, so they cannot suffer the accident
            // this guard exists to catch (ADR-20260803-203455: "the plausible in-crate accident —
            // a scoped-capability helper written `pub` instead of `pub(crate)`"). A new door there
            // is an emitter change, reviewed as codegen, and is additionally held by
            // `client_crates_are_exactly_the_mailbox_actors` +
            // `actor_door_is_named_only_by_generated_client_crates`. What the client crates can
            // reach at all is the four methods below, and those ARE scanned.
            // #331 froze this guard's TECHNIQUE; the ADR calls a moved door maintenance, which is
            // what this edit is — the scan root is deliberately unchanged.
            ("crates/actor_client/src/door.rs", "send_command", false, "typed command door (delegate behind every generated `{Actor}Client::send`)"),
            ("crates/actor_client/src/door.rs", "record_fact", false, "typed inbound-fact door (delegate behind `{Actor}Client::record`)"),
            ("crates/actor_client/src/door.rs", "schedule_command", false, "reminder door (delegate behind `{Actor}Client::schedule`)"),
            ("crates/actor_client/src/door.rs", "cancel_scheduling", false, "reminder withdrawal, ADR-20260731-150500 §3 (delegate behind `{Actor}Client::cancel_scheduling`)"),
            // The one generic read door (PROP-20260802-130500 D4).
            ("crates/actor_client/src/client.rs", "get_operation_status", false, "ActorClient: the ONLY read path over inbound_messages status"),
            // The reminder constructor the in-transaction `schedules:` upsert binds from.
            ("crates/actor_client/src/reminders.rs", "declare", false, "the pool-backed reminder declaration (ADR-20260731-214500)"),
            // The D8 bulk fact door — additionally gated by the `bulk-door` feature, which
            // `bulk_door_feature_is_granted_only_to_infrastructure` allows only infrastructure to enable.
            ("crates/actor_client/src/enqueue.rs", "enqueue_inbound_facts", true, "the UNTYPED bulk fact door, `bulk-door` feature (#290 review BLOCKING-1a)"),
            // Test-only reference implementations (never in a release graph).
            // Syntactically `pub` because `door.rs::record_fact` calls it in a RELEASE build, but
            // `mod enqueue` is private and the re-export in lib.rs is `cfg(any(test,
            // feature = "test-fixtures"))` — so its external reach is exactly the D5 drift guard,
            // which #306 moved out of the crate. Gated, hence taint still flows to `record_fact`.
            ("crates/actor_client/src/enqueue.rs", "enqueue_inbound_fact", true, "single-fact reference impl; private module + `test-fixtures`-gated re-export (#306 out-of-crate drift guard)"),
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
            ("crates/bin_runtime/Cargo.toml", "sqlx",
             "the per-bin composition root (#385): constructs the declared-size PgPool each wired bin injects — same grant, same reason as crates/server above"),
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
            ("crates/gateway_runtime/Cargo.toml", "reqwest",
             "the role gateway IS a proxy (#385, D8): it forwards each GraphQL request to the owning subgraph Service — network reach is its entire job, and it holds neither sqlx nor any domain crate"),
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
        //
        // The generated `crates/clients/*` are boundary crates too, but they are matched by prefix
        // below rather than listed here — see the comment at the match.
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
            // Every GENERATED per-actor client crate is a boundary crate by construction (#306):
            // it exists to BE the door to one actor. Matched by PREFIX rather than enumerated,
            // because the set is spec-derived — a new actor must not be able to join the workspace
            // below the floor just because nobody remembered to extend a list here.
            let boundary = BOUNDARY.contains(&rel.as_str()) || rel.starts_with("crates/clients/");
            if boundary {
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

    /// The PHASE-2 CONTAINMENT guard (#306, PROP-20260802-130500 §6): `actor_client::ActorDoor` is
    /// the opaque facade the per-actor client crates enqueue through, and it may be named by
    /// NOTHING ELSE.
    ///
    /// WHY THIS GUARD IS PART OF THE CHANGE THAT INTRODUCED THE DOOR. Splitting the clients into
    /// their own crates needed those crates to build mailbox rows, and the two things row-building
    /// needs — `MailboxEntry`'s private fields and the `MailboxAccess` mint — are exactly what D1
    /// and #304 keep inside `actor_client`. The proposal named two exits: widen the constructors to
    /// `pub` (which slides the port boundary from compiler-enforced to allowlist-enforced) or hand
    /// out an opaque facade. The facade keeps the entry and the witness at level 4 — but the facade
    /// ITSELF is a public, string-keyed door: `send_command("Restaurant", 5, id, "…", payload, env)`
    /// addresses any actor with any message, which the sealed `{Actor}Command` traits make
    /// impossible on the typed path. That capability did not exist before phase 2 (`command_entry`
    /// was `pub(crate)`), so it is a real widening and this is what contains it: naming `ActorDoor`
    /// outside the generated client crates is CI-red — the loud, reviewable act.
    #[test]
    fn actor_door_is_named_only_by_generated_client_crates() {
        const DOOR: &str = "ActorDoor";
        // (file, WHY) — the door's own definition and the crate root that re-exports it. Both are
        // asserted to still MENTION it, so a rename cannot leave this guard scanning for nothing.
        const ALLOWED: &[(&str, &str)] = &[
            ("crates/actor_client/src/door.rs", "the ActorDoor definition itself"),
            ("crates/actor_client/src/lib.rs", "the crate-root re-export"),
        ];

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root resolves");

        for (rel, why) in ALLOWED {
            let src = std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| {
                panic!(
                    "allowlisted file {rel} ({why}) cannot be read ({e}) — if it moved, move this \
                     allowlist WITH it; do NOT let this guard silently no-op"
                )
            });
            assert!(
                src.contains(DOOR),
                "allowlisted file {rel} ({why}) no longer names `{DOOR}` — the door moved or was \
                 renamed; move this guard with it so it stays real"
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
        for f in &files {
            let rel = f.strip_prefix(&root).unwrap_or(f).to_string_lossy().replace('\\', "/");
            let src = std::fs::read_to_string(f)
                .unwrap_or_else(|e| panic!("cannot read {rel} ({e}) — a partially-scanned tree is a silent no-op"));
            // The GENERATED client crates are the sanctioned holders; so are the two allowlisted
            // files. Comment lines are dropped first, so prose explaining this very rule (and the
            // generated crates' own header comments) cannot trip it.
            if rel.starts_with("crates/clients/") || ALLOWED.iter().any(|(a, _)| rel == *a) {
                continue;
            }
            for (idx, line) in src.lines().enumerate() {
                let t = line.trim_start();
                if t.starts_with("//") || !line.contains(DOOR) {
                    continue;
                }
                offenders.push(format!("  {rel}:{}: {}", idx + 1, line.trim()));
            }
        }
        assert!(
            offenders.is_empty(),
            "`{DOOR}` is named outside the generated per-actor client crates:\n{}\n\n\
             Fix: address the actor through its typed client crate (`client-<actor>`, adding the \
             dependency to your Cargo.toml — that manifest line IS the permission). Why: the door \
             is string-keyed, so anything holding it can send any message to any actor, bypassing \
             the sealed {{Actor}}Command/{{Actor}}Fact traits that make a non-received message a \
             compile error. It exists solely so the per-actor crates need neither `MailboxEntry`'s \
             private fields nor the `MailboxAccess` mint.",
            offenders.join("\n")
        );
    }

    /// The generated client crates must be EXACTLY the mailbox actors — no stale crate for an
    /// actor the spec dropped, no actor without a door.
    ///
    /// The emitter prunes stale directories, so this cannot drift through a normal regeneration;
    /// the guard exists for the abnormal one — a hand-created directory under `crates/clients/`
    /// joins the workspace through the members GLOB without any spec ever mentioning it, which is
    /// precisely a door with no declared owner (ADR-20260802-170059).
    #[test]
    fn client_crates_are_exactly_the_mailbox_actors() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let model = load_model(&root.join("specs")).expect("load real specs");
        let expected: std::collections::BTreeSet<String> =
            emit_client_crates(&model).iter().map(|c| kebab(&c.actor)).collect();
        assert!(!expected.is_empty(), "the actor scan went blind — no client crates expected at all");

        let dir = root.join("crates/clients");
        let mut found: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for e in std::fs::read_dir(&dir).expect("crates/clients is readable").flatten() {
            let p = e.path();
            if p.is_dir() {
                found.insert(p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string());
            }
        }
        assert_eq!(
            found, expected,
            "the crates/clients/ directories do not match the mailbox actors in actors.yaml.\n\n\
             Fix: run `make generate` (the emitter creates missing crates and removes stale ones). \
             Why: the workspace members list is a glob, so a directory here is a workspace crate — \
             one that no spec declares is a mailbox door with no declared owner."
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

        // Since phase 2 (#306) each actor's surface is its OWN crate, so the per-actor unit of
        // inspection is a generated `lib.rs` rather than a block of one shared file — which makes
        // the assertion strictly stronger: a method leaking into the wrong actor's crate is now
        // impossible to miss by mis-parsing a separator.
        let crates = emit_client_crates(&model);
        let mut seen_blocks = 0usize;
        for c in &crates {
            let name = c.actor.as_str();
            let block = c.lib.as_str();
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
    fn every_read_model_has_a_declared_reader() {
        // #305 — the read-side mirror of the write side's spec-gated surface (ADR-20260802-170059).
        // A read model is legitimate only if something DECLARES that it reads it: an api.yaml output
        // type (`reads:`), a c4-l3 component (`components.*.reads`, for the readers no GraphQL type
        // can speak for), or an explicit `internal: true`. This asserts BOTH directions, because a
        // rule that cannot fire is worth nothing.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let mut model = load_model(&root.join("specs")).expect("load real specs");

        // GREEN — the committed specs satisfy it.
        let report = validate(&model);
        let offenders: Vec<&String> =
            report.issues.iter().filter(|i| i.rule == "read-model-no-reader").map(|i| &i.location).collect();
        assert!(offenders.is_empty(), "committed specs must declare a reader for every read model: {:?}", offenders);

        // Blindness guard: the rule must be REACHABLE. `SlugAlias` is read only by the tenant host
        // router (a 301 that never goes through GraphQL), so it is declared ONLY by the c4-l3
        // component — no api.yaml type binds it. Dropping that one declaration must trip the gate.
        // If this ever passes, either SlugAlias gained an api binding or the rule stopped firing.
        let comps = model
            .defs
            .get_mut("architecture/c4-l3.yaml")
            .and_then(|v| v.get_mut("components"))
            .and_then(|v| v.as_mapping_mut())
            .expect("c4-l3 declares components");
        let router = comps
            .get_mut(Value::from("tenant-host-router"))
            .and_then(|v| v.as_mapping_mut())
            .expect("tenant-host-router is a declared component");
        router.remove(Value::from("reads")).expect("tenant-host-router declares reads");

        // RED — SlugAlias now has no declared reader anywhere.
        let report = validate(&model);
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.rule == "read-model-no-reader"
                    && i.location.ends_with("SlugAlias")
                    && i.level == Level::Error),
            "removing the only declared reader of SlugAlias must be an ERROR; got: {:?}",
            report
                .issues
                .iter()
                .filter(|i| i.rule == "read-model-no-reader")
                .map(|i| &i.location)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn every_api_mutation_has_a_handler() {
        // The silent half of the "declared but does nothing" family. `wired_mutation_dispatch`
        // returns None for any mutation missing from its table, and the emitter then writes an
        // `Err("not implemented")` body with no command_router arm -- while api.yaml declares the
        // mutation, a story step covers it and a role guard protects it. Nothing in the SPEC gates
        // can see that, because the table lives in the emitter.
        //
        // recordDeliverySatisfaction and escalateDelivery sat in exactly that state with their
        // handlers already written in application::commands, missing only a table row.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let model = load_model(&root.join("specs")).expect("load real specs");
        let api = parse_api(&model);

        let declared: std::collections::BTreeSet<&str> =
            crate::emit::server_graphql::UNWIRED_MUTATIONS.iter().copied().collect();
        let unwired: Vec<&str> = api
            .mutations
            .iter()
            .map(|m| m.name.as_str())
            .filter(|n| crate::emit::server_graphql::wired_mutation_dispatch(n).is_none())
            .filter(|n| !declared.contains(n))
            .collect();
        assert!(
            unwired.is_empty(),
            "every api.yaml mutation needs a handler in `wired_mutation_dispatch`, or an explicit \
             UNWIRED_MUTATIONS entry saying it is deliberately not wired yet. Undeclared: {:?}",
            unwired
        );

        // The allowlist is a pressure valve, not a parking lot: an entry that is actually wired is
        // stale and must be removed, or it silently permits a future regression on that name.
        let stale: Vec<&&str> = declared
            .iter()
            .filter(|n| crate::emit::server_graphql::wired_mutation_dispatch(n).is_some())
            .collect();
        assert!(stale.is_empty(), "these UNWIRED_MUTATIONS entries are wired -- remove them: {:?}", stale);
    }

    #[test]
    fn screen_actions_supply_every_required_command_input() {
        // A screen ACTION is the CALLER of its mutation, so its `variables:` are the whole input --
        // unlike a resolver's pinned `args:`, which are static defaults the runtime merges caller
        // variables over (hence `validate_resolver_args` deliberately does NOT check required-arg
        // coverage). Nothing used to look inside `variables` at all: `action-not-a-mutation` proves
        // only that the $ref names a mutation, and `op-uncovered-by-story` is satisfied by a story
        // STEP, which is not a screen. A form that cannot submit stayed invisible until a human
        // pressed the button.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let mut model = load_model(&root.join("specs")).expect("load real specs");

        // GREEN — the restaurant profile screen wires UpdateRestaurant correctly, so it is absent
        // from the findings. (The rule has a KNOWN non-empty baseline elsewhere: ten screens across
        // the backoffice, storefront and rider apps are genuinely unsubmittable, which is what it was
        // built to surface. Asserting on this one screen keeps the test about the RULE, not the
        // backlog it exposed.)
        let flagged = |m: &Model| -> Vec<String> {
            validate(m)
                .issues
                .iter()
                .filter(|i| i.rule == "action-missing-required-input")
                .map(|i| i.location.clone())
                .collect()
        };
        assert!(
            !flagged(&model).iter().any(|l| l.contains("restaurant_profile")),
            "the profile screen supplies every required UpdateRestaurant input: {:?}",
            flagged(&model)
        );

        // RED — drop the one REQUIRED input (`restaurantId`) from that screen's save action.
        let screens = model
            .defs
            .get_mut("screens/restaurant_backoffice.yaml")
            .and_then(|v| v.get_mut("screens"))
            .and_then(|v| v.as_sequence_mut())
            .expect("backoffice declares screens");
        let profile = screens
            .iter_mut()
            .find(|s| s.get("id").and_then(|x| x.as_str()) == Some("restaurant_profile"))
            .expect("the restaurant_profile screen exists");
        let components = profile
            .get_mut("components")
            .and_then(|v| v.as_sequence_mut())
            .expect("the screen has components");
        let save = components
            .iter_mut()
            .find(|c| c.get("id").and_then(|x| x.as_str()) == Some("save_profile"))
            .expect("the save button exists");
        save.get_mut("action")
            .and_then(|a| a.get_mut("variables"))
            .and_then(|v| v.as_mapping_mut())
            .expect("the save action passes variables")
            .remove(Value::from("restaurantId"))
            .expect("restaurantId was being supplied");

        let issues = validate(&model).issues;
        let hit = issues
            .iter()
            .find(|i| i.rule == "action-missing-required-input" && i.location.contains("restaurant_profile"))
            .unwrap_or_else(|| panic!("dropping a required input must fire the rule; got: {:?}", flagged(&model)));
        assert!(
            matches!(hit.level, Level::Warning),
            "the rule WARNS rather than errors: ten pre-existing screens violate it, and a new gate that \
             fails the build on inherited debt gets weakened instead of paid down"
        );
        assert!(hit.message.contains("restaurantId"), "{}", hit.message);
    }

    #[test]
    fn screen_actions_do_not_pass_undeclared_command_inputs() {
        // The write-side mirror of `resolver-unknown-arg`: a variable naming no property of the
        // command is dropped on the floor, while the spec reads as though the input were wired.
        // The rider's Accept button is the case that earned it -- it passes `orderId`, which
        // AcceptDelivery does not declare, and supplies neither of its required inputs.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let mut model = load_model(&root.join("specs")).expect("load real specs");
        assert!(
            validate(&model).issues.iter().any(|i| i.rule == "action-unknown-input"
                && i.location.contains("accept_delivery")
                && i.message.contains("orderId")),
            "the rider accept action passes an undeclared `orderId` and must be flagged"
        );

        // RED-to-GREEN: declaring `orderId` on AcceptDelivery clears exactly that finding, proving
        // the rule reads the command rather than pattern-matching the variable name.
        model
            .defs
            .get_mut("commands.yaml")
            .and_then(|v| v.get_mut("AcceptDelivery"))
            .and_then(|v| v.get_mut("properties"))
            .and_then(|v| v.as_mapping_mut())
            .expect("AcceptDelivery declares properties")
            .insert(Value::from("orderId"), Value::from("placeholder"));
        assert!(
            !validate(&model).issues.iter().any(|i| i.rule == "action-unknown-input"
                && i.location.contains("accept_delivery")),
            "declaring the property must clear the finding"
        );
    }

    #[test]
    fn view_fedby_tombstone_counts_as_a_use() {
        // #346 — `tombstone:` is a first-class fold output: the projector routes the event to row
        // DELETION (emit/projectors.rs), so by construction it can never map to a column `from`.
        // The rule used to count only column sources, so a correct spec using the feature warned
        // `view-fedby-unused` spuriously — latent, because the reorg deleted the only declaring
        // view. Fixture: the Restaurant projection table, whose `RestaurantListingOptedOut` fedBy
        // entry is the committed catalog's one baseline instance of the warning.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let mut model = load_model(&root.join("specs")).expect("load real specs");
        let flagged = |m: &Model| -> Vec<String> {
            validate(m)
                .issues
                .iter()
                .filter(|i| i.rule == "view-fedby-unused")
                .map(|i| format!("{}: {}", i.location, i.message))
                .collect()
        };

        // Inverse guard: an event that is neither a column source NOR the tombstone still warns —
        // the committed baseline (1 × view-fedby-unused, on views.yaml/Restaurant).
        assert!(
            flagged(&model).iter().any(|m| m.contains("RestaurantListingOptedOut")),
            "baseline: RestaurantListingOptedOut is fed in but mapped by no column: {:?}",
            flagged(&model)
        );

        // Declare that event as the view's tombstone (in memory only): row deletion is a use, so
        // the warning must clear — and no other view may start warning.
        model
            .defs
            .get_mut("database/tables/projection_tables.yaml")
            .and_then(|v| v.get_mut("Restaurant"))
            .and_then(|v| v.as_mapping_mut())
            .expect("the Restaurant projection table exists")
            .insert(
                Value::from("tombstone"),
                serde_yaml::from_str("{ $ref: 'events.yaml#/RestaurantListingOptedOut' }").unwrap(),
            );
        assert!(
            flagged(&model).is_empty(),
            "a fedBy event consumed as the tombstone is used, not a design hole: {:?}",
            flagged(&model)
        );
    }

    #[test]
    fn view_tombstone_must_be_routable_through_fedby() {
        // #399 — the projector dispatch routes ONLY `fedBy` events (emit/projectors.rs), so a view
        // declaring `tombstone: X` without listing X under `fedBy` would never delete anything:
        // the erasure fold silently doesn't happen. ERROR severity — unlike "fed in, not yet
        // consumed" there is no legitimate transitional state where an unroutable tombstone is
        // intended. Fixture: the Restaurant projection table, mutated in memory only.
        //
        // Negative-verified: with the `view-tombstone-not-fedby` check removed from
        // validate/core.rs, this test fails at the "planted violation" assert with
        // `an unroutable tombstone must be an error: []` — the rule never fires. Seen red
        // 2026-08-08 before the check landed.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let mut model = load_model(&root.join("specs")).expect("load real specs");
        let flagged = |m: &Model| -> Vec<String> {
            validate(m)
                .issues
                .iter()
                .filter(|i| i.rule == "view-tombstone-not-fedby")
                .map(|i| format!("{}: {}", i.location, i.message))
                .collect()
        };

        // Live baseline: no committed view declares `tombstone:`, so the rule is silent.
        assert!(
            flagged(&model).is_empty(),
            "committed specs must not trip view-tombstone-not-fedby: {:?}",
            flagged(&model)
        );

        // Plant the violation: OrderPlaced is a real event but NOT in Restaurant's fedBy, so a
        // tombstone naming it could never dispatch — the check must reject it as an ERROR.
        model
            .defs
            .get_mut("database/tables/projection_tables.yaml")
            .and_then(|v| v.get_mut("Restaurant"))
            .and_then(|v| v.as_mapping_mut())
            .expect("the Restaurant projection table exists")
            .insert(
                Value::from("tombstone"),
                serde_yaml::from_str("{ $ref: 'events.yaml#/OrderPlaced' }").unwrap(),
            );
        assert!(
            validate(&model).issues.iter().any(|i| {
                i.rule == "view-tombstone-not-fedby"
                    && i.level == Level::Error
                    && i.message.contains("OrderPlaced")
            }),
            "an unroutable tombstone must be an error: {:?}",
            flagged(&model)
        );

        // Inverse guard: a tombstone that IS a fedBy member is routable — the error must clear.
        model
            .defs
            .get_mut("database/tables/projection_tables.yaml")
            .and_then(|v| v.get_mut("Restaurant"))
            .and_then(|v| v.as_mapping_mut())
            .expect("the Restaurant projection table exists")
            .insert(
                Value::from("tombstone"),
                serde_yaml::from_str("{ $ref: 'events.yaml#/RestaurantListingOptedOut' }").unwrap(),
            );
        assert!(
            flagged(&model).is_empty(),
            "a fedBy-member tombstone is routable and must not error: {:?}",
            flagged(&model)
        );
    }

    #[test]
    fn graphql_reached_read_models_are_not_re_declared_on_the_gateway() {
        // The whole gate rests on the two declarations NOT overlapping: a read model reached through
        // GraphQL is declared by its api.yaml type `reads:` binding, so re-listing it on
        // `graphql-gateway` would let one blanket component declaration satisfy the rule for every
        // model permanently. c4-l3.yaml says so in prose; this is the executable form, because prose
        // a spec can violate on its face gets violated (the header said "re-listed HERE" and six
        // legitimate non-GraphQL declarations contradicted it).
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let mut model = load_model(&root.join("specs")).expect("load real specs");
        assert!(
            !validate(&model).issues.iter().any(|i| i.rule == "gateway-declares-reads"),
            "committed specs must not declare reads on graphql-gateway"
        );
        let comps = model
            .defs
            .get_mut("architecture/c4-l3.yaml")
            .and_then(|v| v.get_mut("components"))
            .and_then(|v| v.as_mapping_mut())
            .expect("c4-l3 declares components");
        comps
            .get_mut(Value::from("graphql-gateway"))
            .and_then(|v| v.as_mapping_mut())
            .expect("graphql-gateway is a declared component")
            .insert(
                Value::from("reads"),
                serde_yaml::from_str("[{ $ref: 'database/tables/projection_tables.yaml#/Restaurant' }]").unwrap(),
            );
        assert!(
            validate(&model)
                .issues
                .iter()
                .any(|i| i.rule == "gateway-declares-reads" && i.level == Level::Error),
            "a read declared on graphql-gateway must be an ERROR"
        );
    }

    #[test]
    fn a_component_reading_an_unknown_read_model_is_rejected() {
        // Two independent catches, asserted separately because they come from different places and it
        // is easy to credit the wrong one. A target that does not exist is `ref-dangling` from §1,
        // which walks EVERY `$ref` in every file and owes nothing to REF_CONTRACT. A target of the
        // wrong KIND is `ref-kind`, and that one is the §1b contract row added for this site — so the
        // second half is what proves the refs.rs row actually engages. Without both, `reads:` could
        // "declare" a reader for something that is not a read model.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let mut model = load_model(&root.join("specs")).expect("load real specs");
        let router = model
            .defs
            .get_mut("architecture/c4-l3.yaml")
            .and_then(|v| v.get_mut("components"))
            .and_then(|v| v.get_mut("tenant-host-router"))
            .and_then(|v| v.as_mapping_mut())
            .expect("tenant-host-router is a declared component");
        router.insert(
            Value::from("reads"),
            serde_yaml::from_str("[{ $ref: 'database/tables/projection_tables.yaml#/GhostModel' }]").unwrap(),
        );
        assert!(
            validate(&model).issues.iter().any(|i| i.rule == "ref-dangling" && i.message.contains("GhostModel")),
            "a component reading an unknown read model must not resolve"
        );

        // Wrong KIND: `Order` exists, but as an actor — not something anyone can read. This is the
        // half that fails if the REF_CONTRACT row for `components.*.reads[*]` is removed.
        let router = model
            .defs
            .get_mut("architecture/c4-l3.yaml")
            .and_then(|v| v.get_mut("components"))
            .and_then(|v| v.get_mut("tenant-host-router"))
            .and_then(|v| v.as_mapping_mut())
            .expect("tenant-host-router is a declared component");
        router.insert(
            Value::from("reads"),
            serde_yaml::from_str("[{ $ref: 'actors.yaml#/Order' }]").unwrap(),
        );
        assert!(
            validate(&model).issues.iter().any(|i| i.rule == "ref-kind" && i.message.contains("Order")),
            "reads must accept only projection views/tables — an actor is the wrong kind"
        );
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
        let crates = emit_client_crates(&model);
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
            let c = crates
                .iter()
                .find(|c| c.actor == *actor)
                .unwrap_or_else(|| panic!("no generated client crate for mailbox actor `{actor}`"));
            let item = format!("pub struct {actor}Client");
            assert!(c.lib.contains(&item), "{}/src/lib.rs lacks `{item}`", c.dir);
            // The MANIFEST is generated too, and its package name is what a dependent writes to
            // earn the permission — if it drifts from the actor, the dependency nobody can spell
            // is a door nobody can open.
            let name = format!("name = \"client-{}\"", kebab(actor));
            assert!(c.manifest.contains(&name), "{}/Cargo.toml lacks `{name}`", c.dir);
            // The seal must be present in EVERY crate — without the private supertrait module the
            // compile-time guarantee (no impls outside the generated crate) evaporates for that
            // actor alone, which is exactly the kind of per-actor hole the split could hide.
            assert!(c.lib.contains("mod sealed {"), "{}: the privacy seal module is missing", c.dir);
        }
        assert_eq!(
            crates.len(),
            actors.len(),
            "one client crate per mailbox actor, no more: {:?} vs {:?}",
            crates.iter().map(|c| c.actor.as_str()).collect::<Vec<_>>(),
            actors
        );
    }

// ─── Per-scope spec folders: fragment merge (ADR-20260807-183024 D1, #375) ──────────────────────
//
// `$ref`s are KIND-logical, so the loader merges `specs/{scope}/{kind}.yaml` into the same logical
// catalog keys with per-item origin tracking. These tests build a minimal specs tree on disk (the
// loader is a filesystem walk, unlike the in-memory fixtures above).

mod scope_loader {
    use super::super::*;

    /// A minimal on-disk specs tree: every REQUIRED (non-splittable) source file present, all
    /// splittable catalogs absent — the per-scope fragments under test supply them.
    pub(super) fn scaffold(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("cf-scope-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let specs = root.join("specs");
        for d in ["architecture", "database"] {
            fs::create_dir_all(specs.join(d)).expect("mkdir");
        }
        for f in [
            "services.yaml",
            "database/projection_views.yaml",
            "stories.yaml",
            "tests.yaml",
            "translations.yaml",
            "translations.code_refs.yaml",
            "observability.yaml",
            "architecture/c4-l2.yaml",
            "architecture/c4-l3.yaml",
        ] {
            fs::write(specs.join(f), "version: 1\n").expect("write");
        }
        specs
    }

    #[test]
    fn fragments_merge_into_logical_catalogs_with_origin() {
        let specs = scaffold("merge");
        fs::create_dir_all(specs.join("ordering")).expect("mkdir");
        fs::create_dir_all(specs.join("common")).expect("mkdir");
        fs::write(
            specs.join("ordering/events.yaml"),
            "version: 1\nOrderPlaced: { type: object }\n",
        )
        .expect("write");
        fs::write(specs.join("common/scalars.yaml"), "version: 1\nOrderId: { type: string }\n")
            .expect("write");
        let model = load_model(&specs).expect("loads");
        // Both fragments land under their LOGICAL catalog key — refs need no rewriting.
        assert!(model.defs.get("events.yaml").and_then(|v| v.get("OrderPlaced")).is_some());
        assert!(model.defs.get("scalars.yaml").and_then(|v| v.get("OrderId")).is_some());
        // Origin scope recorded per item; scope folders discovered.
        assert_eq!(
            model.origins.get(&("events.yaml".into(), "OrderPlaced".into())).map(|s| s.as_str()),
            Some("ordering")
        );
        assert_eq!(model.scopes, vec!["common".to_string(), "ordering".to_string()]);
        assert!(model.load_issues.is_empty());
    }

    #[test]
    fn duplicate_item_name_across_files_is_an_error() {
        let specs = scaffold("dup");
        fs::create_dir_all(specs.join("ordering")).expect("mkdir");
        fs::create_dir_all(specs.join("payments")).expect("mkdir");
        fs::write(specs.join("ordering/events.yaml"), "OrderPlaced: { type: object }\n")
            .expect("write");
        fs::write(specs.join("payments/events.yaml"), "OrderPlaced: { type: object }\n")
            .expect("write");
        let model = load_model(&specs).expect("loads");
        assert!(
            model
                .load_issues
                .iter()
                .any(|i| i.rule == "scope-duplicate-item" && i.level == Level::Error),
            "a name defined in two files mapping to one catalog must be an error"
        );
    }

    #[test]
    fn section_kinds_merge_per_section() {
        let specs = scaffold("sections");
        fs::create_dir_all(specs.join("catalog")).expect("mkdir");
        fs::write(
            specs.join("catalog/api.yaml"),
            "version: 1\nqueries:\n  menu: { description: q }\ntypes:\n  Menu: { properties: {} }\n",
        )
        .expect("write");
        fs::write(
            specs.join("catalog/configuration.yaml"),
            "keys:\n  HUBRISE_API_KEY: { type: string }\n",
        )
        .expect("write");
        let model = load_model(&specs).expect("loads");
        let api = model.defs.get("api.yaml").expect("api catalog");
        assert!(api.get("queries").and_then(|q| q.get("menu")).is_some());
        assert!(api.get("types").and_then(|t| t.get("Menu")).is_some());
        assert_eq!(
            model.origins.get(&("api.yaml".into(), "queries/menu".into())).map(|s| s.as_str()),
            Some("catalog")
        );
        assert_eq!(
            model
                .origins
                .get(&("configuration.yaml".into(), "keys/HUBRISE_API_KEY".into()))
                .map(|s| s.as_str()),
            Some("catalog")
        );
        // An unknown section in a fragment is flagged, not silently merged.
        fs::write(specs.join("catalog/api.yaml"), "bogus:\n  x: {}\n").expect("write");
        let model = load_model(&specs).expect("loads");
        assert!(model.load_issues.iter().any(|i| i.rule == "scope-unknown-section"));
    }

    #[test]
    fn structural_dirs_are_not_scopes() {
        let specs = scaffold("nonscope");
        // `database/` and `architecture/` exist in the scaffold; neither may register as a scope,
        // and a scope dir with NO scoped kind files is not a scope either.
        fs::create_dir_all(specs.join("emptydir")).expect("mkdir");
        let model = load_model(&specs).expect("loads");
        assert!(model.scopes.is_empty(), "found scopes: {:?}", model.scopes);
    }
}

// ─── §14 scope rules: placement, DAG, kernel purity, api nesting (#375) ─────────────────────────

mod scope_rules {
    use super::super::*;
    use super::scope_loader;

    fn issues_for(specs: &PathBuf) -> Vec<Issue> {
        let model = load_model(specs).expect("loads");
        let mut issues = Vec::new();
        validate_scopes(&model, &mut issues);
        issues
    }
    fn rules_of(issues: &[Issue]) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = issues.iter().map(|i| i.rule).collect();
        v.sort();
        v.dedup();
        v
    }

    #[test]
    fn command_and_event_placement_follow_the_actor_wiring() {
        let specs = scope_loader::scaffold("place");
        fs::create_dir_all(specs.join("ordering")).expect("mkdir");
        fs::create_dir_all(specs.join("payments")).expect("mkdir");
        fs::write(
            specs.join("ordering/actors.yaml"),
            r#"
Order:
  type: aggregate
  receives:
    - message: { $ref: 'commands.yaml#/PlaceOrder' }
      emits: [{ $ref: 'events.yaml#/OrderPlaced' }]
"#,
        )
        .expect("write");
        // WRONG folders: the handled command and the authored event both live in payments.
        fs::write(
            specs.join("payments/commands.yaml"),
            "PlaceOrder: { type: object, properties: {} }\n",
        )
        .expect("write");
        fs::write(
            specs.join("payments/events.yaml"),
            "OrderPlaced: { type: object, properties: {} }\n",
        )
        .expect("write");
        let issues = issues_for(&specs);
        assert!(
            issues.iter().any(|i| i.rule == "scope-placement-command" && i.location.contains("PlaceOrder")),
            "misplaced handled command must be flagged: {:?}",
            rules_of(&issues)
        );
        assert!(
            issues.iter().any(|i| i.rule == "scope-placement-event" && i.location.contains("OrderPlaced")),
            "misplaced authored event must be flagged: {:?}",
            rules_of(&issues)
        );
        // Correct folder: clean. Kernel PROMOTION: also clean (a legitimate design act).
        for home in ["ordering", "common"] {
            fs::remove_file(specs.join("payments/commands.yaml")).ok();
            fs::remove_file(specs.join("payments/events.yaml")).ok();
            fs::create_dir_all(specs.join(home)).expect("mkdir");
            fs::write(
                specs.join(format!("{}/commands.yaml", home)),
                "PlaceOrder: { type: object, properties: {} }\n",
            )
            .expect("write");
            fs::write(
                specs.join(format!("{}/events.yaml", home)),
                "OrderPlaced: { type: object, properties: {} }\n",
            )
            .expect("write");
            let issues = issues_for(&specs);
            assert!(
                !issues.iter().any(|i| i.rule.starts_with("scope-placement")),
                "{} placement must be clean: {:?}",
                home,
                issues.iter().map(|i| (&i.rule, &i.message)).collect::<Vec<_>>()
            );
            fs::remove_file(specs.join(format!("{}/commands.yaml", home))).ok();
            fs::remove_file(specs.join(format!("{}/events.yaml", home))).ok();
        }
    }

    #[test]
    fn an_echo_record_does_not_author_and_multi_author_requires_common() {
        let specs = scope_loader::scaffold("echo");
        for d in ["ordering", "payments"] {
            fs::create_dir_all(specs.join(d)).expect("mkdir");
        }
        // ordering AUTHORS Fact; payments only echo-records it (receives Fact, re-emits Fact):
        // the event belongs to ordering, and payments' echo must NOT drag it to common.
        fs::write(
            specs.join("ordering/actors.yaml"),
            "A:\n  type: aggregate\n  receives:\n    - message: { $ref: 'commands.yaml#/Do' }\n      emits: [{ $ref: 'events.yaml#/Fact' }]\n",
        )
        .expect("write");
        fs::write(
            specs.join("payments/actors.yaml"),
            "B:\n  type: aggregate\n  receives:\n    - message: { $ref: 'events.yaml#/Fact' }\n      emits: [{ $ref: 'events.yaml#/Fact' }]\n",
        )
        .expect("write");
        fs::write(specs.join("ordering/commands.yaml"), "Do: { type: object }\n").expect("write");
        fs::write(specs.join("ordering/events.yaml"), "Fact: { type: object }\n").expect("write");
        let issues = issues_for(&specs);
        assert!(
            !issues.iter().any(|i| i.rule == "scope-placement-event"),
            "echo-record must not count as authorship: {:?}",
            issues.iter().map(|i| &i.message).collect::<Vec<_>>()
        );
        // Now payments genuinely AUTHORS Fact too (emits it on a command): common becomes the
        // only legal home for the shared contract.
        fs::write(
            specs.join("payments/actors.yaml"),
            "B:\n  type: aggregate\n  receives:\n    - message: { $ref: 'commands.yaml#/Pay' }\n      emits: [{ $ref: 'events.yaml#/Fact' }]\n",
        )
        .expect("write");
        fs::write(specs.join("payments/commands.yaml"), "Pay: { type: object }\n").expect("write");
        let issues = issues_for(&specs);
        assert!(
            issues.iter().any(|i| i.rule == "scope-placement-event" && i.message.contains("'common'")),
            "multi-scope authorship must require specs/common/: {:?}",
            issues.iter().map(|i| &i.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn cross_scope_refs_must_form_a_dag_with_pms_exempt() {
        let specs = scope_loader::scaffold("dag");
        for d in ["ordering", "payments"] {
            fs::create_dir_all(specs.join(d)).expect("mkdir");
        }
        // entity-level mutual references = a scope cycle = error.
        fs::write(
            specs.join("ordering/entities.yaml"),
            "OrderThing:\n  type: object\n  properties:\n    p: { $ref: 'entities.yaml#/PayThing' }\n",
        )
        .expect("write");
        fs::write(
            specs.join("payments/entities.yaml"),
            "PayThing:\n  type: object\n  properties:\n    o: { $ref: 'entities.yaml#/OrderThing' }\n",
        )
        .expect("write");
        let issues = issues_for(&specs);
        assert!(
            issues.iter().any(|i| i.rule == "scope-cycle"),
            "mutual cross-scope refs must be a cycle error: {:?}",
            rules_of(&issues)
        );
        // Break one direction: a DAG is fine (declared edges are allowed, only cycles are not).
        fs::write(
            specs.join("payments/entities.yaml"),
            "PayThing:\n  type: object\n  properties: {}\n",
        )
        .expect("write");
        assert!(!issues_for(&specs).iter().any(|i| i.rule == "scope-cycle"));
        // The same mutual coupling expressed by PROCESS MANAGERS is a declared bridge (#373):
        // orchestrators legitimately close loops between scopes.
        fs::write(
            specs.join("ordering/processmanager.yaml"),
            "OrderSaga:\n  type: process-manager\n  receives:\n    - message: { $ref: 'events.yaml#/PayFact' }\n",
        )
        .expect("write");
        fs::write(
            specs.join("payments/processmanager.yaml"),
            "PaySaga:\n  type: process-manager\n  receives:\n    - message: { $ref: 'events.yaml#/OrderFact' }\n",
        )
        .expect("write");
        fs::write(specs.join("ordering/events.yaml"), "OrderFact: { type: object }\n").expect("write");
        fs::write(specs.join("payments/events.yaml"), "PayFact: { type: object }\n").expect("write");
        let issues = issues_for(&specs);
        assert!(
            !issues.iter().any(|i| i.rule == "scope-cycle"),
            "PM bridges must be exempt from the acyclicity check: {:?}",
            issues.iter().map(|i| &i.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_kernel_references_no_scope() {
        let specs = scope_loader::scaffold("purity");
        for d in ["common", "ordering"] {
            fs::create_dir_all(specs.join(d)).expect("mkdir");
        }
        fs::write(specs.join("ordering/scalars.yaml"), "OrderId: { type: string }\n").expect("write");
        fs::write(
            specs.join("common/entities.yaml"),
            "Shared:\n  type: object\n  properties:\n    id: { $ref: 'scalars.yaml#/OrderId' }\n",
        )
        .expect("write");
        let issues = issues_for(&specs);
        assert!(
            issues.iter().any(|i| i.rule == "scope-kernel-purity"),
            "a common item referencing a scoped item must fail: {:?}",
            rules_of(&issues)
        );
        // Promote the scalar to common: purity restored.
        fs::remove_file(specs.join("ordering/scalars.yaml")).expect("rm");
        fs::write(specs.join("common/scalars.yaml"), "OrderId: { type: string }\n").expect("write");
        assert!(!issues_for(&specs).iter().any(|i| i.rule == "scope-kernel-purity"));
    }

    #[test]
    fn api_types_nest_only_intra_scope_or_kernel() {
        let specs = scope_loader::scaffold("nest");
        for d in ["catalog", "ordering", "common"] {
            fs::create_dir_all(specs.join(d)).expect("mkdir");
        }
        fs::write(
            specs.join("catalog/api.yaml"),
            "types:\n  Menu:\n    properties:\n      line: { $ref: '#/types/OrderLine' }\n",
        )
        .expect("write");
        fs::write(
            specs.join("ordering/api.yaml"),
            "types:\n  OrderLine:\n    properties: {}\n",
        )
        .expect("write");
        let issues = issues_for(&specs);
        assert!(
            issues.iter().any(|i| i.rule == "api-nested-cross-scope"),
            "a catalog type nesting an ordering type must fail (D8): {:?}",
            rules_of(&issues)
        );
        // A KERNEL nested type is fine — cross-scope data pre-joined in views stays queryable.
        fs::remove_file(specs.join("ordering/api.yaml")).expect("rm");
        fs::write(
            specs.join("common/api.yaml"),
            "types:\n  OrderLine:\n    properties: {}\n",
        )
        .expect("write");
        let issues = issues_for(&specs);
        assert!(
            !issues.iter().any(|i| i.rule == "api-nested-cross-scope"),
            "kernel nesting must be allowed: {:?}",
            issues.iter().map(|i| &i.message).collect::<Vec<_>>()
        );
    }
}

#[test]
fn a_root_catalog_beside_scope_folders_is_forbidden() {
    // A recreated flat specs/commands.yaml would carry no per-item origin and bypass every scope
    // gate (placement, DAG, purity) — the loader flags it the moment scope folders exist.
    let specs = tests::scope_loader::scaffold("rootforbid");
    fs::create_dir_all(specs.join("ordering")).expect("mkdir");
    fs::write(specs.join("ordering/events.yaml"), "E: { type: object }\n").expect("write");
    fs::write(specs.join("commands.yaml"), "SneakyCommand: { type: object }\n").expect("write");
    let model = load_model(&specs).expect("loads");
    assert!(
        model.load_issues.iter().any(|i| i.rule == "scope-root-catalog-forbidden"
            && i.location == "commands.yaml"
            && i.level == Level::Error),
        "{:?}",
        model.load_issues.iter().map(|i| (&i.rule, &i.location)).collect::<Vec<_>>()
    );
    // Without scope folders (fully flat layout, e.g. unit fixtures) root catalogs stay legal.
    fs::remove_dir_all(specs.join("ordering")).expect("rm");
    let model = load_model(&specs).expect("loads");
    assert!(model.load_issues.is_empty());
}

// ─── Per-scope domain crates + kernel + crate graph (#373, ADR-20260807-183024 step 2) ──────────
//
// The emitter turns the spec's coupling into Cargo's: each scope's type fragments become a
// generated crate whose [dependencies] are DERIVED from the cross-scope `$ref` edges, the kernel
// (`common`) carries the shared error mechanics, the `domain` facade re-exports everything, and
// the crate-graph artifact records the actor/PM → scope-crate links step (3)'s bin emitter
// consumes. These tests pin every one of those derivations to a small on-disk fixture, plus one
// whole-tree gate over the real specs.

mod domain_scope_crates {
    use super::super::*;
    use super::scope_loader;

    /// common: a uuid scalar + an entity; ordering: a scalar, an entity nesting the kernel's, an
    /// event reaching both scopes' scalars, a command, an error.
    fn fixture(tag: &str) -> PathBuf {
        let specs = scope_loader::scaffold(tag);
        fs::create_dir_all(specs.join("common")).expect("mkdir");
        fs::create_dir_all(specs.join("ordering")).expect("mkdir");
        fs::write(
            specs.join("common/scalars.yaml"),
            "OrderId: { type: string, format: uuid }\nMoneyCents: { type: integer }\n",
        )
        .expect("write");
        fs::write(
            specs.join("common/entities.yaml"),
            "Money:\n  type: object\n  properties:\n    amountCents: { $ref: 'scalars.yaml#/MoneyCents' }\n  required: [amountCents]\n",
        )
        .expect("write");
        fs::write(
            specs.join("common/errors.yaml"),
            "KernelBroke:\n  description: kernel-owned error\n  messages: { en: broke, fr: casse }\n",
        )
        .expect("write");
        fs::write(specs.join("ordering/scalars.yaml"), "OrderStatus: { enum: [PLACED] }\n")
            .expect("write");
        fs::write(
            specs.join("ordering/entities.yaml"),
            "OrderLine:\n  type: object\n  properties:\n    price: { $ref: 'entities.yaml#/Money' }\n  required: [price]\n",
        )
        .expect("write");
        fs::write(
            specs.join("ordering/events.yaml"),
            "OrderPlaced:\n  type: object\n  properties:\n    orderId: { $ref: 'scalars.yaml#/OrderId' }\n    status: { $ref: 'scalars.yaml#/OrderStatus' }\n  required: [orderId, status]\n",
        )
        .expect("write");
        fs::write(
            specs.join("ordering/commands.yaml"),
            "PlaceOrder:\n  type: object\n  properties:\n    orderId: { $ref: 'scalars.yaml#/OrderId' }\n  required: [orderId]\n",
        )
        .expect("write");
        fs::write(
            specs.join("ordering/errors.yaml"),
            "OrderNotFound:\n  description: no such order\n  context: { orderId: { $ref: 'scalars.yaml#/OrderId' } }\n  messages: { en: 'Order {orderId} not found', fr: 'Commande {orderId} introuvable' }\n",
        )
        .expect("write");
        specs
    }

    fn crate_of<'a>(crates: &'a [DomainScopeCrate], scope: &str) -> &'a DomainScopeCrate {
        crates.iter().find(|c| c.scope == scope).unwrap_or_else(|| panic!("no {} crate", scope))
    }
    fn file_of<'a>(c: &'a DomainScopeCrate, name: &str) -> &'a str {
        c.files
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| c.as_str())
            .unwrap_or_else(|| panic!("{}: no {}", c.scope, name))
    }

    #[test]
    fn manifests_and_imports_derive_from_the_same_ref_reach() {
        let specs = fixture("crates");
        let model = load_model(&specs).expect("loads");
        let crates = emit_domain_scope_crates(&model);
        assert_eq!(
            crates.iter().map(|c| c.scope.as_str()).collect::<Vec<_>>(),
            vec!["common", "ordering"],
        );
        // Kernel: no domain deps at all; uuid earned by the uuid scalar; serde_json by interpolate.
        let common = crate_of(&crates, "common");
        assert!(
            !common.manifest.contains("{ path = \"../"),
            "kernel must depend on no scope:\n{}",
            common.manifest
        );
        assert!(common.manifest.contains("uuid = { workspace = true }"), "{}", common.manifest);
        assert!(common.manifest.contains("serde_json = { workspace = true }"), "{}", common.manifest);
        // Scope: exactly the derived kernel edge, and NO unearned deps.
        let ordering = crate_of(&crates, "ordering");
        assert!(
            ordering.manifest.contains("domain-common = { path = \"../common\" }"),
            "{}",
            ordering.manifest
        );
        assert!(!ordering.manifest.contains("uuid ="), "no uuid code emitted here:\n{}", ordering.manifest);
        assert_eq!(ordering.dep_scopes.iter().collect::<Vec<_>>(), vec!["common"]);
        // The SAME reach feeds the use lines: events reach kernel + own scalars; entities reach
        // kernel entities; commands reach kernel scalars only.
        let events = file_of(ordering, "src/events.rs");
        assert!(events.contains("use crate::scalars::*;"), "{}", events);
        assert!(events.contains("use domain_common::scalars::*;"), "{}", events);
        let entities = file_of(ordering, "src/entities.rs");
        assert!(entities.contains("use domain_common::entities::*;"), "{}", entities);
        assert!(!entities.contains("use crate::scalars::*;"), "unearned import:\n{}", entities);
        let commands = file_of(ordering, "src/commands.rs");
        assert!(commands.contains("use domain_common::scalars::*;"), "{}", commands);
        assert!(!commands.contains("use crate::"), "unearned import:\n{}", commands);
        // lib.rs lists exactly the emitted modules, in kind order.
        let lib = file_of(ordering, "src/lib.rs");
        for m in ["scalars", "entities", "events", "commands", "errors"] {
            assert!(lib.contains(&format!("pub mod {};", m)), "{}", lib);
        }
    }

    #[test]
    fn kernel_owns_error_def_and_scope_error_catalogs_build_on_it() {
        let specs = fixture("errors");
        let model = load_model(&specs).expect("loads");
        let crates = emit_domain_scope_crates(&model);
        let common_errors = file_of(crate_of(&crates, "common"), "src/errors.rs");
        assert!(common_errors.contains("pub struct ErrorDef"), "{}", common_errors);
        assert!(common_errors.contains("pub fn interpolate"), "{}", common_errors);
        assert!(common_errors.contains("pub const KERNEL_BROKE: ErrorDef"), "{}", common_errors);
        let ordering_errors = file_of(crate_of(&crates, "ordering"), "src/errors.rs");
        assert!(
            ordering_errors.contains("use domain_common::errors::ErrorDef;"),
            "{}",
            ordering_errors
        );
        assert!(ordering_errors.contains("pub const ORDER_NOT_FOUND: ErrorDef"), "{}", ordering_errors);
        assert!(
            !ordering_errors.contains("pub struct ErrorDef"),
            "ErrorDef is defined ONCE, in the kernel:\n{}",
            ordering_errors
        );
        // The context `$ref`s under errors are documentation, never imports (they would be unused).
        assert!(!ordering_errors.contains("use crate::scalars"), "{}", ordering_errors);
    }

    #[test]
    fn facade_reexports_every_scope_and_keeps_the_cross_scope_artifacts() {
        let specs = fixture("facade");
        let model = load_model(&specs).expect("loads");
        let scalars = emit_domain_scalars(&model);
        assert!(scalars.contains("pub use domain_common::scalars::*;"), "{}", scalars);
        assert!(scalars.contains("pub use domain_ordering::scalars::*;"), "{}", scalars);
        assert!(!scalars.contains("pub struct"), "facade defines nothing:\n{}", scalars);
        // Events facade: re-exports + the ONE cross-scope union (the single log speaks every scope).
        let events = emit_domain_events(&model);
        assert!(events.contains("pub use domain_ordering::events::*;"), "{}", events);
        assert!(events.contains("pub enum DomainEvent"), "{}", events);
        assert!(events.contains("    OrderPlaced(OrderPlaced),"), "{}", events);
        // Errors facade: the GLOBAL catalog + lookup stay here; the consts live in the scopes.
        let errors = emit_domain_errors(&model);
        assert!(errors.contains("pub use domain_common::errors::*;"), "{}", errors);
        assert!(errors.contains("pub use domain_ordering::errors::*;"), "{}", errors);
        assert!(errors.contains("pub const ERRORS: &[ErrorDef]"), "{}", errors);
        assert!(errors.contains("    KERNEL_BROKE,"), "{}", errors);
        assert!(errors.contains("    ORDER_NOT_FOUND,"), "{}", errors);
        assert!(errors.contains("pub fn find(code: &str)"), "{}", errors);
        // A scope with no items of a kind is NOT re-exported (no phantom module references).
        let entities = emit_domain_entities(&model);
        assert!(entities.contains("pub use domain_ordering::entities::*;"), "{}", entities);
        let commands = emit_domain_commands(&model);
        assert!(
            !commands.contains("pub use domain_common::commands::*;"),
            "common defines no command in this fixture:\n{}",
            commands
        );
    }

    #[test]
    fn bin_links_union_actor_and_pm_declarations_and_name_mechanically() {
        let specs = fixture("bins");
        fs::create_dir_all(specs.join("payments")).expect("mkdir");
        fs::write(specs.join("payments/events.yaml"), "PaymentCaptured: { type: object }\n")
            .expect("write");
        fs::write(
            specs.join("ordering/actors.yaml"),
            r#"
Order:
  type: aggregate
  receives:
    - message: { $ref: 'commands.yaml#/PlaceOrder' }
      emits: [{ $ref: 'events.yaml#/OrderPlaced' }]
PlaceOrderProcess:
  type: process-manager
  receives:
    - message: { $ref: 'commands.yaml#/PlaceOrder' }
"#,
        )
        .expect("write");
        // The SAME PM also has a processmanager.yaml definition whose refs reach payments — the
        // bin link is the UNION (one actor, one bin), the PM-bridge doctrine made load-bearing.
        fs::write(
            specs.join("ordering/processmanager.yaml"),
            r#"
PlaceOrderProcess:
  type: process-manager
  receives:
    - message: { $ref: 'events.yaml#/PaymentCaptured' }
"#,
        )
        .expect("write");
        let model = load_model(&specs).expect("loads");
        let links = actor_scope_links(&model);
        let of = |name: &str| {
            links
                .iter()
                .find(|(n, _, _)| n == name)
                .unwrap_or_else(|| panic!("no link row for {}", name))
        };
        let (_, order_pm, order_scopes) = of("Order");
        assert!(!order_pm);
        assert_eq!(order_scopes.iter().collect::<Vec<_>>(), vec!["ordering"]);
        let (_, pop_pm, pop_scopes) = of("PlaceOrderProcess");
        assert!(*pop_pm);
        assert_eq!(pop_scopes.iter().collect::<Vec<_>>(), vec!["ordering", "payments"]);
        // Mechanical bin names (D5 addendum directive): actor-{kebab} / pm-{kebab minus Process}.
        assert_eq!(actor_bin_name("Order", false), "actor-order");
        assert_eq!(actor_bin_name("MailboxSupervision", false), "actor-mailbox-supervision");
        assert_eq!(actor_bin_name("PlaceOrderProcess", true), "pm-place-order");
        assert_eq!(actor_bin_name("RefundProcess", true), "pm-refund");
        // The artifact carries both maps with crate names, JSON-parseable.
        let graph: serde_json::Value =
            serde_json::from_str(&emit_crate_graph(&model)).expect("valid JSON");
        assert_eq!(
            graph["bins"]["pm-place-order"]["domain_crates"],
            serde_json::json!(["domain-ordering", "domain-payments"]),
        );
        assert_eq!(graph["domain_crates"]["domain-ordering"]["deps"], serde_json::json!(["domain-common"]));
    }

    /// The per-bin Config filter (#374 Q4, ADR-20260807-183024 D5) must actually FILTER: a bin's
    /// generated reader carries its scopes' + common's keys and NOTHING else — "a pod reading
    /// another scope's key" is the drift D5 names, and this is its executable form. Over the
    /// REAL specs because the failure that earned it was real: origins key section kinds as
    /// `keys/{name}`, and a bare-name lookup silently matched nothing, defaulting every key to
    /// `common` — every bin got the full config and the filter was inert.
    #[test]
    fn real_specs_per_bin_config_keys_are_scope_filtered() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model = load_model(&root.join("specs")).expect("real specs load");
        let ordering: BTreeSet<String> =
            ["ordering".to_string(), "common".to_string()].into_iter().collect();
        let names: BTreeSet<String> =
            scoped_config_keys(&model, &ordering).into_iter().map(|k| k.name).collect();
        for must in ["DATABASE_URL", "DATABASE_POOL_MAX_CONNECTIONS", "MAILBOX_LEASE_SECONDS", "PORT"] {
            assert!(names.contains(must), "{must} is a common key every bin reads");
        }
        for must_not in ["STRIPE_SECRET_KEY", "DELIVERY_OFFER_MAX_TTL_SECONDS", "RUN_DELIVERY_OFFER_TIMEOUT"] {
            assert!(
                !names.contains(must_not),
                "{must_not} belongs to another scope -- an ordering bin's Config must not carry it (D5)"
            );
        }
        // And the FULL filter is strictly smaller than the server's key set — inert filtering is
        // exactly the bug this test exists to catch.
        let all: BTreeSet<String> = parse_config_keys(&model)
            .into_iter()
            .filter(|k| k.consumer == "server")
            .map(|k| k.name)
            .collect();
        assert!(names.len() < all.len(), "the scope filter must exclude something");
    }

    /// The #393 consumer widening: a bin that declares it HOSTS a consumer (worker-sirene-sync
    /// hosts `sirene_ingest`) reads that consumer's declared keys — scope-filtered like every
    /// other key — while the plain scope filter keeps excluding them for everyone else. Inert
    /// widening (keys appearing for bins that host nothing) is the bug the last assertion
    /// catches.
    #[test]
    fn real_specs_worker_consumer_keys_widen_only_the_hosting_bin() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model = load_model(&root.join("specs")).expect("real specs load");
        let scopes: BTreeSet<String> =
            ["network".to_string(), "common".to_string()].into_iter().collect();
        let plain: BTreeSet<String> =
            scoped_config_keys(&model, &scopes).into_iter().map(|k| k.name).collect();
        assert!(
            !plain.contains("INSEE_API_TOKEN"),
            "a sirene_ingest-consumer key must stay out of the plain server filter"
        );
        let widened: BTreeSet<String> = scoped_config_keys_with_consumers(
            &model,
            &scopes,
            &BTreeSet::from(["sirene_ingest".to_string()]),
        )
        .into_iter()
        .map(|k| k.name)
        .collect();
        for must in ["INSEE_API_TOKEN", "SIRENE_DEPARTMENTS", "RUN_SIRENE_WORKER"] {
            assert!(widened.contains(must), "{must} must reach the hosting worker's Config");
        }
        // Scope filtering still applies to consumer keys: the widening never reaches past the
        // bin's own scopes.
        let common_only: BTreeSet<String> = scoped_config_keys_with_consumers(
            &model,
            &BTreeSet::from(["common".to_string()]),
            &BTreeSet::from(["sirene_ingest".to_string()]),
        )
        .into_iter()
        .map(|k| k.name)
        .collect();
        assert!(
            !common_only.contains("INSEE_API_TOKEN"),
            "consumer keys must stay scope-filtered (network key, common-only scopes)"
        );
    }

    #[test]
    fn real_specs_crate_graph_is_a_dag_with_existing_targets() {
        // The whole-tree gate: over the REAL specs, every derived manifest dependency must name an
        // emitted crate, never itself, and the edges must be acyclic (the §14 validator proves the
        // scope-level graph; this re-proves the DERIVED crate edges so the two derivations cannot
        // drift apart). Also pins the kernel's position: no dependencies, ever.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model = load_model(&root.join("specs")).expect("real specs load");
        let crates = emit_domain_scope_crates(&model);
        assert!(crates.len() >= 8, "expected the 8 ADR scopes, got {}", crates.len());
        let scopes: BTreeSet<&str> = crates.iter().map(|c| c.scope.as_str()).collect();
        for c in &crates {
            for d in &c.dep_scopes {
                assert!(scopes.contains(d.as_str()), "{} depends on unemitted scope {}", c.scope, d);
                assert_ne!(d, &c.scope, "{} depends on itself", c.scope);
            }
        }
        let common = crates.iter().find(|c| c.scope == KERNEL_SCOPE).expect("kernel crate");
        assert!(common.dep_scopes.is_empty(), "the kernel depends on no scope: {:?}", common.dep_scopes);
        // Kahn's algorithm over the derived edges: everything must peel.
        let mut indeg: BTreeMap<&str, usize> = scopes.iter().map(|s| (*s, 0)).collect();
        for c in &crates {
            for _ in &c.dep_scopes {
                *indeg.get_mut(c.scope.as_str()).unwrap() += 1;
            }
        }
        let mut queue: Vec<&str> =
            indeg.iter().filter(|(_, d)| **d == 0).map(|(s, _)| *s).collect();
        let mut peeled = 0;
        while let Some(s) = queue.pop() {
            peeled += 1;
            for c in &crates {
                if c.dep_scopes.contains(s) {
                    let d = indeg.get_mut(c.scope.as_str()).unwrap();
                    *d -= 1;
                    if *d == 0 {
                        queue.push(c.scope.as_str());
                    }
                }
            }
        }
        assert_eq!(peeled, scopes.len(), "derived crate edges contain a cycle");
    }
}

#[test]
fn kernel_errors_module_exists_whenever_any_scope_declares_errors() {
    // The kernel owns ErrorDef; a scope's error catalog must compile even when common/ declares
    // no error items of its own — and the facade's global catalog must still see ErrorDef.
    let specs = tests::scope_loader::scaffold("kernel-errs");
    fs::create_dir_all(specs.join("common")).expect("mkdir");
    fs::create_dir_all(specs.join("ordering")).expect("mkdir");
    fs::write(specs.join("common/scalars.yaml"), "OrderId: { type: string }\n").expect("write");
    fs::write(
        specs.join("ordering/errors.yaml"),
        "OrderNotFound:\n  messages: { en: nope, fr: non }\n",
    )
    .expect("write");
    let model = load_model(&specs).expect("loads");
    let crates = emit_domain_scope_crates(&model);
    let common = crates.iter().find(|c| c.scope == "common").expect("kernel crate");
    let errors = common
        .files
        .iter()
        .find(|(n, _)| n == "src/errors.rs")
        .map(|(_, c)| c.as_str())
        .expect("kernel errors module exists without own error items");
    assert!(errors.contains("pub struct ErrorDef"), "{}", errors);
    assert!(!errors.contains("pub const "), "no consts to emit here:\n{}", errors);
    let lib = common.files.iter().find(|(n, _)| n == "src/lib.rs").map(|(_, c)| c.as_str()).unwrap();
    assert!(lib.contains("pub mod errors;"), "{}", lib);
    // Facade: the kernel's errors module is re-exported even though common has no error ITEMS.
    let facade = emit_domain_errors(&model);
    assert!(facade.contains("pub use domain_common::errors::*;"), "{}", facade);
    assert!(facade.contains("pub use domain_ordering::errors::*;"), "{}", facade);
}

// ─── The generated deployment (#349, ADR-20260807-183024 step 4) ────────────────────────────────

/// Every deployable maps to exactly one image, one pin file, one manifest — and back
/// (PROP-20260806-223656 D5 addendum: "completeness is a codegen TEST" — a new bin without its
/// mapping must be a build failure, never a workload that silently never deploys). Runs against
/// the REAL specs + the REAL `deploy/pins/` ledger, style of `makefile_recipe_lines_are_ascii`:
/// loud, never skips.
#[test]
fn deploy_tree_is_complete_both_ways() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let model = load_model(&root.join("specs")).expect("load real specs");
    let topology = bin_topology(&model);
    assert!(!topology.is_empty(), "real specs must derive a non-empty bin topology");
    let pins = read_image_pins(&root).expect("pins must parse -- a malformed pin would deploy a stale tag");
    let tree = emit_deploy_tree(&model, &pins);
    let files: BTreeMap<&str, &str> = tree.iter().map(|(p, c)| (p.as_str(), c.as_str())).collect();
    let bins: BTreeSet<&str> = topology.iter().map(|b| b.name.as_str()).collect();

    // Bin <-> image: one-to-one, both directions.
    let images: serde_json::Value =
        serde_json::from_str(files["images.json"]).expect("images.json is valid JSON");
    let image_keys: BTreeSet<&str> = images["images"]
        .as_object()
        .expect("images map")
        .keys()
        .map(|s| s.as_str())
        .collect();
    assert_eq!(image_keys, bins, "bin <-> image mapping must be one-to-one");

    let kustomization = files["manifests/kustomization.yaml"];
    for b in &topology {
        let path = format!("manifests/bins/{}.yaml", b.name);
        let manifest = files
            .get(path.as_str())
            .unwrap_or_else(|| panic!("bin '{}' has no generated Deployment manifest", b.name));
        if let Some(schedule) = &b.schedule {
            // PERIODIC WORKER (ADR-20260808-062933 "shape follows cadence"): a CronJob carrying
            // the c4-DECLARED cadence, never overlapping itself, never a probe-served Deployment.
            assert!(manifest.contains("kind: CronJob"), "{}: a scheduled worker must be a CronJob", b.name);
            assert!(
                manifest.contains(&format!("schedule: \"{schedule}\"")),
                "{}: the CronJob must carry the c4-declared cadence '{schedule}'",
                b.name
            );
            assert!(
                manifest.contains("concurrencyPolicy: Forbid"),
                "{}: passes must never overlap (Forbid)",
                b.name
            );
            assert!(
                manifest.contains("restartPolicy: Never"),
                "{}: a pass is retried by its NEXT schedule, not by pod restarts",
                b.name
            );
            assert_eq!(
                manifest.contains("suspend: true"),
                b.suspended,
                "{}: CronJob suspend must mirror the c4-l2 `suspended:` declaration",
                b.name
            );
            assert!(
                !manifest.contains("kind: Service") && !manifest.contains("readinessProbe"),
                "{}: a run-to-completion pass serves nothing — no Service, no probes",
                b.name
            );
        } else {
        // The safety pins the emitter must encode (PROP-20260806-223656 D3 + D5 addendum):
        // Recreate + one replica until #242's leases and fencing, with #193 named in place.
        assert!(manifest.contains("type: Recreate"), "{}: strategy must be Recreate until #242", b.name);
        assert!(manifest.contains("replicas: 1"), "{}: replicas pinned to 1 until #242", b.name);
        assert!(manifest.contains("#193"), "{}: the Recreate pin must cite #193 in place", b.name);
        }
        // D8: a pod gets DATABASE_URL iff the derivation says it touches the stores -- gateways
        // and surfaces never (no DB access by construction), EXCEPT a surface with a DECLARED
        // c4 edge to event-store/read-models (adapters: its ACLs record inbound facts).
        assert_eq!(
            manifest.contains("DATABASE_URL"),
            needs_db(b, &model),
            "{}: DATABASE_URL presence must match the needs_db derivation ({} family)",
            b.name,
            b.family
        );
        assert!(
            kustomization.contains(&format!("bins/{}.yaml", b.name)),
            "kustomization.yaml misses bins/{}.yaml",
            b.name
        );
        // The pin ledger: every bin has its pin file (seeded by `make generate`).
        let pin = root.join("deploy/pins").join(format!("{}.json", b.name));
        assert!(pin.exists(), "missing deploy/pins/{}.json -- run `make generate` and commit it", b.name);
    }

    // No stale pins: a pin for a bin the topology dropped is deploy history for a workload that
    // no longer exists -- delete it in the same change that dropped the bin.
    for entry in fs::read_dir(root.join("deploy/pins")).expect("deploy/pins exists").flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(bin) = name.strip_suffix(".json") else {
            panic!("deploy/pins/{name}: only {{bin}}.json pin files belong here");
        };
        assert!(bins.contains(bin), "stale pin deploy/pins/{name}: no bin '{bin}' in the topology");
    }

    // Ingress derivation: every screens->surface binding names a real bin, and EVERY surface in
    // the topology is bound to a screens file (since ADR-20260808-062432 the webhook adapters
    // are their own family -- a surface serves humans, so an unbound one is unroutable).
    for (_, surface) in screens_surface_bindings() {
        assert!(bins.contains(surface), "screens binding names unknown surface '{surface}'");
    }
    let bound: BTreeSet<&str> = screens_surface_bindings().iter().map(|(_, s)| *s).collect();
    for b in topology.iter().filter(|b| b.family == "surface") {
        assert!(
            bound.contains(b.name.as_str()),
            "surface bin '{}' has no screens->surface binding -- its host would be unroutable",
            b.name
        );
    }
    let ingress = files["manifests/ingress.yaml"];
    for role_seg in ["customer", "public", "restaurant", "restaurant-account", "rider", "admin"] {
        assert!(
            ingress.contains(&format!("/{role_seg}/graphql")),
            "ingress misses the /{role_seg}/graphql role path (role = path, ADR-0006)"
        );
    }

    // One bin per adapter (ADR-20260808-062432), everything derived from the topology, never a
    // hand list of partners:
    let adapters: Vec<&BinSpec> = topology.iter().filter(|b| b.family == "adapter").collect();
    assert!(
        adapters.len() >= 5,
        "expected one bin per crates/adapters/* crate, got {:?}",
        adapters.iter().map(|b| &b.name).collect::<Vec<_>>()
    );
    // The env-prefix narrowing is only sound while no partner's prefix is a prefix of
    // another's (a future `crates/adapters/uber` would swallow UBER_DIRECT_*): assert the
    // derivation's precondition so the sixth crate that breaks it fails HERE, not by silently
    // leaking another partner's secrets into its pod.
    for a in &adapters {
        for b in &adapters {
            let (pa, pb) = (
                adapter_env_prefix(a.partner.as_deref().unwrap()),
                adapter_env_prefix(b.partner.as_deref().unwrap()),
            );
            assert!(
                pa == pb || !pa.starts_with(&pb),
                "partner env prefixes must be disjoint: {pa} vs {pb} — the ADR-20260808-062432 narrowing cannot tell their keys apart"
            );
        }
    }
    for a in &adapters {
        let dir = a.partner.as_deref().expect("adapter bin carries its partner");
        let slug = partner_slug(dir);
        assert_eq!(a.name, format!("adapter-{slug}"), "bin name is derived from the crate dir");
        // The crate-package naming the manifest derivation relies on ({slug}-adapter) is a
        // real workspace fact, not an assumption.
        let manifest_toml =
            fs::read_to_string(root.join("crates/adapters").join(dir).join("Cargo.toml"))
                .expect("adapter crate manifest");
        assert!(
            manifest_toml.contains(&format!("name = \"{slug}-adapter\"")),
            "crates/adapters/{dir} must be packaged as '{slug}-adapter' (the derivation the bin manifests link)"
        );
        // Ingress: the integration host carries this partner's path to ITS service.
        assert!(
            ingress.contains(&format!("- path: /adapters/{slug}")),
            "ingress misses the /adapters/{slug} partner path"
        );
        let bin_manifest = files[format!("manifests/bins/{}.yaml", a.name).as_str()];
        // THE point of the split: no other partner's secret ever reaches this pod. Checked
        // pairwise over the derived family (the delivery scope declares three partners' keys,
        // so scope routing alone would fail this).
        for other in &adapters {
            let other_dir = other.partner.as_deref().unwrap();
            if other_dir != dir {
                let prefix = adapter_env_prefix(other_dir);
                assert!(
                    !bin_manifest.contains(&format!("name: {prefix}")),
                    "{}: pod env carries another partner's key ({prefix}*) -- the ADR-20260808-062432 narrowing broke",
                    a.name
                );
            }
        }
    }
    // The money path positively holds ITS OWN keys (narrowing must never fail closed into a
    // Stripe pod that cannot verify webhooks -- "the customer is charged and the restaurant is
    // never told" is the failure this secret exists to prevent).
    let stripe_manifest = files["manifests/bins/adapter-stripe.yaml"];
    assert!(
        stripe_manifest.contains("name: STRIPE_WEBHOOK_SECRET")
            && stripe_manifest.contains("name: STRIPE_SECRET_KEY"),
        "adapter-stripe's pod env must carry the Stripe secrets"
    );
    // The composed pod is gone for good: no `adapters` bin, no manifest, no pin.
    assert!(!bins.contains("adapters"), "the composed adapters bin must not come back");

    // One bin per worker (ADR-20260808-062933), derived from the c4-l2 worker containers:
    // bam (always-on Deployment) + the periodic `worker-*` CronJobs, shape following cadence.
    let workers: Vec<&BinSpec> = topology.iter().filter(|b| b.family == "worker").collect();
    assert!(
        workers.iter().any(|b| b.name == "bam" && b.schedule.is_none()),
        "bam stays the always-on worker Deployment (do not respawn it as a CronJob)"
    );
    for expected in ["worker-sirene-sync", "worker-retention", "worker-journal-sweep", "worker-erasure"] {
        let w = workers
            .iter()
            .find(|b| b.name == expected)
            .unwrap_or_else(|| panic!("worker bin '{expected}' missing from the topology"));
        assert!(w.schedule.is_some(), "{expected}: a periodic worker carries its declared cadence");
    }
    // The GitHub-Actions residence stays authoritative until the #358 cutover — the SIRENE
    // CronJob lands visibly suspended; the sweeps land live (nothing applies the tree yet).
    assert!(
        workers.iter().any(|b| b.name == "worker-sirene-sync" && b.suspended),
        "worker-sirene-sync must land suspended (sirene-sync.yml residence + the #220 pause)"
    );
    // MINIMAL GRANTS — the auditable GDPR posture the ADR names: the erasure pod's env answers
    // "what could this process reach?" with the database and the telemetry backend, nothing
    // else. Checked against the FULL production secret catalog, not a hand list of offenders:
    // every other secret key must be absent.
    let erasure = files["manifests/bins/worker-erasure.yaml"];
    assert!(
        erasure.contains("name: DATABASE_URL") && erasure.contains("name: HONEYCOMB_API_KEY"),
        "worker-erasure keeps the DB + telemetry floor"
    );
    for (key, _, _, _) in production_secret_keys(&model) {
        if key != "DATABASE_URL" && key != "HONEYCOMB_API_KEY" {
            assert!(
                !erasure.contains(&format!("name: {key}")),
                "worker-erasure's pod env carries '{key}' — the #393 worker floor narrowing broke (the GDPR pod must hold only its own grants)"
            );
        }
    }
    // The consumer widening is exactly as narrow as declared: only worker-sirene-sync hosts the
    // sirene_ingest consumer — and since its keys declare no production from_secret (GitHub
    // Actions injects them until the #358 cutover), even ITS pod env carries none of them yet.
    for b in &topology {
        assert_eq!(
            b.consumers.contains("sirene_ingest"),
            b.name == "worker-sirene-sync",
            "{}: the sirene_ingest consumer belongs to worker-sirene-sync alone",
            b.name
        );
    }
    let sirene = files["manifests/bins/worker-sirene-sync.yaml"];
    assert!(
        !sirene.contains("name: INSEE_API_TOKEN"),
        "INSEE_API_TOKEN has no production from_secret — it reaches the pod only when the #358 cutover gives it a deploy source"
    );
}

/// The pin ledger drives the Deployment image: a recorded digest is baked in (digest-pinned,
/// ADR-20260730-051500 -- never a moving tag); a null pin renders the deliberately-undeployable
/// `:unpinned` tag, so an unpinned bin can never silently deploy `latest`.
#[test]
fn pin_digest_resolves_into_the_deployment_image() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let model = load_model(&root.join("specs")).expect("load real specs");
    let mut pins: BTreeMap<String, ImagePin> = BTreeMap::new();
    pins.insert(
        "actor-order".into(),
        ImagePin { digest: Some("sha256:deadbeef".into()), source_hash: Some("abc".into()) },
    );
    let tree = emit_deploy_tree(&model, &pins);
    let get = |p: &str| tree.iter().find(|(path, _)| path == p).map(|(_, c)| c.as_str()).unwrap();
    let pinned = get("manifests/bins/actor-order.yaml");
    assert!(
        pinned.contains("image: ghcr.io/thecaptaincompany/captain-food/actor-order@sha256:deadbeef"),
        "digest must be baked into the image ref:\n{pinned}"
    );
    assert!(!pinned.contains("UNPINNED"), "a pinned bin must not carry the unpinned warning");
    let unpinned = get("manifests/bins/actor-cart.yaml");
    assert!(
        unpinned.contains("image: ghcr.io/thecaptaincompany/captain-food/actor-cart:unpinned"),
        "a null pin must render the :unpinned tag:\n{unpinned}"
    );
    assert!(unpinned.contains("UNPINNED"), "an unpinned bin must say so in the manifest header");
}

// ─── The hand-written platform tree (#360, deploy/platform/) ────────────────────────────────────
// CNPG is PLATFORM SOURCE like C4 -- nothing in the specs derives it, so no emitter owns it.
// What replaces drift-checking for hand-written manifests is this suite: every YAML document
// parses, the vendored operator matches its recorded upstream pin, and the safety invariants
// that would silently lose the event log hold. Style of `makefile_recipe_lines_are_ascii`:
// loud, runs against the real tree, never skips.

fn platform_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/platform")
}

fn platform_yaml_files() -> Vec<std::path::PathBuf> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in fs::read_dir(dir).expect("platform dir readable").flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|e| e == "yaml" || e == "yml") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(&platform_root(), &mut out);
    assert!(!out.is_empty(), "deploy/platform/ contains no YAML -- the tree moved without this test");
    out.sort();
    out
}

/// Every YAML document in the platform tree parses (kubeconform/kubeval are absent in this
/// container -- docs/claude/sessions.md -- so parseability is the executable floor), and no
/// document is a Secret: values live in the sealed store, a committed Secret in a PUBLIC repo
/// is an incident, not a convenience.
#[test]
fn platform_manifests_parse_and_carry_no_secret() {
    for path in platform_yaml_files() {
        let text = fs::read_to_string(&path).unwrap();
        let mut docs = 0usize;
        for doc in serde_yaml::Deserializer::from_str(&text) {
            let value: serde_yaml::Value = match serde::Deserialize::deserialize(doc) {
                Ok(v) => v,
                Err(e) => panic!("{}: YAML parse failure: {e}", path.display()),
            };
            docs += 1;
            if let Some(kind) = value.get("kind").and_then(|k| k.as_str()) {
                assert_ne!(
                    kind, "Secret",
                    "{}: a Secret manifest may never live in the repo (public repo; sealed store only, s2b practice 7)",
                    path.display()
                );
            }
        }
        assert!(docs > 0, "{}: no YAML documents", path.display());
    }
}

/// The vendored operator file is byte-identical to the upstream release its PIN.json records:
/// re-vendoring without touching the pin (or editing the vendor file "just a little") is a
/// supply-chain event this test refuses to let pass as an ordinary diff.
#[test]
fn platform_operator_vendor_matches_pin() {
    use sha2::{Digest, Sha256};
    let root = platform_root().join("cnpg-operator");
    let pin: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("PIN.json")).expect("PIN.json exists"))
            .expect("PIN.json parses");
    let version = pin["version"].as_str().expect("pin has version");
    let expected = pin["sha256"].as_str().expect("pin has sha256");
    let vendor = root.join(format!("cnpg-{version}.yaml"));
    let bytes = fs::read(&vendor)
        .unwrap_or_else(|_| panic!("vendored operator file {} missing", vendor.display()));
    let actual: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        actual, expected,
        "cnpg-{version}.yaml does not match PIN.json's sha256 -- if this is a deliberate \
         re-vendor, update PIN.json in the same reviewed change"
    );
}

/// The replication-safety pair (ADR-20260807-114122 + PROP-20260806-223656 s2b practice 3):
/// the ENTRY shape must carry no `synchronous` block (quorum-sync with zero replicas blocks
/// every write -- checkout freezes), and the HA ladder overlay must carry BOTH `instances: 3`
/// and quorum-synchronous with strict durability (3 async instances can acknowledge a paid
/// order a failover then loses). Superuser stays disabled in both shapes.
#[test]
fn platform_cluster_entry_and_ha_shapes_are_safe() {
    let cnpg = platform_root().join("cnpg");
    let cluster: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(cnpg.join("cluster.yaml")).unwrap()).unwrap();
    let spec = &cluster["spec"];
    assert_eq!(spec["instances"].as_u64(), Some(1), "entry shape is instances: 1 (ADR-20260807-114122)");
    assert!(
        spec["postgresql"]["synchronous"].is_null(),
        "instances: 1 with a synchronous block would block every write -- the ladder overlay owns sync"
    );
    assert_eq!(spec["enableSuperuserAccess"].as_bool(), Some(false), "superuser stays disabled (D2)");
    assert_eq!(
        spec["affinity"]["podAntiAffinityType"].as_str(),
        Some("required"),
        "required anti-affinity is part of D2 and must already be in place for the ladder"
    );

    let patch: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(cnpg.join("ha/cluster-ha-patch.yaml")).unwrap()).unwrap();
    let hspec = &patch["spec"];
    assert_eq!(hspec["instances"].as_u64(), Some(3), "ladder shape is instances: 3");
    let sync = &hspec["postgresql"]["synchronous"];
    assert_eq!(sync["method"].as_str(), Some("any"), "quorum-based synchronous replication");
    assert!(sync["number"].as_u64().is_some_and(|n| n >= 1), "at least one synchronous standby");
    assert_eq!(
        sync["dataDurability"].as_str(),
        Some("required"),
        "strict durability: writes block rather than silently degrade to async"
    );

    // The ladder stays GATED: no kustomization outside ha/ itself lists the overlay as a
    // resource -- flipping the ladder is a separate recorded decision (ADR-20260807-114122),
    // never a side effect of an ordinary apply.
    for path in platform_yaml_files() {
        if path.file_name().is_some_and(|f| f == "kustomization.yaml")
            && !path.parent().is_some_and(|p| p.ends_with("ha"))
        {
            let kust: serde_yaml::Value =
                serde_yaml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            for entry in kust["resources"].as_sequence().into_iter().flatten() {
                let r = entry.as_str().unwrap_or_default();
                assert!(
                    r != "ha" && !r.ends_with("/ha") && !r.starts_with("ha/") && !r.contains("/ha/"),
                    "{}: resource '{r}' wires in the gated ha/ ladder overlay",
                    path.display()
                );
            }
        }
    }
}

/// The drill's recovery source must mirror the production archive EXACTLY (destination path,
/// endpoint, postgres image) -- a drift here means the weekly drill rehearses restoring the
/// WRONG archive while reporting green. And the drill cluster itself must never archive
/// (`backup:` stanza) nor use the Retain storage class (weekly volume leak).
#[test]
fn platform_drill_env_matches_cluster_backup() {
    let root = platform_root();
    let cluster: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(root.join("cnpg/cluster.yaml")).unwrap()).unwrap();
    let barman = &cluster["spec"]["backup"]["barmanObjectStore"];
    let dest = barman["destinationPath"].as_str().expect("cluster has destinationPath");
    let endpoint = barman["endpointURL"].as_str().expect("cluster has endpointURL");
    let image = cluster["spec"]["imageName"].as_str().expect("cluster has imageName");

    let pin: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("cnpg-operator/PIN.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        pin["postgres_image"].as_str(),
        Some(image),
        "PIN.json postgres_image and cluster.yaml imageName must be the same pin"
    );

    let cron: serde_yaml::Value = serde_yaml::from_str(
        &fs::read_to_string(root.join("restore-drill/cronjob-restore-drill.yaml")).unwrap(),
    )
    .unwrap();
    let containers = &cron["spec"]["jobTemplate"]["spec"]["template"]["spec"]["containers"][0];
    let env = containers["env"].as_sequence().expect("drill cronjob has env");
    let get_env = |name: &str| -> &str {
        env.iter()
            .find(|e| e["name"].as_str() == Some(name))
            .and_then(|e| e["value"].as_str())
            .unwrap_or_else(|| panic!("drill cronjob missing env {name}"))
    };
    assert_eq!(get_env("BARMAN_DESTINATION_PATH"), dest, "drill restores from the production destination path");
    assert_eq!(get_env("BARMAN_ENDPOINT_URL"), endpoint, "drill uses the production object-storage endpoint");
    assert_eq!(get_env("DRILL_IMAGE"), image, "drill restores under the same pinned postgres image");

    // The GitHub token is a REQUIRED secret ref: a missing secret must fail the pod visibly,
    // never run a drill whose failures nobody hears.
    let token = env
        .iter()
        .find(|e| e["name"].as_str() == Some("GITHUB_TOKEN"))
        .expect("drill cronjob has GITHUB_TOKEN");
    assert!(
        token["valueFrom"]["secretKeyRef"]["optional"].is_null()
            || token["valueFrom"]["secretKeyRef"]["optional"].as_bool() == Some(false),
        "GITHUB_TOKEN secret ref must not be optional"
    );

    // Script-level safety: strip comments, then the drill script may neither archive nor touch
    // the Retain class.
    let script =
        fs::read_to_string(root.join("restore-drill/scripts/restore-drill.sh")).unwrap();
    let code: String = script
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !code.contains("backup:"),
        "drill cluster must have no backup stanza -- archiving into the production destination corrupts the recovery path"
    );
    assert!(
        !code.contains("captain-db-retain"),
        "drill cluster must not use the Retain storage class -- a weekly drill on Retain leaks one volume per run"
    );
    assert!(
        code.contains("sslmode=require") && code.contains("claude_ro"),
        "production comparison must run as the SELECT-only claude_ro role (D7)"
    );
}
