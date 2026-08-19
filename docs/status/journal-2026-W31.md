# Status journal — 2026-W31

Journal entries dated **2026-07-27 → 2026-08-02** (ISO week 2026-W31, 2026-07-27 to 2026-08-02). **27 entries**, newest first, in the order they were written.

Split out of `docs/STATUS.md` on 2026-08-19 — the entries are byte-identical, only their relative links gained a `../`. Current state, and the index of recent entries, live in [`../STATUS.md`](../STATUS.md).
> ✅ **2026-08-02 — [#290 "Actor-client crate isolation (PROP-20260728-152752 D9): compiler-enforced door, then per-actor crates"](https://github.com/TheCaptainCompany/captain-food/issues/290)
> phase 1 MERGED ([PR #297](https://github.com/TheCaptainCompany/captain-food/pull/297);
> [PROP-20260802-130500](../proposals/PROP-20260802-130500-isolation-by-construction.md)
> D1+D3+D4+D5, two independent review passes)**: the mailbox door is COMPILER-enforced now.
> #290 and #284 are CLOSED (product owner, "close all the phases"); every remaining item is its
> own backlog issue — #302 lint floor · #303 watch/bus · #304 Mailbox-port hole · #305 View_*
> reads · #306 phase 2 · #307 phase 3 (full links in the proposal header). **#308 and #309 are
> DONE (2026-08-02, same session): the withdrawal method is `cancel_scheduling` (renamed per
> #308 — it cancels a SCHEDULED reminder, never an in-flight command; still `reminders:`-gated,
> `message_id`-keyed, lane-scoping declined) and C4 L3 carries the `actor-client` component
> (approved spec edit, #309).** New boundary crate
> `crates/actor_client` (between `application` and `infrastructure`) owns the `Mailbox` port,
> `MailboxEntry` with **pub(crate) fields + getters** (constructing one outside the crate does not
> compile), `Envelope`, the shared entry constructors, `reminders::scheduled_entry`, the FROZEN
> `stable_partition` (re-homed from `actor_runtime`, golden test moved with it), the GENERATED
> typed per-actor clients (emitter retargeted; addressing tables split into
> `generated/addresses.rs`, re-exported by the infra `command_router` — one definition), and the
> **D4 read door**: one generic `ActorClient.get_operation_status(message_id)` — the only
> sanctioned read over `inbound_messages` status; the generated `operationStatus` query and the
> `operationStatusChanged` snapshot both resolve through it (`watch` was deferred to #303 —
> done 2026-08-03, see the entry above: the bus lives in `actor_client` now, mailbox-keyed).
> `infrastructure` keeps ONLY the SQL side
> (`PgMailbox` binds via getters; `apply_schedules_in_tx` binds the actor_client constructor).
> **Review hardening (independent pass, 2026-08-02)**: (1) the D8-deferred UNTYPED bulk fact door
> (`enqueue_inbound_facts`/`InboundFact`) sits behind the `bulk-door` cargo feature, with
> `infrastructure` (the SIRENE sweep) the ONE manifest allowed to enable it. Honest limits of the
> gate: cargo features UNIFY, so once infrastructure lights it the symbols RESOLVE graph-wide —
> the manifest grant is the loud reviewable act, and the enforcement is the guard
> `bulk_door_feature_is_granted_only_to_infrastructure` (bidirectional, verified red), which also
> SOURCE-SCANS every crate: naming either symbol outside `infrastructure`/`actor_client` is
> CI-red, closing the demonstrated manifest-less evasion. Every bulk fact is validated at the
> door: `event_type` against the generated `ACTOR_INBOUND_FACTS` table (the same actors.yaml
> `receives` scan the sealed `{Actor}Fact` traits come from — the runtime re-proof of the typed
> path's compile check) AND payload-tag coherence (the adjacent `eventType` must equal the row's
> `message_type`, or delivery would route on a lie). (2) the generated
> `ReminderSchedule` is `#[non_exhaustive]`, so an out-of-crate spec literal — the forgery route
> into `scheduled_entry` — is a compile error (E0639); specs come from the generated table only.
> **D3**: codegen guard `capability_dependencies_are_allowlisted` — `sqlx`/`reqwest` and (#558
> ENF-1) `jsonwebtoken`/`aes-gcm` only in an explicit per-crate allowlist with WHYs (server keeps
> both sqlx exceptions: PgPool construction + /health probe; Supabase JWKS fetch; and is the sole
> `jsonwebtoken` holder — ADR-0047 identity verifier; infrastructure is the sole `aes-gcm` holder —
> #112 auth-session secret-at-rest), bidirectional (stale entries fail), verified red on a
> planted grant for every controlled capability. **D5**: cross-crate test access rides the `test-fixtures` cargo feature (mem
> double, `EntryFixture` full-field mirror keeping out-of-crate freeze tests exhaustive, reference
> impls), dev-dependencies only — guard `test_fixtures_feature_never_reaches_a_release_artifact`
> fails any release-graph grant (verified red). The textual door guard stays as belt-and-braces,
> allowlist moved to the actor_client paths. **Surface directive
> ([ADR-20260802-170059](../adr/ADR-20260802-170059-client-surface-is-spec-gated.md), product owner
> 2026-08-02): no client method without a usage declaration in the spec** — `send` ⇔ ≥1 declared
> command, `record` ⇔ ≥1 declared inbound fact, `schedule`/`cancel_scheduling` ⇔ a `reminders:` declaration;
> unjustified methods are ABSENT, not uncallable (`PaymentClient` is record-only, only
> `OrderClient` schedules); guard `client_surface_exists_only_with_a_spec_declaration` re-derives
> the rule from actors.yaml. Behavior frozen: drift guards, `graphql_typed_send`,
> byte-identity codegen tests all green; validator 0 errors. **D6 (lint floor) deliberately NOT
> here** — its own change per the product-owner decision; phase 2 (per-actor client crates) and
> the C4 update follow on #290's checklist.

> 🐛 **2026-08-01 — prod-smoke hotfix: authenticated GraphQL was fully down in production
> (`503 "auth unavailable"` on every non-`/public` role path).** Root cause: `AuthContext::from_env`
> read `SUPABASE_JWKS_URL`/`SUPABASE_URL` straight from `std::env`, but those are **non-secret baked
> config** (ADR-20260729-020000) — present in the resolved `Config`, absent from the Render env — so the
> JWKS URL resolved empty and the verifier fail-closed. Fixed by feeding the resolved `config.*` values
> through a new `AuthContext::from_config(...)` (env-override precedence preserved); regression guard
> `from_config_uses_its_arguments_not_env`. Same trap as `263f2a2` (smoke script), now closed in the
> server. Decision recorded in
> [ADR-20260801-080339](../adr/ADR-20260801-080339-auth-verifier-reads-resolved-config-not-env.md).
> `cargo build -p server` + `cargo test -p server` green; recovers on next deploy.

> 🚧 **2026-08-02 — the isolation program is APPROVED, phase 1 launching
> ([PROP-20260802-130500 "Isolation by construction"](../proposals/PROP-20260802-130500-isolation-by-construction.md),
> [#290 "Actor-client crate isolation (PROP-20260728-152752 D9): compiler-enforced door, then per-actor crates"](https://github.com/TheCaptainCompany/captain-food/issues/290)).**
> All six decisions answered by the product owner (DECISIONS.md §14/§5): dedicated `actor-client`
> crate; phase-2/3 per-actor crates cover aggregates AND both process managers; cargo-deny
> capability allowlist (`sqlx`/`reqwest`) in phase 1; the read door is ONE generic `ActorClient`
> with `get_operation_status(message_id)` (operation status is actor-agnostic — per-actor typed
> clients stay write-side); `test-fixtures` feature + CI release-graph check; lint floor deferred
> to its own change after phase 1 (against the recommendation).

> 🚧 **2026-08-02 — [#284 "Typed actor clients (PROP-20260728-152752 §2.1)"](https://github.com/TheCaptainCompany/captain-food/issues/284)
> slice 1 built (branch `claude/situation-explanation-cj06o2`)**. *(Path/visibility claims in this
> entry describe the pre-#290 layout; the 2026-08-02 #290 phase-1 entry above supersedes them —
> the clients, constructors and door now live in the `actor_client` boundary crate.)* New emitter
> generates the actor clients (then `crates/infrastructure/src/generated/actor_clients.rs`; now
> `crates/actor_client/src/generated/actor_clients.rs`) — one `{Actor}Client` per mailbox actor
> (`send`/`record`/`schedule`/`cancel` — the latter renamed `cancel_scheduling` per #308,
> 2026-08-02) with SEALED per-actor `{Actor}Command`/`{Actor}Fact` marker
> traits, so sending a message the actor does not `receive` is a COMPILE error. Clients delegate to
> the shared crate-internal constructors extracted in `enqueue` (`command_entry`,
> `insert_mapped`, `schedule_mapped`) — MemMailbox drift guards prove typed `send`/`record` rows are
> field-for-field identical to the free-function enqueue; `record` always keys on
> `inbound_message_id(source, external_id)`. The caller-side `Envelope` (transport metadata only, no
> payload/addressing) was hand-written in `application::mailbox` (now `actor_client::mailbox`,
> #290). **No batched send — D8 is answered: not for now.** **Slice 2 built (PR #289)**: the GraphQL resolver emitter no
> longer constructs `MailboxEntry` inline — both the aggregate-routed template and the gated PM
> template's mailbox arm deserialize the typed command and `send` through the generated
> `{Actor}Client` (identity extraction + the birth-command `now_v7` mint stay in the resolver; the
> acceptance / dedupe / Conflict / telemetry contract is unchanged, frozen by the new no-DB
> `crates/server/tests/graphql_typed_send.rs`). One recorded delta: the mailbox row payload is now
> the domain command's own serde form (absent optionals as explicit `null`, defaulted arrays as
> `[]`), not the null-stripped GraphQL input — dedupe is self-consistent post-deploy, but a
> same-`messageId` retry straddling the deploy for a command with absent optional fields maps to
> Conflict instead of replay. **Slice 3 built (PR #292, final)**: every
> adapter is on the typed clients — SIRENE (`MarkRestaurantClosed` via `RestaurantClient::send`
> with the journal-derived envelope, the row-by-row fallback via typed `record`; the BATCHED
> `enqueue_inbound_facts` fast path stayed as the then-crate-internal bulk door, D8 deferred —
> since #290 it is the `bulk-door`-feature-gated, receives-validated door in `actor_client`),
> HubRise connect/enrich (`RestaurantAccountClient`/`RestaurantClient`/`CatalogClient`), and the
> four webhook ACLs (Stripe → `PaymentClient`, Uber Direct/Avelo37/CoopCycle → `DeliveryJobClient`
> — `inbound_fact_for`'s runtime family→lane switch is DELETED; the sealed Fact traits check it at
> compile time). The free-function surface was CLOSED at the then-crate boundary
> (`enqueue_inbound_fact(s)`/`InboundFact` crate-internal; `enqueue_worker_command`/
> `schedule_reminder`/`cancel_reminder` test-only reference implementations for the drift guards —
> all superseded by #290's actor_client crate, where the same closure is compiler-enforced and the
> reference impls ride the `test-fixtures` feature); the public surface is the clients + outcome
> enums + id derivations. Codegen guard
> `mailbox_entry_is_constructed_only_behind_the_typed_doors` fails the build on any new
> `MailboxEntry` construction site (allowlist asserted-to-exist; verified red on a planted
> violation). Same change also restored the LOST `#[test]` on `makefile_recipe_lines_are_ascii`
> (a stray duplicate attribute had orphaned it — the guard silently never ran).

> 🚧 **2026-07-30 — the actor-runtime redesign is APPROVED and in build (ADR-20260730-231500).**
> Three proposals approved in-session by the product owner (*"we can build it now"*):
> **PROP-20260728-135632** (aggregate state as spec: declared `state:` lineage, generated
> `apply`/`fold` ON the actor, `requires` acting/claims), **PROP-20260728-152752** (the write path
> becomes an actor mailbox: `inbound_messages` replaces `command_journal` + `inbound_events`,
> `(actor_type, actor_id)` addressing, partition leases + `ownership_version` fencing, typed
> clients as the only door, reminders, activations), **PROP-20260730-230803** (projection runtime:
> generated unit-of-work batches, `business_key` lanes, `target: redis` for ScopeMembership).
> 🚧 Foundation slice in build on `242-actor-mailbox-foundation` (this PR).
> ✅ **Slice 1 MERGED** ([#268](https://github.com/TheCaptainCompany/captain-food/pull/268) → `87bcec8`,
> auto-merge, CI green incl. the real-Postgres suites): mailbox DSL + addressing + state/requires
> pilot + 12 negative-tested validator rules + runtime knobs. Legacy journal tables stay live until
> slice 3. Realization directives for slices 2–4: extraction-ready runtime crate + Proto.Actor-inspired
> test plan (ADR-20260730-234918). **Prod sequencing (product owner, 2026-07-30: Render prod is
> still DOWN — see the pipeline-isolation note below)**: slices 1–2 are prod-inert (no migrations,
> no behavior flips) and proceed regardless; **slice 3 (mailbox migrations + resolver flip) waits
> until the enum-text release is applied and smoked** — never stack a second unapplied migration
> set on a paused prod.
> ✅ **#270 MERGED (2026-07-31, squash `15864f7`)** — Runtime A+B+C + review fixes + the combined
> actors/projector test are on `main`. **Runtime D continues on
> [#272](https://github.com/TheCaptainCompany/captain-food/issues/272)** (branch
> `272-runtime-d-pm-mailboxes-reminders`), under the APPROVED
> [PROP-20260731-195500](../proposals/PROP-20260731-195500-runtime-d-pm-mailboxes-and-reminders.md)
> choices A2 (two-phase payment delivery) / B2 (chained PM facts) / C2 (event-lineage reminder
> triggers), ADR-20260731-203000.
> ✅ **D2 Order retention pilot LANDED on the #272 branch (2026-08-01)**: `OrderExpired`/
> `OrderDeleted` events + `ORDER_RETENTION_WINDOW_DAYS`; the Order actor's `reminders:`/
> `schedules:`/`deletion:` blocks (explicit-chain shape, ADR-20260801-010134 — window on the
> REMINDER because the expiry must be a recorded, foldable fact); generated `REMINDER_SCHEDULES`
> + `Config::reminder_windows()` + `DELETION_POLICIES` tables; `apply_schedules_in_tx` starts
> the clock INSIDE the completion transaction; the kind-MESSAGE delivery route records the
> promoted fact (Recorded/Duplicate/Ignored — never Rejected); behaviour tests assert schedule +
> reschedule-in-place per terminal receive; E2E `mailbox_retention` proves the loop on PG.
> ✅ **The GENERIC deletion engine is on the branch too (2026-08-01)**: a log-consumer worker
> over the generated `DELETION_POLICIES` (own `projection_checkpoint` row `DeletionEngine`, scan
> BOUNDED by the slowest projection checkpoint = phase-1 fold verification), two restart-safe
> transactions per journey (`$StreamTombstoned` instruction → delete `domain_events` +
> `domain_stream` + `OrderDeleted` receipt on `DeletionLedger-Order` + cursor, atomically);
> `$`-prefixed technical rows are skipped by `PgEventStore::load` and the projector; unsupported
> policy shapes (windowed engine delays, undo, child enumeration) REFUSE construction. GATED
> `RUN_DELETION_ENGINE` default **false** (gate-then-stabilize — the default flip is its own
> one-line ADR after staging smoke); readiness at `GET /deletion`. E2E `deletion_engine` green.
> ✅ **D1 LANDED on the #272 branch (2026-08-01), GATED `PM_MAILBOX_DELIVERY` default false**
> (gate-then-stabilize; default flip = its own one-line ADR after staging smoke): the runtime
> gained the **PREPARE phase** ([ADR-20260801-023000](../adr/ADR-20260801-023000-a2-realizes-as-prepare-phase-single-delivery.md)
> R2 — handler work with NO transaction open, then ONE fenced commit); the three PM commands
> (placeOrder/approveRefund/denyRefund) run their UNCHANGED application handlers in prepare over
> staging stores (new `StagingPaymentProcessState`/`StagingRefundProcessState`; executor-generic
> generated pm-state upserts flush the run rows in-tx), Stripe idempotency keys
> `intent:{orderId}` / `refund:{intent}:{amount}` make redelivery re-runs land on the SAME
> gateway object, and a sync decline commits the byte-identical legacy `REJECTED PaymentDeclined`.
> B2 realized IN-TX ([ADR-20260801-053000](../adr/ADR-20260801-053000-b2-chain-rides-the-completion-transaction.md)):
> the Payment lane chains `PaymentCaptured`/`PaymentFailed`→PlaceOrderProcess and
> `PaymentRefunded`→RefundProcess inside the recording transaction (identity
> `UUIDv5(orderId, factType:causingRow)`, cause-chained, post-commit nudge); the PM lanes run
> the saga event legs fenced; the runner drops exactly the Stripe-fact triggers behind the gate.
> actors.yaml gained the PlaceOrderProcess/RefundProcess entries WITH the wiring; the generated
> PM resolvers carry BOTH arms (gated at request time). `command_completion_ms` now also emits
> from the mailbox delivery's post-commit observer (was dark for every Runtime-C-flipped
> command); observability contracts rewritten in the same change. `operationStatus` reads were
> already mailbox-first; journal DROP rides the default-flip deploy. E2E `pm_prepare_delivery`
> (7 tests incl. the full capture chain) green. The independent multi-lens review (payments
> lens) found 1 critical + 2 major, all FIXED (`32b8605`): deterministic Stripe 4xx now terminal
> on both arms (a Repository class retried a mailbox head row FOREVER — one bogus
> paymentMethodId per partition could wedge every checkout lane); a startup backfill (gate ON)
> enqueues un-reacted Stripe facts past the runner checkpoints so no flip direction loses a saga
> hop; cross-arm duplicate identity (each gated arm replays the OTHER acceptance store's
> messageIds — a retry never re-executes across a flip). Deferred minors: prepare-before-
> authority-precheck rate burn; the pre-existing same-cart check-then-act window (durable fix =
> partial unique index on payment_process_manager). Remaining D: D3 (activations, rebalancing,
> test ports).
> ✅ **D3 LANDED on the #272 branch (2026-08-01)** — the #270 review's deferred runtime findings
> plus PROP-20260728-152752 §3.5's activations, each gate-then-stabilize: **fair-share lane
> rebalancing** (census + steal-one-from-the-largest with fresh counts per steal, stop at
> `floor(total/instances)` — converges ±1 without thrash; cluster fixture `rebalance.rs` proves
> convergence while the victim is ALIVE, then a hard-crash expiry takeover, exactly-once +
> per-actor order + per-identity completeness throughout = ADR-20260730-234918 ports 1–3 + the
> port-5 probe self-test); **ACTIVATIONS gated `ACTOR_ACTIVATIONS` default false** (held-state
> cache scoped to the delivered actor's own stream: fill on load, promote strictly POST-COMMIT,
> invalidate on a lost version race / lane loss / idle expiry / LRU byte bound; per-actor
> `mailbox.activations` DSL + generated policy table; E2E `mailbox_activations`: 1 rehydration
> load across 3 deliveries, a foreign writer under a warm activation aborts→invalidates→the
> retry refolds with no hole and no duplicate); **standalone adapter workers gated
> `RUN_MAILBOX_WORKERS` default false** (each adapter binary can run the monolith-identical
> fleet for its own lanes; OFF because the in-process status/event buses mean adapter-delivered
> facts never reach monolith push subscribers — LISTEN/NOTIFY is the recorded follow-up; E2E
> `standalone_workers`); **birth id-minting unified** (a declared identity property that fails
> to parse errors at the GraphQL door like the worker door — never a silent random lane).
> Stale `inbound_events` narratives in integration_staging.yaml + the SIRENE worker rewritten
> to `inbound_messages`.
> ✅ **D3 review round 2 (2026-08-01, full-branch, three lenses): 1 critical + 4 major, all
> FIXED** — the activation FRESHNESS GUARD (a cache-served delivery re-asserts the stream
> version in the fenced tx: non-append verdicts had no UNIQUE race to lose, so a stale hold
> could durably commit a wrong REJECTED — E2E `stale_hold_cannot_commit_a_wrong_rejection`);
> fill-epoch TOCTOU fence; deletion engine evicts erased streams from the cache; standalone
> money lanes REFUSE an unset PM_MAILBOX_DELIVERY (+ adapter-side backfill parity); Stripe 409
> in-flight idempotency conflicts retry instead of terminally failing a stolen-lane checkout;
> the backfill advances the frozen pm:* checkpoints (no more O(history) restart re-scans).
> Minors: mb-activations-shape negative tests, adapter graceful HTTP shutdown, spec-default
> reminder windows in standalone fleets, SIRENE success-is-enumerated verdict SQL,
> RUN_MAILBOX_WORKERS out of the server Config (`consumer`). Details in the proposal's review
> round 2 section.
> ✅ **#273 MERGED to `main` (2026-08-01, squash `735adbf`, CI green incl. the DB suites) with
> D1 + D2 + D3 ALL COMPLETE** — the three "LANDED on the #272 branch" entries above are on
> `main`. [#275](https://github.com/TheCaptainCompany/captain-food/issues/275) was opened on the
> mistaken premise that only D2 merged (corrected at claim time — a post-merge content diff
> shows `main` strictly ahead of the branch); its real remainder is ADR-20260730-234918 **test
> port 4** (mailbox discipline suite) on `275-runtime-d1-r2-payment-flip`. The
> `PM_MAILBOX_DELIVERY` default flip (+ `command_journal` DROP + runner-group retirement riding
> that deploy) stays gated pending staging smoke — its own one-line ADR.
> 🚧 Remainder (slices 2+3+4 + supervision API/page) CONSOLIDATED on `242-actor-mailbox-runtime`
> (product-owner directive, 2026-07-31: one branch, tests throughout; migrations ride the branch —
> they only APPLY at the manual deploy, ADR-20260730-051500).
> ✅ **THE RESOLVER FLIP IS ON THE BRANCH (2026-07-31, Runtime C3a)**: aggregate-routed mutations
> now ENQUEUE on `inbound_messages` and answer PENDING — the per-actor-type `MailboxWorker`s
> (crates/actor_runtime: leases, `ownership_version` fencing inside the completion transaction,
> head-of-line drain, staged-event flush) deliver through the GENERATED command router (82 arms
> from the same table as the resolvers) and publish terminal status post-commit. The acceptance
> contract is proven unchanged over the mailbox (duplicate replay / payload conflict / session
> scope — `graphql_write_path` green); `operationStatus(+Changed)` reads mailbox-first with the
> journal as pre-flip/PM-leg fallback. PM legs (placeOrder, approveRefund, denyRefund) stay on
> journal+spawn until PM mailboxes (Runtime D). Remaining C3b: worker-channel flip (SIRENE/HubRise
> `dispatch_journaled` → mailbox), adapter inbox → kind EVENT rows, backfill + legacy drop.
> 36 DB suites green on a local PG16 under `DB_TESTS_REQUIRED=1`; `make rust` green.
> ✅ **PR #270 review fixes (2026-07-31, branch `claude/pr-270-review-ajxr9o`)** — the five-lens
> review of [#270 "actor mailbox runtime"](https://github.com/TheCaptainCompany/captain-food/pull/270)
> found 6 criticals; all fixed with regression gates: C1 dropped shutdown sender = zero-sleep
> busy-loop workers (now: held sender + SIGTERM drain + supervisor respawn + `changed() Err` =
> no-signal); C2 `position > checkpoint` drain filter strands late-committing rows after takeover
> (now: `status = 'RECEIVED'` alone defines undelivered; checkpoint = high-water mark only); C3/C4
> transient handler errors and flush version conflicts landed TERMINAL and the enqueue pk-dedupe
> then absorbed Stripe's own retries = permanently lost payment facts (now: abort-and-retry; only
> deterministic outcomes are terminal); C5 the deployed `sweep_retention()` still swept the dropped
> `inbound_events` (now: the drop migration redeploys the function, adds the `inbound_messages`
> window, and `retention_sweep.rs` tests the REAL spec function via include_str — never a mirror);
> C6 the kind-EVENT route never published on the event bus = `paymentStatusChanged` dark (now:
> shared fan-out with the COMMAND route). Plus: mid-drain lease renewal, per-lane error
> containment, enqueue→worker Notify nudges (delivery latency ~10 s → ~immediate), RIDER
> `requires` deny closed (+ `TestRiderPostDenied`), HubRise connect awaits the account leg's
> terminal verdict before dependents, backfill migration gains a write-fence + straggler guard,
> and the stale `inbound_events` spec narratives are rewritten.
> ✅ **Runtime B on the branch (2026-07-31): the actor-supervision surface is live end to end** —
> ADMIN `mailboxLanes` query (api.yaml + story step), the `system.yaml` SDUI surface (first ADMIN
> screen set, `/system/mailbox` lanes page + `system.translations.yaml` sidecar), the
> `20260731063000_actor_mailbox_tables.sql` migration (inbound_messages + mailbox_partitions with
> the drain/scheduler partial indexes — pulled forward from slice 3 so the surface is DB-testable;
> NOTHING writes them until the worker flip), `MailboxLaneRepository` port + Pg lateral-join adapter
> + composition-root wiring, and a DB-gated test that applies the REAL migration file and proves
> counts + ADMIN guard + BIGINT-as-string serialization (verified locally against a real PG16:
> full migration chain from scratch + every DB suite green under `DB_TESTS_REQUIRED=1`).
> Realization starts with [#242 "Write path becomes an actor mailbox…"](https://github.com/TheCaptainCompany/captain-food/issues/242)'s
> foundation slice (claimed, draft PR per protocol); [#235](https://github.com/TheCaptainCompany/captain-food/issues/235)
> and [#267](https://github.com/TheCaptainCompany/captain-food/issues/267) follow. Open veto flag:
> `messages.yaml` as the third payload catalog.

> 🚨 **2026-07-31 — HOSTING MIGRATES TO OVH (ADR-20260731-061609, product-owner decision).**
> Render + Supabase limitations are exhausted (bandwidth/build/disk; Disk-IO budget) and the costs
> do not match the project. **Supabase is kept for IDENTITY ONLY.** The cutover uses the current
> outage: final dump → OVH restore → ALL pending migrations (incl. enum-text) → deploy → smoke →
> DNS; **Render is never resumed** — the "once the Render workspace is restored" runbook below is
> SUPERSEDED by [PROP-20260731-061609 §5](../proposals/PROP-20260731-061609-ovh-migration.md).
> Tracking: [#271](https://github.com/TheCaptainCompany/captain-food/issues/271). #242 slice 3's
> prod-gate becomes "OVH cutover complete".

> 🚧 **2026-07-30 — pipeline isolated: build (auto) / deploy (manual) / migrate (after deploy) —
> ADR-20260730-051500 (product-owner directive).** Render is paused (outbound bandwidth exhausted), which
> exposed the hazard in migrate-on-green-ci: the enum-text schema conversion would have applied underneath
> an old binary no deploy could replace (the first attempt already failed on disk space —
> [#264 "fix: split the enum-text migration so it fits production's disk"](https://github.com/TheCaptainCompany/captain-food/pull/264)
> replaced it with the lean split set). `build-image` now only pushes to GHCR; the NEW manual `deploy`
> workflow is the only thing that touches Render (digest-pinned, `tag` input for rollback); `db-migrate`
> follows `deploy` instead of `ci`. **The enum-text release is merged but NOT live**: once the Render
> workspace is restored — (1) dispatch `deploy` (tag `sha-db738ad` unless a newer image exists), (2)
> `db-migrate` follows automatically and applies `20260730043000`–`0436`, (3) run `prod-smoke`.

> 📋 **2026-07-30 — Uber Eats Marketplace is a NEW integration, and it is specified now rather than
> discovered later ([#260](https://github.com/TheCaptainCompany/captain-food/issues/260),
> PROP-20260730-032306, ADR-20260730-032306).**
> The product owner registered **Captain Food Restaurant** on the Uber **Eats Marketplace** suite and
> accepted the API Licensing Agreement with all seven APIs — a real commercial commitment to an
> integration the specs did not contain. Note the three distinct Uber concerns the repo now holds:
> Uber **Direct** = delivery (`crates/adapters/uber_direct`, ✅ #57); Uber Eats **price comparison** =
> display only (ADR-0022/0023/0024/0025/0030, ✅); Uber Eats **Marketplace** = order centralization +
> menu sync (📋 new, nothing built).
> **Decided** (ADR-20260730-032306): app auth is **asymmetric** (application id + key id + private key,
> retiring `UBER_DIRECT_CLIENT_SECRET`/`SCOPE` and its token manager); private keys stored **base64**
> so a mangled PEM fails validation rather than first-signature; webhook HMAC accepts **either** of two
> signing keys so rotation never drops an order notification; **two Uber Direct organizations** split by
> acquisition surface (storefront first); delivery channels keyed `uber_direct:<surface>` so an
> unconfigured surface is an *unwired channel* that times out and escalates rather than dispatching on
> the wrong org's credentials; per-tenant values (Uber store ids, merchant consent) live in
> `uber_eats_connections`, never in configuration.
> **This forces two things into the open.** The catalog would flow **outbound** for the first time
> (today it only ever flows in, HubRise → `ImportCatalog`), raising menu ownership and price parity —
> restaurants mark Uber prices up to absorb Uber's commission, which is exactly what ADR-0024's
> comparison coefficients assume. And an Uber-originated order **was already paid, on Uber's rails**,
> while `OrderPlaced` implies a Captain PaymentIntent — a money assumption, so it pairs with the payout
> posture in DECISIONS §1.
> **Contractual, not optional**: the Order API clause makes the Provider *"wholly responsible for
> correctly relaying all information … including but not limited to allergy information and special
> instructions"* — with EU FIC 1169/2011 that becomes a `rules.yaml` rule with a test. The Reporting API
> needs a per-restaurant consent record. And licensed data serves the merchant *on Uber*: it must never
> seed the Captain marketplace catalog.
> **Open** (DECISIONS §11): D4 order representation · D5 menu ownership/parity · **D7 — the agreement
> was signed by *Caring Hope Foundation* (RNA W372020229, a loi-1901 association), not
> `TheCaptainCompany`; an API licence follows the entity, so this needs legal input.** Nothing is built
> yet: no adapter, no `UBER_EATS_*` keys declared (deliberately — a declared key with no reader is drift
> too). Five `UBER_EATS_*` repository secrets exist on the GitHub side, `_TEST`-suffixed.

> ✅ **2026-07-29 — the observability contracts finally leave the repo: OpenTelemetry to Honeycomb EU
> ([#191](https://github.com/TheCaptainCompany/captain-food/issues/191), PROP-20260726-170500 D1+D2,
> ADR-20260729-183000).**
> `specs/observability.yaml` had reached 898 lines of contracts — required spans, run identities,
> attributes, metrics and SLOs across eleven workflows — and **none of it was emitted**: no
> `opentelemetry`/`tracing` dependency, no subscriber, and 69 `println!` calls. `correlation_id` and
> `trace_id` are *mandatory* in every contract's `run_identity` and neither existed at runtime, so on the
> acceptance-first write path the whole async half of a command (handler, event append, Stripe call,
> projection) ran with nothing tying it to the request that caused it.
> Now: **`crates/telemetry`** (a new leaf crate) exports OTLP/HTTP to **Honeycomb, pinned to `eu1`** —
> a **GDPR constraint, not a default**, since spans carry `customerId`/`orderId` and ADR-0042 pinned data
> to Frankfurt. The `command-acceptance` contract's three spans + four metrics are emitted from **every**
> generated mutation resolver (via the codegen, not hand-written), and the `place-order` boundaries are
> instrumented: `event.store.append`, `event.publish` (per envelope), `event.consume.projection` (per
> projector) and `payment.intent.create`. Logging is structured/levelled/correlated throughout.
> **Telemetry degrades, never gates**: no telemetry key is `required:`, so a missing ingest key drops the
> exporter and keeps logs rather than refusing to serve orders — the deliberate opposite of a missing
> payment secret, which must stop the boot. The boot report distinguishes `exporting` / `logs-only` /
> `exporter-unavailable`, because an operator who thinks traces are flowing when they are not loses the
> first ten minutes of an incident.
> **D2 answered but NARROWED, against the recommendation**: parent-based **head** sampling at `1.0`, not
> tail-based — tail sampling needs Refinery (a service to run and pay for), contradicting ADR-0042's
> minimal-ops-pre-PMF stance, and D2's own reasoning says the volume is not there yet.
> Layer rule, now **enforced by a dependency test**: `domain` gets neither the OTel SDK nor the `tracing`
> facade; `application` gets the facade only. *It may say things; only boundaries may measure them.*
> A second test reads `observability.yaml` and asserts every required span/attribute/metric of the two
> named contracts is really constructed. **Both guards were validated by breaking them**, which caught two
> vacuous passes (a span rename satisfied by a `#[cfg(test)]` literal; an attribute rename satisfied by a
> substring prefix) — a guard is finished when it has been seen to fail, not when it passes.
> **Known remaining**: the other **nine** contracts are still unemitted; `payment.intent.create` records
> `created`, not the contract's `captured` (capture is an inbound webhook fact, and conflating them would
> make a created-but-never-captured payment look successful); and trace **retention / GDPR erasure reaching
> Honeycomb** is unresolved, belonging with PROP-170000's erasure work.
> [#179](https://github.com/TheCaptainCompany/captain-food/issues/179) (GraphQL hardening) and
> [#193](https://github.com/TheCaptainCompany/captain-food/issues/193) (advisory locks + the missing index)
> are untouched, so PROP-170500 **D3/D4/D5 remain open**.

> ✅ **2026-07-29 — watchdog: `render-config-sync` dry-run fixed at the source (`limit=200 -> 100`).**
> [#252](https://github.com/TheCaptainCompany/captain-food/issues/252) hardened the env-vars parser to be
> shape-agnostic and to fail loud, but kept `?limit=200` — which is the actual cause. Render's env-vars
> endpoint caps page size at 100 and rejects `limit=200` with **HTTP 400** `{"message":"invalid limit:
> too large"}`; that error object (not a real env-vars shape) is what the parser then read as "an object
> wrapper", finding 0 vars and exiting 1. So `main` was still failing the dry-run. Fix: `limit=200 -> 100`
> (the service has ~10 keys, one page covers all), verified against the live Render API — the read now
> returns all 10 vars and the whole dry-run loop runs with zero jq errors, exit 0. `prod-smoke.sh` already
> used `limit=100`, so no other reader was affected. CI-config only; no `specs/**` or generated files touched.

> ✅ **2026-07-29 — configuration RIDES THE ARTIFACT; secrets ride CI; the dashboard owns nothing
> ([#248](https://github.com/TheCaptainCompany/captain-food/issues/248), PROP-20260729-014500,
> ADR-20260729-020000).** All five decisions approved in-session.
> #246 declared configuration; it did not give it an OWNER — values were still typed into the Render
> dashboard, which is how `RUN_SIRENE_WORKER` gated a paused pipeline while written down nowhere and
> `API_SECRET` sat on the service read by nothing. The product owner's question — *"is it possible to
> configure the deployment, not the Render service?"* — reframed it. Render has **no per-deploy env
> override** (its deploy API takes only clearCache/commitId/imageUrl/deployMode), so attaching config to
> the deployment means putting it **inside the artifact**. Now: **non-secret values are BAKED** into the
> binary per profile by the codegen — the digest determines behaviour, and a rollback restores the
> configuration that shipped with that build; **secrets are pushed by CI** from GitHub repo secrets to
> the service env (never baked — the GHCR package is PUBLIC, so a baked `ENV` is world-readable); and
> **`APP_PROFILE` stays service env**, since one image is promoted across environments by digest and
> baking the selector would be circular. Precedence: env var > baked > default, so an operator keeps a
> seconds-fast override for incidents.
> The sync workflow (`render-config-sync.yml`) is **upsert-only** (it cannot delete, so a bad manifest
> can never wipe config; undeclared keys are REPORTED) and **dry-run by default** (it cannot be tested
> outside CI, so its first real run would otherwise be an untested write against live production).
> Validator-enforced: a secret may never declare baked values; a baked value must satisfy its scalar;
> `APP_PROFILE` may not be baked. **Consequence to know**: pausing a pipeline is now a PR + build
> (~minutes), not a dashboard edit — for a flag that stops a production pipeline, reviewed and recorded
> is the point. **Still manual by design**: the first `apply: true` run, and setting
> `APP_PROFILE=production`, which is what arms fail-fast.

> ✅ **2026-07-29 — configuration is DECLARED in the DSL and validated at startup
> ([#246](https://github.com/TheCaptainCompany/captain-food/issues/246), PROP-20260729-004500,
> ADR-20260729-010500).**
> Product-owner directive, approved in-session (*"Fail-fast: approved"*). Configuration was the one part
> of this system with no source of truth — ~21 env vars existing only as scattered `env::var` calls plus
> a stale, unapplied `render.yaml` mirror of 9. That gap is what let `RUN_SIRENE_WORKER` gate a paused
> pipeline while being written down **nowhere** (6,649 rows PENDING for 4h), left `API_SECRET`
> configured on production and read by nothing, and made an unset `STRIPE_WEBHOOK_SECRET` silently
> produce the worst failure this product has (payment captured, domain never told).
> Now: **`specs/configuration.yaml`** declares every key — type, per-profile `required`, `default`,
> `secret`, `consumer`, and **`gates`** (what breaks without it, *printed* in the failure report, so a
> key without one fails validation). Codegen emits the typed reader; startup reports **every** missing
> required key with its purpose and exits `78` (`EX_CONFIG`); a boot report shows what resolved —
> secrets as `set`/`unset`, `STRIPE_SECRET_KEY` additionally as **test/live mode**. The rule that keeps
> it honest is a **drift test**: every `env::var`/`env_flag` call site in `crates/**` must be declared,
> or the build fails — it immediately caught three undeclared `sirene_ingest` keys, and a sixth `RUN_*`
> toggle (`RUN_DELIVERY_OFFER_TIMEOUT`) still on the old strict parsing.
> Reconciles with ADR-0043 rather than contradicting it: **missing configuration cannot self-heal
> (refuse to start); an unavailable dependency can (start, report 503)**. On Render this is strictly
> safer — an exiting container fails the deploy, so a misconfigured build cannot replace a working one.
> **Values are TYPED too** (product-owner directive, same day): each key binds a `scalars.yaml` scalar
> whose `pattern` the reader enforces at startup — *present is not usable*. `ConfigBoolean`
> (true/yes/1/on, case-insensitive), `StripeSecretKeyTest`/`-Live` (a LIVE key in the test slot is now a
> startup failure, not a way to move real money), `StripeWebhookSecret`, `AuthSessionKey` (32 bytes hex
> or base64 — a 31-byte key no longer silently disables login), `PostgresUrl`, `HttpsUrl`,
> `DepartmentList`. The report groups **MISSING** (absent) and **INVALID** (malformed) separately —
> different problems, different fixes — and a secret's value is never printed, only its expected shape.
> **Enforcement follows the PROFILE**: production and staging STOP, development reports and continues.
> The warn-only rollout was dropped rather than deferred: it hedged against a first enforced deploy
> failing, but an exiting container fails the DEPLOY and the previous version keeps serving, so the
> feared outcome is the desired one. Deferred by design: injecting `Config` into `router()` (the drift
> gate already makes every read *declared*, just not yet *injected*) and the presence-only `/config`
> endpoint (PROP D4).

> ✅ **2026-07-28 — the SIRENE mirror's disk is RECLAIMED (655 MB → 14 MB), department 37 is re-swept,
> and every background loop now publishes readiness
> ([#238](https://github.com/TheCaptainCompany/captain-food/issues/238) /
> [#244](https://github.com/TheCaptainCompany/captain-food/issues/244), ADR-20260728-224500).**
> Product-owner decision: **`TRUNCATE external_sirene_restaurants`** rather than compact-and-re-sync in
> place. The mirror is a cache of INSEE (the system of record), nothing domain-side reads it, and the
> only designed dependency on row existence — detect-by-absence — is bounded to prospects and recovered
> by [#243](https://github.com/TheCaptainCompany/captain-food/issues/243) once France is re-swept. The
> truncate returned ~655 MB (table + indexes + TOAST) to the OS **instantly**: no `VACUUM FULL`, no
> dead-tuple churn, and it collapsed most of the #238 runbook. Measured after re-sweeping Tours:
> **6,649 rows / 14 MB** (of which 9,727 kB is payload still awaiting release — steady state ~4 MB).
> The `payload_hash → bytea` migration (PROP-20260728-120931 D2) is now trivial and should land BEFORE
> France repopulates.
>
> The pilot then exposed two operational holes, both now fixed in code (#244): the SIRENE worker was the
> **one in-process loop with no status endpoint** — 6,649 rows sat `PENDING` for four hours and nothing
> outside the process could tell a paused loop from a crashing one — and its `RUN_SIRENE_WORKER` gate was
> an exact `== "true"`, so `TRUE`/`True`/a quoted value silently meant PAUSED. Now `GET /sirene` joins
> `/projector` and `/saga` (`running`/`lastTickAt`/`lastError`/`lastSummary`, with `503` +
> `poll_loop_not_started` vs `sirene_worker_not_available` naming WHICH stopped state it is), and all
> five `RUN_*` toggles share one lenient parser (`true/1/yes/on`, `false/0/no/off`, case-insensitive,
> trimmed, unrecognised → documented default **and a log line**). Note `RUN_INBOUND_DRAIN=0` now means
> OFF (the old `!= "false"` read it as ON). Still config, not code: `INTERNAL_TRIGGER_URL` /
> `INTERNAL_TRIGGER_TOKEN` are unset in BOTH the CI secrets and the Render env, so
> `POST /internal/sirene/drain` answers `503 internal trigger not configured` — until they are set,
> `RUN_SIRENE_WORKER=true` is required and sync latency is the 1-hour poll.

> ✅ **2026-07-28 — enum columns now store the TEXT value verbatim; the `ref_<enum>` lookup tables are
> gone (ADR-20260728-170000, product-owner directive; supersedes the ADR-0037 ordinal scheme).**
> Every enum-typed column (projections, PM state, journals, `domain_events.user_type`, referential
> seeds) is TEXT holding the `scalars.yaml` value (`'PLACED'`, `'EXTERNAL'`, …), so rows are
> self-describing and declaration order is no longer a frozen storage contract. The codegen emits TEXT
> DDL, no ref tables, and text fold-views; `enum_sql` is now `EnumText` (enum ↔ variant-name string);
> the envelope's `user_type` travels as text end to end; hand-written SQL and the DB test suites
> compare values (`status = 'FAILED'`). The conversion ships as the split `20260730043000`–`0436` set:
> `VACUUM FULL` the SIRENE mirror first (its transient-payload dead space was most of the 2 GB disk),
> then one transaction per table group with the CASE folded into `ALTER … USING` (single rewrite, no
> UPDATE pass) and the big tables each alone — the original one-transaction migration rewrote every
> table at once and died on production's disk ("no space left on device", clean rollback). Verified
> locally end to end (old-schema + ordinal data → split set → correct text values; fresh-DB run + the
> full DB-gated suites green on Postgres 16).

> ✅ **2026-07-28 — the pre-#227 syncs were journaled, so compaction can now CONFIRM them; CI runs the
> DB suites; the SIRENE worker tests assert the real contract
> ([#238](https://github.com/TheCaptainCompany/captain-food/issues/238) /
> [#230](https://github.com/TheCaptainCompany/captain-food/issues/230) /
> [#236](https://github.com/TheCaptainCompany/captain-food/issues/236)).**
> Product-owner correction on the #240 consequence: reclaiming the historical 655 MB does NOT have to
> wait for the sweep to resume, because the retired command path recorded its verdicts — every pre-#227
> sync is a `command_journal` row with a deterministic message_id (UUIDv5 over command type + SIRET +
> the staged version's `last_seen_at`) and a SUCCEEDED/REJECTED verdict + `completed_at` written by the
> dispatch. The compaction gained a **journal arm** that transcribes those verdicts (`SYNCED`,
> `synced_at = completed_at`, payload dropped, one statement); rejected/missing/stale-version verdicts
> stay `left_unconfirmed` and fall back to re-sync. **The evidence expires**: `sweep_retention()` deletes
> terminal journal rows after 90 days, so run `mode: compact` before the verdicts age out.
> Alongside it, CI got a real Postgres (#230): the DB-gated integration suites now RUN (migrations
> applied, `--test-threads=1`), and a skip is LOUD when `DB_TESTS_REQUIRED` is set instead of reporting
> `ok` while executing nothing. The three stale worker tests (#236) were rewritten against the
> post-#227 contract (inbound fact → real `InboundEventsDrainWorker` delivery → verdict reconciled) —
> and immediately caught a real bug: the worker staged the BARE `RestaurantRegistered` payload while the
> drain deserializes the adjacently-tagged `DomainEvent` form, so **every staged registry fact was
> undeliverable** ("missing field eventType" → FAILED). Fixed at the staging site; exactly the class of
> drift a silently-skipping suite exists to catch.

> ✅ **2026-07-28 — a payload is now removed ONLY against recorded evidence of a successful sync
> ([#231](https://github.com/TheCaptainCompany/captain-food/issues/231)/[#238](https://github.com/TheCaptainCompany/captain-food/issues/238); PR #240).**
> Product-owner correction, and it caught a real flaw. The first implementation removed payloads on an
> INFERENCE: the compaction read `processed_at >= last_seen_at` as "already translated", wrote `SYNCED`
> itself, then deleted the payload on the strength of its own decision — and the worker deleted it at
> hand-over, before the aggregate had decided anything. But `processed_at` is a CHECKPOINT, not a verdict
> (the worker advances it for unmappable rows and failed writes; the ingestion advances it again on
> unchanged ones), so certainty was being derived from a column that never carried it — for an
> irreversible delete whose only recovery is a ~4h INSEE re-fetch. **The rule is now `status = 'SYNCED'
> AND synced_at IS NOT NULL`** — two independent witnesses, both written by the code that observed the
> fact. The register path drops the payload in `reconcile_staged` (same statement as the verdict), the
> closure path at mark time (the command has executed); `STAGED`/`FAILED`/`POISON`/`UNMAPPABLE` and
> pre-`status` rows all keep theirs. Note the inbound row's copy is the TRANSLATED form — exactly what is
> in question if the ACL mistranslated — so the raw staging payload is the only original.
> **Consequence: the historical 655 MB is reclaimed by RE-SYNCING, not by compaction.** Pre-#231 rows
> keep the hash sentinel, so the first sweep re-pends each exactly once (as migration `20260728040000`
> already documented), and the payload is released on confirmation. Compaction reports `left_unconfirmed`
> so "nothing left to do" cannot be confused with "nothing is confirmed yet". Silver lining: it no longer
> classifies anything, so the ACL gap from running it in CI is gone.

> ✅ **2026-07-28 — the SIRENE compaction is now RUNNABLE against production
> ([#238](https://github.com/TheCaptainCompany/captain-food/issues/238); PR #239).** `sirene_ingest --compact`
> shipped with the change below, but nothing could invoke it: `DATABASE_URL` lives only in CI secrets and
> the `sirene-sync` workflow only ever ran `--once`. A capability that exists and cannot be reached is
> not a capability. The workflow's `workflow_dispatch` now takes `mode` (`sweep` | `compact`, default
> `sweep`), plus optional `budget_minutes` and `departments` — blank meaning "binary default", so an
> untouched form behaves exactly as the schedule does. **Compaction is unaffected by the SIRENE pause**:
> it reads payloads already in staging, makes no INSEE calls and never pings the worker — the pause is
> about the sweep's write-path cost, while compaction is what makes national coverage affordable.
> Expect to run it several times (budgeted + resumable; re-run until `compacted` is 0), and note the
> table will NOT shrink from this alone — plain `VACUUM` makes the space reusable, only a later
> `VACUUM FULL` returns it to the OS, and that becomes affordable only afterwards. **Still not run**:
> [#238](https://github.com/TheCaptainCompany/captain-food/issues/238) carries the ordered runbook
> (compact -> `VACUUM FULL` -> `bytea`) and dropping payloads is irreversible without a ~4h re-fetch, so
> triggering it is a product-owner call.

> ✅ **2026-08-02 — `main` DELIVERED to production, and the #231 lifecycle validated against live INSEE
> data.** `becf202` is running; migrations applied through `20260731143000` (mailbox + enum-text). The
> transient-payload design ran for the first time against real records, and the measured numbers match
> the proposal almost exactly: **196 bytes per SYNCED row vs 1,730 per PENDING one** (PROP-20260728-120931
> predicted ~200 B vs ~1.8 kB). Every state behaved as designed — `SYNCED` rows hold **zero** payloads,
> `STAGED` rows **keep** theirs (the [#240](https://github.com/TheCaptainCompany/captain-food/pull/240)
> correction: the aggregate has not decided yet, so nothing may be discarded), `UNMAPPABLE` rows keep
> theirs as evidence, and **no row reached `FAILED` or `POISON`**. The mailbox split
> `IGNORED 2,923 / SUCCEEDED 47` is ADR-20260728-011344 D6 paying off in production: the sweep can now
> distinguish "registered 47" from "did nothing 2,923 times", which is precisely what it could not do
> before. Coverage is rebuilding — 9 departments, 67k rows and climbing.
>
> ⚠️ **The delivery itself exposed two defects, both now fixed, plus one still open.**
> (1) `REQUIRED_SCHEMA_VERSION` had gone **9 migrations stale**, making `/health`'s readiness gate inert
> for exactly the migrations that needed it ([#279](https://github.com/TheCaptainCompany/captain-food/pull/279)).
> (2) Generated config pattern literals were **double-escaped** — escaped for a normal Rust string and
> emitted into a raw one — so the app rejected its own baked valid default
> (`OTEL_TRACES_SAMPLE_RATIO=1.0`). Harmless on the `development` profile, but **production and staging
> refuse the boot on an invalid key**, so this was a latent production-boot blocker that only stayed
> hidden because production runs the development profile
> ([#280](https://github.com/TheCaptainCompany/captain-food/pull/280)).
> (3) **STILL OPEN — [#281](https://github.com/TheCaptainCompany/captain-food/issues/281):** `deploy` is
> fire-and-forget, so `db-migrate` converted the schema underneath a binary that never arrived.
> Production ran an **11-day-old build (222 commits behind) against a schema 9 migrations ahead** for
> several minutes, workers erroring in a loop. Nothing was lost (0 unprocessed webhooks) only because
> traffic was near zero.
>
> **Known production gaps, unchanged by this delivery:** the service runs the **development** profile
> (which is why the config error above was survivable); `SUPABASE_URL`/`PUBLISHABLE_KEY`/`JWKS_URL` are
> unset so identity fails closed and auth is anonymous-only; and startup shows connection-pool
> contention (2-3.5 s acquires, a 1.1 s `MAX(position)` on `domain_events`) as 16 mailbox workers plus
> the projector, saga runner, retention sweep and SIRENE worker all start at once.

> ✅ **2026-07-28 — the SIRENE mirror now records whether a row actually SYNCED, and quarantines the ones
> that cannot (ADR-20260728-143000 follow-up, [#231](https://github.com/TheCaptainCompany/captain-food/issues/231); PR #237).**
> Follow-up to the transient-payload change below, from three product-owner observations, each of which
> turned out to be a real hole. (1) **`status` was claiming too much.** Since ADR-20260728-011344 the
> register path STAGES an inbound fact and the aggregate decides later, so at hand-over the worker does
> not know whether the record was accepted — marking it `SYNCED` there asserted a success nobody
> observed. There is now a `STAGED` state, resolved on a later drain by joining `inbound_events` on the
> key the ACL already writes (`external_id = '{siret}:{payload_hash}'`, both halves being columns on the
> staging row — no new bookkeeping). `DELIVERED`/`IGNORED`/`DUPLICATE` all resolve to `SYNCED` (a
> no-change verdict is a real answer, not a failure), `FAILED` surfaces as `FAILED`, `RECEIVED` is left
> in flight. (2) **`processed_at` is not a sync time** — it is a checkpoint the ingestion also advances
> on unchanged rows — so `synced_at` (wall clock, survives a re-pend) and `last_attempt_sync_at` (every
> attempt) now exist alongside it. (3) **A failed sync retried forever.** It deliberately leaves the row
> pending WITH its payload, so nothing excluded a permanently-broken record — the 605-row
> `SlugAlreadyTaken` log storm was exactly this shape. `attempt_sync_retry_count` counts CONSECUTIVE
> failures (resetting on any checkpointed outcome, which is what makes it answer "stuck *now*?") and at
> **10** the row becomes `POISON` and the drain skips it. Recovery needs no operator: a CHANGED record
> re-pends the row through the ordinary conflict arm, which writes `PENDING` and releases the quarantine
> — so quarantine holds exactly as long as the record keeps arriving unchanged and broken. Migration
> `20260728160000` (separate from `20260728050000`, which is merged and may be applied — forward-only),
> `REQUIRED_SCHEMA_VERSION` bumped.

> ✅ **2026-07-28 — the SIRENE mirror's payload is now TRANSIENT: ~1.8 kB/row → ~200 B/row
> (ADR-20260728-143000, [#231](https://github.com/TheCaptainCompany/captain-food/issues/231); PR #234).**
> `external_sirene_restaurants` kept the verbatim INSEE record forever to read five fields out of it:
> measured on production, **655 MB for 339k rows — 77% of the whole database** — at department **37 of
> 101**, on a **2 GB disk with ~580 MB free**. Full France is ~2 GB for that one table, so this — not
> pacing — is what gated national coverage ([#218](https://github.com/TheCaptainCompany/captain-food/issues/218)
> made the sweep capable of it; disk did not follow). The fix is a lifetime distinction: the **payload is
> an input to translation** (needed from the moment INSEE reports a change until the worker turns it into
> a domain fact, never again), the **hash is the change-detection key** (needed forever). So the payload
> lives exactly while a row is pending — the ingestion writes it only when the row will pend, and the
> worker NULLs it in the SAME statement that advances the checkpoint. A record the ACL could not map
> KEEPS its payload: it is the only evidence of why INSEE's record was unusable. One-shot compaction of
> existing rows ships as `sirene_ingest --compact` (batched, `VACUUM` interleaved, resumable —
> `payload IS NOT NULL` is its own progress marker), recomputing each real hash BEFORE dropping the
> payload, because every row still carries the `unhashed-pre-20260728` sentinel and dropping payloads
> under it would re-pend all 339k rows and re-write all 655 MB. **Two things to know before reading the
> production numbers:** (1) a plain `VACUUM` makes space reusable but does NOT shrink the file — the
> table stays ~655 MB until a `VACUUM FULL`, which only becomes affordable AFTER compaction (live data
> ~90 MB vs the ~620 MB that made the earlier attempt fail with `No space left on device`); (2) the
> `bytea` hash change (D2, approved) is deliberately NOT in this change — `ALTER … TYPE` rewrites the
> whole table and would fail the same way, so it follows compaction. Compaction runs in the CI job by
> product-owner choice, which means historical ACL-unmappable payloads are dropped (the crate has no
> ACL); D3 holds going forward via the worker. **A `status` column lands with it** (product-owner
> addition): making the payload transient would otherwise leave the table ambiguous — a row that HAS a
> payload is either awaiting translation or kept as evidence, and nothing told them apart. `PENDING` /
> `SYNCED` / `UNMAPPABLE` / `FAILED` answers "has this been synced?" directly instead of by inference
> from `processed_at >= last_seen_at` (which stays the concurrency-safe checkpoint); `GROUP BY status`
> is the per-sweep report. TEXT, not a scalar enum, because the CI crate that writes it cannot see
> domain types (ADR-0045) and would have to hardcode ordinals. Migration `20260728050000`,
> `REQUIRED_SCHEMA_VERSION` bumped. SIRENE stays **paused** — this makes the mirror affordable, it does not resume the sweep.

> ✅ **2026-07-28 — `prod-smoke` back to green: the fixture now sets its slug via `configureRestaurantSlug`
> (watchdog fix).** The daily `prod-smoke` run went red at L3 with `unknown field "slug" of type
> "RegisterRestaurantInput"`: the slug split out of registration into a separate `ConfigureRestaurantSlug`
> command (ADR-20260728-011344, [#225](https://github.com/TheCaptainCompany/captain-food/issues/225))
> left `tools/smoke/prod-smoke.sh` registering with a field the schema no longer has, and — because the
> existing fixture's slug stopped resolving after the projection change — no way to reach its tenant host.
> Fixed by registering without `slug` and issuing `configureRestaurantSlug(restaurantId, slug)` right
> after (same aggregate, so write-side ordering holds; the existing projection-by-slug wait now observes
> the slug becoming resolvable). Verified against live prod: L1-L3 PASS (fixture repaired, `smoke-test`
> resolves ACTIVE with its offer). L4 (money path) needs `sk_test` and runs in CI; the repaired fixture
> means the next scheduled run short-circuits L3 and exercises L4.

> ✅ **2026-07-28 — `idempotent_on_existing` is GONE, and `sirene-sync` has an observability contract
> (ADR-20260728-011344, [#220](https://github.com/TheCaptainCompany/captain-food/issues/220); PR #229).**
> The last of the six slices. All five remaining creation handlers
> (`register_restaurant_account`, `register_restaurant`, `place_replacement_order`, the checkout
> payment-intent, `create_catalog`, `verify_phone`) used to answer *"does this aggregate already
> exist?"* by ATTEMPTING the append and reading the resulting `UNIQUE (stream_name, version)` violation
> as success. Postgres writes the heap tuple and index entries **before** the constraint fires, so every
> no-op left dead tuples in the largest table — and the caller could not tell a real creation from a
> no-op, which is exactly how **`verify_phone` came to report `created: true` for customers who already
> existed**, on a live identity flow. Replaced by `create_if_absent`, which asks before writing and
> answers aggregate-agnostically (an empty stream is version 0 — "does this stream exist" is not a
> domain question, so no fold is needed). A version conflict is no longer swallowed: reaching one now
> means a genuine race, reported as `Created::No` and left visible. `Repository::create` deleted rather
> than left as a trap. Two tests pin the two properties that were lost: the caller can tell creation
> from no-op, and a no-op **appends nothing**. Plus the `sirene-sync` observability contract
> (`specs/observability.yaml`) — the project's own rule is that every critical workflow has one, and
> this one writes to the event store on a loop with nobody watching. Its four business counters
> (created / updated / ignored / failed, plus `event_store_version_conflicts_total`) make *"did this
> sweep do anything, and was it what we meant?"* answerable without reading logs. **#220 is complete in
> code.** ⚠️ Note the standing caveat before resuming SIRENE: the staging SQL is still not exercised
> locally or in CI (`DATABASE_URL`-gated tests skip in both), so the first sweep wants watching. Giving
> CI a Postgres service would turn several existing DB tests from decorative into real.

> ✅ **2026-07-28 — SIRENE is an INBOUND EVENT: the disk-IO write path is fixed end to end
> (ADR-20260728-011344, [#220](https://github.com/TheCaptainCompany/captain-food/issues/220); PRs
> #226/#227/#228).** The Supabase alert was a symptom of three defects, all now closed.
> **(1) A failed INSERT was the idempotency mechanism.** `register_restaurant` never rehydrated the
> aggregate; a deliberate `UNIQUE (stream_name, version)` violation answered "does this exist?", and
> Postgres writes the heap tuple *before* the constraint fires — ~200k dead tuples in `domain_events`
> and its indexes per sweep, for an outcome that is by definition no change. Now the ACL stages
> `RestaurantRegistered` **unconditionally** into `inbound_events` and the **aggregate decides**:
> record it, emit `RestaurantUpdated` for whatever moved, or append nothing.
> `domain::restaurant::changes_from_registry` is where that is decided — pure, and considering **only
> fields the report carries**, because a registry is a partial source and reading its `None` as "clear
> this" would let every sweep wipe data restaurant staff had entered.
> **(2) INSEE updates were silently dropped** — no `UpdateRestaurant` existed in the worker at all, so a
> rename conflicted, was swallowed as success, and vanished. That path now exists and is tested both
> ways (a rename MUST produce an update; an unchanged report MUST produce nothing).
> **(3) The write path asked the read side, unindexed** — `external_identifiers @> $1` against the
> projection, no GIN index, once per staged SIRET. **Deleted.** The aggregate id is UUIDv5(SIRET). The
> same lookup is **kept on the closure path**, deliberately: legacy listings predate that derivation and
> the projection row is the only thing naming them, so deriving would silently fail to close them — and
> that call is bounded (rows absent 21+ days), not per-SIRET.
> Plus **`payload_hash`** on the mirror, so `last_seen_at` can keep advancing for absence detection
> without re-pending ~200k unchanged rows; it hashes the **typed projection**, so an INSEE per-fetch
> timestamp cannot defeat it. **`InboundEventStatus` gained `IGNORED`/`DUPLICATE`** (appended, never
> inserted — the ordinals ARE the storage format, and inserting mid-enum would have reinterpreted every
> stored `FAILED` row), so `SELECT status, count(*) FROM inbound_events WHERE source='sirene'` is now the
> per-sweep report: created+updated / no-change / redelivered / failed. Closure stays a **command**
> (absence is our inference and CAN be refused — partners are flagged, not closed). Migration
> `20260728040000`; `REQUIRED_SCHEMA_VERSION` bumped.
> **⚠️ Before resuming SIRENE:** the staging SQL is **not** exercised locally or in CI — those
> integration tests are `DATABASE_URL`-gated and neither environment provides Postgres, so they skip.
> Watch the first sweep rather than assuming it. Resuming means re-enabling **both** halves together (the
> cron in `sirene-sync.yml` and `RUN_SIRENE_WORKER`). **Still open on #220:** removing
> `idempotent_on_existing` from its five remaining sites (incl. the `verify_phone` `created: true`
> fiction) and the `sirene-sync` observability contract.

> ✅ **2026-07-28 — the storefront slug is an OWNER-CHOSEN lifecycle, live end to end (ADR-20260728-011344,
> [#220](https://github.com/TheCaptainCompany/captain-food/issues/220); PRs #222/#223/#224/#225).** The slug
> was derived at SIRENE seeding time as `slugify(name)-{NIC}` — reserving ~200k hostnames no merchant chose,
> deriving the tenant *host* from INSEE's mutable `denominationUsuelle`, and colliding systematically (the NIC
> only disambiguates within a company, so generic names on the common `00019`/`00021` establishment numbers
> clashed across different SIREN — the 605-row `SlugAlreadyTaken` storm). Now: **`RestaurantRegistered` and
> `RegisterRestaurant` carry no slug**; it arrives via **`RestaurantSlugConfigured`** / **`RestaurantSlugReconfigured`**
> (the latter carrying `previousSlug`), driven by **`ConfigureRestaurantSlug`** — a real command because it
> *can* be refused, so `SlugAlreadyTaken` finally reaches a human who can pick again. **Activation is gated**
> by the new `SlugNotConfigured`, decided **aggregate-locally** from the fold with no read model consulted.
> Uniqueness moved to a **write-side `slug_reservations` table** (a new table category): its pk *is* the
> invariant, so `INSERT … ON CONFLICT DO NOTHING` lets Postgres decide once — where a projection lookup would
> let two simultaneous claims both pass and diverge only after the projector caught up, having told each
> owner "yes". **A released label stays reserved** (`released_at` set, row kept) so its 301 cannot be
> hijacked. **`SlugAlias` + `hosts.rs`** 301 a superseded host to the current address **preserving the request
> path**, resolved through `restaurant_id` so one hop always lands on the live label. `Restaurant.slug` is
> **nullable + UNIQUE** — Postgres allows many NULLs in a unique index, so the ~200k unconfigured listings
> coexist while the DB enforces uniqueness over exactly the configured set. **Neither the SIRENE ACL nor the
> HubRise connect flow invents a slug** any more. Migrations `20260728020000` (DROP NOT NULL + release the
> derived open-data slugs, claimed listings keep theirs) and `20260728030000` (both tables + backfill a
> reservation for every slug a claimed restaurant holds); `REQUIRED_SCHEMA_VERSION` bumped so `/health` holds
> each build until CI has applied the schema. Back office: a dedicated **storefront-address screen** stating
> what a rename does *before* the button. **Declared gaps** (not faked): no as-you-type availability check
> (that query is a public existence oracle and wants its own decision), "previous addresses" not rendered
> (`SlugAlias` is server-internal), and **no `restaurantById` query** — the only single-restaurant read is
> keyed by *slug*, which is circular for a restaurant that has none. 658 tests, validator 0 errors.
> **Still open on #220:** SIRENE → inbound events (slice 5), deleting `idempotent_on_existing` across the five
> remaining sites, observability.

> ⏳ **2026-07-28 — SIRENE sync is PAUSED, both halves (product-owner directive).** Until
> [#220](https://github.com/TheCaptainCompany/captain-food/issues/220) is resolved: the weekly CI cron in
> `.github/workflows/sirene-sync.yml` is commented out (`workflow_dispatch` deliberately kept, so a scoped
> debug run stays possible), and the on-app drain loop's `RUN_SIRENE_WORKER` gate now **defaults to OFF**
> (`crates/server/src/lib.rs`) so the pause survives deploys without depending on a dashboard setting. The
> `POST /internal/sirene/drain` ping is already fail-closed (503) because `INTERNAL_TRIGGER_TOKEN` is unset,
> so no third path can trigger a drain. **Consequence to know:** detect-by-absence is guarded by
> `FRESH_INGESTION_DAYS = 10` (`sync_sirene_worker.rs:71`), so a stalled ingestion skips the absence pass
> entirely — the pause cannot cause false mass closures. Prospect data simply goes stale, and the Tours
> (dept 37) listings already ingested are unaffected. **Resume BOTH halves together** — CI-only piles up
> unprocessed staging rows, worker-only re-drains whatever is already pending.

> 📋 **2026-07-28 — a Supabase disk-IO alert exposed three write-path defects, now proposed as one
> coupled change (PROP-20260728-004616, [#220](https://github.com/TheCaptainCompany/captain-food/issues/220)).**
> A "depleting Disk IO Budget" email led to a trace of the SIRENE write path. The IO was the symptom.
> **(1) A failed INSERT is the idempotency mechanism**: `register_restaurant` never rehydrates the
> aggregate — it hard-codes `expected_version = 0` (`commands.rs:365`) and lets a `UNIQUE (stream_name,
> version)` violation decide whether the restaurant exists, which `idempotent_on_existing` (`:160-166`)
> laundres into `Ok(())`. Postgres writes the heap tuple *before* the constraint fires, so a weekly
> sweep leaves ~200k dead tuples in `domain_events` and its indexes. Six handlers do this
> (`:269`, `:365`, `:2172`, `:2382`, `:2594`, `:3074`) — the last is user-facing, `verify_phone`
> returning `created: true` after a swallowed conflict. The correct pattern is ten lines away
> (`activate_restaurant` `:376-378` folds and returns with no event). **(2) INSEE updates are silently
> dropped**: there is no `UpdateRestaurant` in the SIRENE worker at all, so a renamed établissement
> conflicts, is swallowed as success, and the change is discarded — mirror updates, domain does not.
> **(3) The write path asks the read side, unindexed**: `by_external_identifier`
> (`persistence/restaurant.rs:39-43`) runs `external_identifiers @> $1` against the eventually-consistent
> `Restaurant` projection, and there is **no GIN index anywhere** in the generated schema — a full
> sequential scan per staged SIRET, the likely dominant IO consumer. **All three trace to deriving the
> slug at seeding time** (`sirene.rs:215-216` → `chez-marco-00021`): ~200k reserved hostnames no merchant
> would choose, systematic collisions (the NIC only disambiguates within a company — the 605-row
> `SlugAlreadyTaken` storm), and the tenant *host* derived from a mutable third-party field. Proposed:
> **slug becomes a lifecycle** (`RestaurantSlugConfigured` / `RestaurantSlugReconfigured` carrying
> `previousSlug` for 301s, projection column nullable-unique so the DB enforces uniqueness over exactly
> the claimed set) and **SIRENE becomes an inbound event** (`inbound_events` keyed on the stable
> `(source, external_id)` rather than `command_journal`'s `last_seen_at`-seeded `message_id`, with
> `IGNORED`/`DUPLICATE` persisting the decision the drain worker already makes at
> `inbound_drain_worker.rs:177-179`). **Sequencing is load-bearing**: the slug change must land first, or
> fixing the update path turns an INSEE rename into a live-storefront rename. Reverses part of ADR-0045;
> six decisions are open in [DECISIONS.md §7](../proposals/DECISIONS.md), D2 (when the owner chooses the
> address) gating. Related but distinct: the projector's own IO pathology (groups re-scanning the log
> every 1.5s because checkpoints only advance on matched events) belongs with
> [#190](https://github.com/TheCaptainCompany/captain-food/issues/190).

> ✅ **2026-07-27 — [#151](https://github.com/TheCaptainCompany/captain-food/issues/151) reclamation
> epic COMPLETE — the #158 credit/refund integrations landed (#207 closed).** With PR #213 (refund
> binding) + PR #214 (credit visible + spendable), all three flagged #158 integrations are done, so
> **#158 and #207 are closed**: a FULL/PARTIAL_REFUND resolution now **executes** a real refund via the
> one existing refund path (open→approve driven from the saga — the resolution IS the approval;
> idempotent, amount-capped at captured, `RefundProcess` the sole Stripe driver); goodwill credit is now
> **visible** (`customerCredit` balance query, a materialized `CustomerCreditBalance` projection) and
> **spendable** (applied at `placeOrder` — `min(balance, total)`, PaymentIntent reduced, exactly-once by
> `orderId`: consume no-ops if the order was already debited, `credit_to_apply` retry-stable, no double-
> spend). A generated-projector correctness bug was caught + fixed en route (a second creation-arm event
> reset the row — the emitter now threads `state.as_ref()`, protecting all 6 projections). Deferred
> (noted): the applied-credit receipt line + credit release on abandoned checkout. All money paths
> verified; migrations `20260727000000` applied. **The whole reclamation subject (open → discuss →
> resolve as refund/replacement/goodwill-credit/reject, evidence, timeline, SLA) is now live end to end.**
