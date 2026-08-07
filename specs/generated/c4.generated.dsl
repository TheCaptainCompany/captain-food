workspace "Captain.Food" "Local-first food ordering & delivery for independent restaurants and food trucks (V0: Tours)." {
  model {
    ss = softwareSystem "Captain.Food" "Local-first food ordering & delivery for independent restaurants and food trucks (V0: Tours)." {
      ct_web_client = container "web-client" "Customer mobile-first web client (SDUI renderer, ADR-0033/0034); multi-tenant via {restaurantSlug}.captain.food." "Leptos (Rust → WASM), Crux core, SSR+hydration"
      ct_web_restaurant = container "web-restaurant" "Restaurant web app/dashboard: onboarding (incl. Google Business Profile 'Order online' setup — ADR-019), catalog, order queue, payouts (/restaurant-account/graphql, /restaurant/graphql)." "Leptos (Rust → WASM), Crux core"
      ct_web_admin = container "web-admin" "Platform back-office for Captain.Food staff (/admin/graphql): restaurant approvals, pre-registration pipeline, ops." "Leptos (Rust → WASM), Crux core"
      ct_desktop_restaurant = container "desktop-restaurant" "Restaurant-manager desktop app (ADR-0034): the web-restaurant UI in a native shell, same Rust core in-process." "Tauri 2.0 shell + Leptos + Crux core (Rust)"
      ct_mobile_customer = container "mobile-customer" "Customer mobile app (post-V0); thin native UI over the shared Rust core (ADR-0034); same GraphQL API (/customer/graphql, /public/graphql)." "SwiftUI (iOS) / Jetpack Compose (Android) thin shells → Crux core via UniFFI (Rust)"
      ct_mobile_restaurant = container "mobile-restaurant" "Restaurant-staff mobile app (post-V0): order queue, accept/ready (/restaurant/graphql)." "SwiftUI / Jetpack Compose thin shells → Crux core via UniFFI (Rust)"
      ct_mobile_rider = container "mobile-rider" "Delivery-rider mobile app (post-V0): assigned deliveries + status updates (/rider/graphql)." "SwiftUI / Jetpack Compose thin shells → Crux core via UniFFI (Rust)"
      ct_fo_marketplace = container "fo-marketplace" "Marketplace front office at live.captain.food (captain_frontoffice screens); serves the customer SPA, speaks to gateway-public/gateway-customer." "Rust — Axum bin (assets + SSR)"
      ct_fo_storefront = container "fo-storefront" "Per-restaurant storefront front office at {slug}.captain.food (restaurant_frontoffice screens); resolves the tenant from the Host header." "Rust — Axum bin (assets + SSR + Host routing)" {
        c_tenant_host_router = component "tenant-host-router" "Resolves the tenant from the Host header ({slug}.captain.food) and 301s a SUPERSEDED label to the restaurant's live address (ADR-20260728-011344). Reads a read model WITHOUT going through GraphQL, so its access is declared here rather than by an api.yaml type binding." "Domain"
      }
      ct_bo_restaurant = container "bo-restaurant" "Restaurant back office (restaurant_backoffice screens): order queue, catalog, refunds; speaks to gateway-restaurant/gateway-restaurant-account." "Rust — Axum bin (assets + SSR)"
      ct_bo_rider = container "bo-rider" "Rider back office (rider screens): job list, pickup/deliver flow; speaks to gateway-rider." "Rust — Axum bin (assets + SSR)"
      ct_bo_admin = container "bo-admin" "Platform admin back office (system screens): approvals, prospection, mailbox supervision; speaks to gateway-admin." "Rust — Axum bin (assets + SSR)"
      ct_adapters = container "adapters" "The integration surface: verified inbound webhooks (Stripe, HubRise, delivery partners) recorded as inbound facts through the ACLs, plus the /external/graphql path the SIRENE/Google sync worker calls. No business logic — translation and idempotent recording only." "Rust — Axum bin (webhooks + /external/graphql + integration ACLs)" {
        c_hubrise_acl = component "hubrise-acl" "Anti-Corruption Layer translating HubRise payloads (SKU/option_list/'9.80 EUR') into the domain." "Instrumented"
        c_stripe_adapter = component "stripe-adapter" "Stripe Connect (Separate Charges & Transfers, transfer_group=ORDER_{id}; Captain = merchant of record): creates the PaymentIntent for the buyer total, then after capture transfers restaurantPayout/riderPayout to the connected accounts (3-way split, ADR-0017), keeping captainNet on the platform; refunds reverse the transfers. Records inbound webhook facts (PaymentCaptured/Failed/Refunded)." "Instrumented"
        c_supabase_acl = component "supabase-acl" "Anti-Corruption Layer wrapping Supabase Auth (ADR-0015): sends/verifies phone OTP (Twilio; mock in dev) and email magic links SYNCHRONOUSLY, validates tokens server-side, and translates the Supabase user (id/phone/email) into the domain (authRef). Keeps the Supabase SDK out of the aggregates." "Instrumented"
        c_sirene_google_acl = component "sirene-google-acl" "Anti-Corruption Layer translating INSEE Sirene + Google Maps data into Restaurant commands (RegisterRestaurant / UpdateRestaurantGoogleBusinessProfile / MarkRestaurantClosed) as the owner, and validating Google Business Profile ownership proofs for claim/opt-out (ADR-0019/0021). Keeps Sirene/Google SDKs out of the aggregate." "Instrumented"
        c_prospection_acl = component "prospection-acl" "B2B prospection worker (ADR-0020): consumes the COMPUTED score from ProspectionPipeline, applies the J+0/J+7/J+21 schedule + anti-spam, fires HubSpot/Resend/Slack, then issues RecordProspectContact / MarkProspectCold to record the facts. The score is never an input it stores back. NOTE: carries no `reads:` on purpose -- no such worker exists in Rust yet, and the ProspectionPipeline read that DOES exist lives in the command handlers, which declare it. The declaration follows the code, not this description." "Instrumented"
        c_avelo37_acl = component "avelo37-acl" "Anti-Corruption Layer for the delivery partner (Avelo37; ADR-0031): on DeliveryRequested, dispatches the job to the partner API; translates the partner's webhooks into the inbound facts DeliveryAcceptedByPartner / DeliveryRejectedByPartner / DeliveryStatusUpdated (idempotent on partnerRef). Keeps the partner SDK out of the domain; mirrors stripe-adapter." "Instrumented"
        c_coopcycle_acl = component "coopcycle-acl" "Anti-Corruption Layer for the CoopCycle delivery federation (issue #58, ADR-20260721-122910): the third PARTNER seam, mirroring avelo37-acl. FEDERATION — CoopCycle is many self-hosted co-op instances, so the outbound offer_job resolves a job to an instance (per-instance base URL + OAuth2 client-credentials, env-gated) and each instance's verified webhook (per-instance secret) is translated into the same inbound facts DeliveryAcceptedByPartner / DeliveryRejectedByPartner / DeliveryStatusUpdated (idempotent on partnerRef). Keeps the partner SDK out of the domain." "Instrumented"
        c_uber_direct_acl = component "uber_direct-acl" "Anti-Corruption Layer for the Uber Direct delivery partner (issue #57, ADR-20260721-172500): a PARTNER seam via the Uber DIRECT delivery API (not the Uber Eats marketplace), mirroring avelo37-acl. ONE central API (no federation): the outbound offer_job fetches an OAuth2 client-credentials token (env-gated by UBER_DIRECT_*) and POSTs a Create Delivery; Uber's verified webhook (X-Uber-Signature raw-body HMAC) is translated into the same inbound facts DeliveryAcceptedByPartner / DeliveryRejectedByPartner / DeliveryStatusUpdated (idempotent on partnerRef). Keeps the partner SDK out of the domain." "Instrumented"
      }
      ct_graphql_ordering = container "graphql-ordering" "Ordering subgraph: cart/checkout/order queries over the ordering views schema; mutations enqueue ordering commands on the mailbox." "Rust — async-graphql subgraph bin" {
        c_actor_client = component "actor-client" "The compiler-enforced mailbox door (crates/actor_client — #290 phase 1, PROP-20260802-130500 D1/D4): one GENERATED typed client per actor with a SPEC-GATED surface (send/record/schedule/cancel_scheduling exist only where actors.yaml declares a use, ADR-20260802-170059) plus the generic ActorClient operation-status read door. The only place a mailbox row can be constructed (MailboxEntry fields private); graphql-gateway and the ACLs enqueue through it, and PgMailbox (infrastructure) is the SQL adapter behind its Mailbox port. Port-level assembly, pure — NOT instrumented." "Domain"
      }
      ct_graphql_catalog = container "graphql-catalog" "Catalog subgraph: menu/product/offer queries over the catalog views schema; catalog mutations." "Rust — async-graphql subgraph bin"
      ct_graphql_network = container "graphql-network" "Network (supply-side) subgraph: restaurant accounts/locations/prospection over the network views schema." "Rust — async-graphql subgraph bin"
      ct_graphql_customer = container "graphql-customer" "Customer subgraph: identity, profile, favorites, addresses over the customer views schema." "Rust — async-graphql subgraph bin"
      ct_graphql_delivery = container "graphql-delivery" "Delivery subgraph: jobs, riders, dispatch state over the delivery views schema." "Rust — async-graphql subgraph bin"
      ct_graphql_payments = container "graphql-payments" "Payments subgraph: payment intents, refunds, pricing policy over the payments views schema." "Rust — async-graphql subgraph bin"
      ct_graphql_comms = container "graphql-comms" "Comms subgraph: per-order conversations and messages over the comms views schema." "Rust — async-graphql subgraph bin"
      ct_graphql_common = container "graphql-common" "Kernel subgraph: operation status + mailbox supervision (served from the write-path journals, no View_*)." "Rust — async-graphql subgraph bin"
      ct_gateway_public = container "gateway-public" "/public/graphql: routes top-level fields to the owning subgraphs from the codegen-emitted composition table." "Rust — generated gateway bin"
      ct_gateway_customer = container "gateway-customer" "/customer/graphql: top-level field routing per the generated composition table." "Rust — generated gateway bin" {
        c_graphql_gateway = component "graphql-gateway" "Per-role GraphQL endpoint (/{role}/graphql); applies the @auth/@public ACL; entry span (SERVER)." "Instrumented"
        c_observability_middleware = component "observability-middleware" "Attaches business.* attributes + correlation/cause ids to spans; structured JSON logging; the only place domain context meets OTel." "Instrumented"
      }
      ct_gateway_restaurant_account = container "gateway-restaurant-account" "/restaurant-account/graphql: top-level field routing per the generated composition table." "Rust — generated gateway bin"
      ct_gateway_restaurant = container "gateway-restaurant" "/restaurant/graphql: top-level field routing per the generated composition table." "Rust — generated gateway bin"
      ct_gateway_rider = container "gateway-rider" "/rider/graphql: top-level field routing per the generated composition table." "Rust — generated gateway bin"
      ct_gateway_admin = container "gateway-admin" "/admin/graphql: top-level field routing per the generated composition table." "Rust — generated gateway bin"
      ct_gateway_external = container "gateway-external" "/external/graphql: top-level field routing for the integration ACLs (token-authenticated)." "Rust — generated gateway bin"
      ct_actor_restaurant_account = container "actor-restaurant-account" "Drains RestaurantAccount lanes; appends its domain events." "Rust — mailbox worker bin" {
        a_RestaurantAccount = component "RestaurantAccount" "" "Aggregate"
      }
      ct_actor_restaurant = container "actor-restaurant" "Drains Restaurant lanes; appends its domain events." "Rust — mailbox worker bin" {
        a_Restaurant = component "Restaurant" "" "Aggregate"
      }
      ct_actor_prospect = container "actor-prospect" "Drains Prospect lanes; appends its domain events." "Rust — mailbox worker bin" {
        a_Prospect = component "Prospect" "" "Aggregate"
      }
      ct_actor_catalog = container "actor-catalog" "Drains Catalog lanes (incl. HubRise imports); appends its domain events." "Rust — mailbox worker bin" {
        a_Catalog = component "Catalog" "" "Aggregate"
      }
      ct_actor_customer = container "actor-customer" "Drains Customer lanes; appends its domain events." "Rust — mailbox worker bin" {
        a_Customer = component "Customer" "" "Aggregate"
      }
      ct_actor_cart = container "actor-cart" "Drains Cart lanes; appends its domain events." "Rust — mailbox worker bin" {
        a_Cart = component "Cart" "" "Aggregate"
      }
      ct_actor_order = container "actor-order" "Drains Order lanes (the Friday-peak hot type — #242's leases are the second-replica unlock); appends its domain events." "Rust — mailbox worker bin" {
        a_Order = component "Order" "" "Aggregate"
        c_command_bus = component "command-bus" "Dispatches commands to handlers; span 'command.receive'/'command.validate'/'command.handle'." "Instrumented"
        c_command_handlers = component "command-handlers" "One handler per aggregate; validates invariants then appends events. Pure domain — NOT instrumented." "Domain"
        c_event_store_adapter = component "event-store-adapter" "Appends to domain_events; span 'event.store.append' with business.event_type/stream_id." "Instrumented"
        c_event_publisher = component "event-publisher" "Publishes appended events to the bus; span 'event.publish' (PRODUCER)." "Instrumented"
      }
      ct_actor_payment = container "actor-payment" "Drains Payment lanes (inbound Stripe facts + refund decisions); appends its domain events." "Rust — mailbox worker bin" {
        a_Payment = component "Payment" "" "Aggregate"
      }
      ct_actor_delivery_job = container "actor-delivery-job" "Drains DeliveryJob lanes; appends its domain events." "Rust — mailbox worker bin" {
        a_DeliveryJob = component "DeliveryJob" "" "Aggregate"
      }
      ct_actor_rider = container "actor-rider" "Drains Rider lanes; appends its domain events." "Rust — mailbox worker bin" {
        a_Rider = component "Rider" "" "Aggregate"
      }
      ct_actor_delivery_partner_registration = container "actor-delivery-partner-registration" "Drains DeliveryPartnerRegistration lanes; appends its domain events." "Rust — mailbox worker bin" {
        a_DeliveryPartnerRegistration = component "DeliveryPartnerRegistration" "" "Aggregate"
      }
      ct_actor_conversation = container "actor-conversation" "Drains Conversation lanes (per-order messaging); appends its domain events." "Rust — mailbox worker bin" {
        a_Conversation = component "Conversation" "" "Aggregate"
      }
      ct_actor_reclamation = container "actor-reclamation" "Drains Reclamation lanes; appends its domain events." "Rust — mailbox worker bin" {
        a_Reclamation = component "Reclamation" "" "Aggregate"
      }
      ct_actor_customer_credit = container "actor-customer-credit" "Drains CustomerCredit lanes (goodwill-credit ledger); appends its domain events." "Rust — mailbox worker bin" {
        a_CustomerCredit = component "CustomerCredit" "" "Aggregate"
      }
      ct_actor_mailbox_supervision = container "actor-mailbox-supervision" "Drains MailboxSupervision lanes (operator interventions recorded as facts, #315)." "Rust — mailbox worker bin" {
        a_MailboxSupervision = component "MailboxSupervision" "" "Aggregate"
      }
      ct_pm_place_order = container "pm-place-order" "The checkout saga (acceptance-first): PlaceOrder → PaymentIntent → OrderPlaced on the captured fact." "Rust — mailbox worker bin" {
        a_PlaceOrderProcess = component "PlaceOrderProcess" "" "ProcessManager"
        c_process_managers = component "process-managers" "Sagas coordinating aggregates + externals (checkout, refund, cart binding, delivery dispatch)." "Instrumented"
        c_message_consumers = component "message-consumers" "Consume domain + inbound integration events; span 'event.consume.*' (CONSUMER)." "Instrumented"
      }
      ct_pm_refund = container "pm-refund" "The refund saga: decision facts → outbound Stripe refund → settled on PaymentRefunded." "Rust — mailbox worker bin" {
        a_RefundProcess = component "RefundProcess" "" "ProcessManager"
      }
      ct_pm_cart_binding = container "pm-cart-binding" "Binds anonymous carts to identified customers." "Rust — mailbox worker bin" {
        a_CartBindingProcess = component "CartBindingProcess" "" "ProcessManager"
      }
      ct_pm_delivery_dispatch = container "pm-delivery-dispatch" "Dispatches ready DELIVERY orders to partners/riders (ADR-0031)." "Rust — mailbox worker bin" {
        a_DeliveryDispatchProcess = component "DeliveryDispatchProcess" "" "ProcessManager"
      }
      ct_pm_reclamation = container "pm-reclamation" "Drives reclamations to a resolution (refund / replacement / goodwill credit arms)." "Rust — mailbox worker bin" {
        a_ReclamationProcess = component "ReclamationProcess" "" "ProcessManager"
      }
      ct_projector_ordering = container "projector-ordering" "Projects ordering-scope events into the ordering views schema; own checkpoint." "Rust — projection worker bin" {
        c_projection_updaters = component "projection-updaters" "Update the View_* read models from events; span 'event.consume.projection'." "Instrumented"
      }
      ct_projector_catalog = container "projector-catalog" "Projects catalog-scope events into the catalog views schema; own checkpoint." "Rust — projection worker bin"
      ct_projector_network = container "projector-network" "Projects network-scope events into the network views schema; own checkpoint." "Rust — projection worker bin"
      ct_projector_customer = container "projector-customer" "Projects customer-scope events into the customer views schema; own checkpoint." "Rust — projection worker bin"
      ct_projector_delivery = container "projector-delivery" "Projects delivery-scope events into the delivery views schema; own checkpoint." "Rust — projection worker bin"
      ct_projector_payments = container "projector-payments" "Projects payments-scope events into the payments views schema; own checkpoint." "Rust — projection worker bin"
      ct_projector_comms = container "projector-comms" "Projects comms-scope events into the comms views schema; own checkpoint." "Rust — projection worker bin"
      ct_event_store = container "event-store" "Append-only domain_events + the inbound_messages mailbox (the write model / source of truth; ALL backup/PITR budget — D2/D3)." "PostgreSQL (CNPG) — captain-core"
      ct_read_models = container "read-models" "Per-scope schemas of denormalized View_* projections + admin + bam; queries read here, never domain_events. Excluded from backups — restore is replay (D2)." "PostgreSQL (CNPG) — captain-views"
      ct_sync_worker = container "sync-worker" "Restaurant listing sync (ADR-0020): polls INSEE Sirene + Google Maps and, via the ACL, calls RegisterRestaurant / UpdateRestaurantGoogleBusinessProfile / MarkRestaurantClosed as the owner through the adapters surface. Prospection scoring/outreach is a later step." "Scheduled worker (GitHub Actions cron + Rust binary, shared Crux core)"
      ct_bam = container "bam" "Business Activity Monitoring projector — consumes the same event stream to answer business questions." "Projection worker" {
        c_bam_projector = component "bam-projector" "Business Activity Monitoring projection (runs in the bam container); business_metrics only." "Instrumented"
      }
      ct_otel_collector = container "otel-collector" "Receives traces/metrics/logs from every service bin; exports to the backend(s)." "OpenTelemetry Collector"
    }
    x_stripe = softwareSystem "stripe" "Payments (PaymentIntent capture, refunds); later Stripe Connect." "External"
    x_hubrise = softwareSystem "hubrise" "Existing restaurant catalog/orders systems; import via the Anti-Corruption Layer." "External"
    x_delivery_partner = softwareSystem "delivery-partner" "Delivery partner (e.g. Avelo37): dispatch delivery jobs out, receive courier/status facts inbound via the avelo37-acl (ADR-0031)." "External"
    x_supabase_auth = softwareSystem "supabase-auth" "Passwordless OTP identity for customers (not a domain concern)." "External"
    x_sirene = softwareSystem "sirene" "INSEE Sirene / Recherche d'Entreprises (Etalab open data): SIRET, name, address, NAF, active/closed. Seeds listings via the ACL." "External"
    x_google_maps = softwareSystem "google-maps" "Google Maps Places / Business Profile: rating, reviews, hours, phone, website, place id, and the 'Order online' link (ADR-0021). Enrichment + ownership proof." "External"
    x_hubspot = softwareSystem "hubspot" "CRM for B2B prospection leads (ADR-0020); post-V0." "External"
    x_resend = softwareSystem "resend" "Transactional email for prospection outreach sequences (ADR-0020); post-V0." "External"
    x_slack = softwareSystem "slack" "Ops/prospection alerts (active-partner closures, sequence events); post-V0." "External"

    ct_web_client -> ct_gateway_customer "GraphQL over HTTPS (/customer/graphql)"
    ct_web_client -> ct_gateway_public "GraphQL (/public/graphql, anonymous browse/cart)"
    ct_web_restaurant -> ct_gateway_restaurant "GraphQL (/restaurant/graphql)"
    ct_web_restaurant -> ct_gateway_restaurant_account "GraphQL (/restaurant-account/graphql)"
    ct_web_admin -> ct_gateway_admin "GraphQL (/admin/graphql)"
    ct_desktop_restaurant -> ct_gateway_restaurant "GraphQL (/restaurant/graphql) — Tauri shell"
    ct_mobile_customer -> ct_gateway_customer "GraphQL (/customer/graphql) — post-V0"
    ct_mobile_restaurant -> ct_gateway_restaurant "GraphQL (/restaurant/graphql) — post-V0"
    ct_mobile_rider -> ct_gateway_rider "GraphQL (/rider/graphql) — post-V0"
    ct_fo_marketplace -> ct_gateway_public "Serves the marketplace SPA; SSR reads via the public gateway"
    ct_fo_storefront -> ct_gateway_public "Serves {slug}.captain.food; tenant-resolved SSR reads"
    ct_bo_restaurant -> ct_gateway_restaurant "Serves the restaurant back office"
    ct_bo_rider -> ct_gateway_rider "Serves the rider back office"
    ct_bo_admin -> ct_gateway_admin "Serves the platform back office"
    ct_gateway_public -> ct_graphql_network "Top-level routing: restaurant discovery/browse"
    ct_gateway_public -> ct_graphql_catalog "Top-level routing: menus"
    ct_gateway_public -> ct_graphql_ordering "Top-level routing: anonymous cart + checkout"
    ct_gateway_customer -> ct_graphql_ordering "Top-level routing: cart/checkout/orders (money path)"
    ct_gateway_customer -> ct_graphql_customer "Top-level routing: identity/profile/favorites"
    ct_gateway_restaurant -> ct_graphql_ordering "Top-level routing: order queue, accept/ready"
    ct_gateway_restaurant -> ct_graphql_catalog "Top-level routing: catalog management"
    ct_gateway_restaurant_account -> ct_graphql_network "Top-level routing: account + locations"
    ct_gateway_rider -> ct_graphql_delivery "Top-level routing: jobs + status updates"
    ct_gateway_admin -> ct_graphql_network "Top-level routing: approvals + prospection"
    ct_gateway_admin -> ct_graphql_common "Top-level routing: mailbox supervision (#315)"
    ct_gateway_external -> ct_graphql_network "Top-level routing: SIRENE/Google listing sync writes"
    ct_graphql_ordering -> ct_read_models "SELECT on the ordering schema (GRANT-scoped role)"
    ct_graphql_ordering -> ct_event_store "Enqueue ordering commands on the actor mailbox (acceptance-first)"
    ct_graphql_catalog -> ct_read_models "SELECT on the catalog schema"
    ct_graphql_catalog -> ct_event_store "Enqueue catalog commands"
    ct_graphql_network -> ct_read_models "SELECT on the network schema"
    ct_graphql_network -> ct_event_store "Enqueue network commands"
    ct_graphql_customer -> ct_read_models "SELECT on the customer schema"
    ct_graphql_customer -> ct_event_store "Enqueue customer commands"
    ct_graphql_delivery -> ct_read_models "SELECT on the delivery schema"
    ct_graphql_delivery -> ct_event_store "Enqueue delivery commands"
    ct_graphql_payments -> ct_read_models "SELECT on the payments schema"
    ct_graphql_payments -> ct_event_store "Enqueue payment commands"
    ct_graphql_comms -> ct_read_models "SELECT on the comms schema"
    ct_graphql_comms -> ct_event_store "Enqueue comms commands"
    ct_graphql_common -> ct_event_store "Operation status + mailbox lanes from the write-path journals (no View_*)"
    ct_actor_order -> ct_event_store "Lease Order lanes; append Order events"
    ct_actor_cart -> ct_event_store "Lease Cart lanes; append Cart events"
    ct_actor_payment -> ct_event_store "Lease Payment lanes; append Payment events"
    ct_actor_catalog -> ct_event_store "Lease Catalog lanes; append Catalog events"
    ct_actor_mailbox_supervision -> ct_event_store "Lease supervision lanes; record operator facts"
    ct_pm_place_order -> ct_event_store "Drain PlaceOrderProcess lanes; append PaymentIntentCreated/OrderPlaced"
    ct_pm_place_order -> x_stripe "Create PaymentIntents (outbound payment port)"
    ct_pm_refund -> ct_event_store "Drain RefundProcess lanes; append refund decision facts"
    ct_pm_refund -> x_stripe "Request refunds (outbound payment port)"
    ct_pm_delivery_dispatch -> ct_event_store "Drain dispatch lanes; append dispatch facts"
    ct_pm_delivery_dispatch -> x_delivery_partner "Dispatch delivery jobs to the partner API"
    ct_adapters -> ct_event_store "Record verified inbound facts idempotently through the mailbox"
    x_stripe -> ct_adapters "Payment webhooks (PaymentCaptured/Failed/Refunded)"
    x_hubrise -> ct_adapters "Catalog/inventory callbacks (inbound facts)"
    x_delivery_partner -> ct_adapters "Courier acceptance/status webhooks (inbound facts) — ADR-0031"
    ct_adapters -> x_hubrise "Import catalog / sync inventory via the ACL"
    ct_adapters -> x_supabase_auth "OTP verify / session (out of domain)"
    ct_adapters -> x_google_maps "Verify restaurant ownership (GBP) for claim/opt-out"
    ct_sync_worker -> x_sirene "Poll establishments (SIRET/NAF/address/closures)"
    ct_sync_worker -> x_google_maps "Fetch Business Profile data (rating/reviews/hours/website)"
    ct_sync_worker -> ct_gateway_external "Register/enrich/close listings + record prospect contacts via the ACL (/external/graphql)"
    ct_sync_worker -> x_hubspot "Create/update prospection leads (ADR-0020)"
    ct_sync_worker -> x_resend "Send prospection outreach emails (ADR-0020)"
    ct_sync_worker -> x_slack "Prospection / ops alerts (ADR-0020)"
    ct_projector_ordering -> ct_event_store "Consume the single log filtered to ordering events; own checkpoint"
    ct_projector_ordering -> ct_read_models "Maintain the ordering schema's View_* (GRANT-scoped)"
    ct_projector_catalog -> ct_event_store "Consume catalog events; own checkpoint"
    ct_projector_catalog -> ct_read_models "Maintain the catalog schema's View_*"
    ct_bam -> ct_event_store "Consume the event stream for business metrics"
    ct_gateway_customer -> ct_otel_collector "OTLP export (every gateway bin)"
    ct_actor_order -> ct_otel_collector "OTLP export (every actor/pm bin)"
    ct_projector_ordering -> ct_otel_collector "OTLP export (every projector bin)"
    ct_adapters -> ct_otel_collector "OTLP export"
    ct_bam -> ct_otel_collector "Export traces/metrics/logs (OTLP)"
    c_graphql_gateway -> c_command_bus "dispatches command"
    c_command_bus -> c_command_handlers "invokes handler"
    c_command_handlers -> c_event_store_adapter "appends events"
    c_event_store_adapter -> c_event_publisher "publishes appended"
    c_event_publisher -> c_message_consumers "delivers events"
    c_message_consumers -> c_projection_updaters "feeds projections"
    c_process_managers -> c_command_bus "issues commands"
    c_projection_updaters -> ct_read_models "writes read models"
    c_event_store_adapter -> ct_event_store "appends to domain_events"
  }
  views {
    systemContext ss "SystemContext" {
      include *
      autolayout lr
    }
    container ss "Containers" {
      include *
      autolayout lr
    }
    component ct_fo_storefront "FoStorefrontComponents" {
      include *
      autolayout lr
    }
    component ct_adapters "AdaptersComponents" {
      include *
      autolayout lr
    }
    component ct_graphql_ordering "GraphqlOrderingComponents" {
      include *
      autolayout lr
    }
    component ct_gateway_customer "GatewayCustomerComponents" {
      include *
      autolayout lr
    }
    component ct_actor_restaurant_account "ActorRestaurantAccountComponents" {
      include *
      autolayout lr
    }
    component ct_actor_restaurant "ActorRestaurantComponents" {
      include *
      autolayout lr
    }
    component ct_actor_prospect "ActorProspectComponents" {
      include *
      autolayout lr
    }
    component ct_actor_catalog "ActorCatalogComponents" {
      include *
      autolayout lr
    }
    component ct_actor_customer "ActorCustomerComponents" {
      include *
      autolayout lr
    }
    component ct_actor_cart "ActorCartComponents" {
      include *
      autolayout lr
    }
    component ct_actor_order "ActorOrderComponents" {
      include *
      autolayout lr
    }
    component ct_actor_payment "ActorPaymentComponents" {
      include *
      autolayout lr
    }
    component ct_actor_delivery_job "ActorDeliveryJobComponents" {
      include *
      autolayout lr
    }
    component ct_actor_rider "ActorRiderComponents" {
      include *
      autolayout lr
    }
    component ct_actor_delivery_partner_registration "ActorDeliveryPartnerRegistrationComponents" {
      include *
      autolayout lr
    }
    component ct_actor_conversation "ActorConversationComponents" {
      include *
      autolayout lr
    }
    component ct_actor_reclamation "ActorReclamationComponents" {
      include *
      autolayout lr
    }
    component ct_actor_customer_credit "ActorCustomerCreditComponents" {
      include *
      autolayout lr
    }
    component ct_actor_mailbox_supervision "ActorMailboxSupervisionComponents" {
      include *
      autolayout lr
    }
    component ct_pm_place_order "PmPlaceOrderComponents" {
      include *
      autolayout lr
    }
    component ct_pm_refund "PmRefundComponents" {
      include *
      autolayout lr
    }
    component ct_pm_cart_binding "PmCartBindingComponents" {
      include *
      autolayout lr
    }
    component ct_pm_delivery_dispatch "PmDeliveryDispatchComponents" {
      include *
      autolayout lr
    }
    component ct_pm_reclamation "PmReclamationComponents" {
      include *
      autolayout lr
    }
    component ct_projector_ordering "ProjectorOrderingComponents" {
      include *
      autolayout lr
    }
    component ct_bam "BamComponents" {
      include *
      autolayout lr
    }
    styles {
      element "Element" {
        color #ffffff
      }
      element "Software System" {
        background #2d4f4a
      }
      element "Container" {
        background #313335
      }
      element "External" {
        background #cc7832
      }
      element "Aggregate" {
        background #4ec9b0
        color #11201d
      }
      element "ProcessManager" {
        background #56a0c0
      }
      element "Instrumented" {
        background #c586c0
      }
      element "Domain" {
        background #313335
      }
    }
  }
}
