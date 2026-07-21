# 🚦 Captain.Food — Development & Deployment Status

> Hand-maintained snapshot (NOT generated, outside `specs/` so it never affects the DSL).
> Last updated: 2026-07-21 (04:50 UTC). Legend: ✅ done & verified · 🚧 in progress · ⏳ blocked/waiting · 📋 planned.

> ✅ **2026-07-21 — auto-merge sequencing gap closed (ADR-20260721-044613, amends
> ADR-20260721-042018).** A claim-time draft PR is a near-empty diff and passes CI trivially;
> arming auto-merge at claim time (instead of at completion) would leave it armed for the whole
> task and fire the instant the PR left draft, even before the work was done — closing the issue
> via `Closes #NN` on unfinished work. Fix: auto-merge is armed **exactly once**, together with
> marking the PR ready for review, as one indivisible completion step — never at claim time, never
> separately. CLAUDE.md / BACKLOG.md updated to state this explicitly. Docs-only.

> ✅ **2026-07-21 — #26: service-catalog emitters — the ports are GENERATED
> (ADR-20260721-043033, implements ADR-20260719-214500, codegen-roadmap item 4).** Four new
> emitters over `specs/services.yaml`: `application/src/generated/services.rs` (per-service
> `<Base>Service` trait + typed `<Op>Input`/`Output` structs + the `ServiceCallMeta` ENVELOPE —
> correlation_id + business `refs`, the ADR-0041 move applied to service calls),
> `infrastructure/src/generated/service_clients.rs` (`Http<Base>Service` per service over the
> derived `POST /services/<svc>/<op>` surface, lossless `DomainError` wire round-trip),
> `infrastructure/src/generated/service_bindings.rs` (spec-owned `binding: local | http`
> resolvers; http reads `SERVICE_<NAME>_URL`), and the expose-gated
> `server/src/generated/services_routes.rs` (empty router in V0; http/expose branches covered by
> codegen unit tests). Hand-written `PaymentGateway` → generated `PaymentService`
> (placeOrder + refund PM + Stripe outbound adapter, whose intent `metadata` now copies
> `meta.refs` verbatim) and `DeliveryPartner` → `DeliveryService` (dispatch PM + runner + noop)
> are MIGRATED AT PARITY and deleted. ⏳ `identity` migration deferred on a CATALOG GAP needing a
> product-owner spec change: `identity.verify_email_token.output` lacks the proven `email` the
> handler records (never client input), and `locale` inputs should be `nullable: true` — see the
> ADR. `make rust` green: workspace builds, all tests pass, validate 0 errors.

> ✅ **2026-07-21 — issue workflow tightened: claim-time draft PR + supervised auto-merge
> (ADR-20260721-042018, amends ADR-20260720-233000; product-owner directive).** Claiming an issue
> now means label + claim comment + `NN-slug` branch + an immediate **draft PR** (`Closes #NN`) —
> issue↔branch↔PR are linked before any code, the board flips to In progress at claim time, and
> the reaper sees linked-PR activity. Completion = local gates green → PR **ready** → **enable
> auto-merge** → **supervise checks until MERGED** (fix+push on failure; never end at "CI
> pending"). The ADR also records the auto-merge threat model: repo-level "Allow auto-merge"
> grants no merge authority (per-PR arming needs write access; fork PRs can't arm or merge — an
> outsider's empty PR just sits open), the load-bearing config being the `main` ruleset's
> **required `codegen` check** (⏳ product owner to confirm in Settings — not verifiable from the
> repo). Docs-only change: CLAUDE.md non-negotiable + BACKLOG.md method + ADR.

> ✅ **2026-07-21 — #27: PM state-table rows and Postgres stores are GENERATED
> (ADR-20260721-031734, codegen-roadmap item 5).** Two new emitters in `tools/codegen-rs` over
> `specs/database/tables/process_managers.yaml`: `crates/application/src/generated/pm_state.rs`
> (row structs, `…StateStore` ports with derived `by_*` lookups = pk + UNIQUE columns + the
> registered `paymentStatus(orderId)` read, and the `mem::…` doubles) and
> `crates/infrastructure/src/generated/pm_state.rs` (Pg stores: enum ordinals, `.0` binds,
> `ON CONFLICT (pk) DO UPDATE` upserts stamping `last_update_utc = now()` server-side). The
> hand-written `application/src/pm_state.rs` + `infrastructure/persistence/pm_state.rs` are
> deleted; call-site paths unchanged via re-exports (`application::pm_state`,
> `persistence::Pg…State`); mem-double tests moved to `application/tests/pm_state_mem.rs`.
> Lookup naming is now mechanical (`by_<column minus _id>` — `by_job` → `by_delivery_job`), so
> processmanager.yaml `state.by` keys map 1:1 onto store methods for roadmap item 3. Journal
> stores (`command_journal.rs`/`inbound_events.rs`) stay hand-written — follow-up slice.
> `make rust` green: workspace builds, all tests pass, validate 0 errors, no drift.

> ✅ **2026-07-21 — #16: `surface: graphql` binding kind + the generic `command-acceptance`
> contract (ADR-20260721-031127).** Validator §8 now accepts `workflow.surface` as a binding kind
> (rules `obs-surface-unknown`, `obs-surface-exclusive`; `obs-no-workflow-binding` amended) so a
> contract can bind a whole dispatch surface instead of one command/saga/aggregate; doc emitters
> render it (files under cross-cutting). New `command-acceptance` contract instruments the
> acceptance-first write pipeline (ADR-20260720-015500): spans
> `command.receive`/`command.journal`/`command.dispatch`, ids `message_id`/`correlation_id`/
> `trace_id`/`command_type`/`channel`, metrics `commands_accepted_total{channel}`,
> `command_duplicates_total{channel}`, `command_sync_conflicts_total{command_type}`,
> `command_completion_ms{status}` (REJECTED/FAILED split — #19's decision data). Latency budget
> binds the sync acceptance path only. Runtime emission stays contract-only until the OTel layer
> exists; #15 landed in parallel, so `{channel}` already sees all channels. Validate 0 errors.

> ✅ **2026-07-21 — #15: the WORKER channel journals (ADR-20260720-015300 follow-up).** The command
> journal invariant — ALL command submissions converge on `command_journal`, whatever the channel —
> is now true: the HubRise enricher (`ImportCatalog` + per-SKU `UpdateOfferStock`) and the SIRENE
> sync worker (`RegisterRestaurant` / `MarkRestaurantClosed`) no longer call handlers directly but go
> through the new reusable worker-side journaling dispatch `application::dispatch::dispatch_journaled`
> (`channel: WORKER`, journal-before-handle, same REJECTED/FAILED discrimination as the generated
> GraphQL dispatch; a FAILED duplicate is re-executed under the same id — for a worker, redelivery IS
> the retry). Deterministic idempotency keys: HubRise `message_id` = UUIDv5(callback id, command
> type[, offer id]), `cause_id` = UUIDv5(callback id) → `external_hubrise_callbacks →
> command_journal → domain_events` is fully traceable, and a webhook redelivery dedupes instead of
> double-applying; SIRENE `message_id` = UUIDv5(command type, SIRET, staged `last_seen_at`),
> `cause_id` = UUIDv5(`row:<SIRET>`) — a re-drained staged version dedupes, an ingestion refresh
> journals anew. Worker rejections finally leave a durable REJECTED trace. No spec change; unit tests
> (dispatch + enricher dedup) + Pg-gated worker tests extended with journal/causality assertions;
> workspace tests green, validate 0 errors. Unblocks #16 (`commands_accepted_total{channel}` now sees
> all channels).

> ✅ **2026-07-21 — #18: retention policy for write-path journals & adapter mirrors
> (ADR-20260721-025159).** The unbounded-growth follow-ups of ADR-20260720-015300/-015400 are
> closed: one SQL function **`sweep_retention()`** (source
> `specs/database/functions/sweep_retention.sql`, in the generated schema + migration
> `20260721025159`, `REQUIRED_SCHEMA_VERSION` bumped) owns the windows — `command_journal`
> terminal rows 90 d from `completed_at`, `inbound_events` DELIVERED rows 30 d from
> `delivered_at`, `external_stripe_events`/`external_hubrise_callbacks` processed rows 90 d from
> `processed_at` (also the GDPR storage-limitation cap on verbatim webhook payloads). NEVER
> swept: `domain_events`/`domain_stream` (the function does not reference the log), RECEIVED
> journal rows (stale-RECEIVED sweep marks them FAILED first), FAILED inbound rows (kept until
> resolved), unprocessed mirror rows, and the SIRENE mirror (detect-by-absence needs every row).
> Scheduling: new in-process `RetentionSweepWorker` (first pass at boot, then 6 h;
> `RUN_RETENTION_SWEEP` default on) — a `pg_cron` call of the same function is the documented
> alternative. The table YAMLs carry documentary `retention:` blocks. New DB-gated test
> `retention_sweep.rs` proves the delete-set AND the untouchables. `make validate` 0 errors,
> workspace green.

> ✅ **2026-07-20 — value made explicit per issue (product-owner directive, amends
> ADR-20260720-143000 §1).** New org field **Value Size** (T-shirt XS–XL) = the value the issue
> brings if completed, graded from its Impact section; issue **Type** `Foundation`
> (non-functional) vs `Feature` (functional), matching the two value tiers. The `size/*` labels
> are **renamed `impact/*`** — same T-shirt, same meaning (**Impact = the size of the change on
> the code**), matching the board's Impact field (renamed from "Size"); Effort remains its coarse
> projection. Within a Priority bucket no numeric value ordering — row order on the board.
> Applied to all 15 open issues (+#12/#13/#31 for consistency); process recorded in
> docs/BACKLOG.md.

> ✅ **2026-07-20 — backlog re-ordered by VALUE, not effort (ADR-20260720-213024, product-owner
> directive).** ADR-20260720-143000 §4's simplest-first queue is amended: tier 1 = foundations &
> cross-functional/non-functional, tier 2 = features in value-stream order (customer ordering →
> restaurant onboarding → delivery). New queue: #14 → #22 → #15 → #16 → #19 → #18 (contracts,
> security, invariants, observability, retention) → #27 → #26 → #24 → #25 → #23 (codegen wave) →
> #17 → #21 (customer stream) → #20 (restaurant onboarding) → #28 (delivery, post-V0). The
> ranking is applied to the **GitHub Project "Prioritized backlog"** — the single place priorities are
> defined: Priority field = value bucket (Urgent = tier-1 foundations, High = codegen wave,
> Medium = V0 features by value stream, Low = post-V0), Effort field mirrors the size label; no
> rank stamps in issue bodies. The repo records the **method**: `docs/BACKLOG.md` (process + value
> definition) + a CLAUDE.md non-negotiable ("respect the prioritised backlog" — pick from the top
> of the board; re-prioritising is a product-owner decision made in the project). Sizing &
> pre-task-doc rules unchanged. Docs-only change — no specs, no code.

> ✅ **2026-07-20 — #22: per-edge ACL on FK-derived nav fields (`navRoles`, ADR-20260720-230000).**
> api.yaml types may declare `navRoles: { edge: [roles] }` (literal semantics; absent = open):
> emitted as SDL `@auth` + the operations' guard/visible pair on the generated field; validator
> rule `nav-roles-unknown-field`. Seeded: Restaurant.carts [ADMIN], Restaurant.orders
> [RESTAURANT, RESTAURANT_ACCOUNT, ADMIN], Restaurant/Order.deliveryJobs [+RIDER] — closing the
> PUBLIC-schema PII edges before #21 freezes contracts. New ACL test; validate 0 errors.

> ✅ **2026-07-20 — #14: `orderStatusChanged` keys on orderId + per-row ownership (ADR-20260720-220000).**
> The last pre-acceptance-first convention is gone: the subscription takes `orderId` (what the
> confirmation route holds) and matches exactly the `Order-<id>` stream. Ownership per resolved
> row: ADMIN any; CUSTOMER path must BE the order's customer (auth_ref → Customer), strangers and
> anonymous callers get silence; RESTAURANT/RESTAURANT_ACCOUNT paths stay trusted like `orders`
> (RECORDED GAP: no caller↔restaurant binding exists yet — scoping is one coherent follow-up across
> order/orders/orderStatusChanged); guests follow `paymentStatusChanged` (ADR-20260720-213000 §3).
> Roles literal `[CUSTOMER, RESTAURANT, RESTAURANT_ACCOUNT, ADMIN]`. New ownership test; 7
> subscription tests green; validate 0 errors.

> ✅ **2026-07-20 — #12: anonymous checkout survives restarts (ADR-20260720-213000).**
> `place_order` now takes the dispatch-layer `X-SESSION-ID` as an ENVELOPE parameter (never command
> payload, ADR-0041) and stamps it onto the `payment_process_manager` row — a guest resumes
> `paymentStatus(orderId)` after force-closing the app with only the persisted session id
> (`operationStatus`/cart were already session-keyed). Client rules recorded (web cookie
> `SameSite=Lax` / app keychain; SAME id until a `customerId` exists — CartBindingProcess binds on
> phone verify). Guest `order(id)` reads DEFERRED to phone verification (OrderTracking has no
> session column; revisit with #14). Prod smoke upgraded: sends `X-SESSION-ID` on placeOrder and
> reads the intent via the guest `paymentStatus` on `/public/graphql` — the Stripe-metadata
> workaround is gone, so the daily smoke now proves the real anonymous read path. New behaviour
> test `checkout_stamps_the_anonymous_session_onto_the_run_row`. Validate 0 errors, tests green.

> ✅ **2026-07-20 — #31: LITERAL `roles:` lists (ADR-20260720-191500, product-owner directive).**
> api.yaml `roles:` now means exactly what it says: **omitted** → open to every role path
> (`@public`, no guard); **present** → only the listed paths, PUBLIC being just the anonymous
> `/public/graphql` path. Validator `op-no-authz` retired; story authz + SDL/ACL emitters +
> runtime `role_allows` aligned. Migration: 11 standalone `[PUBLIC]` ops drop the line
> (behaviour-preserving); `paymentStatus`/`paymentStatusChanged` become the literal
> `[PUBLIC, CUSTOMER, ADMIN]` (#13's original intent, now expressible); the pre-existing literal
> lists (`verifyPhone`/`requestPhoneVerification` [PUBLIC, CUSTOMER], listing claims
> [PUBLIC, RESTAURANT_ACCOUNT]) finally gain their intended restriction. ⚠️ Review rule: a missing
> `roles:` line is a positive "open to everyone" claim. New ACL test
> `literal_roles_lists_admit_only_listed_paths`. `make validate` 0 errors, workspace green.

> ✅ **2026-07-20 — #13: `paymentStatus`/`paymentStatusChanged` are PUBLIC + ownership-scoped.**
> api.yaml roles `[CUSTOMER]` → `[PUBLIC]` on both (the issue's recommended option, matching
> `operationStatus`): the generated resolvers' ADMIN/session ownership branches — previously dead
> behind the CUSTOMER guard — are now reachable; strangers resolve null / an empty stream (no
> existence oracle). New `crates/server/tests/graphql_payment_status.rs` covers session-owner /
> stranger / sessionless / ADMIN. The prod smoke keeps its Stripe-metadata stand-in until **#12**
> stamps `session_id` onto the run row (comment updated to say exactly that). `make validate`
> 0 errors, workspace tests green.

> ✅ **2026-07-20 (13:00 UTC) — watchdog: `sirene-sync` 6-hour hang fixed** (ADR-20260720-130045).
> The weekly SIRENE ingestion job ran the full 6h GitHub ceiling and was force-`cancelled` twice
> (07-18 dispatch + 07-20 03:00 cron); build was fine (~40s), the hang was entirely the ingest step.
> Root cause: `SireneClient` used a bare `reqwest::Client::new()` with **no request timeout**, so a
> stalled INSEE read froze the sweep forever. Fix (code/CI only, no specs): per-request
> `timeout(60s)`+`connect_timeout(15s)` on the client (`crates/sirene_ingest/src/client.rs`) plus a
> belt-and-suspenders `timeout-minutes: 90` on the workflow. `cargo build`+`cargo test -p
> sirene_ingest` green (4 tests). Next scheduled sweep (Mon 03:00 UTC) to confirm a clean exit.

> ✅ **2026-07-20 (early) — post-merge wave, all landed directly on `main` (user-directed), each
> workstream gated in an isolated worktree then re-gated integrated (final: 29x tests green,
> validate 0 errors, drift clean):** ① **Production JWT bug fixed** — `jsonwebtoken` v10 had no
> crypto backend selected → every authenticated GraphQL request panicked (502) in prod; fixed with
> the `rust_crypto` feature. ② **Automated prod E2E smoke test (Stripe TEST mode)** —
> `tools/smoke/prod-smoke.sh` (`make smoke-prod`, `.github/workflows/prod-smoke.yml`
> workflow_dispatch + daily cron; needs repo secrets `STRIPE_SECRET_KEY`/`RENDER_API_KEY`, not yet
> configured): layered ping/health → public GraphQL → idempotent `smoke-test` tenant fixture →
> full checkout with `pm_card_visa` confirmed server-side → poll until captured. Stripe test
> webhook endpoint created → `https://api.captain.food/adapters/stripe/webhooks`
> (`payment_intent.succeeded`/`payment_intent.payment_failed`/`charge.refunded`), signature
> verified live; `STRIPE_WEBHOOK_SECRET` set in Render. ③ **Server-side pricing, fail-closed**
> (ADR-20260720-002217): `place_order` reprices every folded cart line from the live catalog
> (`application::pricing::price_cart`) → PaymentIntent amount + frozen snapshot; optional
> `PlaceOrder.expectedTotal` equality check; `PriceMismatch`/`PriceUnresolvable`; rule
> `ServerPriceAuthority`. ④ **`pendingRefunds` read model** (ADR-20260720-003142): new
> `RefundOpened` event on the Payment stream, `View_PendingRefunds` fold view + migration,
> `pendingRefunds` query (RESTAURANT+ADMIN) + story steps, rule `PendingRefundVisibleUntilDecided`.
> ⑤ **Bounded partner re-offer policy** (ADR-20260720-004556): decline → re-offer, cap 3
> (`offer_attempts` in the run row), exhaustion → `DeliveryDispatchFailed` + run FAILED (status
> `FAILED` replaces `REOFFER_REQUIRED`); offer timeouts deferred (no time-based sweep host yet).
> ⑥ **Codegen roadmap item 1, first slice** (ADR-20260720-004419): `lifecycle:` DSL in actors.yaml
> (event-keyed), 8 `lc-*` validator rules + coverage warning, generated
> `domain/src/generated/lifecycles.rs` transition tables + mermaid state diagrams in the docs;
> Order wired end-to-end. Remaining open: fee/split breakdown (ADR-0016/0017), offer timeouts,
> Rider/DeliveryJob/Restaurant lifecycle adoption, worker `DeliveryJob-%` drain, roadmap items 2–7,
> GitHub repo secrets for the smoke workflow.

> ✅ **2026-07-20 (early, cont.) — PRODUCTION SMOKE GREEN (all 4 layers):** `make smoke-prod` passes
> end-to-end against api.captain.food — cart → server-priced `placeOrder` → Stripe TEST confirm →
> webhook → PlaceOrderProcess → order **PLACED / CAPTURED**. Getting there surfaced and fixed five
> production defects: ① deployed schema drift — `Cart.session_id` and
> `OrderTracking.payment_intent_id` never had catch-up migrations, so the projectors skipped every
> Cart/Order event (migrations added + Order/Cart checkpoints refolded); ② the refold exposed a
> panicking generated accessor (legacy `OrderPlaced` without `ref`) that froze the projection worker
> at boot — the projector emitter now emits total folds (`unwrap_or_default`), string scalars derive
> `Default`, and both worker loops panic-isolate every tick (a poison event can no longer kill
> projection or sagas); ③ `payment_status` ordering hole — `PaymentCaptured` always precedes the
> `OrderPlaced` row it should fold into, so the creation arm now seeds CAPTURED (the PlaceOrderProcess
> invariant, recorded in the projection DSL lineage + DB-gated test); ④ smoke confirm needed a
> `return_url` (account has redirect payment methods enabled); ⑤ **Sirene sync idempotency** — prod
> listings predate the UUIDv5(SIRET) derivation, so every pass re-derived colliding ids and retried
> 605 `SlugAlreadyTaken` rejections forever; the worker now adopts the aggregate id the projection
> names via `external_identifiers` (register + close paths) and checkpoints deterministic rejections
> instead of retrying (DB-gated tests: adoption, legacy close, no-churn).

> ✅ **LANDED (2026-07-20): command sourcing + inbound-event sourcing + ACCEPTANCE-FIRST GraphQL**
> (ADR-20260720-015300/-015400/-015500, branch `claude/clarification-needed-5si77x`). The two
> pre-agreed constraints held: journals NEVER write `domain_events` (aggregates own the log) and the
> event log stays the single source of truth. What shipped:
> ① `specs/database/tables/journals.yaml` (fifth table category): **`command_journal`** (pk
> `message_id`, envelope columns, business payload + hash, `RECEIVED→SUCCEEDED|REJECTED|FAILED`,
> records rejections) + **`inbound_events`** (adapted BUSINESS events only, unique
> `(source, external_id)`); adapter-owned raw mirrors `external_stripe_events` /
> `external_hubrise_callbacks` join ADR-0045's staging category. ② **ALL ~70 mutations are
> acceptance-first** (api.yaml v2, MAJOR): optional `metadata: MetadataInput`
> (messageId/correlationId/causeId; `X-SESSION-ID` header = the anonymous session; `traceparent` →
> traceId) → journal insert (idempotent replay `duplicate: true`; payload-mismatch = sync Conflict)
> → spawned handler (events carry `cause_id = messageId`) → uniform `MutationAcceptance`. Outcomes:
> PUBLIC ownership-scoped **`operationStatus(messageId)`** + **`operationStatusChanged`** (journal +
> `OperationStatusBus`, snapshot-first; rejections = `Operation.errorCode`, amending
> ADR-20260719-120000), and checkout's **`paymentStatus(orderId)`** + **`paymentStatusChanged`**
> served from the payment PM row (now carrying `customer_id`/`session_id`/`client_secret`, NULLed on
> resolve — the declared PM-privacy exception). ③ Stripe webhooks: verify → mirror verbatim → stage
> `inbound_events` → ACK + nudge the **`InboundEventsDrainWorker`** (sirene-pattern; also sweeps
> stale-RECEIVED journal rows); HubRise callbacks mirror + dedupe before enrichment. ④ Migration
> `20260720030000_command_inbound_journals.sql` + `REQUIRED_SCHEMA_VERSION` bump; observability:
> `place-order` gains `message_id`/`command.journal`, new `stripe-webhook-ingestion` contract.
> `make validate` 0 errors, no drift, full workspace green incl. the Pg-gated acceptance-first e2e.
> **Follow-ups**: `orderStatusChanged` still keys on correlationId (align with messageId later);
> HubRise enricher command sends not yet journaled (`channel: WORKER`); a generic per-mutation
> observability contract needs a §8 `surface: graphql` binding kind; clients/frontends must adopt
> the two-step model (checkout: acceptance → `paymentStatus` poll/subscribe → Stripe element).
>
> 🧭 **Agreed direction (2026-07-19, late):** generalize the spec→codegen approach — ①
> **service catalog with configurable binding** (ADR-20260719-214500, Proposed): `specs/services.yaml`
> declares the abstract APIs, own spec apart from api.yaml (`/services/payment` `request`/`refund` → Stripe adapter, delivery,
> identity, catalog_sync, …); binding + exposure DECIDED IN THE SPEC (local for all of V0; config carries only addresses); PM
> `ports` will `$ref` the catalog. ② **Codegen roadmap** ([docs/codegen-roadmap.md](codegen-roadmap.md)),
> ranked: aggregate lifecycle state machines → generated behaviour-test harness from tests.yaml →
> PM orchestrator scaffolding → the service catalog → PM state-store generation.
> ① LANDED (2026-07-19): `specs/services.yaml` + validator §2d (`svc-*` rules) are in, PM `ports` now `$ref` the catalog (ADR Accepted); trait/client/route emitters still to come.
>
> ✅ **RUNTIME REIMPLEMENTED (2026-07-19 night) — the state-table PM runtime is live on this branch
> (ADR-20260719-193500), 266 workspace tests green, `make validate` 0 errors, no drift.** Landed:
> the `Payment` (stream `Payment-{intentId}`) + `Rider` aggregates and DeliveryJob partner/issue
> folds; the 4 PM state tables (migration + `pm_state` ports + Pg stores); the full missing command
> surface (Rider ×3, DeliveryJob ops ×7, `bindCartToCustomer`); `placeOrder` delivers
> `PaymentIntentCreated` to the Payment stream and opens the run row (concurrent checkout →
> Conflict); all four orchestrators execute their DSL legs (guards throw typed errors —
> `PaymentEventOrphaned`, `DeliveryJobNotFound`; refund decisions by RESTAURANT/ADMIN via
> `approve_refund`/`deny_refund` + fail-closed `request_refund`; cart binding really binds; close
> order via `send MarkOrderDelivered`); the runner surfaces thrown guards on `/saga`; the Stripe ACL
> is a stateless translator (no more `StripeEvent-%` streams, `CheckoutSnapshotSource` seam
> retired). Since then, ALL THREE remaining runtime gaps closed tonight: ① the **refund decision
> API surface** — `approveRefund`/`denyRefund` mutations (api.yaml, roles RESTAURANT+ADMIN, V0;
> story steps in ManageOrders + admin ArbitrateRefunds), emitted resolvers calling the RefundProcess
> orchestrator legs over the new `WriteDeps.refund_state` (`PgRefundProcessState`) + the
> PaymentGateway. ② The **real outbound Stripe adapter** (`stripe::outbound::StripePaymentGateway`):
> form-encoded create-intent (+ `metadata[orderId]`/`[restaurantId]`/`[cartId]`, which the webhook
> ACL requires) and refunds; the port grew a typed `PaymentIntentRequest`; constructed when
> `STRIPE_SECRET_KEY` is set, else the fail-closed stand-in (logged at startup). ③ The
> **`OrderTracking.payment_status` cross-stream feed**: the projection worker's Order group slices
> BOTH `Order-%` and `Payment-%` under its single 'Order' checkpoint (`stream_name LIKE ANY`), and
> Payment-stream facts key the Order row from the payload's `orderId` (a capture without one is
> log-skipped). Still open (see docs/sagas.md): partner re-offer policy, server-side pricing,
> `pendingRefunds` read model/query.
>
> 📣 **Earlier on this branch (2026-07-19 evening):** ① Guard semantics hardened — **in case of error a
> guard always `throws` a typed exception, on EVENT legs too** (run aborts + error surfaced — e.g.
> `PaymentEventOrphaned` for an orphan Stripe capture/failure, `DeliveryJobNotFound` for partner
> reports on an unknown dispatch run); `skip` is strictly for benign alternatives, and the validator
> enforces exactly-one-outcome per guard. ② The **CI gate (workflow `ci`, ex `codegen-consistency`) now runs on every
> branch push** (was main-only), so no branch escapes validate + test + drift. ③ The **per-PM
> sequence diagrams are now embedded in the product documentation** — `documentation.generated.md`
> (mermaid fences, renders on GitHub) **and** `documentation.generated.html` (in-page mermaid
> renderer, offline-degrades to readable source) — generated from the typed steps, zero drift.
>
> 🚧 **Feature branch — Process-manager re-architecture: DSL layer DONE, runtime pending.** Process
> managers are now **state-table orchestrators specified by a TYPED step DSL** (ADR-20260719-172821):
> `specs/processmanager.yaml` legs are ordered `read`/`guard`/`call`/`deliver`/`send`/`state` steps —
> every field a `$ref` or enum const, state in declared tables (`process_managers.yaml`), command-leg
> guards `throws` / event legs `skip`, emits **derived** from steps, sequence diagrams **generated**
> from steps (`c4.generated.md`). Validator §2b proves the wiring; the ADR-0032 gate applies to PMs
> unexempted. `make validate` **58 → 0 errors** (behaviour tests added for Rider, DeliveryJob ops,
> Payment records, admin-approved RefundProcess incl. `RefundNotPending`). `cargo test --workspace`
> green. The PM **runtime is NOT reimplemented yet** (still the event-sourced runner): see
> **[docs/process-manager-rearchitecture.md](process-manager-rearchitecture.md)** for the phase plan.
> Also on the branch (green): the write-side **`Repository`** refactor (ADR-20260719-031136) + the
> **checkout snapshot** (ADR-20260719-014434) — the runtime rework will rebuild the saga side of these.

## 🌐 Deployment

| Piece | Status | Notes |
|---|---|---|
| Render web service (Docker, Frankfurt) | ✅ | Blueprint IaC (`render.yaml`), cargo-chef cached build, verified live |
| Supabase Postgres (Frankfurt, eu-central-1) | ✅ | Session pooler; Data API off (intentional) |
| CI workflow `ci` (build+test+validate+drift; ex `codegen-consistency`) | ✅ | Gates deploys (`autoDeployTrigger: checksPass`) |
| CI `db-migrate` (sqlx-cli, gated on green build) | ✅ | Applies `migrations/*.sql` out-of-band (ADR-0043) |
| `/health` (schema-version readiness), `/ping`, `/projector` | ✅ | `>=` version gate; in-process projector |
| GraphQL `/{role}/graphql` + `/{role}/voyager` | ✅ | Role-as-path; per-role filtered schema |
| Custom domains `*.captain.food` (Dynadot wildcard → Render) + Host router | ✅ | Wildcard TLS issued; apex+`www` 301→`join` (GitHub Pages); `hosts.rs` dispatches audiences (`live`/`restos`/`riders`/`system`) + `{slug}` tenants; onrender URL disabled. Recorded in **ADR-0036 amendment (2026-07-18) + ADR-0042** |

## 📖 Read side (queries)

| Query | Status | Notes |
|---|---|---|
| `restaurants` / `restaurant` | ✅ | Real data once SIRENE runs |
| `prospectionPipeline` | ✅ | Admin; fed by SIRENE registrations |
| `pricingPolicy` / `uberEstimationPolicy` / `uberSplitPolicy` | ✅ | **Real seeded data** |
| `catalog` / `categories` | ✅ | **Real nested data** — catalog `tree` projector (categories→products→offers/option-lists + derived `stockStatus`) |
| `carts` / `cart` / `orders` / `order` | ✅ wired | Populated as carts/orders are placed |
| `me` / `favoriteRestaurants` | ✅ | `me` resolves the verified ADR-0047 `Principal` → Customer read model; `favoriteRestaurants` joins the customer's favourites |
| Projection worker → registry (per-aggregate checkpoints) | ✅ | In-process; **no batch cap** (drains all pending per tick, loops 1.5s); hardened to **log-skip a poison event** so one bad record can't wedge projection. ⚠️ Free-tier **spin-down** pauses it when the app is idle >15 min → kept warm via **uptimerobot `/ping` every 5 min** |

## ✍️ Write side (mutations)

| Piece | Status | Notes |
|---|---|---|
| `MutationRoot` (all api.yaml mutations generated) | ✅ | |
| Restaurant aggregate (13 commands) | ✅ | Spec invariants (event-stream rehydration) + 25 behaviour tests |
| Cart (3) · Order (11) · DeliveryJob (4) | ✅ | Round 2a — real invariants + tests; **Cart line-checks now enforced** (OfferUnavailable/InsufficientStock/InvalidOptionSelection) via the catalog offer read port |
| Catalog (12) · Prospect (3) · RestaurantAccount (3) | ✅ | Round 2b — real invariants + behaviour tests |
| Customer (14) | ✅ | Wired end-to-end: `customer` read model + Pg repo, fail-closed `AuthProviderGateway` stand-in (real Supabase ACL deferred), injected at the composition root |
| `placeOrder` + process managers (4 sagas) | ✅ wired | `placeOrder` live (fail-closed `PaymentGateway` stand-in); in-process PM runtime (`/saga`) — PlaceOrder/Refund/CartBinding/DeliveryDispatch react to payment/delivery facts → `OrderPlaced`/`OrderDelivered`/… **Real Stripe create-intent = 🅑**; ✅ **checkout-snapshot DSL closed** (ADR-20260719-014434): `PaymentIntentCreated` now carries `checkout` (`CheckoutSnapshot`), frozen by `place_order`, so `OrderPlaced` rebuilds from the log — priced `items`/`breakdown` + retiring the fail-closed `CheckoutSnapshotSource` ride on server-side pricing |
| Structured typed errors | ✅ | `DomainError::Rejected{code,context}` → GraphQL `extensions.code` + interpolated en/fr message (ADR-20260719-120000) |
| GraphQL **subscriptions** | ✅ | `SubscriptionRoot` + in-process event bus + WS transport + per-role ACL (`orderStatusChanged`/`operationStatusChanged`); works while the app is warm |

## 🔐 Authorization

| Piece | Status | Notes |
|---|---|---|
| Per-role ACL — execution guard + per-role introspection/Voyager | ✅ | Spec-derived from api.yaml `roles` (ADR-0006); role now **verified** by JWT (ADR-0047), so Voyager filtering is trustworthy |
| Per-field ACL on FK-derived nav edges | 📋 | api.yaml has **op-level** `roles` only; needs a DSL extension → **plan mode** |
| EXTERNAL machine callers | ✅ | Pre-shared `X-External-Api-Key` (`EXTERNAL_API_TOKENS`, constant-time) or Supabase JWT w/ captain_role EXTERNAL (ADR-0047) |
| Authentication / identity (Supabase JWT) | ✅ | **First cut shipped (ADR-0047)**: verify Supabase JWT via JWKS at `/{role}/graphql` (public keys, no shared secret; ~1h cache, serve-stale-on-refresh-failure — no per-request Supabase call); `app_metadata.captain_role` gates the path (`/public` open, else 401/403), fail-closed on cold cache, asymmetric-only. Verified role + `Principal` injected. **EXTERNAL service tokens** via `X-External-Api-Key` (constant-time, `EXTERNAL_API_TOKENS`) shipped. Per-field `@auth` on FK-nav edges = DSL/plan-mode follow-up |

## 🔎 SIRENE prospection (ADR-0019/0020/0027/0045)

| Piece | Status | Notes |
|---|---|---|
| SIRENE ACL (INSEE → RegisterRestaurant mapping) | ✅ | Unit + DB verified |
| Interim direct-write `sirene_sync` binary | ✅ | **Retired** (ADR-0045) — replaced by the split below |
| `external_sirene_restaurants` staging table | ✅ | Migration applied by CI |
| Thin CI ingestion crate `sirene_ingest` (fetch → UPSERT raw rows, France-wide by department, active-only) | ✅ | No domain deps; scheduled workflow builds only this crate |
| On-app `sync_sirene_worker` (ACL on deployed version) + deletion reconciliation | ✅ | Per-row checkpoint; detect-by-absence (21d debounce) + explicit `F`/`C`; NON_PARTNER auto-close, partners flagged; `POST /internal/sirene/drain` (token-gated, fail-closed) |
| `INSEE_API_TOKEN` repo secret | ✅ | Added; SIRENE runs live on deploy (scheduled ingestion → staging → worker) |
| `INTERNAL_TRIGGER_TOKEN` (Render env + repo secret) to enable the CI→worker ping | ⏳ | Optional; without it CI ingests and the worker drains on its own poll loop (`RUN_SIRENE_WORKER`, default on) |

## 🔌 External integrations — partner adapters & M2M (ADR-20260718-145856 / -213352)

**Partner webhook adapters are self-contained crates** under `crates/adapters/*` — each an ACL +
axum shell + standalone binary, mountable into the monolith **or** deployable as its own web service.
Two directions: partner-**push** webhooks (below) vs external-**drive** `/external/graphql` (M2M).

| Piece | Status | Notes |
|---|---|---|
| **Stripe** — `crates/adapters/stripe` (`POST /adapters/stripe/webhooks`, `stripe-webhook` bin) | ✅ | `Stripe-Signature` HMAC over raw body (constant-time, 300s replay, fail-closed); ACL → `PaymentCaptured`/`PaymentFailed`/`PaymentRefunded`; idempotent by Stripe event id. 12 tests |
| Checkout must set `metadata.restaurantId` (+`orderId`) on the PaymentIntent/charge | ✅ | `StripePaymentGateway` sends `metadata[orderId]`/`[restaurantId]`/`[cartId]` on create-intent — the webhook ACL maps `charge.refunded` from them; exercised by the green prod smoke |
| **HubRise** — `crates/adapters/hubrise` (`POST /adapters/hubrise/webhooks`, `hubrise-webhook` bin) | ✅ | **Ingress** ✅ (HMAC-SHA256 hex, fail-closed, envelope parse). **Outbound OAuth2 client** ✅ (`api.rs`: `X-Access-Token`, non-expiring token from `HUBRISE_ACCESS_TOKEN`, `exchange_code` connect helper, catalog/inventory pull). **Domain wiring** ✅ (`enrich.rs`): verified catalog/inventory callback → API pull → enrichment ACL → `ImportCatalog` / per-SKU `update_offer_stock` handlers. **Deterministic UUIDv5-of-HubRise-id** ids reconciled with the **Catalog aggregate** (offer seeded from the SKU `ref` = inventory's `sku_ref`, so a stock update hits the imported `OfferId`); `"9.80 EUR"`→`Money`, tax-rate strings→`TaxRate`, `data` envelope translated at the boundary; catalog = rejectable command (`CatalogNotFound`→skip), inventory = reported fact (`OfferNotFound`→skip, never rejected). 14 tests. Enricher wired at the server composition root + the standalone bin (both gated on `HUBRISE_ACCESS_TOKEN`). **Open**: the connect flow must create the `Catalog`/`Restaurant` with these derived ids + a token table (→ plan mode) |
| **`/external/graphql`** — M2M standard | ✅ | External entities query/mutate via the `EXTERNAL` role path; API-key auth (`X-External-Api-Key`, ADR-0047); allowlist is per-op `roles: [EXTERNAL]`. **Subscribe** = future (needs `SubscriptionRoot` + WS + `api.yaml`); per-partner keys = future |

## 👤 Ops / user actions

- ✅ Keep the web service **warm via uptimerobot `/ping` every 5 min** (prevents free-tier spin-down so the in-process projector + SIRENE worker keep running).
- 🗑️ `INTERNAL_TRIGGER_TOKEN` / `POST /internal/sirene/drain` — agreed to **remove** (superseded by the `/ping` warmth approach); code removal deferred to avoid colliding with concurrent `routes.rs` edits — harmless meanwhile (fail-closed 503 when the secret is unset).

> **Claim protocol (2026-07-20, ADR-20260720-233000, #39; amended 2026-07-21 by
> ADR-20260721-042018):** before working an issue, add the `status/in-progress` label + a claim
> comment naming the `NN-slug` branch, **create the branch and open a draft PR (`Closes #NN`)
> immediately**; NEVER work a claimed issue; on completion mark ready + enable auto-merge and
> supervise checks until MERGED; the hourly stale-claim reaper releases claims silent for >24h.
> Method: `BACKLOG.md`.

## 📋 Remaining work — todo & session split

> **⚠️ TRACKING MOVED (2026-07-20, user-directed): remaining work now lives in
> [GitHub issues](https://github.com/Captain-Food/captain-food/issues) (#12–#28, typed
> Task/Bug/Feature) managed on the **org-level GitHub Project**
> ([github.com/orgs/Captain-Food/projects](https://github.com/orgs/Captain-Food/projects),
> created 2026-07-20) — not in this file.** Issues carry `size/*` labels + org issue fields
> Priority/Effort (mapping recorded in ADR-20260720-143000); the project's views read those
> directly, so triage state lives on the issue, never in a board-only field. New work items get an
> issue, not a table row; this file stays the narrative deployment/architecture snapshot. The table
> below is the last pre-migration snapshot, kept for history.
>
> **Issue workflow (2026-07-20, ADR-20260720-143000):** every issue is sized once with a
> `size/XXXS`…`size/XXXL` label (AI-native scale: agent sessions + cost + review, see the ADR
> table) and carries standard pre-task sections — *Why now? / What & why? / Impact / Sequence
> diagram / Estimation* (with its rank in the simplest→largest queue). The issue is the pre-task
> contract; the PR is the post-task record — overlap is intentional, divergence is signal. No
> Scrum: flow-based queue, cheapest-impactful first; re-size only on scope change; XXXL must be
> split before starting.

Two sessions run in parallel — 🅐 = this (desktop) session, 🅑 = the iPhone/other session. Pull-rebase before every push.

| # | Item | Owner | Status |
|---|---|---|---|
| 1 | **Checkout saga** — `placeOrder` + `PlaceOrderProcess` + PM runtime | 🅐 | ✅ wired (real Stripe gateway; smoke-proven in prod) |
| 1a | **Checkout snapshot** on `PaymentIntentCreated` (ADR-20260719-014434) — DSL + `place_order` freeze + tests done | 🅐 | ✅ DSL · runtime population + port retirement ride pricing |
| 1b | Stripe **outbound** `PaymentGateway` (create PaymentIntent) in the Stripe adapter crate | 🅐 (landed here, not 🅑) | ✅ `stripe::outbound::StripePaymentGateway` (create-intent + refunds, env-gated by `STRIPE_SECRET_KEY`, fail-closed stand-in otherwise) — exercised by the green prod smoke |
| 2 | **HubRise** domain ACL — webhook → `ImportCatalog`/`OfferStockUpdated` (OAuth2 pull + deterministic ref-mapping) | 🅐 | ✅ landed (`enrich.rs`, 14 tests) |
| 2a | ⚠️ **Connect flow** — provision `RegisterRestaurantAccount` + `Restaurant`(s) + `CreateCatalog` with the enricher's derived UUIDv5 ids, and persist the HubRise **account-scoped** token in a connection/token table keyed by `RestaurantAccount` (HubRise Account⇔RestaurantAccount, Location⇔Restaurant; `HUBRISE_ACCESS_TOKEN` today = one account). See `docs/integrations/hubrise-process.md` §0 | plan mode | 📋 |
| 3 | **Process managers** — Refund/CartBinding/DeliveryDispatch + PM runtime (event-driven, `/saga`) | 🅐 | ✅ (outbound refund via the real gateway; bounded partner re-offer landed — offer timeouts deferred, ADR-20260720-004556) |
| 4 | **Cart line invariants** + catalog `tree` projector + offer read port | 🅐 | ✅ |
| 5 | **Frontend** — Leptos/WASM SDUI renderer (customer/restaurant/rider apps) | unassigned | 📋 |
| 6 | GraphQL **subscriptions** (`SubscriptionRoot` + bus + WS + ACL) | 🅐 | ✅ |
| 7 | **Structured typed errors** (ADR-20260719-120000) | 🅐 | ✅ |
| 8 | **Per-field nav-edge ACL** — optional `roles:` on nav fields (default public), same guard/visible as ops; design agreed | 🅐 | 📋 plan mode (after ACL emitter free) |
| 8b | Delivery/account read queries + catalog `tree` + `me`/favorites | 🅐 | ✅ (read surface complete except `phoneCountries`=client-const, `operation`) |
| 9 | Remove `INTERNAL_TRIGGER_TOKEN`/drain endpoint (use `/ping` warmth) | 🅐 | 🗑️ deferred |
| 10 | Projection worker robustness (poison-skip) + spin-down mitigation (uptimerobot `/ping`) | 🅐 | ✅ |

## 🧭 Architecture decisions
See [`docs/adr/`](adr/) — latest: 0047 (API auth — Supabase JWT/JWKS), 20260719-120000 (structured domain rejections), **20260719-014434 (checkout snapshot on `PaymentIntentCreated`)**, **20260719-031136 (write-side `Repository` / event-sourced actors — handlers + saga runner route through it, never the raw `EventStore`)**, 20260718-145856 amendment (adapter webhook routes → `/adapters/{partner}/webhooks`). **ADR ids are now date-time** to avoid concurrent-session collisions (ADR-20260718-135417).

> Convention: keep this file current with every substantive change, and record cross-cutting decisions as an ADR in the same change.
