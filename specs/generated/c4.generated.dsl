workspace "Captain.Food" "Local-first food ordering & delivery for independent restaurants and food trucks (V0: Tours)." {
  model {
    ss = softwareSystem "Captain.Food" "Local-first food ordering & delivery for independent restaurants and food trucks (V0: Tours)." {
      ct_web_client = container "web-client" "Customer mobile-first web client; multi-tenant via {restaurantSlug}.captain.food." "Next.js (App Router), React, TypeScript, Tailwind"
      ct_web_restaurant = container "web-restaurant" "Restaurant web app/dashboard: onboarding (incl. Google Business Profile 'Order online' setup — ADR-019), catalog, order queue, payouts (/restaurant-account/graphql, /restaurant/graphql)." "Next.js, React, TypeScript"
      ct_web_admin = container "web-admin" "Platform back-office for Captain.Food staff (/admin/graphql): restaurant approvals, pre-registration pipeline, ops." "Next.js, React, TypeScript"
      ct_mobile_customer = container "mobile-customer" "Customer mobile app (post-V0); same GraphQL API as web-client (/customer/graphql, /public/graphql)." "React Native / Expo (iOS + Android)"
      ct_mobile_restaurant = container "mobile-restaurant" "Restaurant-staff mobile app (post-V0): order queue, accept/ready (/restaurant/graphql)." "React Native / Expo"
      ct_mobile_rider = container "mobile-rider" "Delivery-rider mobile app (post-V0): assigned deliveries + status updates (/rider/graphql)." "React Native / Expo"
      ct_api = container "api" "CQRS-light write+read API. Hosts command handlers, projections, GraphQL gateway. Role = path (/{role}/graphql)." "Node + TypeScript (Hono or NestJS), GraphQL" {
        group "restaurant" {
          a_RestaurantAccount = component "RestaurantAccount" "Restaurant provider domain: accounts, locations, lifecycle, order-acceptance mode (incl. catalog & order-fulfilment operations performed by restaurant staff)." "Aggregate"
          a_Restaurant = component "Restaurant" "Restaurant provider domain: accounts, locations, lifecycle, order-acceptance mode (incl. catalog & order-fulfilment operations performed by restaurant staff)." "Aggregate"
        }
        group "catalog" {
          a_Catalog = component "Catalog" "Catalog tree, products, offers (SKUs), option lists, per-offer stock; HubRise import." "Aggregate"
        }
        group "order" {
          a_Cart = component "Cart" "Cart selection → checkout → order lifecycle, incl. the checkout & refund sagas (the V0 risk point: external Stripe)." "Aggregate"
          a_Order = component "Order" "Cart selection → checkout → order lifecycle, incl. the checkout & refund sagas (the V0 risk point: external Stripe)." "Aggregate"
          a_PlaceOrderProcess = component "PlaceOrderProcess" "Cart selection → checkout → order lifecycle, incl. the checkout & refund sagas (the V0 risk point: external Stripe)." "ProcessManager"
          a_RefundProcess = component "RefundProcess" "Cart selection → checkout → order lifecycle, incl. the checkout & refund sagas (the V0 risk point: external Stripe)." "ProcessManager"
        }
        group "customer" {
          a_Customer = component "Customer" "Customer-facing consumer domain: discovery/browse, identity (phone-keyed), favorites, profile, address book, cart & ordering use-cases; cart binding." "Aggregate"
          a_CartBindingProcess = component "CartBindingProcess" "Customer-facing consumer domain: discovery/browse, identity (phone-keyed), favorites, profile, address book, cart & ordering use-cases; cart binding." "ProcessManager"
        }
        group "Infrastructure" {
          c_graphql_gateway = component "graphql-gateway" "Per-role GraphQL endpoint (/{role}/graphql); applies the @auth/@public ACL; entry span (SERVER)." "Instrumented"
          c_observability_middleware = component "observability-middleware" "Attaches business.* attributes + correlation/cause ids to spans; structured JSON logging; the only place domain context meets OTel." "Instrumented"
          c_command_bus = component "command-bus" "Dispatches commands to handlers; span 'command.receive'/'command.validate'/'command.handle'." "Instrumented"
          c_command_handlers = component "command-handlers" "One handler per aggregate; validates invariants then appends events. Pure domain — NOT instrumented." "Domain"
          c_process_managers = component "process-managers" "Sagas coordinating aggregates + externals (checkout, refund, cart binding)." "Instrumented"
          c_event_store_adapter = component "event-store-adapter" "Appends to domain_events; span 'event.store.append' with business.event_type/stream_id." "Instrumented"
          c_event_publisher = component "event-publisher" "Publishes appended events to the bus; span 'event.publish' (PRODUCER)." "Instrumented"
          c_message_consumers = component "message-consumers" "Consume domain + inbound integration events; span 'event.consume.*' (CONSUMER)." "Instrumented"
          c_projection_updaters = component "projection-updaters" "Update the View_* read models from events; span 'event.consume.projection'." "Instrumented"
          c_bam_projector = component "bam-projector" "Business Activity Monitoring projection (runs in the bam container); business_metrics only." "Instrumented"
          c_hubrise_acl = component "hubrise-acl" "Anti-Corruption Layer translating HubRise payloads (SKU/option_list/'9.80 EUR') into the domain." "Instrumented"
          c_stripe_adapter = component "stripe-adapter" "Creates PaymentIntents / refunds; records inbound webhook facts (PaymentCaptured/Failed/Refunded)." "Instrumented"
          c_supabase_acl = component "supabase-acl" "Anti-Corruption Layer wrapping Supabase Auth (ADR-0015): sends/verifies phone OTP (Twilio; mock in dev) and email magic links SYNCHRONOUSLY, validates tokens server-side, and translates the Supabase user (id/phone/email) into the domain (authRef). Keeps the Supabase SDK out of the aggregates." "Instrumented"
          c_sirene_google_acl = component "sirene-google-acl" "Anti-Corruption Layer translating INSEE Sirene + Google Maps data into Restaurant commands (RegisterRestaurant / UpdateRestaurantGoogleBusinessProfile / MarkRestaurantClosed) as the owner, and validating Google Business Profile ownership proofs for claim/opt-out (ADR-0019/0021). Keeps Sirene/Google SDKs out of the aggregate." "Instrumented"
        }
      }
      ct_event_store = container "event-store" "Append-only domain_events table (the write model / source of truth at runtime)." "Managed PostgreSQL (e.g. Supabase)"
      ct_read_models = container "read-models" "Denormalized View_* projection tables fed from the event log; queries read here, never domain_events." "Managed PostgreSQL"
      ct_sync_worker = container "sync-worker" "Restaurant listing sync (ADR-0020): polls INSEE Sirene + Google Maps and, via the ACL, calls the api's RegisterRestaurant / UpdateRestaurantGoogleBusinessProfile / MarkRestaurantClosed as the owner. Prospection scoring/outreach is a later step." "Scheduled worker (GitHub Actions cron + Node)"
      ct_bam = container "bam" "Business Activity Monitoring projector — consumes the same event stream to answer business questions." "Projection worker"
      ct_otel_collector = container "otel-collector" "Receives traces/metrics/logs from the api and bam containers; exports to the backend(s)." "OpenTelemetry Collector"
    }
    x_stripe = softwareSystem "stripe" "Payments (PaymentIntent capture, refunds); later Stripe Connect." "External"
    x_hubrise = softwareSystem "hubrise" "Existing restaurant catalog/orders systems; import via the Anti-Corruption Layer." "External"
    x_delivery_partner = softwareSystem "delivery-partner" "Delivery partner (e.g. Avelo37); post-V0." "External"
    x_supabase_auth = softwareSystem "supabase-auth" "Passwordless OTP identity for customers (not a domain concern)." "External"
    x_sirene = softwareSystem "sirene" "INSEE Sirene / Recherche d'Entreprises (Etalab open data): SIRET, name, address, NAF, active/closed. Seeds listings via the ACL." "External"
    x_google_maps = softwareSystem "google-maps" "Google Maps Places / Business Profile: rating, reviews, hours, phone, website, place id, and the 'Order online' link (ADR-0021). Enrichment + ownership proof." "External"

    ct_web_client -> ct_api "GraphQL over HTTPS (/customer/graphql, /public/graphql)"
    ct_web_restaurant -> ct_api "GraphQL (/restaurant-account/graphql, /restaurant/graphql)"
    ct_web_admin -> ct_api "GraphQL (/admin/graphql)"
    ct_mobile_customer -> ct_api "GraphQL (/customer/graphql, /public/graphql) — post-V0"
    ct_mobile_restaurant -> ct_api "GraphQL (/restaurant/graphql) — post-V0"
    ct_mobile_rider -> ct_api "GraphQL (/rider/graphql) — post-V0"
    ct_api -> ct_event_store "Append domain events (command side)"
    ct_api -> ct_read_models "Read projections (query side) + projection updates"
    ct_api -> x_stripe "Create PaymentIntents, request refunds; receive webhooks (inbound facts)"
    ct_api -> x_hubrise "Import catalog / sync inventory via ACL (inbound facts)"
    ct_api -> x_supabase_auth "OTP verify / session (out of domain)"
    ct_api -> x_delivery_partner "Dispatch + status (post-V0; inbound facts)"
    ct_sync_worker -> x_sirene "Poll establishments (SIRET/NAF/address/closures)"
    ct_sync_worker -> x_google_maps "Fetch Business Profile data (rating/reviews/hours/website)"
    ct_sync_worker -> ct_api "Register/enrich/close listings via the ACL, as the owner (/external/graphql)"
    ct_api -> x_google_maps "Verify restaurant ownership (GBP) for claim/opt-out"
    ct_bam -> ct_event_store "Consume the event stream for business metrics"
    ct_api -> ct_otel_collector "Export traces/metrics/logs (OTLP)"
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
    component ct_api "ApiComponents" {
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
