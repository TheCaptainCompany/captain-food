# 🚦 Captain.Food — Development & Deployment Status

> Hand-maintained snapshot (NOT generated, outside `specs/` so it never affects the DSL).
> Last updated: 2026-08-07. Legend: ✅ done & verified · 🚧 in progress · ⏳ blocked/waiting · 📋 planned.

> 🚧 **2026-08-07 — ADR-183024 REALIZATION STEP (1) IMPLEMENTED, PR IN REVIEW — the spec reorg
> ([#375](https://github.com/TheCaptainCompany/captain-food/issues/375) "Spec reorg: specs/{scope}/
> folders + common, api/config fragments, scope validator rules, c4-l2 container split",
> [PR #376](https://github.com/TheCaptainCompany/captain-food/pull/376)).** Landed on the branch:
> the loader merges `specs/{scope}/{kind}.yaml` fragments into the logical catalogs (refs stay
> KIND-logical — zero ref rewrites); ~826 items split into the 8 scope folders per the #374
> membership map (semantic round-trip verified); validator §14 gates placement, the cross-scope
> $ref DAG (PMs exempt bridges), kernel purity and api nesting; c4-l2's `api` container split into
> the ~45-bin deploy topology with per-bin `realizes:`. Validate 0 errors / 43 warnings (baseline
> kinds identical).

> ✅ **2026-08-07 — ONE DECOMPOSITION AXIS — APPROVED AS RECOMMENDED
> ([ADR-20260807-183024](adr/ADR-20260807-183024-one-decomposition-axis.md), D1–D8 with D2/D8 in
> their product-owner-revised forms; critical-path-growth accepted knowingly).** Final shape:
> `specs/{scope}/` folders + common (8 scopes) · **`captain-core`** (log+mailbox, ALL backup budget)
> / **`captain-views`** (per-scope projection schemas, NO backups — restore is replay) ·
> per-scope projectors over the single log · **`graphql-{scope}` services + a boring generated
> gateway per role** (top-level routing from a codegen composition table; nested types intra-scope
> by validator rule) · per-scope configuration. Three standing reviewers now exist: `architect`
> (microservice/actor lens), `dba` (Postgres/food-service), `graphql-architect` (API composition).
> **Realization order** (ADR consequences): spec reorg → #373 crates → bin crates → #349 emitter →
> #363 build matrix → core/views in #360 → #358+#361 with the product owner live. Was:
> ([PROP-20260807-174246](proposals/PROP-20260807-174246-one-decomposition-axis-specs-schemas-projectors.md),
> [#374](https://github.com/TheCaptainCompany/captain-food/issues/374) — DECISION OPEN).** Product-owner
> directive (screaming architecture): **spec folders per business domain + `common/`**, per-domain
> storage, per-domain `configuration.yaml`, per-domain projectors. Completes the §17 chain:
> `specs/{scope}/` → `domain-{scope}` crate ([#373](https://github.com/TheCaptainCompany/captain-food/issues/373))
> → `actor-{scope}` image → `{scope}` schema → `projector-{scope}` — a boundary violation becomes
> visible (folder), unspellable (crate link), undeployable (image) and unqueryable (GRANT), all
> generated. **Recommended storage rung: schema-per-scope in ONE CNPG database with per-scope roles —
> NOT database-per-scope** (Postgres cannot join across databases natively, which would kill the admin
> cross-scope SQL the product owner explicitly requires; `admin_ro` across schemas is plain SQL). **The
> event log stays single in `core`** (global ordering, PM causality, one PITR timeline, GDPR path);
> projectors split per scope over it with independent checkpoints. Proposed scope list (8, from PM
> coupling evidence): ordering · catalog · network · customer · delivery · payments · comms · common.
> **Start-clean makes the storage split FREE at cutover** — the window that does not recur. Seven
> decisions + a critical-path-growth concern open in [DECISIONS.md §18](proposals/DECISIONS.md).

> ⏳ **2026-08-06 (later) — THE DESTINATION IS REOPENED FOR KUBERNETES
> ([PROP-20260806-223656](proposals/PROP-20260806-223656-kubernetes-as-the-deployment-substrate.md),
> product owner: *"Reopen the ADR for Kubernetes"*).** The Clever Cloud decision immediately below is
> **NOT in force**. **Why**: ADR-20260806-151122's decisive argument was *"a team of one product owner
> plus agents should not be operating a PostgreSQL server"* — a premise about the OPERATOR that was
> **wrong**, since the product owner has run Kubernetes professionally. Three further arguments, none
> of them in that ADR: **ingress as a light API gateway** (wildcard TLS is required on every
> destination anyway), **lock-in** (previously dismissed as "a Dockerfile and env vars", which
> under-weighted Clever Tasks/Cellar/add-ons compounding), and **manifests as a codegen target** — a
> cluster can consume generated deployment descriptors, a PaaS cannot, which gives
> PROP-20260805-181926's surviving **D7** a target that finally fits.
> **Everything factual in that ADR stands and is reused**: prices, the 10 TB egress finding, the
> Docker-vs-Rust-runtime correction, the sizing work.
> **Three findings that shape the choice.** (1) **A RollingUpdate runs two write paths at once** —
> exactly what [#193](https://github.com/TheCaptainCompany/captain-food/issues/193) forbids until
> [#242](https://github.com/TheCaptainCompany/captain-food/issues/242)'s leases and fencing land, so
> V0 needs `strategy: Recreate` and **the headline benefit of Kubernetes is unavailable for now**;
> probes and ingress are the day-one gains. (2) **The database gets HARDER, not easier** — a cluster
> supplies none, in-cluster Postgres is the wrong home for an append-only log of paid orders, and
> managed Postgres was ruled out on cost on 2026-08-05. (3) **OVH MKS is GA with free egress**
> (including object storage) while **CKE is public beta** — beta is the wrong risk for the money path.
> **Decided so far (2026-08-06, product owner, in-session)**: **D2 — Postgres runs IN-CLUSTER via
> CNPG** (with ≥3 nodes, required anti-affinity, WAL archiving and executed restore drills as part of
> the answer) and **D7 — GitOps is the only change path** (*"Of course gitops"*): the agent gets
> cluster + Postgres READ access for diagnostics and repairs production through repo changes; the
> operating practices are the proposal's §2b (generated manifests reconciled by Argo CD, CI commits
> the digest, sealed secrets for the public repo, symptom alerts that wake sessions, weekly restore
> drill). **✅ 2026-08-07 — FULLY DECIDED
> ([ADR-20260807-002705](adr/ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md), superseding the
> Clever Cloud ADR): OVH MKS (Paris) · CNPG in-cluster (≥3 nodes, anti-affinity, WAL archiving,
> executed restore drills) · `Recreate` until #242 · ingress-nginx + cert-manager with the DNS zone
> HOSTING moving Dynadot → OVH DNS (Dynadot stays registrar — no Dynadot cert-manager solver exists) ·
> manifests GENERATED from the specs · GitOps-only operations (agent: read-only diagnosis + repo
> changes + per-incident break-glass) · straight to the cluster with production STARTING CLEAN — empty
> schema, all migrations fresh, NO dump restore, crash-test data discarded by explicit decision.**
> The dump/restore/checksum workstream is deleted; #242 slice 3's prod-gate becomes "MKS cutover
> complete"; realization issues land under
> [#271](https://github.com/TheCaptainCompany/captain-food/issues/271). PROP-20260806-223656 is
> `Approved`; §2b carries the ten operating practices.
> **Realization backlog CREATED and STARTED** (ordered index on
> [#271](https://github.com/TheCaptainCompany/captain-food/issues/271)):
> 🚧 [#358](https://github.com/TheCaptainCompany/captain-food/issues/358) MKS bootstrap — **claimed,
> draft PR [#365](https://github.com/TheCaptainCompany/captain-food/pull/365)**, OVH auth shape
> established; **RE-SIZED on budget (ADR-20260807-114122): the EUR 67.80 trio is over budget — entry
> is ONE d2-8 + LB S = EUR 26.60/mo ex-VAT**, CNPG `instances: 1` with WAL/PITR non-negotiable,
> Prometheus dropped for Honeycomb triggers, ladder up (node-pool resize → `instances: 3`) when #242
> lands or first paying restaurants; project id recorded in the runbook; console steps need the
> product owner live ·
> [#361](https://github.com/TheCaptainCompany/captain-food/issues/361) NS Dynadot→OVH DNS (**product
> owner live — Dynadot login**) ·
> [#359](https://github.com/TheCaptainCompany/captain-food/issues/359) Argo CD ·
> [#360](https://github.com/TheCaptainCompany/captain-food/issues/360) CNPG ·
> [#362](https://github.com/TheCaptainCompany/captain-food/issues/362) ingress/TLS + sealed secrets ·
> [#349](https://github.com/TheCaptainCompany/captain-food/issues/349) manifests emitter (D5) ·
> [#363](https://github.com/TheCaptainCompany/captain-food/issues/363) deploy.yml→GitOps ·
> [#364](https://github.com/TheCaptainCompany/captain-food/issues/364) observability/alert loop.
> (#366–#372 were an accidental duplicate set — created after a context compaction hid this very
> session's own claim — and are closed as duplicates; the lesson is in sessions.md.)

> 🚨 **2026-08-06 — THE HOSTING DESTINATION IS CLEVER CLOUD, NOT OVH — ⚠️ REOPENED, see above
> ([ADR-20260806-151122](adr/ADR-20260806-151122-hosting-destination-is-clever-cloud-not-ovh.md),
> product-owner decision: *"Instead of OVH"*).** This supersedes **only point 1** of
> ADR-20260731-061609 — the destination. **Points 2–4 survive verbatim**: Supabase stays
> IDENTITY-ONLY, the build side does not move (GitHub Actions + GHCR + the isolated
> build → manual deploy → migrate pipeline, target renamed), and the cutover still uses the existing
> outage. **The reasons for leaving Render/Supabase are unchanged and were not revisited.** OVH
> remains the SMS provider (ADR-20260722-174500) — this changes where the app and database run,
> nothing else.
> **Why it changed**: choosing an OVH instance meant owning a host OS for the first time, and working
> that through generated a tail of work with no customer value — a WireGuard overlay (OVH **VPS cannot
> join a vRack**, a confirmed fact: the vRack page lists Bare Metal, Hosted Private Cloud, Public
> Cloud, Additional IP, Enterprise File Storage and Load Balancer, and VPS is in none of them), block
> volumes for the database disk, an upscale-only resize ratchet, and **WAL archiving we would have to
> build**. Clever Cloud (French PaaS, Paris) removes all of it: managed PostgreSQL with daily backups
> at 7-day retention on **paid** plans (the free `DEV` plan has had NO backups since 2025-10-01 — the
> same trap as the Supabase free tier), PITR via pgBackRest on request, Docker-image deploys.
> Sovereignty improves too: France, European jurisdiction, explicitly outside the Cloud Act.
> **Consequence**: [PROP-20260805-181926](proposals/PROP-20260805-181926-host-provisioning-and-configuration-ownership.md)
> is **mostly moot** — D1–D6 have no subject without a host we own, **only D7 survives**, and D3
> (SaltStack) is settled by construction. **One blocking precondition before any spend**: whether
> Clever Cloud meters **egress** the way Render did. Render's outbound-bandwidth exhaustion is one of
> the incidents that started this migration, and repeating it on a new PaaS is the single way this
> decision fails. **✅ That blocker CLEARED 2026-08-06: Clever Cloud includes 10 TB/month egress at no
> charge** — orders of magnitude above what the WASM bundle plus GraphQL can produce at V0 peak (get it
> in writing before it is load-bearing). **But object storage is a separate meter**: Cellar egress is
> **EUR 0.09/GB**, so the planned file-attachment framework
> ([PROP-20260725-120055](proposals/PROP-20260725-120055-generic-file-attachment-framework.md)) —
> restaurant and menu **photographs**, in an image-heavy marketplace — is the Render bandwidth failure
> returning through a different door unless where images are served from is decided deliberately.
> **Remaining before purchase** (all on the ADR): the estimator's cheap selection is **under-specced**
> (`pico` = 256 MiB, `XXS Small Space` = 1 GiB disk / 512 MiB / 45 connections — the latter barely
> above the Supabase free tier being escaped); pick the **Docker runtime, not `Rust`**, or the platform
> compiles the workspace on every deploy and digest pinning dies with it; and declare the sqlx pool
> ceiling against the 45-connection limit. Prices/specs come from the vendor estimator only — a
> third-party spec table already produced wrong VPS-2 figures once (corrected 2026-08-05).

> 📋 **2026-08-05 — Who owns the OVH host: provisioning IaC + host configuration
> ([PROP-20260805-181926](proposals/PROP-20260805-181926-host-provisioning-and-configuration-ownership.md),
> [#349](https://github.com/TheCaptainCompany/captain-food/issues/349)) — DECISION OPEN, nothing built.**
> Asked whether SaltStack is useful here. The question is live because the OVH cutover
> ([#271](https://github.com/TheCaptainCompany/captain-food/issues/271), ADR-20260731-061609) gives us
> a **host OS of our own for the first time** — on Render nothing about the machine was ours — and a
> grep for `saltstack`/`ansible`/`terraform`/`pulumi`/`nixos`/`cloud-init` across `specs/**`,
> `docs/**` and `.github/**` returns **zero hits**: no file says which OVH resources exist or what is
> installed on the box. That is the `RUN_SIRENE_WORKER`/`API_SECRET` dashboard failure one layer
> deeper, and this time the unrecorded thing is the machine.
> **The question splits into three layers, and Salt addresses only the middle one**: provisioning
> (which resources exist — unowned, and Salt does not do this), host configuration (what runs on the
> box — unowned), and application configuration (**already owned** by `specs/configuration.yaml` +
> the codegen'd reader + the `env::var` drift test, and it must stay that way — Salt pillars would be
> a second config store).
> **Recommended: reject Salt** — its ~30×-at-1,000-nodes advantage is a fleet advantage and
> [#193](https://github.com/TheCaptainCompany/captain-food/issues/193) caps us at ONE instance until
> #242's leases land; master/minion adds a listening root-equivalent control plane (ZeroMQ 4505/4506)
> to the box terminating payment traffic; and its convergence model contradicts the immutable-artifact
> doctrine PROP-20260729-014500 D5 just established (digest-pinned, config baked in, rollback = old
> digest). Salt earns a genuine revisit **only** for restaurant-side hardware fleets (tablets/KDS/
> printers) — a different problem, decided on its own merits.
> **Recommended instead**: OpenTofu + the official `ovh/ovh` provider for provisioning, cloud-init for
> the host (~80 lines, no agent), the host treated as **disposable — rebuild, never converge** (safe
> only because PROP-20260731-061609 D2 put the event log on a separate managed PG). Ansible is the
> named escape hatch at 3+ hosts; NixOS is the honest best conceptual fit, deferred on ecosystem cost.
> **D6 exists so none of this blocks the cutover**: prod is DOWN, so cloud-init first, cut over, then
> `tofu import` the live resources. Registered as an unchecked `Concerns` entry, which mechanically
> blocks `Approved`. **Seven** open decisions in [DECISIONS.md §16](proposals/DECISIONS.md).
> **D7 added 2026-08-05** after the product owner challenged the NixOS rejection — *"based on the spec
> in YAML you can generate it, so I don't need to know this ecosystem myself because it's encapsulated
> in the codegen"*. The challenge lands and the authoring-cost objection is **conceded**: "ecosystem
> cost" is a weak reason to reject anything in a repo whose operating model is generate-everything.
> What replaces it: **codegen encapsulates authoring, not operating** (a failed boot is debugged in the
> GENERATED artifact, and "never hand-edit generated output" closes the shortcut by design), and the
> test derivable from our own emitters is **semantic level + fan-out** — `entities.yaml` declares
> `Order` once and reaches SQL, GraphQL, Rust and docs, whereas a `specs/host.yaml` would be
> NixOS-options-in-YAML: same level as its output, one target, no fan-out, and the repo's first emitter
> with no abstraction gain. Two supporting facts: Nix generates YAML/JSON rather than the reverse (the
> idiomatic path is `builtins.fromJSON`, i.e. Nix READS the data, so a Nix emitter is the expensive
> route and the cheap one still leaves a hand-written Nix module), and **codegen removes authoring cost
> for cloud-init too**, so it does not differentially favour NixOS. NixOS is now deferred on
> **bootstrap risk** — OVH has no first-class NixOS image (`nixos-infect`/`nixos-anywhere`/custom
> upload), a poor thing to learn while prod is down — and stays reachable later as a contained
> emitter-target swap. **The durable idea in the challenge is kept as D7**: derive infra artifacts from
> the specs that ALREADY exist (`configuration.yaml`, `observability.yaml`, `services.yaml`, C4), which
> has real fan-out and makes infra structurally unable to drift from the app's declaration.

> ✅ **2026-08-04 — Screen actions are checked against their command's inputs
> ([ADR-20260804-154700](adr/ADR-20260804-154700-screen-actions-are-checked-against-their-command-inputs.md))**.
> Asked whether anything declared the screen-form ↔ mutation-input gap, the answer was **no**:
> `action-not-a-mutation` proves only that the `$ref` names a mutation, `op-uncovered-by-story` is
> satisfied by a story STEP (not a screen), and `validate_resolver_args` deliberately skips required-arg
> coverage for QUERIES (a pin is a static default). Nothing read a mutation action's `variables`. Two new
> WARNING rules now do: **`action-missing-required-input`** (a screen action is the CALLER, so its
> variables are the whole input) and **`action-unknown-input`** (the write-side mirror of
> `resolver-unknown-arg`). The validator now walks screen component trees, which it never did.
> **17 pre-existing violations on the first run** — hence warnings, not errors: a gate that fails the
> build on inherited debt gets weakened instead of paid down. Tracked in
> [#342](https://github.com/TheCaptainCompany/captain-food/issues/342). Sharpest case: the rider's
> **Accept button passes an `orderId` that `AcceptDelivery` does not declare and supplies neither of its
> required inputs** — the screen's primary action cannot work.
> Also landed: the **restaurant profile screen** (`/settings/profile`) wiring `updateRestaurant` — the
> reason `Restaurant.description` was a column no event fed was that the mutation which sets it had
> **zero screens**, while being story-covered. It declares four `gaps` (no `restaurantById` query;
> `openingHours`, `contact`/`address` and the ADMIN-only `marginRate` deliberately off the form).
> Also closed here, the SILENT twin of the same family: a mutation missing from the emitter's dispatch
> table shipped an `Err("not implemented")` resolver body with no `command_router` arm, while api.yaml
> declared it, a story step covered it and a role guard protected it. **`recordDeliverySatisfaction` and
> `escalateDelivery` were in that state with their handlers already written** — only a table row was
> missing. Both wired; the omission is now impossible: the emitter asserts the stub-arm set equals an
> explicit **`UNWIRED_MUTATIONS`** allowlist (empty), so an unwired mutation FAILS generation. A
> generation-time assertion, NOT a validator rule or a source scan — the table lives in the emitter where
> no `specs/**` gate can see it, and grepping generated Rust for the stub string would be #329 verbatim.
> **Warning baseline 26 → 43** — a deliberate new-rule change, not drift. Compare against 43 from here.

> ✅ **2026-08-04 — Two dead read-model columns populated; refund facts carry their payment identity
> ([ADR-20260804-041227](adr/ADR-20260804-041227-populate-the-two-dead-columns-and-address-refund-facts.md))**.
> An audit of the 31 standing warnings found **none of them were lint noise** — each is an unbuilt
> feature, a tracked deferral, or a real hole. Five were actionable and are fixed:
> `Restaurant.description` and `Catalog.slug` now have event lineage (`RestaurantUpdated` gains a
> nullable `description` on a new dedicated `RestaurantDescription` scalar; `CatalogCreated` gains a
> REQUIRED `slug` — safe because only `CreateCatalog` emits it, the HubRise path emits `CatalogImported`).
> `Catalog.slug` had been a **non-null GraphQL field over a column the projector could only fill with the
> empty string**. The three refund events (`RefundOpened`/`Approved`/`Denied`) now carry
> `paymentIntentId` — they are delivered as messages to the `Payment` aggregate, whose identity that is.
> **Two hand-written projector shims deleted** (`CatalogCompute::slug`, `RestaurantCompute::description`)
> and **one runtime gate deleted because the compiler subsumes it**: tightening
> `refund_process_manager.payment_intent_id` to NOT NULL (a run cannot exist without a captured payment)
> made the `RefundNotPending` unwrap-guard unspellable. `slugify` moved to **`domain::shared::text`** —
> it had no callers outside its own tests and the HubRise catalog import is its second consumer.
> **Warning baseline 31 → 26**, no new kind. The remainder: unbuilt delivery/rider ×18, credit/cart ×6,
> [#341](https://github.com/TheCaptainCompany/captain-food/issues/341) (listing opt-out does nothing —
> the `view-fedby-unused` symptom), and one correct-as-is `identity-property-not-on-command`.

> ✅ **2026-08-04 — Unread read models deleted
> ([ADR-20260804-032640](adr/ADR-20260804-032640-delete-unread-read-models.md))**, product-owner
> directive following the #305 gate. **`View_RestaurantAccount`** (the ONLY `internal: true` exemption in
> the database spec — no api binding, no component read, zero literal hits in `crates/**`) and
> **`PhoneCountry`** (a `reference: true` table, which the gate does not check at all — zero references
> anywhere) are gone. **No `crates/**` file changed** as a result: direct proof nothing read them.
> **"No declared reader" ≠ "unused"** — the bounded claim biting the other way. A trial deletion of the
> view raised **3 errors**: `Restaurant.restaurant_account_id` carried an `fk:` into it (read-navigation
> graph) and `projection-updaters` listed it in `updates[*]`. Both removed; the column stays, still
> indexed, since `restaurantLocationsByAccount` queries by it.
> **A known hole, deliberately accepted** (product owner chose this over keeping the view or folding the
> event first): `RestaurantAccountUpdated` and `RestaurantAccountDeleted` now reach NO read model — an
> account legal-name/timezone change, and an account deletion, land in the log and propagate nowhere.
> Account data is correct at creation only, because the `Restaurant` projection folds
> `RestaurantAccountRegistered` for `default_currency`, and silently stale after. A back-office account
> surface needs a **projection**, not a query. `nonProjectedEvents`' documented meaning was **widened**
> to carry two reasons — (a) transient/saga-internal, (b) **recorded but unread** — rather than file
> these two under (a), which would have been false.
> **Warning baseline 32 → 31** (`view-fedby-unused` 2 → 1; `event-not-projected` held at 11, no new kind).

> ✅ **2026-08-04 — [#305 "View_* read declarations: no spec says which surface reads which view"](https://github.com/TheCaptainCompany/captain-food/issues/305)
> ([ADR-20260804-014546](adr/ADR-20260804-014546-read-models-declare-their-readers.md))**: the READ-side
> equivalent of the #304 hole. `components.*.reads[*]` in `specs/architecture/c4-l3.yaml` — the mirror
> of the existing `updates[*]`, one row in `refs.rs` — declares which component consumes which read
> model, and **`read-model-no-reader` (error) replaces `view-no-query` (warning)**. Three ways to pass,
> all declarations rather than exemptions: an `api.yaml` output type binds it, a component declares it,
> or it is `internal: true`. A GraphQL-reached model is declared by its api.yaml type binding and is
> deliberately NOT re-listed on `graphql-gateway`, so the two cannot drift.
> **Why a gate and not the compiler** (ADR-20260803-234035): the property is a fact about YAML — rustc
> cannot read `api.yaml`. Nothing here scans Rust, so it is not #329 repeating. The compiler answer
> (a generated `ReadPorts` bundle, undeclared pair → `E0609`) needs a declaration to generate FROM,
> which is what this lands; it is the **prerequisite**, tracked as successor B in the ADR.
> **Bounded claim, stated in the ADR**: this proves every read model has *a* declared reader, NOT that
> every actual reader is declared — the Rust side stays undeclared until the port bundle. Do not close
> that with a source scan; that is #329 verbatim.
> Satisfied with four declarations: a new **`tenant-host-router`** component (`crates/server/src/hosts.rs`
> had no C4 representation at all despite being a live entry point) covering `SlugAlias` — the one
> `view-no-query` warning on `main`, read legitimately by the 301 — plus command handlers, process
> managers and the HubRise ACL. C4 now renders `reads` beside `updates` in both doc surfaces.
> `phoneCountries` **deleted** (product-owner call): the only V0 query reached by no screen and the only
> one of 32 with no wired resolver body — it advertised a `reads:` binding while returning
> `Err("not implemented")`; the `PhoneCountry` reference table stays *(reversed hours later — see the
> 032640 entry above; the table was deleted too)*.
> **Warning baseline 33 → 32** (`view-no-query ×1` gone, nothing else moved).

> ✅ **2026-08-03 — [#306 "Isolation phase 2: one crate per actor client (aggregates AND process managers)"](https://github.com/TheCaptainCompany/captain-food/issues/306)
> (PROP-20260802-130500 phase 2, [ADR-20260803-214500](adr/ADR-20260803-214500-actor-door-contains-the-phase-2-widening.md))**:
> the 17 typed clients (15 aggregates + both process managers — the proposal header's "16" predates
> `CustomerCredit` and `MailboxSupervision`) now live in **one generated crate each** under
> `crates/clients/<actor>`, manifest AND code emitted from actors.yaml. **Depending on a crate is
> the permission to address that actor**: `server` names 15 and reaches neither `Payment` nor
> `CustomerCredit`; each delivery adapter names `client-delivery-job` alone; Stripe names
> `client-payment` alone. Workspace members carries `crates/clients/*` as a GLOB, so a new actor's
> crate joins by being generated; the emitter also REMOVES a stale crate whose actor left the spec
> (a content diff would never notice a directory that simply stopped being regenerated).
> **The wall, and what it cost** (proposal §6 predicted both): the per-actor crates must build
> mailbox rows, and both `MailboxEntry`'s fields and the `MailboxAccess` mint are what D1/#304 keep
> private. Neither was widened — they enqueue through the opaque **`ActorDoor`** facade, which
> builds the row and mints the witness inside `actor_client`. Honest accounting: `ActorDoor` is
> string-keyed and public, so it *could* address any actor with any message — a capability that did
> not exist before (`command_entry` was `pub(crate)`). It is contained at level 3 by
> `actor_door_is_named_only_by_generated_client_crates` (naming it outside `crates/clients/**` is
> CI-red), landed in the same change; the entry and the witness stay level 4. A `client-door` cargo
> feature was considered and rejected — feature unification makes it the same tier for real dead-code
> cost. **Guards**: the two new ones were negative-tested (each fails on a planted violation, not
> merely green); the lint floor now matches `crates/clients/` by PREFIX so a new actor cannot join
> below it; the witness scan extends to the client crates; `client_crates_are_exactly_the_mailbox_actors`
> refuses a hand-made directory the glob would otherwise silently enlist. The typed-send drift guard
> moved OUT of the crate (`crates/actor_client/tests/drift_guard.rs`) and now runs as a consumer
> does, comparing rows through the D5 `EntryFixture` mirror over a dev-dependency cycle Cargo
> permits. Validator unchanged at **0 errors / 33 warnings** (main's baseline).
> **Not in this change**: C4 (`specs/architecture/**`) is source DSL and needs plan mode — it rides
> [#309](https://github.com/TheCaptainCompany/captain-food/issues/309)'s "repeat per phase" rule.
> Phase 3 ([#307](https://github.com/TheCaptainCompany/captain-food/issues/307), per-actor
> implementation crates) is unstarted and still owes its costing first.

> ✅ **2026-08-03 — [#329 "Narrow the #304 residual class: every public mailbox door must be declared"](https://github.com/TheCaptainCompany/captain-food/issues/329)
> ([ADR-20260803-203455](adr/ADR-20260803-203455-mailbox-doors-are-declared-by-reachability.md))**:
> the class [#304](https://github.com/TheCaptainCompany/captain-food/issues/304)'s witness guard
> could not see — a public in-crate item that MINTS internally and hands the capability out through
> a signature that never names the witness — is **narrowed, not closed**.
> `every_public_mailbox_door_is_declared` seeds on witness CONSTRUCTIONS read from the AST,
> propagates through `actor_client`'s call graph to a fixpoint (call edges include bare references,
> since `let f = MailboxAccess::granted;` and `.map(insert_mapped)` pass a function as a value), and
> requires every publicly-reachable tainted function to sit on an explicit door list keyed by
> `(file, name)`. Taint stops at an UNGATED door only — a wrapper does not inherit the cargo feature
> that contains a gated one, which would otherwise have re-exposed the untyped bulk door to crates
> `bulk-door` exists to exclude. The door list is the deliverable as much as the check: ten entries
> (seven non-test) enumerating what can reach the mailbox, so an eleventh is an edit to that list.
> **The scope is honest and was got wrong first**: the parameter-or-construction dichotomy is sound
> value provenance, but this scan is a SYNTACTIC approximation of the call graph (idents, no type
> resolution), so it does not discharge a semantic completeness argument — review proved four
> ordinary counterexamples against the first version. A complete rule needs type resolution (rustc
> lint / HIR / MIR) and is a proposal-level scope decision, not a test —
> [#331](https://github.com/TheCaptainCompany/captain-food/issues/331).

> ✅ **2026-08-03 — [#304 "The Mailbox port surface hole: insert/by_message are pub to any port holder"](https://github.com/TheCaptainCompany/captain-food/issues/304)
> (PROP-20260802-130500 §5 directive, [ADR-20260803-172654](adr/ADR-20260803-172654-mailbox-port-demands-a-capability-witness.md))**:
> holding the `Mailbox` port is no longer holding the door. Every port method takes a
> `MailboxAccess` witness whose only mint is `pub(crate)` to `actor_client`, so **no out-of-crate
> CALLER can invoke a `Mailbox` method at all** — the generated typed clients (write) and
> `ActorClient` (read) are the only paths, by compiler rather than by convention. (Level 4 against
> callers; weaker against IMPLEMENTORS — an out-of-crate `impl Mailbox` decorator is handed a real
> witness when a door calls it. What contains that is the composition root, not the witness: a
> decorator only receives calls once someone wires it into `server/src/lib.rs`. Recorded honestly
> in the ADR's consequences rather than claimed away.) The write
> methods were already closed incidentally (a `MailboxEntry` cannot be built outside the crate);
> the two keyed by a bare `Uuid` were wide open: `by_message` (the D4 read side — its own doc
> comment claimed a convention two callers were breaking) and `cancel_scheduled`, which would
> withdraw any scheduled reminder for anyone while `cancel_scheduling` above it is emitted only
> for actors declaring `reminders:` (ADR-20260802-170059). Both direct readers moved onto
> `ActorClient::get_operation_status`: the HubRise connect flow's terminal-status poll (it now
> holds an `ActorClient`; a standalone adapter has no shared bus, and that is fine because the
> flow only pulls the durable row) and the generated legacy-arm cross-arm duplicate check.
> Integration tests seed through `MailboxAccess::for_tests()` on the D5 `test-fixtures` feature
> that never reaches a release graph. No generated per-actor client names the witness any more
> (`cancel_scheduling` delegates to `enqueue::cancel_scheduled_mapped` like every other method),
> which is what keeps PROP-20260802-130500 phase 2 a visibility change rather than a redesign.
> `ActorClient::pull_only` + `watch -> Option<OperationWatch>` put the no-shared-bus posture of a
> standalone adapter in the type, instead of a default bus whose `watch` would hang forever.
> `every_mailbox_port_method_demands_the_access_witness` (tools/codegen-rs) catches the
> SIGNATURE-LEVEL widenings the compiler cannot — they are all EDITS TO THE BOUNDARY CRATE. It
> parses the AST (syn, a new dev-dependency): for every release-reachable public item the WHOLE
> signature (generics, where-clause, inputs, output, field/variant types) must not mention the
> witness, against an explicit exemption list (the `Mailbox` trait's items, `impl Mailbox for _`,
> the cfg-gated `for_tests`); the port trait's own parameters keep an EXACT type check, because
> there `Option<MailboxAccess>` would let the caller pass `None`. **Parameter and output positions
> are opposite problems.** Six review passes each defeated an earlier version, every one of which
> asked *where* the witness appears and left a slot uninspected. The claim is bounded, not closed:
> a public in-crate wrapper that mints internally and never names the witness in its signature
> (`pub fn cancel_any(&self, id)` on a blanket `impl<T: Mailbox>`) is invisible to any signature
> analysis, and cannot even be banned as a construct because the sanctioned bulk door
> `enqueue_inbound_facts` is a member of that class — what contains the residue is the same thing
> that contains the decorator case: an edit to the boundary crate, visible in any diff. Macro
> expansion is likewise invisible, so `include!`, `#[path]`/`cfg_attr`-path modules and any
> item-position macro carrying the witness are refused as a CLASS (matched on the last path
> segment, after `std::include!` and `cfg_attr(.., path=..)` each walked past a narrower check).
> The threat model is safe Rust, so the workspace-wide `unsafe_code = "forbid"` is load-bearing.
> Twenty-nine bypass shapes verified red against a green baseline, plus the legitimate refactors
> that must stay green.
> §5 audit: the `Mailbox` port row moves ❌ → ✅ compiler;
> `View_*` reads ([#305](https://github.com/TheCaptainCompany/captain-food/issues/305)) and
> `PgEventStore` append stay open.

> ✅ **2026-08-03 — [#303 "ActorClient::watch — relocate OperationStatusBus behind the actor-client boundary"](https://github.com/TheCaptainCompany/captain-food/issues/303)
> (PROP-20260802-130500 D4 tail, PROP-20260728-152752 §2.1)**: the operation-response bus is
> behind the boundary now. `OperationStatusBus`/`OperationUpdate` moved from
> `infrastructure::persistence::status_bus` to `actor_client::status_bus`, re-keyed from the
> legacy `CommandJournalStatus` to the mailbox-native `InboundMessageStatus` — the mailbox
> workers' `StatusBusObserver` publishes the HONEST verdicts (IGNORED/DUPLICATE stay themselves;
> the API mapping folds them into SUCCEEDED at the edge), and the legacy journal+spawn path maps
> in losslessly (`journal_status_mailbox`). The generic read door gained the push half:
> `ActorClient::watch(message_id)` returns a per-operation stream (filtered to the handle, lag
> explicit as a re-read cue, ends when the bus closes); `OperationStatusBus::subscribe` is
> `pub(crate)` so the typed watch is the ONLY consumer surface (ADR-20260802-170059 posture).
> The generated `operationStatusChanged` resolver now subscribes through `watch` before the
> snapshot read (race still closed) and maps updates via `mailbox_status_api`; the generated
> `operationStatus`/snapshot reads are unchanged. `actor_client` gains `tokio` (`sync`) as its
> bus dependency — still no sqlx/reqwest (D3 allowlist untouched).

> ✅ **2026-08-03 — [#315 "Admin requeue mutation for poisoned mailbox rows (ADR-20260803-002712 Q1)"](https://github.com/TheCaptainCompany/captain-food/issues/315)
> ([ADR-20260803-143216](adr/20260803-143216-admin-requeue-rides-the-mailbox.md))**: operator
> recovery of a cap-poisoned row is a first-class ADMIN mutation riding the mailbox it
> supervises — new `MailboxSupervision` aggregate (keyed by the SUPERVISED row's messageId, 1
> partition; every intervention = a `MailboxMessageRequeued` audit fact), `requeueMailboxMessage`
> mutation + `poisonedMailboxMessages` discovery query (the messageId behind
> `MailboxLane.poisoned`'s bare count), the `MailboxRequeue` port whose Pg adapter arbitrates AND
> flips in ONE statement (`FAILED`+`DeliveryInfrastructureError` → `RECEIVED`, attempts reset,
> error/backoff cleared, lane `pg_notify`-nudged; already-deliverable converges, anything else
> refuses typed), full ADR-0032 train (rule
> `OnlyCapPoisonedMailboxRowsAreRequeueable` ⇆ 3 behaviour tests, story steps, system-screen
> poisoned list + Requeue button, `platform` bounded context in C4 L2). E2E `mailbox_requeue`
> proves the loop on PG through a real worker fleet. Remaining #313 follow-up: #317 (Honeycomb
> poison alert, ⏳ blocked on Honeycomb re-authorization).

> ✅ **2026-08-03 — [#302 "Lint floor (PROP-20260802-130500 D6): workspace [lints] + cargo-machete in CI"](https://github.com/TheCaptainCompany/captain-food/issues/302)**:
> the D6 lint floor is in force. Workspace `[workspace.lints.rust]` sets `unsafe_code = "forbid"`
> (no crate writes unsafe today; a future FFI crate opts out via its own `[lints]` table — a
> visible one-crate manifest diff, never a workspace-wide relaxation), inherited by every member
> via `[lints] workspace = true`. BOUNDARY crates (`actor_client`, `infrastructure`, `telemetry`,
> the five partner adapters) additionally carry `unreachable_pub = "deny"` in their own `[lints]`
> tables — a dead `pub` on a boundary is now a compile error (the mechanical form of
> [ADR-20260802-170059](adr/ADR-20260802-170059-client-surface-is-spec-gated.md)); measurement
> found the whole set already clean except 5 items narrowed to `pub(crate)` (3 hubrise env-name
> consts, telemetry's `HoneycombHttpClient`). `server` is deliberately NOT in the boundary set:
> 207 findings, mostly in the generated GraphQL layer — widening the floor there is emitter work,
> a recorded follow-up, not part of this pure-configuration change. `cargo-machete` gates CI
> (before the build — static analysis, fails fast) and removed six genuinely unused deps
> (`serde` in actor_runtime/app-core, `chrono` in four adapters — each an unheld capability).
> Codegen guard `lint_floor_covers_every_member` (verified red) asserts the workspace baseline
> exists, every member inherits or restates it (FFI opt-outs must be allowlisted with a reason),
> boundary crates keep the deny, and ci.yml keeps `cargo machete` — a new crate cannot silently
> skip the floor.

> ✅ **2026-08-03 — [#318 "DB-persisted PM_MAILBOX_DELIVERY posture — precondition for adapter worker fleets (ADR-20260803-002712 Q4)"](https://github.com/TheCaptainCompany/captain-food/issues/318)
> ([PR #322](https://github.com/TheCaptainCompany/captain-food/pull/322),
> [ADR-20260803-104819](adr/20260803-104819-db-persisted-pm-mailbox-delivery-posture.md))**: the
> Runtime D1 money gate moved from per-process env into ONE seeded `RuntimePosture` database row
> (`referential.yaml`, migration `20260803104819`, `REQUIRED_SCHEMA_VERSION` bumped) read at
> startup by the monolith composition root and every standalone adapter fleet — steady-state
> posture drift (the drifted-env silent paid-order stall) is structurally impossible now (no
> per-process posture state left to drift; the env key is REMOVED from configuration.yaml/Config);
> the FLIP WINDOW is governed by the restart order prescribed in the ADR (ON: adapter fleets
> first, monolith last; OFF: monolith first — independent-review finding). Fail-closed by cause:
> missing row/table = deterministic legacy arm everywhere (monolith gate off, adapter money lanes
> refused); transient read error = the monolith refuses to start after brief retries, an adapter
> fleet spawns nothing until the row answers. Flip = `UPDATE RuntimePosture …` + ordered full restart.
> The `RUN_MAILBOX_WORKERS` fleet-guidance flip to ON stays its own one-line ADR after smoke
> (gate-then-stabilize), as does the gate's default flip. E2E `runtime_posture` proves the read
> contract incl. seed-never-overwrites-a-flip. Remaining #313 follow-ups: #315 (admin requeue,
> next), #317 (Honeycomb poison alert, ⏳ blocked on Honeycomb re-authorization).

> ✅ **2026-08-02 — [#290 "Actor-client crate isolation (PROP-20260728-152752 D9): compiler-enforced door, then per-actor crates"](https://github.com/TheCaptainCompany/captain-food/issues/290)
> phase 1 MERGED ([PR #297](https://github.com/TheCaptainCompany/captain-food/pull/297);
> [PROP-20260802-130500](proposals/PROP-20260802-130500-isolation-by-construction.md)
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
> **D3**: codegen guard `capability_dependencies_are_allowlisted` — `sqlx`/`reqwest` only in an
> explicit per-crate allowlist with WHYs (server keeps both exceptions: PgPool construction +
> /health probe; Supabase JWKS fetch), bidirectional (stale entries fail), verified red on a
> planted grant. **D5**: cross-crate test access rides the `test-fixtures` cargo feature (mem
> double, `EntryFixture` full-field mirror keeping out-of-crate freeze tests exhaustive, reference
> impls), dev-dependencies only — guard `test_fixtures_feature_never_reaches_a_release_artifact`
> fails any release-graph grant (verified red). The textual door guard stays as belt-and-braces,
> allowlist moved to the actor_client paths. **Surface directive
> ([ADR-20260802-170059](adr/ADR-20260802-170059-client-surface-is-spec-gated.md), product owner
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
> [ADR-20260801-080339](adr/ADR-20260801-080339-auth-verifier-reads-resolved-config-not-env.md).
> `cargo build -p server` + `cargo test -p server` green; recovers on next deploy.

> 🚧 **2026-08-02 — the isolation program is APPROVED, phase 1 launching
> ([PROP-20260802-130500 "Isolation by construction"](proposals/PROP-20260802-130500-isolation-by-construction.md),
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
> [PROP-20260731-195500](proposals/PROP-20260731-195500-runtime-d-pm-mailboxes-and-reminders.md)
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
> gained the **PREPARE phase** ([ADR-20260801-023000](adr/ADR-20260801-023000-a2-realizes-as-prepare-phase-single-delivery.md)
> R2 — handler work with NO transaction open, then ONE fenced commit); the three PM commands
> (placeOrder/approveRefund/denyRefund) run their UNCHANGED application handlers in prepare over
> staging stores (new `StagingPaymentProcessState`/`StagingRefundProcessState`; executor-generic
> generated pm-state upserts flush the run rows in-tx), Stripe idempotency keys
> `intent:{orderId}` / `refund:{intent}:{amount}` make redelivery re-runs land on the SAME
> gateway object, and a sync decline commits the byte-identical legacy `REJECTED PaymentDeclined`.
> B2 realized IN-TX ([ADR-20260801-053000](adr/ADR-20260801-053000-b2-chain-rides-the-completion-transaction.md)):
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
> SUPERSEDED by [PROP-20260731-061609 §5](proposals/PROP-20260731-061609-ovh-migration.md).
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
> six decisions are open in [DECISIONS.md §7](proposals/DECISIONS.md), D2 (when the owner chooses the
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

> ✅ **2026-07-26 — [#151](https://github.com/TheCaptainCompany/captain-food/issues/151) reclamation
> (claim/dispute) epic — 7 of 8 slices done (ADR-20260726-124204).** A first-class `Reclamation`
> aggregate (keyed by `reclamationId`, multiple per order) tying the order conversation (discussion),
> the refund path (money), and the attachment framework (evidence) into one lifecycle — built out V0 per
> the product-owner decisions (PROP-20260726-013207 §9). Merged: **#153** aggregate core (open/resolve/
> reject/reopen, live mutations) · **#154** read model + queries (my-claims / claims-queue / detail;
> customerId/restaurantId scoping; the 14-day window flagged — no domain clock seam) · **#155** claim
> lifecycle woven into the order conversation timeline (`claim_events`) · **#156** evidence (opaque ref,
> storage deferred to #134) · **#157** the full SDUI (customer open-claim/my-claims/detail; staff
> claims-queue/resolve-panel) · **#159** replacement-order automation (the saga's REPLACEMENT arm places
> a genuine no-charge `replacementOf` order; deterministic-id idempotency, no double-placement) ·
> **#160** SLA (read-time `overdue` flag + staff-queue filter/badge + the `reclamation-sla` observability
> contract). **#158** landed the FOUNDATION (the `CustomerCredit` ledger — grant/consume/balance fold,
> over-spend rejected, grant idempotent per claim — + the `ReclamationProcess` saga's goodwill-credit
> arm); its three harder integrations (**refund-resolution binding**, **checkout-consume / spend credit**,
> **credit read-model query**) are flagged and tracked in
> [#207](https://github.com/TheCaptainCompany/captain-food/issues/207), so #158 stays open. Each slice:
> full ADR-0032 completeness, `make rust` green, supervised auto-merge; the two automations carry their
> own ADRs (ADR-20260726-163737 credit-saga, ADR-20260726-171736 replacement) with sequence diagrams +
> mockups + per-option pros/cons per the 2026-07-26 proposal convention. **Money paths verified:** refund
> reuses the one existing path (deferred binding), credit grant is idempotent, replacement is genuinely $0
> (no Stripe) with double-placement prevented.

> 🔎 **2026-07-26 — full functional + technical architecture review; 32 issues + 5 epics + 5 proposals
> filed.** A critical review of the whole system (domain, money, authorization, catalog, delivery,
> event store, runtime) against `main` at `835da95`. The engineering substrate reviewed **well** —
> event log, typed rejections, command-journal idempotency, webhook signature verification (all six
> adapters, fail-closed), server-side price authority, 560 tests, a real prod smoke moving Stripe test
> money end to end. The gaps cluster in three places, and they are structural rather than unpolished:
> **(1) the operational loop does not close** — no notification of any kind exists, so a paid order
> produces no signal anywhere and the back office declares no subscription; nothing times out an
> unaccepted order; there is no order detail screen and no catalog UI. **(2) the money model is a
> placeholder** — `pricing.rs` hard-zeroes every fee/split leg (0% take, no delivery fee), there is no
> Stripe Connect or payout destination of any kind, VAT is stored and never computed, and no invoice
> exists. **(3) authorization is per-role but not per-instance or per-tenant** — already tracked
> read-side by [#144](https://github.com/TheCaptainCompany/captain-food/issues/144); the **write** half
> ([#178](https://github.com/TheCaptainCompany/captain-food/issues/178)) was untracked, and restaurant
> A can accept restaurant B's orders today. Plus three runtime bugs: the projection/saga drains can
> **permanently skip events** (`position` is allocated before commit, no visibility guard,
> [#189](https://github.com/TheCaptainCompany/captain-food/issues/189)); `/projector` lag is computed
> as `head - head` so it is **structurally always 0** ([#190](https://github.com/TheCaptainCompany/captain-food/issues/190));
> and poison events advance the checkpoint with **no reprojection tooling** to repair them. Two
> compliance blockers with no owner: **allergens do not exist** anywhere in the model (EU FIC
> 1169/2011 governs distance selling, [#184](https://github.com/TheCaptainCompany/captain-food/issues/184))
> and **GDPR erasure has no technical answer** for PII in the immutable log
> ([#194](https://github.com/TheCaptainCompany/captain-food/issues/194) — [#18](https://github.com/TheCaptainCompany/captain-food/issues/18)
> deliberately excluded it). One **meta-finding worth acting on first**: `make validate` proves the API
> answers the UI on the read side, but never checks that a screen action's `variables` satisfy the
> mutation's `required` fields — which is exactly why four back-office buttons cannot submit
> (reject/cancel omit `reason`; both refund buttons send a `refundId` neither command accepts) while
> the gate reports 0 errors ([#168](https://github.com/TheCaptainCompany/captain-food/issues/168),
> [#169](https://github.com/TheCaptainCompany/captain-food/issues/169)). Filed as
> [#166](https://github.com/TheCaptainCompany/captain-food/issues/166)–[#197](https://github.com/TheCaptainCompany/captain-food/issues/197)
> with five epics ([#198](https://github.com/TheCaptainCompany/captain-food/issues/198) operational
> safety · [#199](https://github.com/TheCaptainCompany/captain-food/issues/199) economics ·
> [#200](https://github.com/TheCaptainCompany/captain-food/issues/200) catalog ·
> [#201](https://github.com/TheCaptainCompany/captain-food/issues/201) event log ·
> [#202](https://github.com/TheCaptainCompany/captain-food/issues/202) observability/scale) and five
> proposals in [docs/proposals/](proposals/) carrying the option analysis. **Prioritisation is a
> product-owner decision in the GitHub Project — nothing here is self-started.** Recurring check: a
> daily 07:00 Europe/Paris routine re-runs this review against `main` and reports only *new* drift.

> 🔁 **2026-07-26 (follow-up) — the review is now a repeatable capability, not a one-off.** Three
> more proposals close the findings that had no proposal home: **PROP-20260726-171500** write-side
> per-instance authorization (extends PROP-20260725-185140 / [#144](https://github.com/TheCaptainCompany/captain-food/issues/144)
> to commands — [#178](https://github.com/TheCaptainCompany/captain-food/issues/178), tracking
> [#205](https://github.com/TheCaptainCompany/captain-food/issues/205)); **PROP-20260726-172000**
> spec-to-UI contract integrity ([#203](https://github.com/TheCaptainCompany/captain-food/issues/203));
> **PROP-20260726-172500** delivery execution ([#204](https://github.com/TheCaptainCompany/captain-food/issues/204)).
> Scheduling + order modification ([#197](https://github.com/TheCaptainCompany/captain-food/issues/197))
> folded into PROP-20260726-164500 §5bis. **All 40 issues (#166–#205) now carry the full triage set** —
> Type, `impact/*` label, and the org fields Priority + Value Size + Impact + Effort per
> [docs/BACKLOG.md](BACKLOG.md) (Effort projected from Impact: XS/S→Low, M→Medium, L/XL→High; Priority
> is the documented default value bucket — **row order in the Project stays a product-owner decision**).
> New skill **`.claude/skills/architecture-review/`** encodes the whole procedure — dedup table against
> #144/#151/#127/#134 and the epics, a probe checklist recording the 2026-07-26 baseline for every
> check, the triage rules, and the proposal template — so the review is reproducible by any session and
> the daily run needs no prompt engineering. The loop itself is a **scheduled GitHub Action**
> (`.github/workflows/architecture-review.yml`) rather than a session-bound routine: it runs at
> **07:00 Europe/Paris year-round** (two UTC cron entries plus a timezone guard, since GH cron has no
> DST), reuses the `CLAUDE_CODE_OAUTH_TOKEN` secret the repo already has, and is version-controlled —
> so it survives sessions and needs nothing from an operator. It is fenced: no `specs/**` edits, no
> issue claims, no implementation work, and a two-line report on a quiet day.

> ✅ **2026-07-25 — [#129](https://github.com/TheCaptainCompany/captain-food/issues/129) messaging:
> functional customer send + the restaurant staff screen.** Two more green PRs finish the usable loop.
> **[#147](https://github.com/TheCaptainCompany/captain-food/issues/147) (PR #148) — functional send:**
> the customer compose couldn't produce a valid `postMessage` (missing the client-minted `messageId`
> and `originalLocale`). Added two **dispatch-time synthesized tokens** to the SDUI executor —
> `{{ $uuid }}` (a fresh UUIDv7 minted at dispatch, `executor::fill_mint_tokens`, persisted with the
> pending write so a retry reuses it — idempotent, like checkout's `orderId`) and `{{ $locale }}` (the
> client locale from the #110 `<html lang>`, `fill_locale_tokens`). The compose now sends all six
> required fields; a native completeness test proves it. **[#149](https://github.com/TheCaptainCompany/captain-food/issues/149)
> (PR #150) — restaurant back-office thread screen:** staff read BOTH the PUBLIC timeline and the
> INTERNAL notes (two resolvers merged under `conversation`), post to either visibility (a PUBLIC/
> INTERNAL `chip_multi_select` toggle → `postMessage`), and moderate — `escalateToAdmin` (reason) and
> `muteParticipant` (target role + required reason). No new component kinds/story steps. Both PRs: web
> 88 tests, wasm compiles, validate 0 errors, `check-drift` clean. **Messaging is now usable end to end
> on the customer + restaurant surfaces.** Remaining niceties: mute-duration picker, richer muted-list
> row, rider thread screen; and #133 refund-binding / #137 quick-reply catalog (now unblocked by the
> screens).

> ✅ **2026-07-25 — [#129](https://github.com/TheCaptainCompany/captain-food/issues/129) messaging is
> now REAL end-to-end (runtime + UI over the domain slices).** On top of the three domain slices
> (below), the runtime + a customer UI landed as three more green PRs, so a conversation now works
> through the live stack. **[#141](https://github.com/TheCaptainCompany/captain-food/issues/141)
> (PR #142) — write path live:** the 6 conversation mutations were auto-stubbed; adding them to the
> codegen `wired_mutation_dispatch` allowlist (all `Extra::None`) makes the regenerated resolvers
> journal (acceptance-first) + spawn the real handlers, so commands are accepted and events land in
> `domain_events`. **[#131](https://github.com/TheCaptainCompany/captain-food/issues/131) (PR #143) —
> read path live:** the full `OrderConversation` projection-table pipeline mirroring `OrderTracking` —
> a forward migration (`20260725000000`, `REQUIRED_SCHEMA_VERSION` bumped, **applied cleanly on `main`
> — `db-migrate` green**), the hand `OrderConversationProjector` (PUBLIC/INTERNAL split, translation
> merge, cross-aggregate order-status fold, admin/mute state), the Pg store + read repo, a projection-
> worker group slicing **both** `Conversation-` and `Order-` streams (keyed by orderId), schema
> injection, and the wired query bodies + emitted `From<OrderConversationRow>` — so `orderConversation`
> / `orderConversationInternalNotes` return live data. **[#145](https://github.com/TheCaptainCompany/captain-food/issues/145)
> (PR #146) — customer chat screen:** a `sdui` `order_conversation` screen (`/orders/:orderId/chat`)
> rendering the PUBLIC timeline bound to the live query with the order status woven in, `message_bubble`
> + `quick_reply_chips` component kinds (bespoke renderer arms), and a compose row. Each PR green
> (`make rust` / web 84 tests / wasm / `check-drift`), supervised auto-merge. **The one remaining gap
> to a working customer SEND:** `postMessage`'s client-minted `messageId` idempotency key isn't
> injected yet (the generic SDUI executor doesn't mint business UUIDs — only the bespoke checkout
> `place_order` flow does); a `checkout.rs`-style driver hook is the top follow-up (documented as a
> screen `gap` on #145). **Cross-cutting dependency:** per-instance read authorization (a customer may
> read only their own order's thread) is the parallel-track concern in
> [#144](https://github.com/TheCaptainCompany/captain-food/issues/144) / PROP-20260725-185140 — the
> conversation queries' "ownership enforced server-side" note relies on it. Still deferred: restaurant/
> rider thread screens; [#133](https://github.com/TheCaptainCompany/captain-food/issues/133) in-thread
> refund binding + [#137](https://github.com/TheCaptainCompany/captain-food/issues/137) quick-reply
> catalog (need the screens); [#132](https://github.com/TheCaptainCompany/captain-food/issues/132) push
> (needs [#127](https://github.com/TheCaptainCompany/captain-food/issues/127));
> [#134](https://github.com/TheCaptainCompany/captain-food/issues/134) attachments (framework).

> ✅ **2026-07-25 — [#129](https://github.com/TheCaptainCompany/captain-food/issues/129) messaging epic:
> the three spec-able DOMAIN slices are built + merged (`Conversation` aggregate).** After the epic
> approval (below), the whole spec-able domain surface landed as three green PRs, each with its full
> ADR-0032 completeness set and real domain + application Rust (the `crates/` workspace exists — the
> CLAUDE.md "deferred" note is stale). **[#130](https://github.com/TheCaptainCompany/captain-food/issues/130)
> (PR #138) — text messaging + visibility ACL:** the event-sourced `Conversation` aggregate (id =
> orderId), `OpenConversation`/`PostMessage`, PUBLIC/INTERNAL visibility as **two role-pathed query ops**
> (`orderConversation` incl. CUSTOMER; `orderConversationInternalNotes` staff-only, absent from the
> customer schema = the privacy guarantee), customer-chat **opt-out default**, and the `OrderConversation`
> projection table that folds order-status events into the thread (cross-aggregate; the
> [#131](https://github.com/TheCaptainCompany/captain-food/issues/131) status-fold, projector deferred).
> **[#136](https://github.com/TheCaptainCompany/captain-food/issues/136) (PR #139) — escalation + mute:**
> `EscalateToAdmin`→`AdminInvitedToConversation`, `MuteParticipant`→`ParticipantMuted` with the reason
> **required by the write model** (`MuteReasonRequired`, reason held out of the schema `required` on
> purpose), `UnmuteParticipant` guarded by `ParticipantNotMuted`; mute authz at the API-role level
> (CUSTOMER/RIDER excluded), the restaurant-can't-mute-admin refinement a noted resolver follow-up; mute
> state on the staff read type only. **[#135](https://github.com/TheCaptainCompany/captain-food/issues/135)
> (PR #140) — translation cache:** `RecordMessageTranslation`→`MessageTranslationAdded`, idempotent per
> (message, locale) (`MessageNotFoundInConversation`/`TranslationAlreadyRecorded`), the cached
> `MessageTranslation` riding on each `ConversationMessage`. Each PR: `make validate` 0 errors (25
> baseline warnings, none new), `make rust` green (domain 42 + application 247 tests at the tip),
> `check-drift` clean, supervised auto-merge. **Deferred/blocked (not spec-able now):** the SDUI thread
> screens; GraphQL resolver wiring + the app projector (auto-stubbed, the accepted deferred state);
> [#133](https://github.com/TheCaptainCompany/captain-food/issues/133) in-thread refund binding + [#137](https://github.com/TheCaptainCompany/captain-food/issues/137)
> quick replies (need the thread screens — no standalone domain DSL);
> [#132](https://github.com/TheCaptainCompany/captain-food/issues/132) push (needs
> [#127](https://github.com/TheCaptainCompany/captain-food/issues/127));
> [#134](https://github.com/TheCaptainCompany/captain-food/issues/134) images (via the framework).

> ✅ **2026-07-25 — [#129](https://github.com/TheCaptainCompany/captain-food/issues/129): epic
> APPROVED + reserve slice landed — in-app order conversations (messaging)
> (PROP-20260725-013008, ADR-20260725-015921).** The product owner approved the messaging epic ("do
> this completely"). This is **post-V0** and the Rust runtime doesn't exist yet, so the only *buildable*
> slice now is the one the proposal marks "shippable now" (§5/§9.1): **reserve the data model**. Landed
> spec-narrative only — a comment in `specs/entities.yaml` beside `Order` reserving the future
> `Conversation` aggregate as **keyed by `orderId`** (a conversation's identity IS its order) and the
> principle that `View_OrderConversation` folds BOTH the order's status events AND the conversation's
> message events for that `orderId` (so "order status participates in the thread" is free, no retrofit).
> No validated DSL added (would trip ADR-0032 completeness before the §8 decisions are made), so no
> generated drift. The ADR adopts the mechanism reuse (event-sourced aggregate, role-pathed ACL for
> PUBLIC/INTERNAL visibility, acceptance-first posting, the EXISTING refund path for in-thread refunds,
> the #127 cascade for push) and resolves the decidable §8 decisions (translation on-demand+cache via a
> BFF proxy; in-thread refund triggers the existing refund command; mute matrix restaurant→customer/
> rider, admin→anyone, customer→none; post-V0 phasing). **Two decisions left OPEN on their slices**:
> image retention window (GDPR, with [#18](https://github.com/TheCaptainCompany/captain-food/issues/18))
> and rider-participation default. The rest of the epic is decomposed into **8 sub-issues** (§9), each
> independently shippable behind the acceptance-first write model and carrying its own ADR-0032
> completeness set. Spec/docs-only → straight to `main`; `make validate`/`generate` 0 errors, no drift.

> ✅ **2026-07-25 — [#110](https://github.com/TheCaptainCompany/captain-food/issues/110): translation
> hygiene gates + the runtime locale-resolution chain (PROP-20260724-133700 §1c,
> ADR-20260725-013315).** The catalog had no gate against rot and the runtime hard-coded `fr`. Now two
> blocking `make validate` rules — **`translation-locale-missing`** (every key carries every
> `SUPPORTED_LOCALES` message; one centralized locale list replaces three hard-coded `["en","fr"]`)
> and **`translation-key-unused`** (a key referenced by no screen `$ref` and no `code_refs` entry is a
> hard error) — plus **`specs/translations.code_refs.yaml`**, the declared manifest of keys consumed
> by hand-written Rust (`order.status.*` via tracking.rs), guarded by `translation-code-ref-unknown`
> and a companion codegen test that greps `crates/**/*.rs` so a stale entry is itself caught. The
> gates caught two real drifts on landing (`order.not_found` referenced by tracking.rs but absent from
> the catalog — added; `order.tracking_title` over-declared in code_refs when it is screen-`$ref`'d —
> removed). **Runtime:** `resolve_locale(Customer.locale → cookie → Accept-Language/device → fr)` with
> `normalize_locale` reducing `fr-FR`/`EN`/`en_US` to a bare SUPPORTED tag; SSR (`hosts.rs`) reads the
> `captain_locale` cookie + `Accept-Language` and threads the resolved locale through every render
> site — no more hard-coded `fr`; `<html lang>` carries it and hydrate reads it back from the DOM so
> the client can't disagree with the shell. Follow-ups (noted): a visible language switcher
> (`changeLanguage` is unreferenced by any screen today — cookie contract + SSR read are in place) and
> a per-request JWT→`Customer.locale` SSR read. 49 codegen tests + web/server suites green; wasm
> hydrate compiles.

> 🚧 **2026-07-25 — [#118](https://github.com/TheCaptainCompany/captain-food/issues/118): OVH SMS
> delivery for phone OTP (PROP-20260724-233605).** The #117 adapter can ask Supabase to send a phone
> OTP, but Supabase has no SMS transport wired — so phone verification silently never delivered. Now
> the Supabase Auth **Send-SMS hook** is fulfilled by OVHcloud (ADR-20260722-174500: OVH over Twilio
> for FR price + EU residency): `OvhSmsClient` (crates/infrastructure/integrations/ovh_sms.rs) signs
> OVH API v1 requests (`X-Ovh-Signature = "$1$"+sha1_hex(AS+CK+METHOD+URL+BODY+TS)`) and POSTs
> `/sms/{service}/jobs` with `noStopClause:true` (transactional OTP, no STOP footer); the hook
> boundary (integrations/supabase_sms_hook.rs) verifies Supabase's **standard-webhooks** signature
> (`webhook-signature` HMAC-SHA256, ±5-min replay window) and extracts `(phone, otp)`. The server
> route `POST /auth/sms-hook` (crates/server/auth_routes.rs) wires them: verify → 401 on bad sig,
> parse → 400, deliver → 204 / 502 on OVH failure, **503 when SMS is unconfigured** (no OVH client /
> no secret — fail-closed, never a half-open delivery path; the Stripe/identity env-gate pattern).
> Env: `OVH_APPLICATION_KEY/SECRET`, `OVH_CONSUMER_KEY`, `OVH_SMS_SERVICE_NAME` (+ optional
> `OVH_ENDPOINT`/`OVH_SMS_SENDER`) + `SUPABASE_SMS_HOOK_SECRET`. 7 unit tests (OVH signature vector +
> env gating, hook verify/tamper/replay/parse, route fail-closed). **Not verifiable live until an OVH
> account exists** (user provisions credentials + the Supabase Send-SMS hook URL later; 20 free SMS
> credits noted); the code/build/deploy-health path is verified. SMS-only for V0 — WhatsApp OTP
> deferred as a post-V0 UX choice ([#125](https://github.com/TheCaptainCompany/captain-food/issues/125)).

> ✅ **2026-07-24 — #117: the real Supabase Auth adapter — login machinery is functional (PR #124,
> PROP-20260724-225804).** The only `IdentityService` was the fail-closed stub, so no login worked.
> New `SupabaseIdentityService` (crates/infrastructure/integrations/supabase_auth.rs, beside the
> stub) over the Supabase Auth REST API: `send_phone_otp`/`send_email_magic_link` →
> `POST /auth/v1/otp`, `verify_*` → `POST /auth/v1/verify` (phone: `{type:sms,phone,token}`; email:
> `{type:email,token_hash}`), `refresh_session` → `POST /auth/v1/token?grant_type=refresh_token`;
> `authRef` = the Supabase `user.id`, and the verify responses' access/refresh/expires flow into the
> #112 parked-session trio. Typed rejections mapped from Supabase 4xx (expired→`VerificationCodeExpired`,
> else `InvalidVerificationCode`/`InvalidVerificationToken`). Env-gated `from_env()` on
> `SUPABASE_URL` + `SUPABASE_PUBLISHABLE_KEY`; the composition root (`identity_service_impl`) uses it
> when set, else the fail-closed stand-in — the Stripe pattern; unconfigured = anonymous-only, never
> half-configured. Project-agnostic (reads `SUPABASE_URL`), so the ADR-20260722-174500 repoint from
> the DATA project to `captain-identity` is a pure env change (verified finding: it currently points
> at the data project `zcshlzhiinwmpzujuiep`; repoint deferred to the captain-identity migration). 3
> adapter unit tests (env gating, 4xx classification, verify-response parsing); wasm + workspace
> tests green. **Email magic-link is verifiable live now (native Supabase email, no OVH);** phone OTP
> verify is the same code path, awaiting SMS delivery via [#118](https://github.com/TheCaptainCompany/captain-food/issues/118).

> 🚧 **2026-07-24 — Captain ID: a shared auth SERVICE for all products (new repo
> [TheCaptainCompany/captain-identity](https://github.com/TheCaptainCompany/captain-identity),
> product-owner directive).** Auth is company-wide, not per-product — the "Captain ID" concept
> reserved by ADR-20260722-225945 / ADR-20260722-174500 is now a real repo. Decision (its
> ADR-20260724-172808 + AskUserQuestion): a **deployable auth service** at `id.thecaptaincompany.com`
> owning identity (phone `authRef`), the Supabase wrapper (ADR-0015), OTP verify/send, httpOnly
> session-cookie minting (the #112 design generalized), the Supabase→OVHcloud SMS hook, and the
> JWKS/`captain_role` contract; products keep their own role paths + `@auth` ACL + domain data (the
> ADR-20260722-174500 controller split). Rollout = **scaffold + reserve**: the repo is
> established structure-first (README, ADR, proposal, tracking issue, operating-model conventions),
> but Captain.Food's LIVE #112 auth code STAYS here and migrates once the service shape is proven —
> nothing in-flight moved. #117 (Supabase adapter) + #122 (OVH SMS hook) stay in captain-food for
> now, cross-linked to their permanent home [captain-identity #1](https://github.com/TheCaptainCompany/captain-identity/issues/1).
> **Repo visibility: created PRIVATE** (auth service default) — revisit vs the public-repo/free-GHA
> operating model before CI/image-build is added there.

> ✅ **2026-07-24 — #114: sheet input dispatch — OTP auto-submit + chip on_change fires the #62
> survey (PR #121).** The #93 delegated driver covered BUTTONS only; two sheet interactions were
> dead. The executor's action parsing generalized to a PREFIX (`ActionSpec::from_node_prefixed`) so
> `on_complete`/`on_change` reuse the entire variable-resolution machinery; `trigger_attrs` stamps
> the DOM contract + `data-trigger`. Renderer: `otp_input` carries its `on_complete` action +
> `data-complete-len` on the `<input>`; `chip_multi_select` renders option chips + a hidden input
> (id = field id) holding the selected value, so the existing form-field binding fill reads it with
> zero new resolution. interact.rs: an `input` listener auto-dispatches when an OTP field reaches
> its length (6th digit → `verify_otp`, the #94 flow), and the click listener stashes a picked
> chip's value into the group's hidden input before dispatching. **This fires the #62
> delivery-satisfaction survey from the UI for the first time** (the timeliness chips carry
> `record_delivery_satisfaction`). Code-only, no spec change. 79 web tests (2 new); wasm green.

> ✅ **2026-07-24 — #113: first-class pagination on `queries/restaurants` (PR #120,
> PROP-20260724-164102).** The #107/#108 OOM hotfix's `LIMIT 200` was an invisible adapter guard —
> the marketplace silently showed ≤200 and couldn't page. Now a contract: new `PageLimit`/
> `PageOffset` scalars, `restaurants` gains optional `limit`/`offset`, and
> `PgRestaurantRepository::list()` applies `LIMIT least(limit ?? 24, 200) OFFSET offset` — the 200
> is a NAMED clamp ceiling (`RESTAURANT_PAGE_MAX`), an over-max request returns the max (never an
> error, never an unbounded scan), default page 24 (`RESTAURANT_PAGE_DEFAULT`). `RestaurantFilter`
> carries the two; the generated resolver maps them off the input (the #97 generated-name
> machinery). No new op ⇒ ADR-0032 unaffected (optional args). Offset/limit chosen over cursor for
> V0's single-`ORDER BY` read model (cursor kept as the deferred alternative in the proposal). Pg
> pagination test (clamp/offset/default). `make rust` green, no drift. Follow-up (client): per-rail
> limits + "load more" offset wiring.

> ✅ **2026-07-24 — #112: client auth-token wiring — the BFF-minted httpOnly session cookie (PR #116,
> PROP-20260724-150500).** Identity stopped at the server: `VerifyPhone` SUCCEEDed but the client
> stayed anonymous (CUSTOMER path unreachable, staff surfaces unusable, WS/SSR tokenless,
> `sign_out` dead). Fixed WITHOUT ever exposing a token to JS. (1) **Spec**: `identity.verify_phone_otp`
> + `verify_email_token` outputs gain `accessToken`/`refreshToken`/`expiresIn` (the #50
> output-extension precedent) + a new `refresh_session` op; new `auth_sessions` transport table
> (integration_connections.yaml — never event-sourced/api.yaml/projected), migration `20260724150500`
> + `sweep_retention()` extension, `REQUIRED_SCHEMA_VERSION` bumped; the codegen `table_sql_type`
> learned `bytea`. (2) **Park**: `application::auth_sessions` port (+ mem/noop doubles); the async
> `verify_phone` handler parks the provider session keyed by the acceptance messageId
> (`actor.cause_id`), owned by the journaling `session_id` — a parking failure never fails the
> verification. (3) **Encrypt at rest**: `PgAuthSessionStore` (aes-gcm) — AES-256-GCM under
> `AUTH_SESSION_KEY`, `claim` is a single-read `DELETE…RETURNING` scoped by owner + unexpired
> (NULL-safe), no oracle. (4) **Exchange**: `POST /auth/session { messageId }` + matching
> `X-SESSION-ID` → `Set-Cookie: captain_auth` (httpOnly/Secure/SameSite=Lax) + `/auth`-scoped
> refresh; `/auth/refresh`, `/auth/logout`. (5) **The one seam**: `AuthContext` gained a cookie
> fallback beside the `Authorization` header — lighting authenticated HTTP, the WS handshake
> (browsers send cookies on upgrade), and #92's SSR 302 in a single change. (6) **Web**:
> `verify_otp` surfaces the messageId; `pickup_session` POSTs it credentials-included. Fail-closed:
> no key/DB ⇒ noop store ⇒ anonymous still works, no plaintext ever. Tokens never in GraphQL, the
> event log, the journal, or client storage. 231 application + 24 server + web/infra tests; wasm
> green. Twilio→OVHcloud SMS spec wording corrected (the launch decision, ADR-20260722-174500).
> **Follow-up [#117](https://github.com/TheCaptainCompany/captain-food/issues/117)**: the real
> Supabase Auth adapter (only the fail-closed `IdentityService` stand-in exists — the machinery is
> built against the port, verify responses will carry the real session then).

> ✅ **2026-07-24 — #95: rider availability EXPOSED — `changeRiderStatus` closes the
> `rider_toggle_online` gap (PR #104; plan-approved spec change).** The domain machinery was
> complete (ChangeRiderStatus command + Rider actor inbox + lifecycle machine (#23) + generated
> handler + TestRiderStatusChanged/TestRiderStatusChangeIsRejected) — only the API surface was
> missing. Landed: api.yaml `changeRiderStatus` (roles [RIDER, ADMIN], the rider's own toggle +
> admin lifecycle), a rider `SetAvailability` story step (op-uncovered-by-story gate), the
> `rider_toggle_online` gap flipped to a mutation binding in `rider.yaml` (+ `actions_used`), and
> ONE codegen arm in the generated-handler dispatch table (`changeRiderStatus →
> change_rider_status`). Regeneration flips `ActionKey::RiderToggleOnline` gap→mutation with
> `ChangeRiderStatusInput` (#97's generated name), so the rider topbar toggle becomes dispatchable
> through the #93 wiring with zero client code. **The known-warning baseline drops 26 → 25**: the
> standing `command-no-mutation commands.yaml/ChangeRiderStatus` warning is RESOLVED by the
> exposure. Tests updated to the new reality (the gap-disabling proof moved to the auth sheet's
> passkey button; a new executor test pins the toggle to `changeRiderStatus`/its input type).
> 77 web + 36 codegen tests, wasm green, `make rust` 0 errors/no drift. `RiderStatusChanged` still
> feeds no `View_*` (dispatch targeting by availability = the deferred read-model decision, noted
> in the issue).

> ✅ **2026-07-24 — #92: SSR pages ship LIVE data via an in-process transport + the hydrate-side
> auth guard (PR #103; ADR-20260723-172013 residuals 1+2).** Split 4 served SSR SHELLS; the screens
> spec contracts `rendering_strategy: SSR_first` / TTFB <= 500ms. (1) **`SchemaTransport`**
> (`crates/server/src/web_ssr.rs`) — the in-process `Transport` impl the seam was DESIGNED for
> (graphql.rs: "an in-process transport for SSR could bypass HTTP entirely"): executes resolver
> documents directly against the role-filtered schema with the PUBLIC role + anonymous Principal +
> no session — SSR can never see more than an anonymous first request (the per-field ACL applies
> identically). Schema built once, shared by the GraphQL routes and the host fallback
> (`SsrExec` Extension). (2) **`render_path_with`** — resolves the matched screen's
> `data_requirements` (route `:params` feeding args exactly like hydrate; gap resolvers refused
> before any call; a resolver error degrades that one slot, never 500s) before rendering:
> marketplace/storefront pages now carry restaurants/catalog in the initial HTML, the #82 pinned
> arg exercised server-side. `requires_auth` screens SKIP the fetch (a document GET carries no
> credentials — their session-scoped reads could only answer empty) and ship as shells.
> (3) **Trait fix en route**: futures holding `&dyn Transport` across awaits weren't `Send` (the
> axum handler requirement) — new platform-conditional `MaybeSync` supertrait (native transports
> must be `Sync`; the browser's reqwest client is not and carries no bound). (4) **Auth guard
> (client-side)** — `requires_auth` screens open the auth sheet OVER the content (the DSL's
> if_guest pattern) or bounce to the surface root; the DoD's server-side 302 is EXPLICITLY
> deferred: auth state lives in browser localStorage, no auth cookie exists yet — recorded on the
> issue as the follow-up landing with the auth-token wiring. 76 web + 23 server-lib tests
> (in-process introspection, error envelope parity, live-data SSR with call-count + shell-no-fetch
> assertions); wasm green; `make rust` 0 errors/no drift.

> ✅ **2026-07-24 — #94: bottom sheets — generated sheet trees, the sheet host, form-field bindings
> + the OTP identity flow (PR #102; ADR-20260723-172013 residual 4 + #17's identity item).** The
> DSL's `bottom_sheets:` (location picker, auth/OTP, item detail, rating — the #62 survey carrier)
> now compile and render. (1) **Emitter**: each surface's `bottom_sheets:` emits a generated
> `SHEETS` table (`Sheet { id, node }`, same Node trees; `sections` joined the child-key
> whitelist); fail-closed vocabulary check caught `type: list` used unregistered by the location
> picker — registered (content group), the same corrective class as #87's `cta_section`.
> (2) **Renderer**: every SDUI screen mounts its surface's sheets HIDDEN after the content
> (`data-sheet-id`; SSR + hydrate identical); real `bottom_sheet`/`list` markup; input fields carry
> their DSL `id` on the `<input>` (the binding target). (3) **Executor**: action variables now
> accept the sheets' BARE spelling (`action.phone`) alongside `action.variables.*`; an unresolved
> `{{ <field>.value }}` binding travels as null AND is reported in a `data-var-bindings` map —
> interact.rs fills those from the LIVE inputs by element id at dispatch time, and
> `open_bottom_sheet`/`close_sheet` toggle the sheet DOM (one sheet at a time). (4) **`auth.rs`** —
> the OTP identity flow with the COMMAND payloads as authority (split `dialingCode`/
> `nationalNumber`, minted `customerId`, the SESSION id in the payload — the CartBindingProcess
> contract): `request_otp` → `verify_otp` → on SUCCEEDED the `me` read is the proof; a wrong code
> is the anticipated `InvalidOtp` rejection, native-tested end-to-end against a fake transport.
> 36 codegen + 75 web tests, wasm32 green, `make rust` 0 errors/no drift. Residuals: `otp_input`
> on_complete auto-submit + chip `on_change` dispatch (click wiring covers buttons only),
> item-sheet option state, passkeys (declared gap).

> ✅ **2026-07-24 — #98: tenant root serves the storefront; unclaimed slugs get the join landing
> (PR #101; production bug found by the owner: `chezmarco.captain.food` answered 404).** The
> storefront surface has no screen at `/` (the restaurant screen is `/r/:slug`; discovery moved to
> the marketplace in #75), so every tenant front door was a dead end. (1) **Tenant-root rule** —
> new `web::router::resolve(host, path)`: on a `{slug}.captain.food` storefront, `/` IS the
> restaurant screen with the slug taken from the HOST (ADR-0036: the host is the tenant selector);
> both the SSR entry (`render_path`) and the hydrate entry go through it so the two paths cannot
> disagree. (2) **Claim landing** — the host fallback now checks a tenant slug against the
> restaurant read model (`hosts::TenantLookup`, the `restaurants` repo Arc shared into an
> Extension): registered → storefront; POSITIVELY absent → a claim-your-subdomain landing
> ("{slug}.captain.food est disponible pour votre restaurant", CTA →
> https://join.captain.food/#rejoindre — every unclaimed subdomain is an acquisition surface,
> product-owner directive) served on EVERY path of the host; lookup error or no database →
> **FAIL OPEN to the storefront shell** (a DB hiccup must never show "available" for a real
> restaurant). Slug reflection is injection-safe (`is_valid_slug` gates `Tenant()`).
> Tests: web tenant-root resolution matrix + 3 server tests over a stub read model (registered /
> unclaimed-on-every-path / fail-open incl. no-DB). 71 web + 21 server-lib tests green; `make rust`
> 0 errors/no drift. Residual: the storefront root still hydrate-fetches its data (#92 pre-fills).

> ✅ **2026-07-24 — #93: SDUI buttons DISPATCH — action wiring + pending UX + push-first verdicts +
> boot resume (PR #100; the #17 UX tail + ADR-20260723-172013 residual 3).** The renderer rendered
> every button's `action.*` props as inert data; now the screens WORK. (1) **`executor.rs`** (pure,
> native-tested against the REAL generated trees): `ActionSpec::from_node` parses a node's dotted
> `action.*` props, resolving `{{ … }}` variable bindings against the screen data AT RENDER TIME
> (what the user saw is what dispatches; unresolved bindings travel as null — the server judges);
> `ActionPlan` = Mutation (key + resolved input) | Client(`ClientEffect`) | Disabled(reason) —
> gap/unknown/unwired actions render DISABLED with the reason as tooltip, never a silent no-op;
> `on_success.route` substitutes `{{ variables.* }}` from the resolved input (the checkout
> confirmation pattern). (2) **DOM contract**: `button_attrs` stamps the plan onto data attributes
> (`data-action`/`data-vars` JSON/`data-loading`/`data-on-success`/route/sheet/number) — SSR'd and
> hydrated DOM identical, so ONE delegated listener drives every button, zero per-button closures.
> (3) **`interact.rs`** (hydrate-only glue): the delegated click listener; mutation clicks freeze
> the button (loading label + `data-busy` double-tap guard) → `pending::dispatch_persisted` →
> **push-first verdict** (`operationStatusChanged` on a shared reconnecting socket, interpreted by
> the SAME pure operation→outcome authority as the poll — extracted `outcome_from_operation`, used
> by `pending::settle_from_push` which clears the record on a terminal frame with zero reads) with
> the bounded poll as fallback after a 2 s push head-start; REJECTED/FAILED → toast
> (server-localized message, errors.yaml code fallback); pre-acceptance transport failure stamps
> `data-retry` = the persisted messageId so the NEXT click goes through `pending::retry` (same id —
> duplicate-proof); boot runs `pending::resume_pending` and toasts settled intents.
> `navigate`/`phone_call` execute; sheet/clipboard/share re-emit as a `captain:action` CustomEvent
> for the #94 sheet host (inspectable, not swallowed). WS `Handle` became Clone + `unsubscribe`.
> 70 web tests (7 new — executor specs against the generated backoffice accept button and the rider
> gap toggle, push-settle, SSR DOM-contract) + wasm32 green; `make rust` 0 errors/no drift.
> Residual to #94: sheet host, auth actions, authenticated WS (staff surfaces' push needs the JWT).

> ✅ **2026-07-24 — #97: GraphQL input-type names are GENERATED, not convention-derived (PR #99;
> closes the #80 honesty residual; prioritized ahead of #93 by product-owner directive).** The
> client built documents with convention-derived input types (`<Pascal>QueryInput`,
> `<PascalMutation>Input`, `<Pascal>SubscriptionInput`). The convention was WRONG for mutations: the
> SDL names a mutation's input after its **COMMAND** (`<Command>Input`), which only coincides with
> the mutation name for most ops — `configureGbpOrderLink` →
> `ConfigureGoogleBusinessProfileOrderLinkInput` and `verifyGbpOrderLink` are live divergences
> (outside today's allowlist, so nothing had broken YET). Now the data-layer emitter also emits
> `ResolverKey::input_type()` (Some iff the bound query takes args), `ActionKey::input_type()`
> (the command-derived name for mutation kinds) and `subscription_input_type(op)`; the web document
> builders (`query_document`/`mutation_document`/`SubscriptionKey::document`) READ them —
> `pascal()` derivation deleted from the crate. Same lesson as #82, one level up: a name the client
> sends is read from the source of truth, never re-derived. Documents byte-identical (existing
> string assertions prove parity); new codegen divergence test + a web totality test (every
> mutation-kind action has an input type, gaps never do). codegen 35 tests, web 63, wasm green,
> `make rust` 0 errors/no drift.

> ✅ **2026-07-24 — #17: two-step writes survive reloads — persisted pending-operation store +
> same-messageId retry (PR #91; realizes ADR-20260720-015500's client rule).** Splits 2-4 of #21
> built the dispatcher/checkout/subscriptions, but the contract's durability half — **persist the
> minted `messageId` until a terminal status is observed** — was unimplemented: a reload mid-flight
> lost the handle (the exact V0 mobile failure). New `crates/web/src/pending.rs`: `PendingWrite`
> (messageId + action + FULL input — for `place_order` the input carries the client-minted
> `orderId`, so a reload recovers BOTH the idempotency id and the confirmation route),
> `PendingStore` seam (localStorage `captain.pending-writes` on hydrate, mirroring session.rs;
> injectable memory double), `dispatch_persisted` (recorded BEFORE the send — a network failure in
> the crash window keeps the id), `settle` (clears ONLY on terminal SUCCEEDED/REJECTED/FAILED;
> poll exhaustion keeps the record), `retry` (re-send under the ORIGINAL id → `duplicate: true`,
> converges on the first outcome — no double order), `resume_pending` (boot-time: re-resolve every
> stored id via the idempotent `operationStatus` read, tight caller-set bounds). Non-dispatchable
> kinds (client/auth/gap) never pin the queue. `actions.rs` gained `dispatch_with_id` (dispatch =
> mint + that); `checkout.rs` gained `submit_persisted`. 62 web tests (9 new incl. the
> record-before-send crash test and the checkout-continuity round trip); wasm32 green; `make rust`
> 0 errors/no drift. Issue #17's remaining UX items (PENDING spinners, generic-button dispatch
> wiring, subscription-push resolve as the poll's fast path) stay in the ADR-20260723-172013
> residual set.

> ✅ **2026-07-23 — #21 frontend renderer split 4/4 (#87 "Frontend split 4/4 - per-component markup,
> customer polish + restaurant/rider screen adoption", PR #89, ADR-20260723-172013) — #21 COMPLETE.**
> The renderer goes GENERIC and the platform SERVES it. (1) New codegen emitter `emit_web_screens`:
> every `screens/*.yaml` surface compiles to `crates/web/src/generated/screens.rs` — static `Screen`
> tables (route/roles/`requires_auth`/`sdui`/`data_requirements` bound to `ResolverKey`) + the
> component tree as `Node` data (translation refs → `I18n(key)`, `{{ … }}` → `Binding(path)`, nested
> config → dotted props; `{ component: topbar }` chrome expands at emit time; children only under
> `components`/`content`/`fields`/`slots`). FAIL-CLOSED on unregistered component types — which
> immediately caught two live spec drifts: `cta_section` used unregistered by the partner landing
> (now the split's ONE registry addition) and `filter_bar.filters[].type: dropdown` being config
> vocabulary, not a component. `sdui:false` screens emit an empty tree but register their route.
> (2) **Two NEW surfaces** (plan-approved DSL): `restaurant_backoffice.yaml` (orders queue/
> deliveries board/refunds queue/#62 satisfaction; RESTAURANT+RESTAURANT_ACCOUNT) and `rider.yaml`
> (job list/detail; RIDER) + en/fr sidecars — ZERO new API ops, existing component vocabulary;
> `rider_toggle_online` is an explicit gap (no rider-status mutation exists). (3) **Generic
> renderer** (`renderer.rs` rewrite): walks the generated trees with real markup for the
> load-bearing kinds (chrome/nav, lists+cards with per-row templates, sections, tab bars, text,
> buttons, inputs, menu sections; the rest render tagged auditable containers), `{{ path | filter }}`
> bindings into resolved resolver data (`format_currency` fr-style), i18n from the EMBEDDED generated
> catalog (fr default/en fallback, missing key renders `[key]` — fail-visible), and the
> `restaurants.featured → featured_restaurants` template-alias convention. (4) **Router**
> (`router.rs`): host→surface per ADR-0036's RESERVED subdomains (`restos.`/`riders.` — mirrored
> with the server's `classify_host`, NOT new `back.`/`rider.` labels), path→screen with `:param`
> capture feeding resolver args (`:orderId`→`order.byId#id` bridge); route-aware `hydrate()` fetches
> `data_requirements` and re-renders. (5) **Full pipeline** (user-approved scope): the server's host
> fallback SSRs matched screens (`web::router::render_path`, SSR-shell + hydrate-fetch model;
> unknown path 404s; non-app hosts keep text landings), `/assets` serves `WEB_ASSETS_DIR`
> (tower-http); the Docker build gains wasm32 + `wasm-bindgen-cli` PINNED to the crate's `=0.2.126`
> (CLI refuses mismatch; bump together) + a second chef cook for the wasm tree, emitting
> `web.js`/`web_bg.wasm` into the image; `ci.yml` adds the cheap `make wasm` compile-check INSIDE the
> required `codegen` job (a separate workflow would not be a required check). Verified: codegen 34
> tests (2 new emitter tests incl. the fail-closed abort), `web` 54 tests (surface-wide SSR sweep,
> fr/en i18n, binding lists, alias convention, router matrix, render_path × 4 surfaces), workspace +
> wasm32 builds green, `make rust` 0 errors/no drift. Honest residuals (in the ADR): server-side
> data resolution for SSR, screen-level auth redirects, generic-button → `ActionKey` dispatch
> wiring, sheets/overlays, runtime JSON screen delivery (ADR-0033's deferred contract), the
> rider-status op. **Post-merge:** the FIRST image build failed — `rustup target add` in an early
> Docker layer targeted the BASE image's toolchain, while cargo-chef's skeleton carries
> `rust-toolchain.toml` so the cook-time toolchain was the file's, freshly installed WITHOUT the
> wasm target. Fixed at the root in PR #90 "🐛 Fix the image build: wasm32 target belongs to the
> toolchain file's toolchain" (`targets = ["wasm32-unknown-unknown"]` in `rust-toolchain.toml` +
> the explicit add moved into the wasm-cook RUN). **VERIFIED LIVE at `255bdc8`**: `/health`
> `X-VERSION: 255bdc8`; `live.captain.food/` SSRs the marketplace home (generated tree, `data-c`
> tags); `restos.` serves `orders_queue` ("File des commandes"), `riders.` serves `jobs`
> ("Mes courses"), a tenant `/checkout` serves the Stripe shell; `/assets/web.js` (36 KB) +
> `web_bg.wasm` (707 KB) serve 200; an unknown path 404s.

> ✅ **2026-07-23 — #21 frontend renderer split 3/4 (#86 "Frontend split 3/4 - checkout + order
> tracking (non-SDUI: Stripe element, subscriptions)", PR #88).** The NON-SDUI MONEY PATH lands in
> `crates/web`, on the #80 data layer. (1) **`subscriptions.rs`** — the graphql-transport-ws client,
> split sans-IO: `WsClient` is a pure text-in/reactions-out protocol state machine (init→ack
> handshake, subscribe QUEUED until ack then flushed, `next`/`error`/`complete` routing with
> unknown-id frames dissolving, `ping`→`pong`), natively unit-tested with zero network; the
> `hydrate`-only browser driver owns one `web_sys::WebSocket` (subprotocol `graphql-transport-ws`)
> and reconnects through bounded exponential backoff (1s→30s cap). Auth + `X-SESSION-ID` ride the
> `connection_init` payload (browsers cannot set WS headers — mirrors the server's
> `on_connection_init`). Subscription selections REUSE the generated resolver selection for the same
> api.yaml type (`orderStatusChanged` ↔ `order.byId` etc.), so push and pull cannot drift; the
> consumer contract is SUBSCRIBE + RE-SYNC on every (re)connect (free-tier sockets die on restarts —
> push is an accelerator, never the only truth). (2) **`checkout.rs`** — the acceptance-first flow:
> the client MINTS `orderId` (spec: client-generated), dispatches `place_order` two-step, and awaits
> the intent by READING `paymentStatus.byOrder` until `clientSecret` exists (bounded poll;
> `paymentStatusChanged` is the push accelerator); `expectedTotal` travels for the server's
> `PriceMismatch` guard; a REJECTED checkout resolves as a normal business outcome. (3)
> **`stripe.rs`** — the element seam: client holds ONLY `clientSecret` + publishable key (card data
> stays in Stripe's iframe); `confirmPayment`'s result is UX-only — the capture verdict is the
> inbound webhook fact read back from our own API. Minimal wasm-bindgen surface (Stripe/elements/
> create/mount/confirmPayment). (4) **`tracking.rs`** — pull-then-push over one `TrackingState`:
> `load` then `apply` with REPLACE semantics + a `statusChangedAt` stale-frame guard (a late
> out-of-order frame never regresses the screen; a null re-read keeps last known state); the status
> hero mirrors the spec's `status_config` (all 9 OrderStatus values); post-delivery `rating_sheet`
> actions (rider thumb, #62 timeliness survey, ADR-012 tips array) dispatch through the two-step
> layer. Both screens render SSR with the renderer's `data-c` tagging from the same tree the
> hydrate build shares. `cargo test -p web` 41 green (21 new); the wasm32 `hydrate` build verified;
> `make rust` green (0 errors, no drift). Deferred to split 4 (#87): router/mount plumbing (live
> form state, interactive sheets), per-component markup, restaurant/rider adoption.

> ✅ **2026-07-23 — #82: pinned SDUI resolver args are validated against the bound query
> (ADR-20260723-145959).** A `screens/*.yaml` resolver may pin static args on its query binding; both
> customer front offices pinned `restaurants.featured` with the key **`listKey`**, but
> `api.yaml#/queries/restaurants` declares it as **`list`** — so the home screen's featured rail would
> have sent an unknown input field and been rejected by the server rather than showing the RECOMMENDED
> shelf. The validator never caught it: §1 proves a `$ref` RESOLVES and §1b (ADR-20260722-152201)
> proves WHAT KIND it resolves to, but neither looks INSIDE `args:` — and this was the only `args:` pin
> in the whole spec, so the typo survived until #80 "Frontend split 2/4" became the first code to
> consume a pin. Both parts landed: the pin is corrected on both surfaces, and (ADR-0032 — the fix for
> a CLASS of error is a new check) new `validate_resolver_args` in validator §11 adds two fail-closed
> rules — **`resolver-unknown-arg`** (pinned key is not an argument of the bound query; the message
> lists the real ones, and an arg-less query rejects every pin rather than skipping the check) and
> **`resolver-invalid-arg-value`** (declared enum-typed arg, pinned literal not a member; `array: true`
> pins check each item). Errors, not warnings. Explicit non-goals: required-arg COVERAGE is not checked
> (a pin is a static default — `execute_resolver` merges caller variables OVER it, caller winning), and
> scope is `resolvers` only (`actions:` has no `args:` pin in the DSL or in `ActionDef`). No
> `REF_CONTRACT` entry — pinned values are plain scalars, not refs. **The rule was proven against the
> live bug before the fix**: with the check in place and the pin uncorrected, `make validate` reported
> `resolver-unknown-arg` on BOTH surfaces. Ripple: `crates/web/src/generated/data_layer.rs`
> regenerates, and the hand-written `crates/web/src/graphql.rs` doc comment + two pin-merge tests move
> to `list` (the override test's `NEARBY` — never a `RestaurantListKey` member — becomes `TOP_DEALS`).
> 4 new codegen tests incl. the #82 regression; `make rust` green (0 errors, no drift).

> 🚧 **2026-07-23 — #21 frontend renderer split 2/4 (#80 "Frontend split 2/4 — resolver/action wiring
> + session layer (#12) + two-step mutations (#17)", PR #81 "Frontend split 2/4: resolver/action wiring
> + session layer + two-step mutations").** The SDUI DATA LAYER lands, following #68 "Frontend split
> 1/4 — Leptos renderer skeleton + generated component registry". (1) New codegen emitter
> `emit_web_data_layer` turns every `screens/*.yaml` `resolvers`/`actions` block into
> `crates/web/src/generated/data_layer.rs`: `ResolverKey` (bound api.yaml query + the DSL-pinned
> static args + a GENERATED GraphQL selection set) and `ActionKey` (`ActionKind`
> client/mutation/auth/gap + bound api.yaml mutation) — a renderer-level SHARED allowlist unioned
> across every surface (same rule as the component registry; a key bound differently by two surfaces
> aborts the emitter), and `gap:` entries emitted UNBOUND so they fail closed at the dispatcher rather
> than silently no-op. Selection sets are expanded from the bound query's `returns` type with a cycle
> guard on the ref path + `SELECTION_MAX_DEPTH`; a truncated descent OMITS the field (a bare object
> field is invalid GraphQL) and the rule bubbles up when a type is left with nothing selectable.
> (2) Three hand-written runtime modules: `session.rs` (client-minted UUIDv7 `SessionId` persisted in
> localStorage so it SURVIVES A RESTART — the anonymous cart and `operationStatus` ownership are keyed
> on it; `X-SESSION-ID` a constant, mirrored with the server boundary), `graphql.rs` (object-safe
> async `Transport` seam + reqwest `HttpTransport` on `/{role}/graphql`; `execute_resolver` refuses a
> gap binding before any network call and merges the pinned DSL args under `$input`, caller winning),
> `actions.rs` (the acceptance-first `dispatch`, #17 / ADR-20260720-015500: refuses every
> non-`mutation` kind with its own error variant, mints the `messageId` into `metadata` — the whole
> idempotency story — and resolves the verdict by READING `operationStatus` with bounded polling
> `POLL_MAX_ATTEMPTS`/`POLL_INTERVAL`, keeping REJECTED — an anticipated errors.yaml business
> rejection — distinct from technical FAILED). `make rust` green (0 errors, 26 known warnings, no
> drift); `cargo build --workspace` green; codegen 27 tests, `web` 20 tests; the wasm32 `hydrate`
> build verified. Honest residual: the operation INPUT type name is still convention-derived
> (`<Pascal>QueryInput`), not read from the SDL. Deferred to later splits: Leptos wiring of the data
> layer into live screens, checkout + Stripe element and order-tracking subscriptions (split 3),
> per-component markup + restaurant/rider screen adoption (split 4).

> 🚧 **2026-07-22 — The Captain Company umbrella + GitHub org rename (ADR-20260722-225945, product-owner directive).**
> Establishing the parent-company layer above Captain.Food: brand = **Captain**, entity = **The Captain
> Company** (`thecaptaincompany.com`, purchased), products keep the `Captain.X` pattern (Captain.Food →
> Captain.Jobs/Captain.Voyage later). **Renaming the GitHub org `Captain-Food` → `TheCaptainCompany`** (the
> org becomes the *company*; products are repos inside it). Repo/crate/product-domain names (`captain-food`,
> `captain-food-*`, `captain.food`) are **unchanged** — only the owner segment moves. In-repo reference
> updates (README badges, SECURITY, CODE_OF_CONDUCT, issue-template, BACKLOG, this file) + the **GHCR image
> path** `ghcr.io/captain-food/captain-food` → `ghcr.io/thecaptaincompany/captain-food` (build-image.yml,
> render.yaml, Dockerfile, README runbook) are staged in a **held PR**. ⏳ Blocked on the manual GitHub
> rename + Render image-URL repoint + GHCR visibility check (see ADR Sequencing) — PR must NOT merge before
> those. Reserved-not-built: Captain ID (`id.`) shared identity behind the ADR-0015 wrapper seam, Captain
> Studio (`studio.`), and `captain-framework` extraction (deferred to product #2).

> 📋 **2026-07-22 — Identity federation & consent-gated cross-tenant personalization (ADR-20260722-174500, PROPOSED).**
> Records the identity/privacy framework for the two customer front offices: **one** Captain.Food identity
> (Supabase Auth, global `Customer` keyed by phone/`authRef`, single-origin per ADR-0036) works across
> `captain.food` + every `{slug}.captain.food` — **no per-restaurant account** (made an explicit invariant).
> Sets the **data-controller boundary** (Captain.Food = controller of the identity + cross-restaurant
> marketplace profile; each restaurant = controller of its own fulfilment data; no restaurant→restaurant
> flow, isolation via the #22 nav-edge ACL). Splits two personal-data uses: a customer's **own** history
> across restaurants (service basis, no new consent) vs. **cross-restaurant behavioural personalization**
> (`RECOMMENDED`) which is **consent-gated, default OFF** — to be modelled as a first-class event-sourced
> consent fact (`CustomerPersonalizationConsent…`), deferred to a follow-up issue. "Login with Captain.Food"
> (OIDC) is post-V0 (single-origin already gives SSO within `*.captain.food`). **Legal basis is explicitly
> pending DPO/CNIL** — the ADR fixes only the technical framework so either outcome is cheap. Doc-only; no
> `specs/**` change. **Realized this session:** a dedicated **`captain-identity`** Supabase project
> (Frankfurt, auth-only) split from the **`captain-food`** data project (clean because Supabase is wrapped
> behind GraphQL + JWKS auth per ADR-0047, and `auth_ref` is a plain UUID, not a FK); company domain
> **`thecaptaincompany.com`** (Dynadot; `thecaptain.company` → redirect) with intended issuer
> **`id.thecaptaincompany.com`**; SMS via a **French/EU provider (OVHcloud SMS)** through the Send SMS hook
> with a **per-product alphanumeric sender** (`CaptainFood`), dev = mock (no cost); **late identification** —
> phone OTP at the cart→checkout boundary before payment (#12 anonymous cart), the verified phone shared with
> restaurant/rider for **transactional** order-status only (number masking deferred). Follow-ups: (i) consent
> gate (ADR-0032 completeness); (ii) privacy notice + DPIA + controller/processor contracts; (iii) OIDC
> provider post-V0; (iv) repoint Food's `supabase-acl`/JWKS at `captain-identity` when its auth crate lands;
> (v) `specs/integrations/supabase.md` two-project update (plan mode).

> ✅ **2026-07-22 — Ref-KIND contract (ADR-20260722-152201).** The validator's §1 proved only that a
> `$ref` *resolves*; what it resolved to was checked ad hoc per site. New **§1b** classifies every ref
> target by KIND — finer than its file (PM state table vs projection/referential/journal/staging table;
> enum vs plain scalar; query vs mutation vs subscription; aggregate vs process manager; a `commands.yaml`
> entry is a *command* only if an actor receives it, else a shared *payload object*) — and matches it
> against `REF_CONTRACT`, one declared table of `(file glob, ref-site glob, allowed kinds)`. **Fail-closed**:
> a ref site with no contract entry is an error (`ref-site-undeclared`), so a new ref-carrying DSL field
> cannot land undeclared. Caught the live case: `state_table` accepted any `database/tables/*` table.
> Two widenings recorded in the ADR (service op input may be an event; the screens UI tree is i18n keys).
> `make validate` 0 errors / 26 known warnings, no generated drift, 21 codegen tests green.

> 🚧 **2026-07-22 — #75: marketplace content-split (ADR-20260722-160000, realizes ADR-20260722-091500/-101500).**
> Extracted the Captain **marketplace** front office out of the storefront: new
> `specs/screens/captain_frontoffice.yaml` (+ sidecar) holds `home`/`search` discovery + `partner_landing`
> marketing (`live.captain.food` → bare `captain.food`); their strings (`home.*`/`search.*`/`partner.*`)
> moved to `captain_frontoffice.translations.yaml`. `restaurant_frontoffice.yaml` keeps the single-restaurant
> journey (catalog → cart → checkout → tracking) **plus** the customer account/order screens (decision:
> account/orders stay in the storefront, reachable cross-host via routing — not duplicated). Shared chrome
> (top bar / nav / cart FAB / location+auth sheets) is duplicated per surface; its `location.*`/`auth.*`
> strings stay in the storefront sidecar and the marketplace cross-refs them (keys globally unique). Codegen:
> the loader now **auto-discovers `screens/*.yaml`** and the **doc emitters (md + html) iterate all surfaces**
> (one block per surface); the SDUI **component registry stays a single shared renderer allowlist** in
> `restaurant_frontoffice.yaml`, so `crates/web/src/generated/registry.rs` is **byte-identical**.
> `translations.generated.json` **byte-identical** (keys re-homed). `make rust` + `cargo build --workspace`
> green (0 errors, no drift). Deferred: the marketplace's own account surface, a per-surface component
> registry, and promoting shared chrome strings to `common.*`.

> 🚧 **2026-07-22 — #73: per-surface translation sidecars (ADR-20260722-101500, refines ADR-0033).**
> Shared strings (`common.*` + future backend text) stay in `specs/translations.yaml`; surface-specific
> strings moved to a co-located sidecar `specs/screens/restaurant_frontoffice.translations.yaml`. Screens
> `$ref` the file holding the key (globally unique; new `translation-duplicate-key` check). Codegen
> merges `translations.yaml` + every `screens/*.translations.yaml` (`is_source_file` + `load_model` glob
> + `translation_entries()`) across the validator, the JSON emitter, and the docs table.
> **`translations.generated.json` byte-identical** (149 keys) — `leptos_i18n` unaffected. `errors.yaml`
> untouched. `make rust` + `cargo build --workspace` green. Follow-up: move marketplace strings to
> `captain_frontoffice.translations.yaml` with the content-split.

> 🚧 **2026-07-22 — #71: SDUI screens taxonomy by audience (ADR-20260722-091500, refines ADR-0037).**
> Renamed `specs/screens/customer_screens.yaml` → **`restaurant_frontoffice.yaml`** (the customer-facing
> storefront at `{slug}.captain.food`, roles PUBLIC+CUSTOMER); files now named by **audience with no
> `_screens` suffix** (folder conveys it). Two customer front offices split by host: the **Captain
> marketplace** `captain_frontoffice.yaml` (cross-restaurant discovery @ `live.captain.food` → bare
> `captain.food`, to be created — the `home`/`search` screens currently in `restaurant_frontoffice.yaml`
> move there in a content-split follow-up) and the per-restaurant `restaurant_frontoffice.yaml`. Then
> `restaurant_backoffice.yaml`/`rider.yaml`/`system.yaml` to follow. Codegen `SPEC_FILES` + doc/translation/registry emitters + generated docs and the
> `crates/web` registry header updated (validator already generic over `screens/*.yaml`; no drift). ADR
> relaxes ADR-0037 §4 to allow a future `system` screen set (impersonation still the "view as" path). No
> API/behaviour change. `make rust` + `cargo build --workspace` green.

> 🚧 **2026-07-21 — #21 frontend renderer STARTED, split 1/4 (#68, PR #69).** The Leptos/WASM SDUI
> renderer (remaining-work item 5) is being built in the 4 sub-issues of #21 (ADR-20260720-143000).
> **Split 1** stands up the runtime client seam: (1) a new codegen emitter (`emit_web_registry`) turns
> `specs/screens/restaurant_frontoffice.yaml#/component_registry` into `crates/web/src/generated/registry.rs`
> — a `ComponentKind` allowlist enum (`as_str`/`from_type`/`group`/`ALL`) the renderer dispatches on, so
> the screens DSL stays the source of truth (codegen roadmap item 6); (2) `crates/web` now depends on
> **Leptos 0.8** with an `ssr` (default, native) / `hydrate` (wasm32) feature split — the `renderer`
> builds one static screen (a `home` chrome subset) from the registry and renders it **server-side to
> HTML** (`render_home_html`), with a `hydrate()` wasm entry attaching to the `data-hydrate` root.
> `make rust` green (0 errors, no drift); `cargo build --workspace` green. Architecture + sequence
> diagrams in `docs/frontend/renderer-architecture.md`. Deferred to later splits: live resolver/action
> wiring + session layer (#12) + two-step mutations (#17) → split 2; checkout/tracking → split 3.

> ✅ **2026-07-21 — #61 (slice 1): delivery partner self-registration — EXTERNAL write-path + admin
> approval (ADR-20260721-202504).** First slice of the L "likely split" #61, built on the #60 dispatch
> foundation. New event-sourced aggregate **`DeliveryPartnerRegistration`** (id = client-generated
> `registrationId`): a delivery partner self-registers availability to serve a city on a catalog channel
> through the **EXTERNAL** GraphQL role (`registerDeliveryPartnerAvailability`, lands PENDING), an admin
> reviews it (`approveDeliveryPartnerAvailability`, ADMIN-only → APPROVED), and the partner/admin may
> revoke (`revokeDeliveryPartnerAvailability`). Invariants are self-contained (already-requested /
> not-found / not-pending — 3 new errors); no referential FK check on channel/city in the domain yet
> (deferred). New fold view **`View_DeliveryPartnerAvailability`** (status derived) backs the **first
> EXTERNAL query** `deliveryPartnerAvailabilities` (partner tracks submissions; admin review queue).
> New scalars `DeliveryPartnerRegistrationId` / `DeliveryPartnerName` / `CityAvailabilityStatus`
> (PENDING/APPROVED/REVOKED); new `delivery_partner` (EXTERNAL) story persona + admin review activity;
> 3 rules-linked behaviour tests. Codegen: `BT_AGGREGATES`, `wired_mutation_dispatch` (3 arms),
> `wired_query_body` + `emit_server_types` `From<DeliveryPartnerAvailabilityRow>`. Migration
> `20260721160000` (the view + `ref_city_availability_status`); `REQUIRED_SCHEMA_VERSION` bumped.
> `make rust` green (build + 227+ tests + validate 0 errors + generate, no drift). **The APPROVED set
> is the substrate the #60 `CityDeliveryRanking` walk will consume — that dispatch wiring (+ the
> channel/city FK checks, per-owner query scoping, the onboarding-request & self-integrate shapes, and
> a partner SDUI app) is the deferred follow-up.**

> ✅ **2026-07-22 — #62: delivery-delay satisfaction survey + post-delivery tip/reward prompt
> (ADR-20260722-181500 — realizes the #60 deferral).** After a delivered DELIVERY order the customer is
> asked one timeliness question (*was the delivery on time?*) and, at the same moment, prompted to tip
> the courier — the Uber Eats / Deliveroo pattern. Tipping already existed (`TipOrder`/`OrderTipped`,
> ADR-012), so the tip is **reused** (recipient RIDER, or RESTAURANT for self-dispatch); the new work is
> the survey signal + the restaurant-facing insight. New: scalar `DeliveryTimeliness`
> `{ON_TIME, ACCEPTABLE_DELAY, TOO_LATE}` + `DeliveryDissatisfactionReason`; command
> `RecordDeliverySatisfaction` → event `DeliverySatisfactionRecorded` on the Order aggregate (guards mirror
> `RateOrder`: DELIVERED-only, record-once via `DeliverySatisfactionAlreadyRecorded`); the verdict folds
> into `OrderTracking` (`Order.deliveryTimeliness`, null until answered → hides the prompt) **and** the new
> single-event fold view `View_DeliverySatisfaction` behind the `restaurantDeliverySatisfaction` query
> (RESTAURANT/RESTAURANT_ACCOUNT/ADMIN — the self-dispatch-vs-Captain signal). Completeness (ADR-0032): 2
> rules, 3 behaviour tests, a customer + a restaurant story step, translations, the enriched post-delivery
> `rating_sheet` (timeliness chips + `tip_amount_selector`). Migration `20260722000000`
> (`ref_delivery_timeliness` + `ordertracking.delivery_timeliness` + the view; `REQUIRED_SCHEMA_VERSION`
> bumped). Codegen: the Order `From<OrderTrackingRow>` template gained the field, and the
> `restaurantDeliverySatisfaction` resolver + `From<DeliverySatisfactionRow>` are now emitted wired (not a
> stub). The **read resolver is fully wired**: `application::queries::DeliverySatisfactionReadRepository`
> + `infrastructure::PgDeliverySatisfactionRepository` (over `view_deliverysatisfaction`) + composition
> root. `make validate` 0 errors, workspace green (222 application tests + full suite). End-to-end complete:
> write path, both projections, the fold view, and the restaurant read query.

> ✅ **2026-07-21 — Deployment build model changed & LIVE: CI builds the image, Render only pulls it
> (ADR-20260721-175411, amends ADR-0042).** Render meters build-pipeline minutes at a $0 cap, so
> compiling the Rust workspace on Render (`runtime: docker`) repeatedly failed deploys under the
> high merge cadence (every merge → a full Render build, incl. spec/doc/tooling merges that don't
> change the binary). New model: `.github/workflows/build-image.yml` builds the same cargo-chef
> Dockerfile in **GitHub Actions** (free/unlimited on this PUBLIC repo — buildx `type=gha` layer
> cache), pushes to **GHCR** (`ghcr.io/thecaptaincompany/captain-food:{sha-<short>,latest}`, package PUBLIC),
> and triggers a **Render deploy hook** pinning the image **by immutable digest** (`@sha256:…`, never
> `latest`) — gated on green `ci`/`main` exactly like db-migrate (ADR-0043). The service is `runtime:
> image` + `autoDeploy: false`, so **Render spends zero build-pipeline minutes**; the running build
> reports its **short git SHA** as the `X-VERSION` response header (all routes), the `/health` `version`,
> and a startup log line. **Rollback** = re-hit the deploy hook with a prior `sha-<commit>`/digest (no
> rebuild) — runbook in ADR-20260721-175411 / README. **Verified live end-to-end at `503a1a7`**
> (`/health` `db:up`, `X-VERSION: 503a1a7`). The Render **Blueprint was retired** (deleted 2026-07-21 —
> it kept "Failed sync" against the manually image-backed service); the service is now dashboard-configured
> + CI-hook-deployed, and `render.yaml` is kept as documentation only (not applied). A narrower
> `buildFilter`-only fallback (keep the Render build, skip spec/doc merges) is prototyped on branch
> `claude/rust-build-pipeline-99uzow`.

> ✅ **2026-07-21 — #57: Uber Direct delivery-partner adapter COMPLETE (ADR-20260721-172500).** A
> `DeliveryProvider=PARTNER` adapter via the Uber **Direct** delivery API (not the Uber Eats
> marketplace; distinct from the price-comparison ADRs 0022/0023/0024/0030), applying the Avelo37/
> CoopCycle pattern and plugging into the #60 dispatch foundation as the `uber_direct` channel (the
> Tours ranking seed already ranks it — no saga change). New crate `crates/adapters/uber_direct`:
> `config.rs` (single-endpoint config + OAuth2 client-credentials, env-gated by `UBER_DIRECT_*`,
> fail-closed; partial config ⇒ error), `outbound.rs` (`UberDirectDeliveryGateway` — OAuth2 token
> manager + Create Delivery, `external_id` = our `deliveryJobId` read-back key), `acl.rs`
> (`X-Uber-Signature` **raw-body HMAC** verify — no timestamp, the delta from the Stripe-style scheme;
> Uber status → `DeliveryAcceptedByPartner`/`RejectedByPartner`/`StatusUpdated`; the two-layer-inbox
> `UberDirectWebhookIngestor`), `raw.rs` (`PgRawUberDirectEvents`), `http.rs`
> (`POST /adapters/uber-direct/webhooks`) + standalone `main.rs`. New staging table
> `external_uber_direct_events` (integration_staging.yaml; migration `20260721150000` also extends
> `sweep_retention()`; `REQUIRED_SCHEMA_VERSION` bumped). Spec surface: `services.yaml`
> `delivery.implementations.uber_direct`, `c4-l3.yaml` `uber_direct-acl`, `uber_direct-webhook-ingestion`
> observability contract, `specs/integrations/uber-direct.md`. No new events/commands/errors (reuses the
> partner-generic facts), so ADR-0032 completeness holds. Composition root wires the channel + webhook
> route. `make rust` green (build + tests + validate 0 errors + generate, no drift).

> ✅ **2026-07-21 — #60: delivery dispatch strategy foundation COMPLETE (ADR-20260721-161939 —
> supersedes ADR-20260720-004556).** Multi-partner routing built ONCE so #57 (Uber Direct) and #58
> (CoopCycle) become an adapter crate + a catalog row + a `services.yaml` implementation, with no
> further saga change. Two-layer model: the **channel CATALOG + spec defaults** live in the spec
> (`DeliveryChannelCatalog`, `DeliveryChannelKey` slug — data-driven, not an enum), while **usage** is
> runtime config — `CityDeliveryRanking` (per-city ordered walk list, `city_id IS NULL` = platform
> default), `RestaurantDispatchConfig` (city + `RestaurantDispatchMode`), `City` a first-class entity.
> `DeliveryDispatchProcess` is now **resolve → walk**: the birth leg resolves the plan (`RESTAURANT`
> mode → `SELF_DISPATCHED`, Captain tracks but never offers; `CAPTAIN` → offer rank-1), and one shared
> advance behaviour reached by three legs (`DeliveryRejectedByPartner` / `DeliveryOfferTimedOut` /
> `DeliveryEscalationRequested`) offers the next-ranked channel or records the terminal
> `DeliveryDispatchFailed` when the list is exhausted (list length is the bound, fail-closed —
> `rules.yaml#/DispatchExhaustionFailsClosed` replaces `DispatchRetriesAreBounded`). New: `offer_job`
> gains a `channel` target routed by a **composite `DeliveryService`** (channel→adapter registry;
> unwired channels fall through via the offer timeout, so V0 Tours without Uber Direct is unchanged);
> `DeliveryOfferTimeoutWorker` (env-gated, TTL = `min(global max, city override ?? channel default)`)
> implements the ADR-004556 §5 deferred timeout; `EscalateDelivery` command (RESTAURANT/ADMIN) is the
> manual escalate. Codegen gained a `{ from_hook: <name> }` PM value form (async, rowless,
> orchestrator-resolved — the strategy/channel hook reads the config tables). Migration
> `20260721140000` (City + 3 config tables + PM columns `current_rank`/`current_channel` + the
> `(process_status, last_update_utc)` sweep index + V0 Tours seed); `REQUIRED_SCHEMA_VERSION` bumped.
> `make rust` green (build + 220+ tests + validate 0 errors + generate, no drift). Follow-ups: #57/#58
> adapters register their channels here; #61 partner self-registration writes the usage config; #62
> delivery-delay satisfaction check; a synchronous-decline path for unconfigured channels.

> ✅ **2026-07-21 — #28: Avelo37 delivery-partner adapter COMPLETE (ADR-20260721-104233 —
> realizes the ADR-20260720-015400 "delivery adopts the inbox" follow-up, the outbound half of the
> #26 `delivery` service, and ADR-20260720-004556's bounded re-offer).** The `DeliveryPartner`
> capability is no longer a no-op: automated dispatch is end-to-end. (1) **New crate**
> `crates/adapters/avelo37` (ADR-20260718-213352 pattern): `acl.rs` (Avelo37-Signature timestamped
> HMAC verify, ±300s replay, fail-closed; partner→domain mapping `delivery.accepted/declined/
> status_updated` → `DeliveryAcceptedByPartner`/`RejectedByPartner`/`StatusUpdated` + partner status
> vocabulary → `DeliveryStatus`; the two-layer-inbox `Avelo37WebhookIngestor`), `raw.rs`
> (`PgRawAvelo37Events`), `outbound.rs` (`Avelo37DeliveryGateway` — real `DeliveryService::offer_job`,
> `from_env` gate on `AVELO37_API_KEY`, `job_reference` read-back key), `http.rs`
> (`POST /adapters/avelo37/webhooks`) + standalone `main.rs`. (2) **Inbound two-layer inbox**: new
> `external_avelo37_events` staging table (integration_staging.yaml v4, 90-day processed retention),
> `inbound_events` source `'avelo37'`, migration `20260721130000` (+ `sweep_retention()` covers the
> new mirror; `REQUIRED_SCHEMA_VERSION` bumped). (3) **Drain routing extended beyond Payment**:
> `application::deliveries::record_inbound_delivery_event` (the payments.rs sibling) records the three
> facts onto `DeliveryJob-<id>` — fold-based dedupe (acceptance by `partnerRef`, status by current
> status), **lifecycle-guarded** append for the machine-bearing facts (an illegal report is kept
> FAILED/inspectable, never appended), rejections always recorded (journal unique = their dedupe →
> bounded re-offer counter advances), orphans recorded (saga guard flags them). (4) **Outbound
> wiring**: composition root resolves the `delivery` binding to `Avelo37DeliveryGateway` when
> `AVELO37_API_KEY` is set, else the logged `NoopDeliveryService` — unconfigured deployments (V0
> Tours) unchanged. (5) **Completeness (ADR-0032)**: `avelo37-webhook-ingestion` observability
> contract (mirrors Stripe), `TestDeliveryJobRecordsPartnerStatusReport` closes the new event message
> gate. `make rust` green: workspace builds, tests pass (13 adapter + recorder + drain), validate
> 0 errors, no drift. Follow-ups: real Avelo37 wire reconciliation on go-live; multi-partner ranking
> (#57 Uber Direct, #58 CoopCycle) is the named extension point.

> ✅ **2026-07-21 — #24: the behaviour-test suite is GENERATED from tests.yaml
> (ADR-20260721-101552, codegen-roadmap item 2).** New `codegen-rs` emitter
> (`emit_behaviour_tests`) → `application/src/generated/behaviour_tests.rs`: one `#[tokio::test]`
> per Given/When/Then case (all 161) — GIVEN seeds fixtures onto their aggregate streams, WHEN
> dispatches through the real write path (emitter-owned command/PM-leg/record dispatch tables),
> THEN asserts payload-level equality of the appended facts across ALL streams (strict per-stream
> diff; `then: []` = strict no-op; `thrown` = typed code + no side effects). Runs on the
> hand-written `application::behaviour_support` runtime (mem store, read-model/service doubles,
> PM-run seeding, UUIDv5 spec-id mapping). Executing the spec surfaced and fixed: a new
> `test-invalid-enum-value` validator rule (caught `serviceType: "PICKUP"`), tests.yaml sample
> corrections (missing birth facts / cross-aggregate givens, per-leg RefundOpened variants, the
> V0 zero-fee money chain per pricing.rs, refund legs asserting the refund they open), and two
> runtime fixes (RegisterRestaurant enforces `RestaurantAccountNotFound` by folding the account
> stream; delivery-issue payloads no longer stamp wall-clock time — ADR-0041). The ten
> hand-mirrored `crates/application/tests/*_behaviour.rs` files (118 cases) are DELETED; in-src PM
> tests and `pm_state_mem.rs` stay. New tests.yaml cases now cost zero Rust. `make rust` green.

> ✅ **2026-07-21 — #20: HubRise CONNECT FLOW — provisioning on OAuth connect + account-scoped
> token store (ADR-20260721-100601; closes the ADR-20260718-145856 §0 "Open contract" / item 2a).**
> Two new adapter routes: `GET /adapters/hubrise/connect` (302 → HubRise authorize, stateless
> HMAC-signed anti-CSRF `state`) and `GET /adapters/hubrise/oauth/callback` (code → token exchange —
> the response itself names the connection scope: `account_id`…). The flow pulls
> `/account`+`/locations`+`/catalogs` and provisions via journaled WORKER sends of the EXISTING
> commands with the enricher's derived UUIDv5 ids (`RegisterRestaurantAccount`, `RegisterRestaurant`
> per location — `PASSIVE_PARTNER`, slug = `slugify(name)-slugify(location id)`, `CreateCatalog` +
> initial `ImportCatalog` per catalog) — NO new domain messages, creations idempotent on the derived
> ids, deterministic rejections warned-never-retried (SIRENE lesson). NEW DSL table category file
> `specs/database/tables/integration_connections.yaml` (plan-mode approved): `hubrise_connections`
> (token keyed by RestaurantAccount = UUIDv5(account), never event-sourced, never in api.yaml — no
> GraphQL edge reaches it) + `hubrise_connection_locations` (callback location → token resolution);
> migration `20260721120000`, `REQUIRED_SCHEMA_VERSION` bumped. **The global `HUBRISE_ACCESS_TOKEN`
> is RETIRED**: `HubRiseApiClient` → token-per-call `HubRiseApi`, the enricher resolves each
> callback's token from the connection (unconnected location = definitive skip), and enrichment now
> needs only `DATABASE_URL`. New env (connect routes only, fail-closed): `HUBRISE_CLIENT_ID`,
> `HUBRISE_CONNECT_REDIRECT_URL`, optional `HUBRISE_OAUTH_SCOPE` (default
> `account[catalog.read,inventory.read]`); `HUBRISE_WEBHOOK_SECRET` doubles as the OAuth client
> secret (it IS the app client secret). Tests: connect provisioning/reconnect-idempotency/no-scope/
> catalog-listing-failure + enricher token-resolution suites (24 adapter tests) + Pg-gated
> `connections_store.rs`. `make rust` green. Follow-ups in the ADR: restaurant-facing connect UI,
> disconnect/revoke + token encryption at rest, confirm `GET /catalogs` & `opening_hours` shapes.

> ✅ **2026-07-21 — #23: aggregate lifecycle state machines COMPLETE (ADR-20260721-093027,
> codegen-roadmap item 1 closed — completes ADR-20260720-004419's first slice).** (1) **Dynamic
> targets**: a lifecycle entry may declare `via: <payloadField>` — the event carries the target
> state (one entry per `from × to`, determinism per event instance); new `lc-via` validator rule
> (field exists, required, same scalar; static/dynamic mixing = `lc-ambiguous`), emitter emits
> guarded arms + payload-read `target()`/`initial()`, mermaid labels dynamic edges `Event(field)`.
> (2) **Full adoption**: Restaurant (static machine), Rider and DeliveryJob (dynamic) declared in
> actors.yaml — 6 machines / 72 transitions, `lc-missing` warnings 3 → 0; the declared DeliveryJob
> machine resolved a real hand-code drift (cancel-from-FAILED allowed by `cancel_delivery` but not
> by `delivery_can_transition`) preserving both behaviours. (3) **Folds rewired**: Cart, Payment,
> Restaurant, Rider, DeliveryJob status now moves ONLY through `lifecycle::initial`/`target`;
> `rider::can_transition` and `delivery_can_transition` are deleted; the remaining hand delivery
> handlers guard through `lifecycle::transition`. (4) **Generated handlers**: new emitter →
> `application/src/generated/handlers.rs` — the 7 Order lifecycle commands + `ChangeRiderStatus` +
> `UpdateDeliveryStatus`/`UpdateDeliveryPartnerStatus` are generated require+guard+append fns
> (event built from same-named command fields; per-aggregate require/reject/stream seams stay
> `pub(crate)` in commands.rs), re-exported so call sites are unchanged; the hand behaviour suite
> is the parity gate until #24. `make rust` green: 57 test suites pass, validate 0 errors, no drift.

> ✅ **2026-07-21 — #25: the PM orchestrator step pipelines are GENERATED
> (ADR-20260721-053456, codegen-roadmap item 3, implements the deferral of ADR-20260719-193500).**
> New `codegen-rs` emitter over `specs/processmanager.yaml` →
> `application/src/generated/process_managers.rs`: one module per process manager, one generated
> `async fn` per leg executing the DSL's ordered typed steps — `state.by/expect/set` over the #27
> generated stores (missing-row policy typed from the spec: bare `guard throws` after `state.by` =
> the orphan error; command legs reuse the first `that`-guard's error; otherwise benign skip),
> structural guards, `call` through the #26 generated service ports, `deliver` with generated
> stream addressing + `Repository::save` under the saga actor, `send` with the event-leg
> rejection-logged-and-skipped semantics, and a pk-admission seam on opening legs. The
> NON-STRUCTURAL seams are per-leg generated HOOK traits (`read_*` with sink-typed structs,
> `build_*`, `input_*`, `should_deliver_*`, `admit`, `finalize`, `compute_*`, `branch`) —
> the four hand orchestrators shrank to hook impls + thin wrappers with UNCHANGED call surfaces
> (runner/server untouched). Two DSL-reading conventions carry the last nuances: self-referential
> `from_state` = orchestrator-computed (the re-offer counter), a mid-leg bare `skip` guard = the
> linear-branch marker (ADR-20260720-004556). The `PlaceOrder` command leg stays hand-written
> (pricing non-goal). Behaviour suite = parity gate, all green (two skip-message substring
> assertions re-worded; never spec'd). `make rust` green: workspace builds, tests pass, validate
> 0 errors, no drift.

> ✅ **2026-07-21 — #50: identity catalog completed + migrated to the generated `IdentityService`
> (owner-approved spec change; closes the #26 deferral, ADR-20260721-043033).** services.yaml:
> `identity.verify_email_token.output` now carries the proven `email` (+ declares
> `VerificationCodeExpired`), and the `locale` inputs of `send_phone_otp`/`send_email_magic_link`
> are `nullable: true`. The hand-written `AuthProviderGateway` (+ `PhoneOtpCheck`/`EmailTokenCheck`)
> is MIGRATED AT PARITY and deleted: the Customer command handlers call the generated
> `IdentityService`, invalid/expired verifications are the canonical typed rejections RAISED BY THE
> ADAPTER (`canonical_phone` is `pub` so adapters build the `phone` context identically), the
> fail-closed stand-in is renamed `FailClosedIdentityService`, and the composition root resolves
> identity through the generated `identity_service` binding. Every service port of the catalog is
> now generated — roadmap item 4's migration debt is fully paid. `make rust` green.

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
| `INSEE_API_TOKEN` repo secret | ✅ | Added. **⏳ PAUSED 2026-07-28** — the scheduled ingestion → staging → worker chain is stopped at both ends until [#220](https://github.com/TheCaptainCompany/captain-food/issues/220) |
| `INTERNAL_TRIGGER_TOKEN` (Render env + repo secret) to enable the CI→worker ping | ⏳ | Optional; unset, so `POST /internal/sirene/drain` is fail-closed (503). `RUN_SIRENE_WORKER` now **defaults OFF** (paused, #220) |

## 🔌 External integrations — partner adapters & M2M (ADR-20260718-145856 / -213352)

**Partner webhook adapters are self-contained crates** under `crates/adapters/*` — each an ACL +
axum shell + standalone binary, mountable into the monolith **or** deployable as its own web service.
Two directions: partner-**push** webhooks (below) vs external-**drive** `/external/graphql` (M2M).

| Piece | Status | Notes |
|---|---|---|
| **Stripe** — `crates/adapters/stripe` (`POST /adapters/stripe/webhooks`, `stripe-webhook` bin) | ✅ | `Stripe-Signature` HMAC over raw body (constant-time, 300s replay, fail-closed); ACL → `PaymentCaptured`/`PaymentFailed`/`PaymentRefunded`; idempotent by Stripe event id. 12 tests |
| Checkout must set `metadata.restaurantId` (+`orderId`) on the PaymentIntent/charge | ✅ | `StripePaymentGateway` sends `metadata[orderId]`/`[restaurantId]`/`[cartId]` on create-intent — the webhook ACL maps `charge.refunded` from them; exercised by the green prod smoke |
| **HubRise** — `crates/adapters/hubrise` (`POST /adapters/hubrise/webhooks`, `hubrise-webhook` bin) | ✅ | **Ingress** ✅ (HMAC-SHA256 hex, fail-closed, envelope parse). **Outbound OAuth2 client** ✅ (`api.rs`: `X-Access-Token`, non-expiring token from `HUBRISE_ACCESS_TOKEN`, `exchange_code` connect helper, catalog/inventory pull). **Domain wiring** ✅ (`enrich.rs`): verified catalog/inventory callback → API pull → enrichment ACL → `ImportCatalog` / per-SKU `update_offer_stock` handlers. **Deterministic UUIDv5-of-HubRise-id** ids reconciled with the **Catalog aggregate** (offer seeded from the SKU `ref` = inventory's `sku_ref`, so a stock update hits the imported `OfferId`); `"9.80 EUR"`→`Money`, tax-rate strings→`TaxRate`, `data` envelope translated at the boundary; catalog = rejectable command (`CatalogNotFound`→skip), inventory = reported fact (`OfferNotFound`→skip, never rejected). 14 tests. Enricher wired at the server composition root + the standalone bin (needs only `DATABASE_URL`). ✅ **Connect flow landed (#20, ADR-20260721-100601)**: OAuth connect provisions account/locations/catalogs with the derived ids + stores the account-scoped token in `hubrise_connections` (env token retired) |
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
> [GitHub issues](https://github.com/TheCaptainCompany/captain-food/issues) (#12–#28, typed
> Task/Bug/Feature) managed on the **org-level GitHub Project**
> ([github.com/orgs/TheCaptainCompany/projects](https://github.com/orgs/TheCaptainCompany/projects),
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
| 2a | **Connect flow** — provision `RegisterRestaurantAccount` + `Restaurant`(s) + `CreateCatalog` with the enricher's derived UUIDv5 ids, and persist the HubRise **account-scoped** token in `hubrise_connections` keyed by `RestaurantAccount`. See `docs/integrations/hubrise-process.md` §0 | #20 | ✅ (ADR-20260721-100601) |
| 3 | **Process managers** — Refund/CartBinding/DeliveryDispatch + PM runtime (event-driven, `/saga`) | 🅐 | ✅ (outbound refund via the real gateway; bounded partner re-offer landed — offer timeouts deferred, ADR-20260720-004556) |
| 4 | **Cart line invariants** + catalog `tree` projector + offer read port | 🅐 | ✅ |
| 5 | **Frontend** — Leptos/WASM SDUI renderer (customer/restaurant/rider apps) | unassigned | 📋 |
| 6 | GraphQL **subscriptions** (`SubscriptionRoot` + bus + WS + ACL) | 🅐 | ✅ |
| 7 | **Structured typed errors** (ADR-20260719-120000) | 🅐 | ✅ |
| 8 | **Per-field nav-edge ACL** — optional `roles:` on nav fields (default public), same guard/visible as ops; design agreed | 🅐 | 📋 plan mode (after ACL emitter free) |
| 8b | Delivery/account read queries + catalog `tree` + `me`/favorites | 🅐 | ✅ (read surface complete except `operation`; `phoneCountries` deleted with #305) |
| 9 | Remove `INTERNAL_TRIGGER_TOKEN`/drain endpoint (use `/ping` warmth) | 🅐 | 🗑️ deferred |
| 10 | Projection worker robustness (poison-skip) + spin-down mitigation (uptimerobot `/ping`) | 🅐 | ✅ |
| 10a | **Push-driven drain loops** ([#300](https://github.com/TheCaptainCompany/captain-food/issues/300), ADR-20260802-200416) — `pg_notify` in the append transaction + one `LISTEN` connection wakes the projector AND the saga runner; safety-net drain kept (NOTIFY has no replay) and the fallback reverts to the 1.5 s poll whenever the listener is down; idle head-gate skips per-group queries when the log has not moved | 🅐 | ✅ idle DB round trips ~70,900/h → ~120/h, and sagas react on commit instead of up to 1.5 s later. **Requires a session-mode pooler** (Supabase 5432); `RUN_EVENT_PUSH=false` forces polling |
| 10b | **Mailbox keyspace width 100 → 5** (ADR-20260802-220402) — post-#301 audit found the mailbox out-polls what #301 removed: 16 actor types × 100 lanes × one per-lane SELECT per 10 s pass ≈ 580k idle queries/h, un-gated. Width 5 in `specs/actors.yaml` + migration `20260802220000` (exact remap: 5 divides 100, so `partition % 5` = the width-5 stamp; rows remapped BEFORE registry shrink) | 🅐 | ✅ idle mailbox queries ~580k/h → ~29k/h. Real fix is 10c |
| 10c | **Push-driven mailbox** ([#313](https://github.com/TheCaptainCompany/captain-food/issues/313), [PROP-20260802-223522](proposals/PROP-20260802-223522-push-driven-mailbox.md) approved D1–D5, ADR-20260802-224532) — `pg_notify` at the `PgMailbox` door (one channel, actor-type payload) wakes workers cross-process; lanes-with-work idle gate; attempts-cap poison policy (`FAILED` + error at the cap); gated `RUN_MAILBOX_PUSH` + `MAILBOX_MAX_DELIVERY_ATTEMPTS` | 🅐 | ✅ door notifies in the enqueue tx (`PgMailbox` + PM chain); listener per process feeds the nudge map cross-process; full pass 60 s under confirmed push (beat stays on heartbeat, degradation = pre-push cadence); poison cap default 5 (`0` = old behaviour); retries back off EXPONENTIALLY since #316 (base x 2^(N-1), ~5 min to terminal at cap 5); heartbeat/lease/cap wired from Config (MAILBOX_* keys were previously unread) |
| 11 | **CoopCycle** delivery partner (#58) — third `PARTNER` adapter; **federated** per-instance registry + OAuth2 (ADR-20260721-122910) | 🅐 | 🚧 PR #59: DSL surface (staging + services + obs + c4 + integration doc) landed; `crates/adapters/coopcycle` + server wiring in progress |

## 🚨 Open incident — production suspended (2026-08-05)

**`captain-food.onrender.com` is DOWN** (HTTP 404). The Render web service
`srv-d9ctcpgk1i2s73cj6820` is **suspended for billing** (`suspenders: ["billing"]`, suspended
~2026-08-04 12:26 UTC). No customer can order — the whole storefront is offline. **Resolution is a
billing/account action in the Render dashboard** (owner-only; not a code fix). CI on `main` is
all-green; this is purely the hosting account. Fixed in the same run: `render-status` now reports
**red** on suspension (ADR-20260805-070138) — previously it read only the last deploy's status and
showed a false green while prod was down.

## 🧭 Architecture decisions
See [`docs/adr/`](adr/) — latest: **20260802-200416 (drain loops woken by Postgres NOTIFY, not a 1.5 s poll — background polling was 95% of outbound bandwidth)**, 0047 (API auth — Supabase JWT/JWKS), 20260719-120000 (structured domain rejections), **20260719-014434 (checkout snapshot on `PaymentIntentCreated`)**, **20260719-031136 (write-side `Repository` / event-sourced actors — handlers + saga runner route through it, never the raw `EventStore`)**, 20260718-145856 amendment (adapter webhook routes → `/adapters/{partner}/webhooks`). **ADR ids are now date-time** to avoid concurrent-session collisions (ADR-20260718-135417).

> Convention: keep this file current with every substantive change, and record cross-cutting decisions as an ADR in the same change.
