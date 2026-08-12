# 🚦 Captain.Food — Development & Deployment Status

> Hand-maintained snapshot (NOT generated, outside `specs/` so it never affects the DSL).

> 📌 **2026-08-12 — THE FOLLOW-UP REGISTER: nine findings from tonight's mob reads are now ISSUES,
> not paragraphs** (records-only). Each is linked from the register row it belongs to, so it is
> reachable from the decision as well as from here:
> - [#508](https://github.com/TheCaptainCompany/captain-food/issues/508) — `hubrise_connections.access_token`
>   is **plaintext** and a non-expiring token, so the physical WAL archives carry it too (linked from
>   [DECISIONS §32 ADP-1](proposals/DECISIONS.md), which called that table non-rederivable without
>   saying it was unencrypted).
> - [#509](https://github.com/TheCaptainCompany/captain-food/issues/509) — the restore drill verifies
>   **1 of the 11** databases the split creates (linked from **STO-6**).
> - [#510](https://github.com/TheCaptainCompany/captain-food/issues/510) — mailbox query ports behind a
>   capability witness: the level-4 half of #506 the validator rule cannot reach (linked from
>   [ADR-20260812-214500](adr/ADR-20260812-214500-a-read-target-is-declared-not-inferred-the-reads-ownership-wall.md)
>   and [PROP-20260802-130500 §1](proposals/PROP-20260802-130500-isolation-by-construction.md)).
> - [#511](https://github.com/TheCaptainCompany/captain-food/issues/511) — JWKS single-flight test flake.
> - [#512](https://github.com/TheCaptainCompany/captain-food/issues/512) — pool + `_sqlx_migrations`
>   schema probe out of `crates/server`, the second level-4 half of #506 (same two links as #510).
> - [#513](https://github.com/TheCaptainCompany/captain-food/issues/513) — the adapter-isolation grant
>   emitter and its **negative-path** test: nothing today proves a pod is REFUSED a database (linked
>   from **ADP-1** and **STO-5**).
> - [#514](https://github.com/TheCaptainCompany/captain-food/issues/514) — per-database migration chains
>   and a `REQUIRED_SCHEMA_VERSION` **map**: eleven databases against today's one chain and one scalar
>   constant (`crates/server/src/lib.rs:170`) — linked from **STO-1** / §35's **CUT-1** cutover row.
> - [#515](https://github.com/TheCaptainCompany/captain-food/issues/515) — `join.captain.food`'s legal
>   pages still lack a postal address, a phone and a named directeur de la publication, and name **no
>   consumer mediator** (linked from **Q-L1**).
> - [#502](https://github.com/TheCaptainCompany/captain-food/issues/502) — re-scoped in a comment: five
>   stale `inbound_event_id` declarations survived [#500](https://github.com/TheCaptainCompany/captain-food/issues/500)
>   in `specs/observability.yaml` (lines 506, 584, 648, 732, 966, each `source: "inbound.inbound_event_id"`
>   against a table dropped by `20260731143000`), and the fix is to **type the reference** rather than to
>   rename the survivors — an untyped `source:` string is the [#413](https://github.com/TheCaptainCompany/captain-food/issues/413)
>   defect class again, invisible to the refs walker and therefore to every rename.

> 🧭 **2026-08-12 — THE FOUNDER ANSWER SHEET: THE FLIP IS TAKEN, THE REGISTRY IS DESTROYED, AND
> NOTHING IS PAID FOR UNTIL A WORKING VERSION CAN BE SEEN** (twelve founder answers + a ten-lens mob
> read; [ADR-20260812-214021](adr/ADR-20260812-214021-the-founder-answer-sheet-of-2026-08-12.md),
> new [DECISIONS §35](proposals/DECISIONS.md); **records-only — no code, no specs, no generated
> artifacts**).
> **The headline is not one of the twelve answers, it is what they add up to: the critical path is
> INVERTED.** *"I'm waiting for a working version before paying OVH"* turns **provision → deploy →
> walk** into **walk → provision → deploy** — and the one leg the team cannot supply is the exit
> condition, because *"a working version"* carries **no acceptance criterion**, which makes it a spend
> gate with no exit. Recorded so it can be confirmed or replaced (**§35 INV-1, the one FOUNDER-OWED
> leg**): **smoke L1→L4 green on local k3s plus a recorded browser walk** — order placed, paid,
> restaurant told, tracking moving without a reload, order completing. Both halves are needed:
> `prod-smoke.sh` never opens a browser, and a browser walk cannot assert a Stripe capture.
> **The path is a MERGE, not a build** — `origin/cutover-local-rehearsal` /
> [PR #486](https://github.com/TheCaptainCompany/captain-food/pull/486) already carries the
> local-rehearsal runbook, the k3s CNPG overlay, the generated monolith overlay and the smoke's
> `SMOKE_SCHEME`/`SMOKE_PUBLIC_BASE` overrides, with **L1+L2 passing and 45/45 migrations on an empty
> database**, while `main`'s `tools/smoke/prod-smoke.sh:41,48-49` still hardcodes an unroutable
> `https://api.captain.food` with no scheme override. Two gaps sit outside the merge:
> `SUPABASE_SECRET_KEY` as its own repository secret (hard-stops L3, and L4 is downstream — presence
> is a **confirmation**, since STATUS already records a secret of that name existing on 2026-08-09)
> and **a webhook ingress for L4's `CAPTURED` assertion**. **Local is demo, never evidence**: the
> overlay strips `barmanObjectStore`, so the **restore drill is the first post-provisioning act** and
> no recovery claim may cite the rehearsal.
> **The answers, and what each cost to check.** **JRN-1 = A** — take the `PM_MAILBOX_DELIVERY` flip
> now in [PR #500 "#242 Runtime D: retire command_journal"](https://github.com/TheCaptainCompany/captain-food/pull/500)
> inside the empty-log window, with **L4 as the release gate before traffic is routed**; verified
> consequence: option (a)'s interim `command_journal` grant is **not owed at all** once #500 merges
> (it drops the table and empties `RuntimePosture`), and that PR also already removes
> `dispatch_outcome: spawned` and deletes the `CommandChannel`/`CommandJournalStatus` scalars.
> **CUT-1 = B** — the cutover gets a **rule**, not a list: *IN = only what the empty log or a traffic
> pause makes cheaper*, admitting **the eleven-database storage split** and excluding the pooler, the
> API-tier split and the runtime decomposition. **DB-HA = A** (three instances, inside the cutover) is
> **recorded, not incurred**: with `enablePodAntiAffinity` + `podAntiAffinityType: required` on a
> hostname topology, `instances: 3` on one node leaves **two pods `Pending` forever**, so A is the
> **EUR 67.80** trio and its +EUR 41.20 is unpayable until the EUR 26.60 base is — and the **60 Gi of
> PVC it implies is unpriced anywhere in the repo**, because the runbook ADR-20260807-114122 cites for
> the sizing detail (`docs/runbooks/mks-bootstrap.md §2`) **does not exist**. **SIR-1 = all NO**
> (*delete and record the destruction*) closes the retroactive SIRENE risk **on attestation, not
> inspection** — so the record owes how/when the rows ceased, a project list captured while absence is
> still inspectable, whether any backup/PITR window survives, and a named attester; and **two
> neutralisations are owed before any re-sync**, both live today (`sirene-sync.yml` is paused only by a
> commented-out cron and **deliberately keeps `workflow_dispatch`**, writing the staging table from
> `secrets.DATABASE_URL`, which must be revoked and the revocation logged). **The Art. 21 blocker
> survives forward-looking** ([#505](https://github.com/TheCaptainCompany/captain-food/issues/505)):
> `RestaurantListingOptedOut` folds into **nothing** (`generated/projectors.rs:59` is `=> state`).
> **Q-L1 partially resolves** — `join.captain.food` publishes the association, RNA W372020229 and the
> rights contact, and publishes **no postal address, no phone, no named directeur de la publication and
> no consumer mediator**; its host block is GitHub Pages, so *verify, do not copy*. **Q-L3 = no real
> phone-verified end user** — which both supports the empty-log window and dates the trigger (first
> real customer order = DPIA + erasure + mediator deadline). **BND-6 = B** (kitchen time labelled
> "ready" — the label IS the decision) · **BND-7 = A** (estimate, no remedy) · **Q1 = A**
> (authenticated server-side only — graded: plausibly no consent banner, but **Art. 13 transparency
> and lawful basis remain**) · **Q2 = A** (yes after the DPIA; it makes the restaurant a
> controller/joint controller) · **Q7 = A** (not now, converging with MET-Q7 a day later) ·
> **KEY-1** delete the stray key now — ⚠️ **its referent is recorded nowhere in the repo** and this
> record does not invent one.
> **Two recorded corrections landed with the sheet, as corrections rather than silent edits.**
> (1) **STO-4's sequencing is WITHDRAWN**: its ~185/~235-of-220 arithmetic is a **57-pod bin-fleet**
> figure and the fleet is OUT of the cutover, so with the monolith deployed eleven databases × one pod
> is **~55 backends of 220**; the pooler is re-targeted as a blocking precondition of the
> [#358](https://github.com/TheCaptainCompany/captain-food/issues/358) bin flip, plus a
> recommendation to cap the monolith's per-database pool. (2) **PROP-20260809-021351's gap table was
> STALE and is corrected in place**: **G5, G6 and G7 are FIXED** (#420/#451/#424 — including the
> subscription that now accepts the order's `DeliveryJob-` stream and dedupes on `updated_at`),
> **C1 is only HALF fixed** (the total prices live on read; the competitor comparison still never
> computes and moved from the projector to `cart_read.rs:187`), and **G7b, G8 and C2 are live** — G8
> being *nobody is told about a paid order*, with `crates/application/src/ports.rs` declaring four
> traits and **zero notification anything**.
> **Backlog — a previously stated order is REVERSED, and the method clause is on the record**
> ([ADR-20260810-215503](adr/ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md) +
> [BACKLOG.md](BACKLOG.md)): **[#429 "Production with test data"](https://github.com/TheCaptainCompany/captain-food/issues/429)
> is re-pointed off OVH onto local k3s WITHOUT re-scoping** (ADR-20260809-050000 fixed its target as
> the production deployment; the inversion changes the host, and under *"local is demo, never
> evidence"* a local walk satisfies the spend gate and does **not** close #429), and the
> **[#494](https://github.com/TheCaptainCompany/captain-food/issues/494) storage chain drops below
> it** on *value-first: foundations first* — a foundation that cannot be applied is not first, and
> #494 lands at a cutover now downstream of a payment decision it cannot unblock. **Nothing was
> re-ranked to make it dispatchable.**

> 🧾 **2026-08-12 — THE FOUNDER IS THE FOUNDER, AND EVERY FOUNDER MESSAGE GOES TO THE WHOLE TEAM**
> (two founder directives, verbatim: *"Stop calling me product owner. I'm the founder / Tech CEO."*
> and *"When I say something ask the team for answers never answer directly without asking the whole
> team."*;
> [ADR-20260812-143619](adr/ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md)).
> The mob principle ([ADR-20260809-013142](adr/ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md))
> extends from **dispatches to founder messages**, and coordinator-never-authors
> ([ADR-20260810-011500](adr/ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md))
> from **the diff to the answer**: no answer is composed and no record lands before the whole roster
> has been asked, with *"nothing in my lens"* a complete one-line answer. Three carve-outs, each
> attributed: an **external-clock fact** is relayed in the same turn (business lens), **executing an
> already-recorded rollback/abort path** needs no consult while going FORWARD through an incident does
> (release lens), and **no lens output or aggregation of lenses is legal advice or clearance** (legal
> lens). New rule: a record created from a founder directive carries a **`Consulted:` block, one line
> per lens** — because a lens that was never asked is indistinguishable from a lens with nothing to
> say (testing/UX/observability lenses, convergent). "Product owner" is swept from the LIVING
> operating docs (`CLAUDE.md`, `PLAYBOOK`, `BACKLOG`, `docs/claude/*`, `proposals/README`, and the
> register's `PRODUCT-OWNER-OWED` → `FOUNDER-OWED`); **historical ADRs and proposals keep their
> vocabulary** and verbatim quotes stay verbatim. Legal caveat: the title is right for repo records
> and is **not** a French corporate mandate — external artifacts must name the statutory capacity.

> 🔒 **2026-08-12 — EACH ADAPTER OWNS ITS OWN, COMPLETELY ISOLATED DATABASE — decided, then
> CORRECTED the same day** (founder directive, verbatim: *"Each adapter must have there own database
> completely isolated"*;
> [ADR-20260812-115930](adr/ADR-20260812-115930-each-adapter-owns-its-own-completely-isolated-database.md);
> register row **ADP-1** in [DECISIONS §32](proposals/DECISIONS.md); records-only — no code, no
> specs; execution rides
> [#494 "Storage boundaries and least-privilege database users"](https://github.com/TheCaptainCompany/captain-food/issues/494)).
> Supersedes
> [PROP-20260811-093000](proposals/PROP-20260811-093000-storage-boundaries-and-least-privilege-database-users.md)
> §11's placement of integration staging in `DomainCommonDb` (map amended in place): **six adapter
> databases** — `adapter-stripe` · `adapter-hubrise` (staging + the credential tables) ·
> `adapter-uber-direct` · `adapter-coopcycle` · **`adapter-avelo37`** · `adapter-sirene` (the 655 MB
> mirror) — each reachable by ONE app and nothing else, in the shared business cluster (STO-3's math
> already priced per-thing clusters out; the wall is role + `CONNECT`, BND-3's mechanism). **Eleven
> databases total** (5 business + 6 adapter).
> **A full-roster mob found two defects in the first record of this and both are fixed**: (1) it
> claimed *"avelo37 owns no table today"* — **false**, `external_avelo37_events` is declared
> (`integration_staging.yaml:178`) and already retention-swept (`sweep_retention.sql:60`), so avelo37
> would have been the ONE partner mirror left holding `CONNECT` on the write database while every
> sibling moved out; (2) it recommended an `adapter-identity` database for `auth_sessions` on a
> rationale that runs **backwards** — that table is AES-256-GCM encrypted under `AUTH_SESSION_KEY`
> while `hubrise_connections.access_token` is **plaintext**, there is no such adapter crate or bin,
> and its users are the actor path plus the BFF login route. The count did not move; the **membership**
> did. **Both legs are now CLOSED**: leg 1 **(a)**, the `inbound_messages` front door stands — an
> outbox+relay would hold a *bidirectional* platform grant inside each adapter database, and
> `LISTEN`/`NOTIFY` being per-database would need an inward connection to all six or a forbidden
> permanent poll; leg 2 **(b)**, `auth_sessions` **stays platform on `captain-write`**. The GraphQL
> lens's dissent is recorded as the final-vision alternative (an identity bin owning the table AND
> `/auth/session`+`/auth/refresh`+`/auth/logout`, which would also home the
> [#385](https://github.com/TheCaptainCompany/captain-food/issues/385) routes that have no bin home
> today) — a larger slice, not taken now. Reframing finding: `AUTH_SESSION_KEY` is granted to **53 of
> 56 pods** while exactly **two** decrypt a session, so narrowing the grant
> ([#491](https://github.com/TheCaptainCompany/captain-food/issues/491) slice A4, emitter + negative
> test in [#513](https://github.com/TheCaptainCompany/captain-food/issues/513)) buys more here than
> the database wall. **That figure was first recorded as "53 of 57, every group but the four periodic
> workers" and the correction makes it WORSE**: [#500](https://github.com/TheCaptainCompany/captain-food/issues/500)
> deleted `worker-journal-sweep`, which was one of the four EXCLUDED workers, so the denominator fell
> and the numerator did not — read the smaller number as a widened blast radius, not as progress
> (three excluded workers remain: `worker-erasure`, `worker-retention`, `worker-sirene-sync`).
> Named consequences: STO-4's pooler-first sequencing **hardens** (every adapter
> bin holds two pools), `hubrise_connections` is the one NON-rederivable adapter table (a
> non-expiring token only a human re-connect replaces) so it needs a backup story while staging
> mirrors take the refetch posture — and that token is **plaintext**, so the same backup copies it
> into the WAL archives ([#508](https://github.com/TheCaptainCompany/captain-food/issues/508)) — and
> `sweep_retention()` forks per adapter database **including
> the avelo37 leg the first record did not know existed**.

> 🗂️ **2026-08-12 — THE APP INDEX IS GENERATED, AND IT SAYS THE SPLIT IS NOT CLEAN**
> ([PROP-20260811-141654](proposals/PROP-20260811-141654-per-app-declaration-folders.md) slice A1,
> [#491 "Per-app declaration folders"](https://github.com/TheCaptainCompany/captain-food/issues/491);
> emitter + generated output only — **no `specs/apps/` folder, no source moved, no manifest touched**.)
> `specs/generated/apps.generated.md` now renders all **57 deployables**: family, boundary, what each
> hosts, its pod grant, and the two columns the product-owner question turns on — **declared** domain
> crates vs **resolved** ones, the second MEASURED from the workspace graph with cargo's own resolver
> rather than inferred from the spec. It is the first emitter that measures rather than derives, which
> is why it runs last in `main` (after the manifests the same pass writes) and refuses to emit at all
> if the workspace cannot be resolved.
> **The verdicts it renders**: **8 of 57 apps are honest** (resolved == declared) — the 7 `gateway-*`
> plus `bam`, which links all 8 domain crates *and declares all 8*, so it is honest-though-fat and must
> not be counted with the other 49. **3 apps declare crates from two business boundaries**
> (`pm-cart-binding`, `pm-delivery-dispatch` — legitimate bridges — and `bam` by design); on the graph
> that actually links, **50 span all five**. **No crate the apps reach is boundary-exclusive** — all
> 44 are linked from at least one app of every boundary — but that signature saturates (the 8
> `graphql-*` subgraphs alone cover all six boundaries, so any crate one of them links scores the
> maximum), so section 3 groups by **how many of the 57 apps link each crate** instead: 57 for
> `telemetry`/`bin_probes`, 50 for `domain` + the `domain-*` set, 45 for the runtime spine, and **8
> for twelve crates** — the shared-kernel reading the boundary column invites is not what the data
> says.
> ⚠️ **The number that names the work**: `bin_runtime` carries the `domain` facade into **45 of 57**
> apps, `infrastructure` into 10, `server` into 8, `surface_runtime` into 5. Decomposing the first is
> the single largest isolation move available — which is
> [PROP-20260811-090000](proposals/PROP-20260811-090000-scope-isolation-runtime-decomposition.md)'s job,
> untouched here: an index renders the debt, it does not repay it.
> The **needed-and-not-granted** column has exactly one row and it is the recorded trap:
> `worker-sirene-sync` needs `INSEE_API_TOKEN` and its pod does not carry it (no production
> `from_secret` — GitHub Actions still injects it), which is correct today and breaks at the
> [#358](https://github.com/TheCaptainCompany/captain-food/issues/358) cutover. Grants are now ONE
> derivation (`bin_secret_env_keys`) shared by the pod manifest and the index, asserted app-by-app
> against the committed manifests, so the least-privilege slice (A4) cannot start from two answers.
> Two more things only this artifact shows: **`client-customer-credit` is reached by no deployable**
> (a generated actor client nothing links, while `client-restaurant` is reached by 45), and
> **`ADMIN`/`EXTERNAL` are claimed by no bounded context**, so two gateways sit under `platform` —
> and, through its gateway, the `bo-admin` surface — because nothing else is derivable — named out
> loud rather than left as a default that reads like a decision.
> **BND-1 closed the same day** (entry below; [DECISIONS §31](proposals/DECISIONS.md)): the index
> reads the boundary set from `c4-l2.yaml` `boundedContexts`, which IS that closed answer — five
> business contexts plus `platform` — so the index needed no edit when the row closed, and needs
> none if the set ever moves again.

> ✂️ **2026-08-11 — THE API TIER IS THE WIDEST APP IN THE TOPOLOGY, AND `server` IS ONE EDGE AWAY
> FROM EIGHT PODS** (docs-only; amendments in place to
> [PROP-20260811-090000](proposals/PROP-20260811-090000-scope-isolation-runtime-decomposition.md)
> §1/§4.1-§4.4/§5.1/§5.2 and
> [PROP-20260811-150242](proposals/PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md)
> §5.1.9/§8; new register section [DECISIONS §34](proposals/DECISIONS.md) — API-1, API-2, API-3, all
> **team-owned**). Product-owner directive, 2026-08-11: *"Remove the damn server crate it's currently
> the purpose of what we are doing"*.
>
> **Measured**: each of the 8 `graphql-*` subgraph bins **declares 3 workspace crates and links 44**
> -- 14x, against 1.5x for the 7 `gateway-*` bins. **25 of the 44 are reachable only through
> `server`**: `web`, `app-core`, `surface_runtime`, all five partner adapters
> (`stripe-adapter`, `uber-direct-adapter`, `hubrise-adapter`, `coopcycle-adapter`,
> `avelo37-adapter`), `shared_types`, and **14 of the 15 `crates/clients/*`**. A pod whose whole job
> is `catalog` and `categories` links the Stripe integration and the entire SSR renderer, and can
> spell `client_order::OrderClient`. The cause is a recorded design choice, not drift:
> `crates/server/src/bin_support.rs:1-8` says a subgraph IS the monolith's surface filtered by a
> scope **string** — defect 3 of that proposal's §1, reproduced in the API tier.
>
> **Three findings reorder work elsewhere.**
> **(1) REP-4 does NOT gate the API tier.** `EventStore` — the only port whose signature names the
> all-scopes `DomainEvent` — appears in **three** resolvers
> (`crates/server/src/graphql/generated/mutation.rs:4942,6384,6584`, i.e.
> `placeOrder`/`approveRefund`/`denyRefund`), and in all three inside the **`else` branch of the
> `pm_mailbox_delivery` gate**. Queries name it **zero** times; the subscription path carries
> `AppendedEvent = {String, String, Uuid, i64}`
> (`crates/infrastructure/src/persistence/event_bus.rs:20-31`), not the union. **Six of eight
> subgraphs never name it** — so the API tier is cuttable **before** the event split, which reverses
> that proposal's own "subgraphs are last cuttable" ranking.
> **(2) The real blocker is a GATE HOLE, and it outranks everything it let through.**
> `api-nested-cross-scope` forbids an api type in scope S from nesting another scope's type
> (`tools/codegen-rs/src/validate/scopes.rs:21-24`) and `make validate` reports 0 errors — while
> `specs/generated/schema.generated.graphql` contains **ten** such edges. The rule walks `$ref`s in
> the spec; the emitter **derives** these fields from FKs (`tools/codegen-rs/src/emit/server_graphql.rs:229`)
> and from `navRoles:`. **Four of the ten are cycles** (`network <-> ordering`, `network <-> delivery`,
> `network <-> catalog`, `delivery <-> ordering`), so per-scope API crates cannot exist at all — Rust
> has no cyclic crate graph. **Five of the ten resolve `Vec::new()` unconditionally**
> (`crates/server/src/graphql/generated/types.rs:1101-1105,1230`:
> `Restaurant.deliveryJobs/catalogs/carts/orders`, `Order.deliveryJobs`) and deleting them makes the
> graph **acyclic**. That deletion is a schema removal — register row **API-2**, with the migration
> story recorded (provably empty; zero first-party selections in `specs/screens/**` or
> `crates/web/src/**`; no third-party client; production down with an empty log — the free window
> closes at the [#358](https://github.com/TheCaptainCompany/captain-food/issues/358) cutover).
> **(3) The 8->6 subgraph reshape lands AFTER the cut, not before.** The compositions are
> **generated**, so cutting 8 costs the same as cutting 6, and the cycle set is identical either side
> of the merge. The cut is gated on nothing; the reshape still owes the superseding ADR on
> ADR-20260807-183024 D1's scope list.
>
> **What the directive can be satisfied by NOW**: removing `server` from the eight subgraph
> manifests (slice **A1** — extract `api_runtime` + `api_graph`; `server` keeps compiling by
> re-export and stays the monolith's composition root). **Deleting the crate** additionally needs the
> #358 cutover plus homes for three route sets — the SSR host fallback (slice 5's undrawn view-model
> boundary), `POST /auth/session` (already a recorded
> [#385](https://github.com/TheCaptainCompany/captain-food/issues/385) cutover precondition, **no bin
> home exists**) and `/internal/sirene/drain`. Both readings are written out in
> PROP-20260811-090000 §5.2 so the smaller one is never delivered silently.
>
> **Also corrected**: PROP-20260811-150242 §5.1.2's *"coarser is forbidden"* CONNECT argument is
> already violated **at boundary granularity** — five of eight subgraphs hold another boundary's read
> model inside a resolver (`crates/server/src/graphql/generated/query.rs:21,124-125,311-312,418-419`),
> and only `graphql-customer` and `graphql-platform` are clean. Register row **API-1**; the
> doctrine's own answer (*"pre-joined in a projector-owned view"*) is the recommendation. And
> **API-3**: `crates/gateway_runtime/src/lib.rs:121-122`'s *"any subgraph answers the role-filtered
> shape"* becomes false the day composition is per-scope — introspection must move to the gateway, or
> `graphql-platform` answers with 5 operations instead of 121.

> ✅ **2026-08-11 — BND-1 IS CLOSED: THE BOUNDARY SET IS FIVE, AND THE REGISTER'S LONGEST-STANDING
> ROW IS ANSWERED**
> (product-owner answer sheet, 2026-08-11; [DECISIONS.md](proposals/DECISIONS.md) §5 + §31;
> [PROP-20260811-150242](proposals/PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md)
> §0; [#493 "Two partitions, one domain: boundedContexts and specs/{scope}/ home 6 of 20 actors differently, and nothing reconciles them"](https://github.com/TheCaptainCompany/captain-food/issues/493)).
> Verbatim: *"I'm ok for the 5 / Customer / Order / Catalog / Restaurant / Delivery"*.
>
> **The boundary set is CLOSED as recommended: five business boundaries -- `customer` - `order` -
> `catalog` - `restaurant` - `delivery` -- plus the `platform` bucket and the `common` kernel** (a
> linkage concept with no pod, never a boundary). `catalog` stays a boundary; **`comms` and
> `payments` dissolve into `order`**; `public` stays a role of `customer`. **This unblocks slices
> 1-5 of [PROP-20260811-090000](proposals/PROP-20260811-090000-scope-isolation-runtime-decomposition.md)
> and 15 of the 28 crates in
> [PROP-20260811-173223](proposals/PROP-20260811-173223-repository-crates-and-the-infrastructure-split.md)
> REP-2(a)** -- the BND-1-GATE concern on that file is now checked. It also beats the clock:
> ADR-20260807-183024 D7's *"start-clean makes the storage split free -- the window that does not
> recur"* closes at the [#358](https://github.com/TheCaptainCompany/captain-food/issues/358) cutover.
> **Still owed before that proposal can be marked approved**: the superseding ADR on
> ADR-20260807-183024 D1's named scope list.
>
> **Four more rows close in the same message.** **BND-2** -- the boundary is **`delivery`, not
> `rider`**, reasoning endorsed. **BND-7** -- *"Estimate for now"*: the ETA frozen onto `OrderPlaced`
> is an **estimate, not a promise with a remedy**, and that must be reflected wherever the freeze is
> specified. **BND-6** -- *"Prep time only + labelled"*: when the travel leg cannot resolve, show the
> prep-time estimate **explicitly labelled as what it is** (which is precisely the defect already
> shipped at `specs/screens/restaurant_frontoffice.yaml:490`). **BND-4(i)** -- *"I agree it was the
> write side"*: actors and projectors read the **WRITE** side to load events, so the permission
> matrix may now be emitted on that reading. **APP-1** is **delegated to the team** with one
> deliverable demanded and not delegated: the app list plus all dependencies
> ([#491](https://github.com/TheCaptainCompany/captain-food/issues/491) slice A1).
>
> **NEW: in-between units for translating process managers are GRANTED, and BOUNDED (BND-8/BND-9).**
> Verbatim: *"I'm ok if we create in between boundaries for process managers that are making the
> translation between 2 boundaries thanks to the fact that we have one crate per actor client type."*
> The team has bounded it with **the `CONNECT` test**: a PM earns its own in-between unit only when
> it **writes two boundaries and reads at most one** -- because every PM write lands in ONE database
> (`domain_events` + `inbound_messages` + PM state are all inside `captain-write`, STO-1), so
> widening write reach widens an *enumeration*, while a second READ is a second `CONNECT` through the
> strongest wall in the matrix, i.e. BND-3's stop condition. **Classified: the concession creates
> ZERO units today and reserves exactly ONE candidate** (`DeliveryDispatchProcess`);
> `CartBindingProcess` is **CONFIRMED in `order`** under the new third option, because it commands
> one boundary and reads one boundary -- both `order` -- and its customer-side trigger is a mailbox
> fact, not a data reach.
>
> ⚠️ **Two measured findings arrived with it, and both correct things already written down.**
> **(1) The concession's premise is not true of process managers today**: `deliver:` is a DIRECT
> append to the target aggregate's stream (`crates/application/src/generated/process_managers.rs:118-122`)
> and `send:` runs the target's command handler **in-line** (`:786`) -- neither goes through
> `crates/clients/{actor}`, the target's mailbox lane, or its lease. **The DSL's own doctrine header
> says the opposite verbatim** (`specs/common/processmanager.yaml:7-9`: *"a process manager never
> appends to `domain_events` itself"*). That is a spec claiming something the code does not do, on
> the write path, and it is the concrete caller that makes **ISO-3** load-bearing. **(2) BND-3's stop
> condition already fires, twice**: `PlaceOrderProcess` reads the **restaurant** boundary's
> `Restaurant` read model on the CHECKOUT path (`specs/ordering/processmanager.yaml:38-41`, feeding
> four guards) and `DeliveryDispatchProcess` reads it for the pickup address
> (`specs/delivery/processmanager.yaml:42-46`). D9's claim that a `customer`-homed
> `CartBindingProcess` *"would be the first such grant in the system"* is **wrong** -- two exist
> today. Recommended remedy: the `restaurant` boundary publishes the five slow-moving fields and each
> consumer's projector folds a slim snapshot into its OWN read database -- the same
> composition-in-the-projector answer STO-2(a) already gave for `ScopeMembership`.
>
> 🧱 **2026-08-12 — A READ TARGET IS DECLARED, NEVER INFERRED: the `reads:` ownership wall is a gate**
> ([#507 "fix(codegen): a read target is DECLARED, never inferred"](https://github.com/TheCaptainCompany/captain-food/pull/507),
> MERGED as `158c85a`, closing [#506](https://github.com/TheCaptainCompany/captain-food/issues/506);
> [ADR-20260812-214500](adr/ADR-20260812-214500-a-read-target-is-declared-not-inferred-the-reads-ownership-wall.md)).
> Retiring `command_journal` cost 110 files because the table had leaked out of its encapsulation into
> resolver bodies -- founder verdict: *"it should never be used directly because we have to pass through
> the actor clients that encapsulate the insert"* and *"it's unacceptable"*. Two absences allowed it, both now ERRORS, both **seen red on `786bcfa` first**:
> (1) `reference: true` was an unguarded opt-in whose only counter-argument was a header comment, and a
> BARE-NAME `reads: ['inbound_messages']` bypassed even the §1b ref-kind contract (a bare name is
> invisible to the refs walker -- the #413 defect class) -- planting both **passed `make validate` with
> zero errors**; (2) transience was inferred from a MISSING `reads:`, so deleting one line silently
> exempted a query from every read-side rule -- also zero errors. That second one is what actually let
> the journal through: the journal queries declared no `reads:` at all. Five new errors
> (`reference-flag-not-a-read-target`, `reads-infrastructure-owned`,
> `reads-infrastructure-with-read-model`, `transient-type-undeclared-infrastructure`,
> `reads-not-a-ref`) in the new `validate::read_targets`, keyed on `refs::classify`'s `Kind` -- never on
> a name pattern (`external_%` matches 1 of 7 categories) and never on the author's own `staging: true`.
> The allowlist fails CLOSED in both directions, but only ONE of them is the compiler, and the precise
> version is the reusable one: `refs::read_target_kind`'s match is exhaustive, so a new **`Kind`** does
> not compile until it is classified -- while a new catalog **FILE** is accepted by `classify`'s
> `_ => None` and fails closed at VALIDATE instead (`ref-kind-unknown` + `reads-unknown-view`).
> `reservations.yaml` is the proof: no arm for months, built fine. Level 4 for the kind, level 3 for the
> file. It gained the classifier arm it never had. Four transient types now DECLARE their table
> (`readsInfrastructure:`): `MailboxLane` + `PoisonedMailboxMessage` + `Operation` -> the mailbox,
> `PaymentIntent` -> the saga row -- and the key admits **`JournalTable` + `PmStateTable` ONLY**. That
> narrowing came from the independent review, which found the first cut had wired it to the whole
> infrastructure partition: `hubrise_connections` and `domain_events` under `readsInfrastructure:` on the
> PUBLIC-reachable `Operation` type validated with ZERO errors, while the same `$ref` under `reads:` had
> always been refused -- the new key had **opened a door that was shut**. Fixed, with the missing
> mutation test added; the lesson is that a new permission needs its own red-first plant, not just the
> rule it was added to serve. **Deliberately untouched**: `c4-l3` `components.*.reads`, the correct
> home for infrastructure readers. **Honest limits, now FILED as the compiler-first halves of this
> change** -- a validator rule is level 3, and both of these are reachable at level 4:
> [#510 "mailbox query ports behind a capability witness"](https://github.com/TheCaptainCompany/captain-food/issues/510)
> -- `crates/actor_client`'s `MailboxAccess(pub(crate) ())` witness closes the mailbox WRITE door but not
> `MailboxLaneRepository`/`MailboxRequeue` (`crates/application/src/queries.rs`), and the existing witness
> cannot be reused because `actor_client` depends on `application`; and
> [#512 "pool + schema probe out of `crates/server`"](https://github.com/TheCaptainCompany/captain-food/issues/512)
> -- `sqlx` canNOT simply be dropped from `crates/server/Cargo.toml`: there is no `sqlx::query`, but there
> IS `sqlx::raw_sql` (the `_sqlx_migrations` probe, `lib.rs:1497`) plus `PgPool`/`PgPoolOptions`/`Row` in
> the composition root. **And the rule that earned this entry a correction of its own**: the
> `reference: true` guard, taken alone, would NOT have caught `command_journal` -- the journal's queries
> declared no `reads:` at all, so only the second absence (transience inferred from a missing key) closed
> the path that was actually used.
>
> ✅ **2026-08-12 — THE JOURNAL CONCERN IS CLOSED: `inbound_messages` is the only journal (#242
> Runtime D, [ADR-20260812-000000](adr/ADR-20260812-000000-the-pm-mailbox-flip-rides-the-journal-retirement.md)).**
> Product-owner direction, 2026-08-11: *"Remove inbound events and command journal from the dsl, the
> only tables that must remain is inbound messages"* -- answering the earlier *"make sure we don't do
> both."* `inbound_events` was backfilled and DROPPED by `20260731143000`; `command_journal` is
> dropped by `20260812000000`. With it go: the legacy journal+spawn arm of
> `placeOrder`/`approveRefund`/`denyRefund` (the emitter now FAILS GENERATION on an unaddressed
> mutation rather than falling back), the `operationStatus`/`operationStatusChanged` fallback and the
> cross-arm duplicate read, the `worker-journal-sweep` CronJob (**57 apps -> 56**), the
> `command_journal` leg of `sweep_retention()`, and the `CommandJournalStatus`/`CommandChannel`
> scalars. **The `PM_MAILBOX_DELIVERY` gate is deleted, not defaulted ON**: its OFF arm WAS the
> journal, so with the table gone OFF would have meant "mailbox mutations, no B2 chaining, saga
> triggers back" -- the silent paid-order stall. The `RuntimePosture` mechanism (#318) stays with no
> tenant; its fail-closed read keeps its test, which exercises the CONTRACT over an arbitrary key AND
> the migration's idempotence over `PM_MAILBOX_DELIVERY` -- the only key the seed statement names, and
> therefore the only one on which "an operator flip survives a re-apply" can fail for its stated
> reason.
>
> 🧹 **The guard's PROSE outlived the guard, and three lens reviews plus the product owner missed it**
> (found by the automated PR reviewer, corrected on the branch). The bin emitter still promised a
> mechanism this change deletes: `pm-place-order`/`pm-refund` shipped *"the fleet reads the money
> posture itself and refuses the lane when it is unprovable"*, and all fifteen `actor-*` bins shipped
> *"posture-gated money lanes"* -- on lines no diff hunk touched, in the file an operator opens first
> when a money PM pod is stuck at peak. The sibling that hid the same way: the five-line doc comment of
> the deleted `pm_mailboxes` field, which Rust re-attached to the `only` field beside it, so
> `ProcessManagerRunner` documented a gate flip on a field that picks a PM. All now say what is true
> (the fleet drains exactly the lane set it is handed). **No gate is reachable** -- catching it needs a
> source-text scanner over comment prose, the class ADR-20260803-234035/#329 rule out -- so the defence
> is recorded as procedure in the ADR: when a mechanism is deleted, grep its VOCABULARY, not just its
> identifiers, across the emitter and the generated output.
>
> ⚠️ **A leg reserved to the product owner was TAKEN, and it is recorded rather than assumed**:
> [DECISIONS.md](proposals/DECISIONS.md) §32 JRN-1 held that flipping `PM_MAILBOX_DELIVERY` is a
> money-path posture change needing *"a staging smoke and a one-line ADR"*. The ADR exists; **the
> staging smoke does not, and was not performed** -- the flip was taken inside the empty-log /
> production-down window, where a smoke of the gated form has nothing to smoke against. JRN-1 is
> CLOSED saying exactly that, and is the place to object: while the log is still empty the reversal is
> a `git revert` plus a down-migration, and it gets more expensive with every real order.
>
> ❗ **The other half of the API-lens finding STANDS and is NOT fixed here**: the permission matrix in
> [PROP-20260811-093000](proposals/PROP-20260811-093000-storage-boundaries-and-least-privilege-database-users.md)
> §6.1.2 grants the *query* path **no `CONNECT` to the write database at all**, so the acceptance poll
> breaks on the mailbox read too -- up to 30 polls at 1 s per action, i.e. every checkout, every
> restaurant acceptance, every rider transition. The recommended `command_journal` grant-with-expiry is
> now moot (the table is gone) and the proposal is updated in place; the `inbound_messages` read grant
> is still owed.

> ⏱️ **2026-08-11 — THE ETA IS THE PRODUCT, AND NOTHING COMPUTES IT; PLUS: ONE EVENT LOG**
> ([PROP-20260811-150242](proposals/PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md)
> D9/D10/D13/D14,
> [#493 "Two partitions, one domain: boundedContexts and specs/{scope}/ home 6 of 20 actors differently, and nothing reconciles them"](https://github.com/TheCaptainCompany/captain-food/issues/493),
> register rows BND-5..BND-7 in [DECISIONS.md §31](proposals/DECISIONS.md)). **Four questions that had
> been surfaced to the product owner are answered by the team instead** -- they were answerable from
> doctrine plus the code and should not have been routed out.
>
> **The headline: nothing computes an ETA anywhere, and two shipped surfaces already promise one.**
> Zero repo-wide hits for an ETA function. **No pre-order estimate exists at all** -- the two
> `estimated*` values the system holds both arrive AFTER the customer has paid (`estimatedReadyAt` <--
> `OrderAcceptedByRestaurant`; `estimatedDropoffAt` <-- `DeliveryAcceptedByPartner`, and that one is
> **unfed on the partner path**, `projection/worker.rs:441-444`). Meanwhile:
> `specs/screens/restaurant_frontoffice.yaml:490` renders an `eta_bar` labelled *"Estimated arrival" /
> "Arrivee estimee"* bound to `{{ order.estimatedReadyAt }}` -- the KITCHEN READY time -- and it is
> visible during `OUT_FOR_DELIVERY`, exactly when ready-at is already in the past; the right field
> (`estimatedDropoffAt`) sits unused on the same GraphQL type (`specs/ordering/api.yaml:62`). And
> `specs/screens/captain_frontoffice.yaml:206` offers four marketplace sort options including
> `delivery_time_asc` over `queries/restaurants`, which declares 11 args and **no sort**
> (`specs/network/api.yaml:66-83`). **A wrong ETA outranks a missing one.** Both are screen-spec
> defects independent of every boundary question.
>
> **D13 -- the ETA is a READ-SIDE COMPOSITION owned by `order`, frozen onto `OrderPlaced` at
> checkout.** Not a projection: Young's fold rule (current state is a left fold of the event stream)
> kills it, because the pre-order estimate depends on *now* -- queue depth, rider supply, an address
> typed thirty seconds ago and in no stream -- so a replay cannot reproduce it. Not a process manager:
> a PM's output is commands, and the ETA changes nothing. It is the pattern this repo already proved
> for pricing -- `price_cart` live on every read, authoritative freeze once at checkout, fail-closed to
> an honest no-value state. **Its durable output is naming the THIRD sanctioned cross-boundary
> mechanism the architecture was missing -- a read-time query contract** -- beside the projection fold
> and the PM bridge.
>
> **D14 -- ONE event log; boundaries are write-isolated and read-shared on it.** Stated because it was
> only ever implied. `domain_events.position` is the global total order and **two** projection groups
> fold across boundaries on it (`Order` at `worker.rs:447-450`, `ScopeMembership` at `:507-510`), and
> **no boundary reshape removes them** -- so a per-boundary log would break replay determinism.
> **REP-4 is orthogonal** (storage is already untyped). **ISO-3 is no longer orthogonal and rises in
> priority**: under a shared log, write-exclusivity per stream category IS the write-side boundary,
> and `EventStore::append` takes a bare `stream_name: &str`.
>
> **D9 -- `CartBindingProcess` -> `order`**, the one member that makes the two partitions identical.
> The losing side has a concrete price: a customer-boundary PM would need the system's first `GRANT`
> spanning two boundaries. **D10 -- notification is THREE parts, not two**: policy in `order` (the
> `reminders:` mechanism is already declared on `Order` at `specs/ordering/actors.yaml:92-96` and used
> only for GDPR retention, while `OrderPlaced` schedules **nothing**), **recipient contract in
> `restaurant`** (absent entirely), transport in `platform`.
>
> **Two genuinely product-owner-owed rows are new**: **BND-6** (what the customer sees pre-order when
> the travel leg cannot resolve) and **BND-7** (is the frozen ETA a promise with a remedy, or an
> estimate?) -- BND-7 **before** the freeze lands, since adding a field to an already-stored event is a
> migration and it is nearly free before the [#358](https://github.com/TheCaptainCompany/captain-food/issues/358)
> cutover. **BND-1 (the boundary set) was answered on 2026-08-11 -- see the entry at the head of
> this file; BND-6 and BND-7 are answered too.**

> 📦 **2026-08-11 — REPOSITORY CRATES: TWO OPEN ROWS CLOSE, AND THE COUPLING NOBODY HAD NAMED**
> ([PROP-20260811-173223](proposals/PROP-20260811-173223-repository-crates-and-the-infrastructure-split.md),
> [#497 "Repository crates and the dissolution of `infrastructure`: read and write are separate crates, and \"inherit\" is right on the log and wrong on the read model"](https://github.com/TheCaptainCompany/captain-food/issues/497),
> register rows REP-1..REP-5 in [DECISIONS.md §33](proposals/DECISIONS.md)). Product-owner direction:
> *"We also have to create crates for repositories. There is read repositories and writes
> repositories, the write repositories generally inherit from the read repositories"* / *"The
> infrastructure has to be split in multiple crates to be able to regulate permissions of apps based
> on what they need nothing more."* Third message of the day and the third face of one idea — §31
> decides which units exist, §32 what shares a recovery posture and a database role, §33 what a unit
> may link.
>
> **ISO-1 and ISO-2 are CLOSED, both as (a)** (register §29 + §5). Both (b) options end with a bin
> linking a crate that carries every other boundary's code -- ISO-1(b)'s own wording is *"the bin
> keeps linking `infrastructure`"* -- which is what *"nothing more"* forbids.
> **[#423 "Design record for the per-scope infrastructure split"](https://github.com/TheCaptainCompany/captain-food/issues/423)
> slice 1 is no longer blocked on those two rows.**
>
> **"Inherit" is right on the log and wrong on the read model, and the code already argues it.** There
> are TWO read contracts on every read model: the **query** port (`CartReadRepository`, 5 methods;
> `by_id` returns `None` for a CHECKED_OUT cart, `queries.rs:277-279`) and the **row-state** port
> (`cart_store::load`, unfiltered). The projection write repository inherits the row-state one --
> supertraiting it onto the query port is over-privilege **and** a correctness bug, and
> `persistence/cart.rs:67-70` says exactly why in a comment written for another reason. On the write
> side the supertrait is right unqualified: `EventStore: EventStreamReader` creates the **log-read
> port that does not exist today** (three components read `domain_events` three different ways --
> `EventStore::load`, `projection/worker.rs:753`, `deletion.rs:255,320`).
>
> **The blocker nobody had named (REP-4)**: `DomainEvent` is ONE enum over all 8 scopes, defined in
> the facade (`domain/src/generated/events.rs:20`) and named by `EventStore` and the projector
> `Envelope`. A per-boundary repository crate that traffics in it links everything, so slice 1 as
> written would deliver a smaller module tree and the **identical** closure. It is **not** an
> event-versioning question -- storage is already `(event_type TEXT, payload jsonb)`
> (`event_store.rs:203`), so no stored contract moves.
>
> **Topology**: ~28 net-new crates -- 3 per boundary (`ports-{B}` with no `sqlx` · `read-{B}` SELECT
> adapters · `projections-{B}` folds + load/upsert) plus 13 platform crates (`store_core`,
> `eventstore`, `mailbox_pg`, `projection_runtime`, `read-platform`, `erasure`, 7 `acl-{partner}`).
> **`crates/infrastructure` (~13,200 lines) is dissolved**, surviving the
> [#358 "MKS bootstrap"](https://github.com/TheCaptainCompany/captain-food/issues/358) window only as
> a monolith-only composition crate behind a codegen guard.
>
> ✅ **BND-1 ([#493 "Two partitions, one domain"](https://github.com/TheCaptainCompany/captain-food/issues/493))
> is CLOSED (2026-08-11): B = 5**, so the 15 per-boundary crates are unblocked and the BND-1-GATE
> concern on that proposal is checked. **Dispatchable today, boundary-agnostic**: the ratchet
> dimension on [#490 "Scope-closure ratchet"](https://github.com/TheCaptainCompany/captain-food/issues/490),
> `store_core` + `eventstore` + the reader split + the ISO-3 witness, `projection_runtime`, the 7
> partner ACL crates.

> 🧮 **2026-08-11 — THE WARNING BASELINE IS A GATE, NOT A NUMBER IN A DOC**
> ([ADR-20260811-170559](adr/ADR-20260811-170559-the-validator-owns-the-warning-baseline.md)).
> `tools/codegen-rs/warning-baseline.json` holds the per-rule warning histogram and validator §17
> asserts it on every `make validate` / CI run, **in both directions**. Nothing to re-measure: a green
> validate already proves "no new warning". If a change moves the warning surface, run
> `make warning-baseline` and commit the refreshed artifact in the same commit (the `+1 <kind>` diff is
> the record; say in the PR body why an added warning is accepted). The old prose pin went stale three
> times (32 → 43 → 37) and cost four agents a pristine-`main` validator run each in one day.
> **Every field is asserted, `doc` string included** — review caught the artifact shipping a `doc`
> naming the wrong validator section, hand-patched in the one file whose own text forbids hand-editing.
> `make warning-baseline` refuses to write from a model with errors, so a red spec cannot mint a
> blessed baseline.

> 🗄️ **2026-08-11 — THE STORAGE SPLIT IS COSTED, AND IT FOUND TWO DEFECTS THAT ARE NOT ABOUT THE
> SPLIT**
> ([PROP-20260811-093000](proposals/PROP-20260811-093000-storage-boundaries-and-least-privilege-database-users.md),
> [#494 "Storage boundaries and least-privilege database users: the write-side transactional unit, the five-database split, and the last five View_*"](https://github.com/TheCaptainCompany/captain-food/issues/494),
> register rows STO-1..STO-6 in [DECISIONS.md §32](proposals/DECISIONS.md)). Product-owner directive:
> five databases (`DomainEventLogDb`, `DomainCommonDb`, `CatalogDb`, `OrderDb`,
> `BehaviorEventTrackingDb`) plus a per-app least-privilege database user derived from the spec. The
> access model is **accepted and correct**; the dba lens completed it, priced it, and named where it
> does not close.
>
> **The `View_*` blast radius is real and small — and it points the other way.** Measured: **5** SQL
> fold views vs **11** already-materialized projection tables; **9 of 32** GraphQL queries break if
> `domain_events` leaves the read database, **23 survive**, and **zero of the broken ones are on the
> money path** (`Cart`, `OrderTracking`, `Catalog`, `Restaurant`, `Customer` are all tables already).
> The five stragglers are the rider board, the restaurant delivery board, claims, the refund queue and
> the timeliness insight. Recommended way out: **convert them to materialized projection tables** —
> which the product owner's own rule already implies (*"the writing of the read side is done only by
> the projectors"* is vacuous for a SQL VIEW nobody writes). `postgres_fdw` and logical replication are
> rejected with reasons in the proposal.
>
> **Defect 1 — the erasure engine fails OPEN.** `crates/infrastructure/src/deletion.rs:229-233` bounds
> its scan at `COALESCE(MIN(position), i64::MAX) FROM projection_checkpoint`, clamped to log head. A
> database with **zero** checkpoint rows therefore erases at head with **no** fold verification —
> exactly the database the split creates, and exactly what a start-clean production is. Fix: a
> `projection_watermark` table in the write DB, heartbeated monotonically by each projector, with a
> **fail-closed** default. Precondition for the split; worth landing even without it.
>
> **Defect 2 — 8 indexes that do not exist.** The 5 views declare 8 secondary indexes; a Postgres view
> cannot be indexed and `views.generated.sql` emits **zero** `CREATE INDEX`. `myDeliveries` therefore
> folds every delivery job in history to return the 3 a rider holds: at ~120 jobs/day, month 6 is
> ~21,600 jobs × 8 correlated subqueries ≈ **173,000 index probes per call**, polled by every rider at
> Friday peak. This is due whether or not the split happens.
>
> **The one thing the directive must change**: `DomainEventLogDb` cannot hold the log alone.
> `actor_runtime/src/completion.rs:71-100` commits appends + PM state + reminders + the
> `inbound_messages` flip + the fenced `mailbox_partitions` advance in ONE transaction — separating log
> from mailbox does not weaken atomicity, it **deletes the fencing token** (a paused pod waking at
> 20:40 with a stolen lease would have its appends commit). Widen it to `captain-write`. The
> transaction the product owner *asked* about — projector fold + checkpoint — **survives the split
> untouched**, because a co-located checkpoint plus an idempotent fold is at-least-once + idempotence,
> not 2PC.
>
> **Also priced**: one CNPG cluster with five databases (five clusters do not fit the node), a
> **session-mode** pooler as a prerequisite (the split puts the fleet at ~235 against
> `max_connections: 220`; transaction mode silently kills `LISTEN`), five migration chains with
> `REQUIRED_SCHEMA_VERSION` becoming a map, and behaviour tracking at **~17.5 GB/yr — ~13× the business
> log** — which needs a declared retention policy shipping *with* its first table, not after.
>
> **Reconciled on landing, and both matter to whoever generates the grants.** (1) It pairs with
> [PROP-20260811-150242](proposals/PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md)
> ([DECISIONS §31](proposals/DECISIONS.md)) — **boundaries decide which units exist, storage decides
> what shares a recovery posture and a buffer pool** — and storage deliberately does **not** follow the
> boundary one-to-one (BND-3), with the stop condition worth becoming a validator rule: *if any app's
> `GRANT` spans two boundaries' schemas outside the declared exceptions, the shared database has
> silently become an integration database.* (2) ⚠️ **The permission matrix omitted the mailbox, and the
> omission is load-bearing**: GraphQL mutation resolvers write `inbound_messages`
> (`crates/server/src/graphql/generated/mutation.rs:42`), so *"the writing of the write side is done
> only by the actors"*, taken literally as a `GRANT`, **makes every mutation fail at runtime**. The
> matrix now names the mutation-resolver row explicitly — CONNECT to `captain-write` plus **INSERT and
> SELECT** on `inbound_messages` and nothing else (SELECT because `RETURNING` needs it, and because the
> idempotent-retry arm is a plain `SELECT`) — proposal §6.1.1, which also flags that the directive's
> fourth bullet is a transcription slip for the **write** side and must be confirmed before it becomes
> a role.

> 🗂️ **2026-08-11 — THE 57-APP LIST, AND THE PER-APP KNOWLEDGE THAT LIVES IN RUST**
> ([PROP-20260811-141654](proposals/PROP-20260811-141654-per-app-declaration-folders.md),
> [#491 "Per-app declaration folders"](https://github.com/TheCaptainCompany/captain-food/issues/491),
> [DECISIONS §30](proposals/DECISIONS.md); docs-only, no `specs/**` touched.)
> Product-owner request: *"Give me the app list to be on the same page… create a sub folder for each
> app/worker and indicate what it contains."* **Half of it needed no decision** — the 57 apps grouped
> by family, with what each family contains, are §1 of the proposal (15 `actor-*` · 5 `pm-*` ·
> 7 `projector-*` · 8 `graphql-*` · 7 `gateway-*` · 5 `fo-*`/`bo-*` · 5 `adapter-*` · 4 `worker-*` ·
> `bam`).
> **The other half is a "no" inside a "yes".** The app list already exists as source
> (`specs/architecture/c4-l2.yaml` `containers:`), and a folder in `specs/**` **cannot** make a scope
> boundary real — only the crate graph does, which is
> [PROP-20260811-090000](proposals/PROP-20260811-090000-scope-isolation-runtime-decomposition.md)'s
> job and is untouched. So the recommendation is deliberately narrower than the request: **source for
> deploy-owned facts only, generated for everything derivable, and the `containers:` block MOVED
> rather than copied** — a folder that restates the derivation is a drift surface, which is the one
> outcome worse than doing nothing.
> **What the folder is genuinely FOR**: the per-app knowledge that today lives in **Rust, inside the
> generator** — `worker_config_consumers()` is a literal `match name { "worker-sirene-sync" => … }`
> (`tools/codegen-rs/src/emit/bins.rs:217-224`), the grant narrowings are per-family `if`s (`:111-139`),
> and `replicas: 1` / `strategy: Recreate` are string literals (`tools/codegen-rs/src/emit/deploy.rs:335-340`)
> under a comment promising *"Flipping either value is a SPEC change"* while **no spec key exists to
> flip**.
> ⚠️ **The measured finding is a credential boundary, not a code one.** `adapter-stripe` — the pod
> whose stated reason to exist is *"holds ONLY this partner's secrets"* (`c4-l2.yaml:125`,
> `emit/bins.rs:415`) — carries **13** secrets in its generated pod env, including `AUTH_SESSION_KEY`,
> `SUPABASE_SECRET_KEY`, `EXTERNAL_API_TOKENS`, `INTERNAL_TRIGGER_TOKEN` and the four `OVH_*` SMS
> credentials; `gateway-public` (*"no DB access, no business logic, no state"*) carries **10**;
> `bam` carries **18**, including `STRIPE_SECRET_KEY`. The narrowing mechanism exists and works —
> `worker-erasure` carries exactly **2** (`worker_key_allowed`, `emit/bins.rs:131-139`) — it is applied
> to one family. The derivation is also too NARROW somewhere: `worker-sirene-sync`'s pod env has no
> `INSEE_API_TOKEN`, and `SireneClient::from_env` returns `Err` without it
> (`crates/sirene_ingest/src/client.rs:100-102`) — correct today, a live trap at the #358 cutover.
> **Sequencing is the one product-owner row** (§30 APP-1), because this and the 2026-08-11 enforcement
> directive compete for the same weeks; recommended answer is slice A1 (the generated app index, no
> source moved) now and the rest after §29 slice 1, so nothing displaces the enforcement track.
> **[#490 "Scope-closure ratchet"](https://github.com/TheCaptainCompany/captain-food/issues/490) is
> unaffected and stays dispatchable** — with one accuracy note for its executor: recomputing the
> closure over the workspace manifests gives **49** violating bins, not 50, and the clean set is the
> 7 `gateway-*` **plus `bam`** (which declares all 8 domain crates, so under the issue's own equality
> rule it passes — listing it in `PENDING_DECOMPOSITION` would land the ratchet red).

> ⚖️ **2026-08-11 — THE ERASURE-FREE ZONE, CORRECTLY FRAMED: THE STREAMS WERE **ALREADY** PERSONAL
> DATA, AND TWO FORWARD TRAPS ARE NOW ON THE RECORD**
> ([BRIEF-20260811-erasure-zone-and-retention.md](legal/BRIEF-20260811-erasure-zone-and-retention.md);
> docs-only, no `specs/**` touched).
> **The correction first, because it was recorded wrong.** The legal-lens pass over
> [PR #488 "The open GraphQL path verifies credentials, and `current` is tenant-scoped by Host"](https://github.com/TheCaptainCompany/captain-food/pull/488)
> / [#469 "`current` leg 1 is dead on the web AND is not tenant-scoped — fix both together or neither"](https://github.com/TheCaptainCompany/captain-food/issues/469)
> said `Cart-*`, `Customer-*`
> and `Restaurant-*` *"were an erasure-free zone and are now subject-attributable"*. **The second
> half is wrong**, and the error is not cosmetic — "became personal data" invites the reading that
> the obligations attach from now on, which would waive storage limitation, transparency and Art. 30
> records for everything already designed. These streams were personal data by construction:
> `CartStarted` **requires** `sessionId` (`specs/ordering/events.yaml:33-51`), the `SessionId` scalar
> describes itself as *"used to bind carts and track the user across devices"*
> (`specs/common/scalars.yaml:13-16`), `CartBoundToCustomer` writes the domain customer id onto the
> same stream via a **designed** linking process manager, and `CustomerRegistered` **requires**
> `phone` — `Customer-*` never needed the pseudonymity argument at all. Art. 4(1), Recital 30,
> Art. 4(5) and CJEU C-582/14 *Breyer* all land the same way for a controller that operates the
> linking mechanism.
> **What #469 genuinely creates is narrower and different in kind**: seven open-path commands now
> stamp `domain_events.user_id` with the **Supabase `sub`** (`crates/server/src/auth.rs:112-116`),
> putting an **external identity-provider identifier** into the immutable write envelope of three
> stream categories with **no erasure path** — and it **survives deletion of the Supabase identity**,
> leaving an orphan in an append-only column. Whether that orphan is anonymous under Recital 26 or
> still personal data is the question that decides whether **crypto-shredding** is optional or
> mandatory (counsel packet **G4**).
> **This is NOT a pre-existing breach and is not filed as an incident.** The production event log is
> **empty by decision** (start-clean, ADR-20260807-002705 D6 — *"the window is open only while the
> log is empty"*), so there is no data subject for an Art. 17 path to have failed. It is an **unmet
> launch precondition, already correctly filed as
> [#194 "GDPR erasure"](https://github.com/TheCaptainCompany/captain-food/issues/194)**. What changed is #194's
> **size**, and it is boundable: **three stream categories, one identifier kind, no new obligation
> class**. Trigger moment: the **first real customer order** — the same deadline as the Art. 35 DPIA
> and the médiation-de-la-consommation registration.
> ⚠️ **Trap 1, and it is the dangerous one — `Restaurant-*` must NEVER get an `Order`-shaped deletion
> policy.** `RestaurantListingOptedOut` (`specs/network/events.yaml:344-356`) **is** the Art. 21
> objection register; [the 0808 brief](legal/BRIEF-20260808-listing-opt-out-objections.md) Q1/Q4
> states the historical event must be retained because *"it is the register, not stale data"*. The
> one built erasure mechanism is *tombstone → delete the whole stream → receipt*
> (`specs/ordering/actors.yaml:97-103`), and `Restaurant-*` will arrive at the #194 sweep as one of
> the three categories with no path. Giving it that block would delete the proof of objection and
> **permit re-listing** — the exact ProspectionPipeline failure the 0808 brief exists to prevent.
> Nothing is broken today (`Restaurant` declares no `deletion:` block), so this is
> **BLOCKER-on-arrival**, not a live defect.
> **A gate was assessed and is NOT buildable today, for one reason**: the deletion DSL is well-formed
> and already validated (`deletion-ref-unresolved` / `-match-untyped` / `-tree-cycle`), so the rule's
> shape is easy — but the spec has **no way to say "this event is a legal register"**, and the only
> alternative is hard-coding the event name in the validator, which is a comment written in Rust
> rather than a spec-derived gate. **The fix is one small spec addition and it belongs to #194**: a
> `legalRetention:` clause on the event naming its instrument and horizon, `$ref`-able from the
> MET-W retention-window catalog; the rule then writes itself — *an actor whose `emits` reaches a
> `legalRetention` event may not declare a stream-deleting `deletion:` block*. Until it lands the
> hazard is **prose**, which is the weaker form on purpose-built record.
> ⚠️ **Trap 2 — the retention control is asserted and inert.**
> `specs/database/tables/eventstore.yaml:38-39` states that ephemeral streams such as `Cart` get a
> retention row; **none does**. `domain_stream` has **zero production writers** — the only `INSERT`
> in the tree is a test fixture (`crates/infrastructure/tests/main/deletion_engine.rs:99`); every
> other reference is a `DELETE`, a comment or a validator note. So `$maxAge`/`$maxCount` bind
> nothing and abandoned guest carts accumulate forever. Compounding it, [the erasure
> brief:82](legal/BRIEF-20260808-account-erasure-two-path.md) claimed the written retention schedule
> already existed *"in the DSL"* — false, as [DECISIONS MET-W](proposals/DECISIONS.md) recorded, and
> **corrected in place in this change**. Under Art. 5(2) that ordering matters: a controller document
> asserting a schedule its own system does not implement is **worse evidence than silence**. The fix
> is decided and only needs sequencing — MET-W's **named catalog of approved retention windows**,
> landing **with** #194.
> 🔎 **One open question of FACT, team-owned, not counsel's**: does any **non-production** environment
> hold real subject data? The empty-log argument collapses if it does. **Established from the repo**:
> no staging/preview environment is declared anywhere it could hold data (`render.yaml` declares no
> staging service; `staging` is a supported `APP_PROFILE` value with no service bound to it); CI's
> database is an ephemeral per-job `postgres` container; the 2026-08-11 k3s rehearsal migrated an
> **empty** database and never ran the auth/money smoke legs; there is no `docker-compose`, no `.env`
> and the single `*seed*` artifact is referential policy rows. **NOT established, and it is the part
> that matters**: the `DATABASE_URL` repo secret is opaque, `sirene-sync.yml` writes **real INSEE
> rows** (which include *entrepreneurs individuels* — personal data per *Manni*) through it, and
> `db-migrate.yml:29` documents the same secret as the Supabase pooler string; this repo's own
> history records ~200k SIRENE-derived listings and ~200k `domain_events` tuples in the **pre-cutover**
> database. Start-clean governs the **new** cluster; **the disposition of the old store is an
> operational fact nobody has recorded**. Also unanswerable from the repo: whether any Supabase Auth
> project holds real end-user identities. **Two answers are owed in writing before §2 of the brief
> can be relied on in a DPIA.**
> **Counsel packet extended to G1–G8** (appended to the consolidated packet in
> [BRIEF-20260808-listing-opt-out-objections.md](legal/BRIEF-20260808-listing-opt-out-objections.md)):
> empty-log reliance and the trigger moment · whole-stream deletion as the Art. 17 mechanism · **G3,
> marked blocking** — L123-22/L102 B vs Art. 17, and which closure (10-year window + projection
> tombstones, or export a financial skeleton first), blocking because the built path deletes the
> whole stream on **one** window with **no per-category split**, as `specs/ordering/configuration.yaml:10-21`
> says of itself · the orphaned `sub` and crypto-shredding · the Art. 21 register's minimum field set
> keyed on SIREN/SIRET · a per-category schedule validated against CNIL délib. 2021-044 · **G7**,
> `dietaryTags` as an unconstrained `array<Tag>` where `halal`/`kosher`/`allergy:peanut` are spellable
> **today**, with the DPIA unfinalisable while it is open · **G8**, Art. 18 restriction of processing,
> distinct from erasure and entirely unbuilt.
> **Two items reported for routing, deliberately not acted on here**: the `SessionId` scalar
> description (*"track the user across devices"*) **overstates the implementation** — an origin-scoped
> `localStorage` UUIDv7 (`crates/web/src/session.rs:14-31`) that tracks nothing across devices — and
> that wording is what decides whether the **Art. 82 LIL / ePrivacy 5(3) shopping-cart exemption**
> covers it, so the spec text is the riskier artifact than the code (a `specs/**` change); and
> `/public/graphql` now varies by the `captain_auth` cookie (ADR-20260811-113000) while **no `Vary` or
> `Cache-Control` exists anywhere in the tree**, so `Cache-Control: private, no-store` is recommended
> **on the #469 branch**, not here.

> 🔓 **2026-08-10 (night) — THE `specs/**` FREEZE IS LIFTED: THE DSL IS THE TEAM'S WORK**
> ([ADR-20260810-221840](adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md),
> product-owner directive: *"I'm surprise that I read that the spec was untouchable now that we have
> the team working together we don't need to have this constraint anymore… I'm pretty sure the team
> will ensure the right naming and scope. Just keep me informed."*). The last of four delegations in
> three days, and the one that reaches the work: prioritisation (ADR-20260810-215503), self-starting
> sessions (ADR-20260810-011500) and product ownership (ADR-20260808-144738) each delegated
> *judgement*; this delegates *capability*.
>
> **The boundary is NOT content-vs-structure** — that split is anti-correlated with risk in both
> directions (a scope-folder move rewrites no refs and is free, because `$ref`s are kind-logical; a
> one-word type change on an emitted event is irreversible). It is **three questions in order**:
> (1) does it contradict or create a **recorded decision**? → stop, file a `DECISIONS.md` row;
> (2) is the shape already **emitted, stored or promised**? → it is a **migration**, record the
> versioning story first (upcasting, never mutation); (3) otherwise it is the team's, **including
> structure and including `specs/common/`** (a high-fan-out shared kernel, not a no-go zone — freezing
> it would freeze the one place "one name = one dedicated scalar" is enforced). Structure gets **no
> separate gate**: proportionality already routes any real option space to a proposal + register row,
> which *is* the discussion the product owner offered.
>
> **Reporting replaces the freeze**: [docs/SPEC-LOG.md](SPEC-LOG.md) is created and usable now — one
> sentence per landed spec change, in product language, in the **same commit**. No cadence, no digest
> to send; it is a pull surface kept current by a gate. The gate's shape is `DECISIONS.md` **§26
> SPEC-1** (recommendation (d), ~30 seconds to answer); until it lands the page is prose.
>
> **Queue effect, measured**: 8 open issues carried an explicit AMBER flag and 4 more routed a
> sub-task to plan mode — [#468](https://github.com/TheCaptainCompany/captain-food/issues/468),
> [#476](https://github.com/TheCaptainCompany/captain-food/issues/476),
> [#466](https://github.com/TheCaptainCompany/captain-food/issues/466) and the already-approved
> 451-B `currency_mismatch` line are now **GREEN and dispatchable**. The "one plan-mode window for
> #468 + #476" recommendation **dissolves** — the window was the only thing binding them, and #476
> touches a key with **0 occurrences** in `specs/screens/**` and `specs/*/api.yaml`. #466 and #468
> still sequence together (same validator area; a rule and the spec fix that keeps it green must land
> in one change), #476 is independent.
>
> ⚠️ **Newly load-bearing and absent**: `event_version` has **zero occurrences** across `specs/`,
> `crates/`, `migrations/` and `tools/`, while PROP-170000 D2 decided *"add `event_version` now
> (cheaper before the log grows)"* on 2026-08-08. The freeze was silently standing in for it — a
> payload nobody could change needed no versioning story. This is the structural work the delegation
> calls for, and the window is open only while the log is empty (ADR-20260807-002705 D6, start-clean).

> 🧾 **2026-08-11 — PER-BIN SCOPE ISOLATION: THE MANIFESTS NOW SAY WHAT THE BUILD ENFORCES**
> ([#475 "Per-bin scope isolation is nominal: every actor/pm/projector bin transitively links all 8 domain scopes…"](https://github.com/TheCaptainCompany/captain-food/issues/475), comment half). Measured on
> the resolved dependency graph: **50 of the 57 bins link the `domain` facade** — hence all eight
> scope crates — behind their own scope list, through `bin_runtime` (actor/pm/projector/worker/
> adapter), `server` (the 8 `graphql-*` subgraphs) or `web` → `app-core` (the 5 `fo-*`/`bo-*`
> surfaces, which really do hold no server/infrastructure). Only the **7 `gateway-*` bins** are
> domain-free end to end. The emitted manifest header claimed the opposite for all 57 ("linking a
> domain crate is the ONLY way that scope's vocabulary exists in this deployable … *unspellable*
> rather than merely unrouted") — **this supersedes the "step-2's facade limit is now closed FOR THE
> BINS" line in the [#382 "Bin crates: per-actor/per-PM/per-projector/per-subgraph/per-gateway/per-surface
> binaries from the c4-l2 topology"](https://github.com/TheCaptainCompany/captain-food/issues/382) /
> [PR #383 "Bin crates: per-deployable binaries emitted from the c4-l2 topology (ADR-20260807-183024
> step 3)"](https://github.com/TheCaptainCompany/captain-food/pull/383) entry below**, which was true
> of each bin's SOURCE and never of its
> image. The header now separates the two: the crate's own source still cannot NAME an undeclared
> scope (real, compiler-first), while what bounds the pod today is a runtime string — but only for
> the families that HAVE one: `spawn_actor_fleet(LANES)` / `with_only(PM)` / `with_scope(SCOPE)` on
> the 28 mailbox/projection bins (15 `actor-*`, 5 `pm-*`, 7 `projector-*`, `bam`), **nothing at all**
> on the other 9 of the 37 that reach the facade through `bin_runtime` — the 5 `adapter-*` and 4 cron
> `worker-*` bins (an adapter's
> one real link fact is its partner slice; a cron bin is bounded by the single pass it calls per
> Job). For the subgraphs, `bin_support::subgraph_app` registers EVERY actor mailbox and slices the
> master schema by a scope string, so one can enqueue to any aggregate.
> A codegen test (`bin_manifest_scope_claim_matches_the_measured_closure`) now derives the sentence
> from the guppy closure in **both** directions, over the WHOLE emitted text of both artifacts —
> header, manifest `description`, `src/main.rs` module doc and const docs — after the first cut
> checked the header only and left the retired claim standing verbatim in 40 files, one of them
> contradicting itself 14 lines apart. So the prose cannot lag the graph once `bin_runtime` is
> decomposed. **The measurement also resized the program**: PROP-20260811-090000
> and DECISIONS §29 said 45, counting the 5 surfaces as clean because their manifest's *true* note
> ("no database, no server, no infrastructure") reads as isolation — so the debt ledger
> ([#490 "Scope-closure ratchet: a bin's transitive domain set must equal its declared set…"](https://github.com/TheCaptainCompany/captain-food/issues/490)) starts at **49 rows**
> (50 bins reach the facade; under #490's *equality* rule `bam` is honest — it declares all 8 and
> its closure is those 8 — so it is fat by design, not lying), and
> the proposal gains a **slice 5** for the surface family, whose path no other slice touches.
> Structural half (decompose `bin_runtime`, per-scope `infrastructure`
> [#423 "Design record for the per-scope infrastructure split…"](https://github.com/TheCaptainCompany/captain-food/issues/423), `crates/clients/*`) stays
> open on #475. Validate 0 errors / 37 warnings — equal to the freshly measured `482fa76` baseline,
> same six kinds.

> 🧭 **2026-08-11 — BEHAVIOUR TRACKING IS ISOLATED END TO END, AND A FAULTED WORKER PRE-DIAGNOSES
> ITSELF — BUT "SAY IT IN /health" WOULD TAKE THE STOREFRONT DOWN AS STATED**
> ([ADR-20260811-120828](adr/ADR-20260811-120828-behaviour-tracking-isolated-end-to-end-and-a-faulted-worker-pre-diagnoses-itself.md),
> [DECISIONS §27bis](proposals/DECISIONS.md) TRK-ISO / HEALTH-2 / HEALTH-2a / HEALTH-2b; docs-only).
> **TRK-ISO — behaviour tracking gets its own database AND its own projector worker**, *"completely
> isolated… to avoid dependencies between the behaviour event tracking and the business events"*. That
> is **further than [PROP-20260811-000946](proposals/PROP-20260811-000946-behaviour-event-tracking-in-the-screens-spec.md)
> D5** asked, and it matters more under the halt decision than it did before it: now that a rejected
> fold halts its group, a **shared** worker would let a malformed behaviour event wedge a group sitting
> beside the order read models. Separate workers make that unspellable rather than unlikely. The
> distinction is settled — behaviour events: own database, own worker, written by the UI through a
> `sink:` mutation, never `domain_events`; **business metrics: the `bam` schema and the `bam`
> projector**, a fold over `domain_events`. **C4 owes a new container plus edges**, and
> `specs/architecture/*.yaml` is **source DSL, not generated** — an executor spec change when the work
> lands.
> **HEALTH-2 — a faulted worker reports unhealthy and is NOT restarted.** *"K8s does not need to
> restart the worker"* is **independently the same conclusion** the team reached from the failure
> analysis (a deterministic fault re-fails after a restart, so liveness gives CrashLoopBackOff and
> takes sibling groups down) — the convergence is recorded, not just noted. And *"it's a pre
> diagnostic"* is the substantive requirement: **the payload is the deliverable, the status code is
> only the transport.** A health endpoint returning `{"status":"unhealthy"}` satisfies the code and
> fails the requirement, so the per-group breakdown — group, `haltedSince`, position, `eventType`,
> stream, error — becomes the point of the feature. **This is
> [no polling, only pushing](adr/ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)
> applied one layer up**: the failure pushes its own diagnosis into a surface already being watched,
> instead of a human polling pod logs to reconstruct it. On `500` — k8s treats any non-2xx as a failed
> probe, so **keep the existing `503`**, which is also semantically right; nobody should "fix" it for
> literal compliance.
> ⚠️ **HEALTH-2a — the edge, reported rather than discovered at cutover.** Verified on `37642cd`: the
> monolith runs the API **and** the projection worker in **one process** (`RUN_PROJECTOR`, default on,
> `crates/server/src/lib.rs:641-648`), serves `/{role}/graphql`, **has a `Service`**, and its `/health`
> is the ADR-0043 **deploy interlock** knowing only DB reachability and schema version (`:1503-1526`).
> So *"say it in `/health`"* there would make the **API** unready because a **read model** halted — a
> degraded projection turned into a **customer-facing outage**, and a halted projection blocking the
> deploy that would fix it. **The rule is restated so the edge cannot occur**: *the endpoint a pod's
> **readiness probe points at** returns non-2xx when a component **that pod is responsible for** is
> faulted* — not "`/health` returns 500". Projector bins probe `/projector`; the monolith keeps
> `/health` on API components only, with its in-process projector observable at `/projector`, **which
> is not its probe**. Final shape after cutover: which components a deployable hosts is already
> declared, so the probe path and the health composition can both be **generated from that
> declaration**.
> ⚠️ **HEALTH-2b — "any worker" does not apply unchanged, and the reason is a real asymmetry.** The
> actor-mailbox workers **already quarantine**: a repeatedly-failing message hits the delivery-attempts
> cap and is parked as poison (`journals.yaml:69`), **the lane keeps draining**, and an operator
> requeues it (`common/api.yaml:158,170,202`). Making them *stop* would turn a parked message into a
> **stopped order lane** — the platform's worst failure mode. **The principle: halt is right where
> there is no quarantine, and quarantine is better wherever it exists** — projections halt precisely
> *because* they have none, which is why quarantine stays their tracked follow-up. Actor workers still
> owe the pre-diagnostic half: poison data is reachable **only through the admin GraphQL API** today
> (**no `/mailbox`, no `MailboxStatus` — verified absent**), so the monitoring app cannot see a
> poisoned lane without admin auth. A `/mailbox` surface is owed, **report-only — it must not gate
> readiness**, because a poisoned message is a normal recoverable state, not an unhealthy pod.

> 🛑 **2026-08-11 — A REJECTED FOLD NOW HALTS ITS GROUP — AND THE FLIP CANNOT LAND ALONE, BECAUSE A
> HALTED PROJECTOR CURRENTLY REPORTS ITSELF HEALTHY**
> ([ADR-20260811-105024](adr/ADR-20260811-105024-projection-halt-default-and-health-visibility.md),
> [DECISIONS §27bis](proposals/DECISIONS.md) MET-G/MET-G2; docs-only).
> Product owner, verbatim: *"A. The projector has to stop and indicates it in the health. So k8s will
> detect it and we will be informed."* `DbFaultPolicy` flips **`Skip` → `Halt`** — the
> gate-then-stabilize default flip, the gated form having shipped inert in
> [#478](https://github.com/TheCaptainCompany/captain-food/pull/478). The team recommended building
> quarantine first and was **overruled**; recorded as a choice, not a concession — `Skip` leaves a read
> model permanently and *silently wrong*, which for a money- or authorization-bearing projection is
> worse than stuck.
> ⚠️ **Verified on `5fdc519`, and this is a precondition rather than a caveat**: under `Halt` the
> worker does **not** stop — the slice rolls back and the loop keeps ticking
> (`worker.rs:800-816,688-700`) — so `running` stays `true` (`:688`), so `/projector` returns
> **`200 OK`** (`server/src/lib.rs:1377-1392`); **and neither Kubernetes probe looks at projection
> status at all**, because projector bins probe `readinessProbe: /health` (the DB+schema gate) and
> `livenessProbe: /ping` (*"process is up; touches nothing"*)
> (`deploy/generated/manifests/bins/projector-ordering.yaml:102-111`). **Flipping today would produce a
> projector that wedges permanently and reports itself completely healthy on both probes** — turning a
> silent-wrong-answer failure into a silent-no-answer one. So the flip and the health surface land
> together.
> **The health design, settled in the ADR**: **halt stays PER-GROUP with the process alive** (already
> true by construction — process-level would turn one poisoned read model into a *scope-wide*
> projection outage, since `projector-ordering` hosts every ordering group); **READINESS, not
> liveness** — projector bins have **no `Service`**, so readiness is a **pure signal channel with no
> side effect** (visible to `kubectl`, Argo CD and `kube_pod_status_ready`), whereas liveness kills and
> restarts, a restart cannot fix a deterministic schema fault, and the resulting **CrashLoopBackOff
> stops every sibling group** — manufacturing exactly the outage the per-group shape prevents; re-point
> readiness to `/projector`; and the payload gains a **per-group** breakdown naming the halted group,
> position, `eventType` and error, because `ProjectionStatus` is per-worker today
> (`projection/mod.rs:13-28`) and structurally cannot say *which* group halted. **The signal does not
> exist**: `specs/observability.yaml` declares **no projection contract at all** (`:11`, prose only).
> ⚠️ **Known consequence accepted by flipping now (MET-G2) — the role-revocation wedge.**
> `ScopeMembership` is *"the single index every read-side authorization question resolves against, for
> every role and every surface"* (`projection_tables.yaml:801-810`) — **and it is a projection**. A
> halted group freezes read-side authorization: grants stop arriving and **revocations stop applying**,
> so a removed staff member or deactivated rider keeps access until a human clears the fault. That
> touches the *"explicit and immediate"* revocation guarantee of the §6.4 closure
> ([ADR-20260810-194548](adr/ADR-20260810-194548-six-decision-answer-sheet-claim-staleness-closed.md)).
> **Accepted, not solved** — under `Skip` the event is skipped and the index left permanently *wrong*,
> worse in kind for an authorization index. **Quarantine remains the real fix** and stays a tracked
> follow-up; until then a halted `ScopeMembership` is an **incident, not a ticket**.

> ✅ **2026-08-11 — THREE MORE DECISIONS SETTLED; ONE IS WITH LEGAL**
> ([DECISIONS §27bis](proposals/DECISIONS.md) MET-Q7 / COOP / MET-W / TRK-scope; docs-only).
> **MET-Q7 — approved as recommended: no hosted analytics SDK.** Ours, server-side. **Plus an addition
> that matters architecturally**: *"We will use a different database from the business database to
> isolate the activity."* Behavioural data lands in a **separate database from the business data**,
> which independently arrives at the legal lens's instruction and **confirms
> [PROP-20260811-000946](proposals/PROP-20260811-000946-behaviour-event-tracking-in-the-screens-spec.md)
> D5** — its own time-partitioned store, so erasure is a partition drop rather than an immutability
> problem. **One distinction not to conflate**: this is the *behaviour* store. **Business metrics stay a
> fold over `domain_events` in the `bam` schema**, because they are business data derived from business
> facts. **Implication to carry**: the C4 needs a **new container** for the behaviour database and its
> edges — and `specs/architecture/*.yaml` is **source DSL, not generated**, so that is an executor spec
> change when the work lands, not a regeneration.
> **COOP — approved as recommended**: all three cooperative properties are designed in **now**, in the
> first slice — the customer reads their own trail, the **restaurant** is the beneficiary of the
> aggregate, and the taxonomy refuses things checkably so it can be published
> ([#377](https://github.com/TheCaptainCompany/captain-food/issues/377)). They belong in slice 1 for the
> reason they were raised: each is a property of the **declaration mechanism**, so retrofitting them onto
> an undeclared firehose is a project while on a declared taxonomy it is a rendering.
> **MET-W — approved as recommended**: a **named catalog of approved retention windows**, sequenced
> **with** the erasure work ([#194](https://github.com/TheCaptainCompany/captain-food/issues/194)) rather
> than ahead of it.
> **TRK-scope — still OPEN, and it is with LEGAL, not with the product owner.** Their idea: *"using a
> generated identifier uncorrelated to the person… without the need to know the person is doing what
> but a persona"*, plus a clarification that **changes an earlier legal finding** — the "help AI agents"
> sentence was **internal**, explaining to the team why the data is wanted, **not** a user-facing
> personalisation feature. Legal is working out whether a pseudonymous journey identifier fits the
> **audience-measurement exemption** or whether per-journey continuity exceeds it. **The proposals are
> deliberately NOT amended until legal reports.** The mechanical half is being thought about but not
> committed: if the answer is *"lawful provided the join never happens"*, then **"never joined" has to
> be structural rather than promised** — the separate database (MET-Q7) does most of it, plus no foreign
> key, no shared column name the validator would accept, and an `identifierClass` that **cannot** be
> `CUSTOMER` for an anonymous-funnel event. Note this pulls against D8 option A, so the two are
> alternatives **per event kind**, not one answer.

> ✅ **2026-08-11 — THE REVERSAL IS CONFIRMED, AND THE SPEC GETS STRONGLY TYPED**
> ([ADR-20260811-014129](adr/ADR-20260811-014129-a-business-metric-is-a-projection-and-every-reference-is-a-ref.md),
> [DECISIONS §27bis](proposals/DECISIONS.md); docs-only).
> Product owner, verbatim: *"Confirm the reversal, go with the projections"* and *"But we need to
> heavily strongly typed the spec no string in it"*. MET-R closes.
> **ADR-20260810-234225 is SUPERSEDED IN PART, never rewritten** — clauses 1–3 (persona activity as
> the unit; declared + emitted + asserted; a metric states its question) are carried forward; clause 4
> (*"never entity ids"*) and the enforcement table (*"generated instruments"*) are reversed. The old
> file stays as the record of what was decided on 2026-08-10, including the reasoning that turned out
> to be wrong.
> **The second sentence is a separate decision and it landed on a real defect in the team's own
> grammar.** `increment: orders`, `groupBy: [day]` and `value: { sum: orders }` were **bare names
> pointing at declarations elsewhere in the same file** — so a typo was not a broken reference the
> loader could catch, it was a *silently wrong metric*: the exact failure class the whole proposal
> exists to remove, sitting inside the proposal. The product owner spotted it before the team did.
> It is now four categories: a **declaration** may introduce a name; a **reference** is a `$ref` the
> loader resolves (including same-file, which the repo already does at `specs/ordering/actors.yaml:102`);
> a **value from a closed set** stays a bare token *unless a domain scalar already declares that set*,
> where the `$ref` is mandatory; **prose stays prose**. The receipt that this is structural:
> [#413](https://github.com/TheCaptainCompany/captain-food/issues/413) — a plain-string `tombstone:`
> is *"silently invisible everywhere"*, including to the rule written for it.
> **The sharpest single fix**: `attributes: [{ values: [DELIVERY, COLLECTION] }]` in the tracking
> catalog was a **verbatim copy of the `ServiceType` kernel scalar** (`specs/common/scalars.yaml:260-262`)
> — now `{ $ref: 'scalars.yaml#/ServiceType' }`, so adding a third service type never leaves the
> tracking spec silently disagreeing with the domain.
> **And the `serviceType` problem dissolved: it was a GRAIN error, not a missing field.** Measured:
> **every one of the 11 `Order*` events carries `orderId`** (`OrderExpired` carries it and nothing
> else), so a projection at `grain: ENTITY` is **total over the whole lifecycle** — a cancellation is
> `set: status → CANCELLED` on the order's own row, and the grouping moves to read time. **The
> versioning story is withdrawn; no event needs a new field.** The rule earned its place twice:
> `fold-key-not-on-every-event` was written to catch a missing field, and what it actually catches is
> a wrong grain.
> ⚠️ **One dependency surfaced (MET-W)**: `retention: P90D` as a free duration string contradicts a
> recorded legal position — [the erasure brief:82](legal/BRIEF-20260808-account-erasure-two-path.md)
> says the retention windows are *"declared once, in the DSL, feeding both the sweep and the DPIA"*.
> No duration scalar exists. The fix is a declared retention-window catalog `$ref`'d by both, and it
> belongs to [#194](https://github.com/TheCaptainCompany/captain-food/issues/194) rather than to
> either metrics or tracking issue.
> **Not swept, deliberately**: the existing bare-name sites (`data_requirements:`/`actions_used:` 40,
> `roles:` 112) are each covered by a bespoke validator rule today. Their conversion is its own
> sequenced issue (MET-T2), not part of this.
> **A fork closed WITHOUT taking it (MET-F), with the numbers.** Product owner raised projection
> "state" as a JSON blob saved with the checkpoint, versus doing the fold in a generated SQL stored
> procedure. **① The state already exists and is already transactional**: measured in
> `crates/infrastructure/src/projection/worker.rs`, the projector holds **no fold state at all** —
> load → project → upsert per event, `drain_group` folding up to 500 events and writing
> `projection_checkpoint` **in the same transaction**. So *"loaded once and saved with the checkpoint
> transactionally"* is what it already does; there is no blob to build and **no memory risk** — an
> incomplete order is a row, and 100k of them is **12 MB**. The precedent for the JSON idea exists and
> was deliberately *not* JSON (process-manager runs are typed columns). **② The SQL option is already
> built and is the V0 default** — [ADR-0039](adr/0039-projection-views-generated-from-lineage.md)
> generates a `CREATE OR REPLACE VIEW` state-fold over `domain_events`; `OrderFacts` is the same shape
> as the shipped `View_DeliveryJob`. **③ The grammar is runtime-agnostic**; the one construct that
> binds a runtime is `alertable:`, and it binds at the tap, not the fold. **④ Measured** (200k events /
> 100k orders): set-based SQL **2.15 s** · plpgsql row-at-a-time **4.92 s** · Rust projector
> **≈65–70 s** — but only **2.3×** of the 30× gap is set-versus-row, the rest is round trips that
> [#267](https://github.com/TheCaptainCompany/captain-food/issues/267) attacks without leaving Rust.
> 70 s to rebuild every metric from 100k orders is **~500 days of Tours trading**; read-time grouping
> is **27 ms**. **⑤ The argument that survives any volume assumption**: testing a generated procedure
> means golden comparison against a Rust reference fold, so **SQL does not remove the Rust fold — it
> adds a second one. Recorded recommendation: hybrid, deferred** — a total `(state, event) -> state`
> vocabulary with **no host-language escape hatch** (what makes it both runtime-agnostic and
> replay-deterministic), emit Rust today, optional per-projection `emit: sql` only if a rebuild ever
> hurts. **⑥** The testability objection weakened the same day —
> [#478](https://github.com/TheCaptainCompany/captain-food/pull/478) made DB tests required by default;
> the real gap is that **no test loads `views.generated.sql` and asserts fold behaviour at all**.
> ⚠️ **Separate finding, not part of the fork (MET-G)**: the projector's per-event **log-and-skip** is
> correct for a read model and **wrong for a money-adjacent metric** — a skipped event leaves the count
> permanently wrong with only an ERROR log. Wants a projection-lag/parity check, and is adjacent to the
> `DbFaultPolicy` decision still open from [#474](https://github.com/TheCaptainCompany/captain-food/issues/474).
> **Follow-up answered, no design change (MET-S2).** Product owner: *"this kind of counter must be
> computed once the order is completed so a process manager can handle it."* **The first half is right
> and is already what the entity-grain design does** — the fold `set`s status, the metric asks
> `countRows where status equals DELIVERED`, so the count comes from the terminal event and nothing
> else; there is no increment to compensate. **But taken literally as a fold shape it does not work**:
> **no terminal event carries `serviceType`** (`OrderDelivered` = `[orderId, restaurantId]`), so
> completion-only hits the same wall — the entity grain is what solves it. It would also be **strictly
> weaker**: with no row until completion, *"which orders are placed and still unaccepted right now"*
> becomes unanswerable, and that is the platform's worst failure mode. The shape is **one projection
> read two ways**. ⚠️ **The process-manager half is the wrong tool and is refused on the record**: PMs
> here are state-table orchestrators in the actor mailbox with leases, fencing and head-of-line, so a
> counter there could **stall an order lane**, and a PM **is not replayable** — it carries a live state
> row and issues commands, so "rebuild the metric" would re-drive Stripe. Replayability is the one
> property the whole reversal chose projections for. **And no new event**: `OrderDelivered` already IS
> the completion fact for both service types; adding `serviceType` to it would denormalise the log so a
> projection need not do its job. **The instinct does brush a real gap though** — `OrderCompleted`,
> `Receipt` and `Invoice` are **zero hits across every `specs/*/events.yaml`**, and a compliant receipt
> is a French legal precondition. That is [#200](https://github.com/TheCaptainCompany/captain-food/issues/200)
> + legal work with its own decision, deliberately not folded in here.

> ♻️ **2026-08-11 — A BUSINESS METRIC IS A PROJECTION, NOT A COUNTER — THE TEAM CHANGED ITS OWN
> RECOMMENDATION, AND FILED THE REVERSAL RATHER THAN EXECUTING IT**
> ([#484 "26 of the 29 declared `business_metrics` emit nothing…"](https://github.com/TheCaptainCompany/captain-food/issues/484),
> [PROP-20260810-234225](proposals/PROP-20260810-234225-business-metrics-for-every-persona.md) D4/D6/D8/D9,
> [DECISIONS §27bis MET-R](proposals/DECISIONS.md); docs-only).
> The product owner held their own design back until the proposal existed, so the two would be
> independent: *"for the metrics I have in mind the approach of the projection… we will have to create
> a query in the graphql to allow access to these metrics."* **The team evaluated it and moved.** Not
> out of deference — the generated-instrument design recommended one day earlier loses on four
> measured points. **(1) It forfeits replay by construction**: `crates/infrastructure/tests/orders_placed_metric.rs:129`
> asserts the counter does **not** fire on a rebuild, so a metric added later would carry **zero
> history**, where a fold replays the whole log. The team's own audit standard — *"a `View_*` whose
> restore path is not replay is a finding"* — rejects the design the team wrote. **(2) Ratios and
> distinct-identity denominators are structurally inexpressible** as monotonic counters, so the
> counter design needs an escape hatch for the most interesting questions; under a fold they are
> ordinary and the plain counter becomes a one-line `value:`. **(3) It had diverged from the C4**,
> which already declares `bam` as a **projector** with a schema in read-models
> (`c4-l2.yaml:343,370,484`) — a schema with **zero tables** (`grep bam specs/database/` = 0).
> **(4) Erasure**: identity-bearing metrics are personal data either way, and in our Postgres they are
> inside the deletion engine's path instead of a vendor store with no per-subject deletion API.
> **The mechanical question is answered** (D8): a `projections:` block declaring `key` / `measures` /
> `fold` (`increment`/`decrement`/`add`/`subtract`/`set`/`max`/`min` per event), and a `metrics:` block
> declaring `over` / `groupBy` / `value` / `exposedAs` — every field reference a `$ref` into the
> **specific event**, so the validator proves the field exists there. **The rule that earns the whole
> shape fails on `main` today**: `serviceType` is on `OrderPlaced` and on **no other Order event**
> (`OrderExpired` carries `orderId` alone), so a projection keyed by it **cannot be decremented by a
> cancellation** — a counter design cannot even see that, and ships two numbers that quietly disagree.
> ⚠️ **Two clauses of [ADR-20260810-234225](adr/ADR-20260810-234225-business-metrics-for-every-feature-and-every-persona.md)
> are contradicted** (*"never entity ids"* — relaxed to *bounded declared population*, which is what
> makes `groupBy: [restaurantId]` and the restaurant-facing panel possible; and *"generated
> instruments"*). The ADR is `Accepted`, so this is a **decision reversal**: filed as MET-R, **not
> executed**, and the ADR will be **superseded, never rewritten**. Its principle is untouched.
> **Q7 (a hosted analytics SDK) is now recommended for CLOSURE as "no"** — the projection design kills
> its order-side motivation and the behaviour store kills its browse-side one.

> 🔍 **2026-08-11 — BEHAVIOUR EVENT TRACKING GETS A DECLARATION SITE — AND THE ARTICLE 9 EXPOSURE
> IS ALREADY IN THE SPEC, NOT IN THE FUTURE**
> ([#485 "Behaviour event tracking has no declaration site…"](https://github.com/TheCaptainCompany/captain-food/issues/485),
> [PROP-20260811-000946](proposals/PROP-20260811-000946-behaviour-event-tracking-in-the-screens-spec.md);
> docs-only, no code and no spec moved).
> Product-owner directive: *"We need to integrate the metrics in the spec. And integrate the behaviour
> event tracking inside the screens spec."* The first clause **endorses the §27 metrics work below**
> and changes none of it; this is the second.
> **The finding that shapes it is not the absence of tracking — that is expected. It is that
> special-category-adjacent data is ALREADY declared and ALREADY stored**:
> `SetCustomerPreferences.dietaryTags` is `array<Tag>`, `Tag` is a free-form `string` with
> `maxLength: 80` and **no enum**, persisted to `View_Customer.preferences` jsonb
> (`specs/customer/commands.yaml:179-182`, `specs/common/scalars.yaml:145-148`,
> `specs/database/tables/projection_tables.yaml:337`). **`halal` and `kosher` are spellable values
> today.** No screen binds it, so nothing is running — but no review caught it, because no artifact
> existed that would make anyone look.
> **Why the screens spec is the right location, and it is not aesthetic**: `specs/screens/**` is the
> **only** artifact in the repo that knows a `filter_bar` is an allergen filter — the api layer sees
> an argument, the store sees a string, an analytics SDK sees a payload. So it is the only place the
> rule *"this control may never be tracked"* can be written. **The window is open now and closes
> soon**: `allergen` has **zero occurrences in `specs/catalog/*.yaml`** while the model is
> decided-and-unbuilt ([#184](https://github.com/TheCaptainCompany/captain-food/issues/184),
> ADR-20260808-171056), so the refusal can be built **before** the control exists.
> **Shape**: a root `specs/behaviour_events.yaml` (legal fields — `purpose`, `lawfulBasis`,
> `retention`, `identifierClass`, `specialCategoryRisk`, `dpia` — required, no defaults) bound by a
> `tracking:` `$ref` on screen/action nodes; `kind:` is `VIEW | INTERACTION` and **`IMPRESSION` and
> session replay are absent from the grammar, not discouraged in a comment**; records go to their
> **own time-partitioned store**, never `domain_events` (a behaviour event is not a decided fact, so
> the left-fold invariant would stop holding) and never the order path's instance
> ([#443](https://github.com/TheCaptainCompany/captain-food/issues/443)); ten ERROR rules, of which
> **R10 makes the emitter produce nothing while no DPIA exists** — the build gate that turns
> "sequenced behind [#194](https://github.com/TheCaptainCompany/captain-food/issues/194)" from a
> promise into a failure.
> **First slice is the mechanism with ZERO live events**: instrumentation before a DPIA is processing
> that should not have started. Register: [DECISIONS §28](proposals/DECISIONS.md) — D1–D7 team-owned;
> **Q1 (client storage, and therefore whether a consent banner exists at all — note `X-SESSION-ID`
> already exists, `crates/server/src/graphql/session.rs:1-15`) and Q2 (does the restaurant see its own
> storefront's behaviour data) are product-owner-owed.** Every legal claim is **VERIFY-FIRST**; no
> licensed-counsel review has taken place.
> **Independent convergence on the write path** (D10): the product owner's own design for this half —
> *"name the interaction and the properties… the principal context will be sent with the jwt. A
> mutation should be exposed to send these events"* — matches the proposal on the name and properties,
> and the **JWT clause is D8 option A reached from the other direction**. It is also ADR-0041's
> envelope doctrine applied to a non-domain write without being asked. ⚠️ **One measured blocker**:
> `op-missing-command` is an **ERROR** and all **86** mutations bind a command handled by an actor
> (`tools/codegen-rs/src/validate/core.rs:292,295,301`), so a mutation today **cannot** be a
> non-command — declaring `recordBehaviourEvent` the only way the validator accepts would enqueue it
> on the actor mailbox and append it to `domain_events`, **silently, with the gate green**. The fix is
> a small api.yaml shape: a mutation declaring **`sink:`** where a command declares `command:` — *this
> write is recorded, not decided*. It must land before this half is buildable.

> 📏 **2026-08-11 — BUSINESS METRICS BECOME A DECLARED, GATED OBLIGATION — AND 26 OF THE 29 WE
> ALREADY DECLARE EMIT NOTHING**
> ([#484 "26 of the 29 declared `business_metrics` emit nothing…"](https://github.com/TheCaptainCompany/captain-food/issues/484),
> [ADR-20260810-234225](adr/ADR-20260810-234225-business-metrics-for-every-feature-and-every-persona.md),
> [PROP-20260810-234225](proposals/PROP-20260810-234225-business-metrics-for-every-persona.md);
> docs-only, no code moved).
> Product-owner directive (Jeff Patton): *"we must have business metrics for all features for each
> persona … must be developed with the test and the code."* Auditing the slot that already exists
> found it almost empty: **`specs/observability.yaml` declares 29 `business_metrics` across 14
> contracts, and 26 have ZERO occurrences in `crates/`, `tools/` or `deploy/`** — no constant, no
> instrument, no call site. Exactly three are emitted (`orders_placed_total`,
> `checkout_payment_failures_total`, `scope_membership_lag_positions`). The gate that should have
> caught it (`tools/codegen-rs/src/tests.rs:1500`) covers **3 of 14 contracts** by a hardcoded
> allowlist and asserts only that the metric NAME exists as a string constant — two of those three
> contracts declare no business metrics at all, so its effective coverage is **2 of 29**.
> **The recorded principle**: the unit is the persona **ACTIVITY** (8 personas, 25 activities), not
> the story step (144 — two of which `$ref` the same query and one of which is a poll loop); a
> metric declares the **question** it answers; attributes are bounded sets, never entity ids.
> **Declaration is enforced like ADR-0032 and emission is not** — `make validate` cannot see a call
> site — so the chain is validator (coverage) → **generated instruments** (names, attribute types,
> arity; deletes the scanner's metric half) → **`InMemoryMetricExporter` behaviour test** (it fires,
> once, not on a replay). No source-text scanner is added.
> ↑ ⚠️ **HISTORICAL — the two emphasised clauses in this paragraph were REVERSED the next day.** See
> the 2026-08-11 "the reversal is confirmed" entry above: a business metric is a **projection**, not a
> generated instrument, and grouping keys need a bounded *population* rather than being barred from
> entity ids. This entry is left as written because STATUS is a chronological record.
> **Sequencing**: gate forward now with an enumerated, monotone-shrinking `unmeasured:` waiver list,
> backfill in value-stream order — a one-sweep backfill was already run at this scale and the 26 dead
> declarations are its receipt. Register: [DECISIONS §27](proposals/DECISIONS.md) (D1–D7 team-owned,
> **Q7 product-owner-owed**); §22's *"Business-signal observability contracts"* row closed by
> subsumption. The per-persona metric GRID is the `ux-designer` lens's parallel deliverable, not this.

> ✅ **2026-08-10 — THE LOCAL TEST GATE IS HONEST: `make test-crates` RUNS FROM THE STOP HOOK, AND A
> MISSING DATABASE NOW FAILS**
> ([#474 "`make rust` runs no workspace tests at all, and DB-gated tests skip silently — \"local gates green\" is a false signal"](https://github.com/TheCaptainCompany/captain-food/issues/474),
> branch `474-honest-test-gate`, mob protocol).
> **The hole**: `make rust` = `rust-build rust-test validate check-drift`, and `rust-test` is the
> **codegen crate alone** — the documented pre-push gate never ran a line of `crates/**`. #451's
> migration defect passed `cargo check`, six hand-run suites and three green `make rust` rounds.
> **Now**: `make test-crates` (`cargo test --workspace --no-fail-fast`) is invoked by
> `.claude/hooks/stop-gate.sh` whenever the turn's diff touches `migrations/ | crates/ | the
> emitters | Cargo.{toml,lock}` — scope decides whether the DB half is MANDATORY, never whether it
> silently vanishes. **Polarity inverted** (`crates/db_test_gate`, new dev-only crate): a database is
> REQUIRED by default, a missing `DATABASE_URL` PANICS with the command to fix it, and the only way
> out is `DB_TESTS_REQUIRED=0`, which leaves a receipt `make test-crates` reads back into a summary
> naming **every** skipped suite — count it with
> `cut -f1 target/db-test-skips.log | sort -u | wc -l` rather than trusting a number in prose. The
> receipt exists because **libtest swallows a passing test's stderr**: `grep -c SKIP` over the
> 990-test baseline log returns **0**, so the old per-suite SKIP lines were not merely quiet, they
> were unobservable. The decision was hand-written at 17 call sites across 5 crates and now lives in
> one place, guarded by a codegen rule that also rejects the PRE-#474 shape
> (`std::env::var("DATABASE_URL")` under `crates/**/tests/**`, which never mentions the opt-out
> variable and so slipped past the polarity scan); `actor_runtime` keeps one local copy because
> `dependency_rule.rs` forbids ANY path dependency into the workspace (ADR-20260730-234918), and the
> allowlist names each file with its reason. That copy **also writes the receipt** — until it did,
> its five DB-gated binaries skipped without appearing in the summary, so the line named fewer
> suites than had actually skipped.
> **Two new gates, both seen RED against a deliberately re-planted #451**: the checkpoint no longer
> advances past a fold the DATABASE rejected (`FoldFault::{PayloadShape,Database}` — a compiler-
> enforced classification the loop never had, since every failure used to collapse to
> `DomainError::Repository`; there is deliberately no `From<DomainError>`, so `?` cannot pick a class
> and a row key that will not resolve is `PayloadShape`, which keeps one unparseable stream name from
> wedging its group), **shipped GATED**: `DbFaultPolicy::Skip` remains the default and today's
> behaviour is unchanged on every deployed path — flipping it is a separate decision
> ([ADR-20260810-225036](adr/ADR-20260810-225036-projection-db-fault-policy-gated-halt.md)); and
> validator §16 `schema-writer-missing-column` proves, with no database and in under a second, that
> every `NOT NULL`-without-`DEFAULT` column appears in its writer's insert list. Measured red set on
> the real repo: **exactly the two planted columns**, no pre-existing violations anywhere on the
> projection surface. Gates: `make rust` green, `make validate` **0 errors / 37 warnings, warning
> profile byte-identical to `origin/main`** (CLAUDE.md's pinned 43 was stale; re-measured on `main`
> at `d7087fb` and repinned to 37 when `main` was merged into this branch — re-measure, as it says).

> ✅ **2026-08-11 — #469: THE OPEN PATH READS CREDENTIALS, AND `current` IS TENANT-SCOPED BY HOST**
> ([#469 "`current` leg 1 is dead on the web AND is not tenant-scoped — fix both together or neither"](https://github.com/TheCaptainCompany/captain-food/issues/469),
> branch `469-auth-leg-and-tenant-scope`, PR [#488](https://github.com/TheCaptainCompany/captain-food/pull/488),
> [ADR-20260811-113000](adr/ADR-20260811-113000-the-open-path-reads-credentials-and-current-is-tenant-scoped-by-host.md)).
> Both halves land together because either alone is worse than neither: the auth half on its own
> ships a live cross-tenant cart.
>
> **Half 1 — `/public` is no longer credential-blind.** It reads the `captain_auth` cookie/bearer and
> verifies it, and it is the ONE path that DEGRADES instead of refusing: absent, expired, tampered,
> JWKS-unreachable and non-CUSTOMER credentials all serve `200` anonymous (a stale cookie is the
> common case; `/public` worked with no JWKS at all before and still must). Each degrade is counted —
> `public_credential_degraded_total{reason}` — and the JWKS fetch is now bounded at 3 s, because key
> refresh has moved onto the storefront's critical path — and the refresh itself is **single-flight
> with a negative cache** (N concurrent requests at the TTL boundary cost ONE fetch; a failed fetch
> silences retries for 10 s; an attacker-supplied unknown `kid` can drive a refetch at most once per
> 5 s), because a Supabase blip at Friday 19:00 would otherwise tax every storefront request 3 s.
> **It grants at most the CUSTOMER identity**: a verified ADMIN/RESTAURANT/RIDER token there stays
> anonymous, enforced **by the type**: `Principal` holds ONE private `Identity` enum whose role is
> DERIVED from the identity, so "role says CUSTOMER, claim absent" is not a field combination anyone
> can spell — it is the named `Identity::Unbound`, which `/public` cannot reach. (Round 2 of review
> corrected an overstatement here: the previous `pub`-fields struct made that state a legal literal
> inside AND outside the crate, so the guarantee lived in a doc comment, not in the compiler.)
>
> **Half 2 — the tenant is a request datum.** `Host` → `{slug}` → `RestaurantId` resolved ONCE at the
> GraphQL edge (POST and WebSocket) and injected beside `ReadScope`, never folded into it; `current`
> stays ZERO-ARGUMENT (an argument would let a client assert the tenant) and both legs are bounded by
> the tenant **in SQL**, through two port methods whose signatures make it non-optional. A host that
> names no restaurant serves `null`, never "the newest cart anywhere"; `carts` remains the
> across-restaurants query. `graphql_routes` now TAKES the tenant lookup, so mounting the surface
> without one does not compile.
>
> **The test that could not previously exist.** Every cart test injected `ReadScope` by hand, which is
> exactly how a dead auth leg survived a green suite. `tests/graphql_cart_read.rs` now drives a real
> `POST /public/graphql` — signed cookie, `Host`, loopback JWKS — through the production router and
> asserts the PRICED payload per host. **Standing rule: a test of an auth-derived value may not
> `.data()` that value.** Each half was mutation-tested separately (restore `Principal::anonymous()`
> ⇒ the auth test reds with `null`; drop the host filter ⇒ the tenant tests red showing restaurant
> B's cart and total on A's storefront; neutralise the SQL predicate ⇒ the DB test reds with 2 rows
> where 1 is expected).
>
> **Blast radius, named** (ADR §Consequences): on `/public` a signed-in customer now also reaches
> `paymentStatus` ownership by claim, matches `operationStatus`/`operationStatusChanged` ownership by
> their own `sub` — **only once claim-stamped**: those two read `user_id` directly, so for the
> pre-claim window ownership rests solely on `X-SESSION-ID`, exactly as on `main` — and the open
> mutations' journal/`domain_events` envelope stamps `user_id`/`user_type = CUSTOMER` instead of
> `PUBLIC`. SSR stays anonymous ON PURPOSE (identity there would emit personalised HTML with no
> `Cache-Control`). `/public` GraphQL responses now vary by cookie, so the whole GraphQL surface
> answers `Cache-Control: private, no-store` — one response layer, not per-handler, so a new route
> cannot forget it; serving one customer's cart to another out of a shared cache would be an
> Art. 32(1)(b) confidentiality failure, and "nothing fronts POSTs with a cache" is an assumption
> about deployments we have not made yet, not a technical measure. **That guarantee holds for the
> MONOLITH only**: the gateway rebuilds each subgraph response from status + `content-type` + body
> alone (`crates/gateway_runtime/src/lib.rs:268-285`) and sets none on its own error paths
> (`:244-255`, `:292-301`), so once the #358 cutover makes the gateway the browser-facing
> `/public/graphql` the header is stripped exactly where a shared cache would sit — propagating it
> there is a **cutover precondition** (recorded in the ADR beside the tenant-host one). Exposure
> today is zero: the monolith is the deployed runtime and nothing fronts it with a cache.
>
> **Three things independent review added, all landed here.** (1) A verified CUSTOMER token with no
> `captain_customer_id` — the pre-claim-stamp window, i.e. EVERY signed-in customer for one token
> lifetime after rollout — now degrades to anonymous and is counted `public_credential_degraded_total{reason=claim_absent}`,
> instead of falling through to `read_authorization_bridge_unresolved_total`, whose contract says
> *"never ordinary user denial"*: a normal rollout would otherwise have bumped a provisioning-gap
> counter on every storefront GraphQL request and read to an operator as an incident. **Both branches
> are now PROVED emitted** (`crates/server/tests/public_credential_degraded_metric.rs`): the same
> claimless token bumps `claim_absent` on `/public` while leaving the bridge counter silent, and bumps
> the bridge counter on `/customer` — so the "stays zero" half is an observation, not a metric name
> nobody checked. `read-authorization` also joined the codegen guard's contract list, so a rename of
> either counter now fails the build. (2) The envelope widening reaches the **mailbox handler**
> (`resolve_actor` branches on `user_type == "CUSTOMER"` ALONE — so a claim-stamped customer with a
> lagging projection takes the branch too): one extra `by_auth_ref` read per delivery on the cart
> mutations at peak. A lagging projection returns `Ok(None)`, not `Err`, so it does NOT abort the
> delivery; only a genuine read-model failure does. Outcomes are unaffected either way — the single
> `domain_id` consumer is unreachable from `/public`. (3) The stored identity now puts an **external
> IdP identifier** (the Supabase `sub`) into the immutable write envelope of `Cart-*`, `Customer-*`
> and `Restaurant-*`, where it **survives deletion of the Supabase identity** — and those streams have
> no erasure path (only `Order` declares one; the deletion engine is stream-keyed). They were NOT
> "made subject-attributable" — `CartStarted` already requires `sessionId` and `CustomerRegistered`
> already requires `phone`; what is new is narrower and different in kind. The production log is empty
> by decision, so this is an unmet launch precondition already filed as
> [#194](https://github.com/TheCaptainCompany/captain-food/issues/194), not a pre-existing breach.
>
> **Round 2 of review also landed**: the `Identity` reshape above; the JWKS single-flight + negative
> cache; `Cache-Control: private, no-store` across the GraphQL surface; and three recorded
> consequences the code cannot enforce — the `captain_auth` cookie is **host-only**, so identity is
> per-storefront (cross-storefront identity is an open authn-scope decision, not taken here);
> `X-Forwarded-Host` is now an authorization input, so the ingress must OVERWRITE it rather than
> append; and in the #358 surface-bin topology the SSR transport drops `Host`, which would resolve
> every tenant-scoped read to `TenantScope::None`.

> 🚧 **2026-08-10 — #451 PHASE 2 LANDED (code): THE CART IS PRICED LIVE ON READ — BUT THE CUSTOMER
> STILL CANNOT SEE IT**
> ([#451 "cart.current returns the authenticated customer's priced cart"](https://github.com/TheCaptainCompany/captain-food/issues/451),
> branch `claude/epic-429-production-test-order-9atwb8`, PR [#460](https://github.com/TheCaptainCompany/captain-food/pull/460), mob protocol).
> **What now works, server-side.** `make generate` wired the three cart resolvers: `current` resolves
> TWO-LEG (claim, then `X-SESSION-ID` with `customer_id IS NULL OR = claim`), the by-id `cart`
> enforces claim-ownership in the BODY — retiring the live IDOR, the dispatch's hard DONE-WHEN — and
> `carts` prices each row from its restaurant's live catalog. All three go through the ONE
> `price_cart` seam (`crates/server/src/graphql/cart_read.rs`) over a one-read memoized catalog
> snapshot. The generated `From<(CartRow, RestaurantRow)> for Cart` — which could only fabricate the
> 0,00 EUR payable — is DELETED, so the fabrication is now unspellable rather than merely unused.
> `by_customer` is OPEN-only + `LIMIT 50` in SQL (a CHECKED_OUT cart's money was frozen at intent;
> repricing it is a receipt-adjacent lie, and one stale line used to error the customer's whole cart
> list). `open_by_session` lost its `Ok(vec![])` trait default, so a fake that forgets it now fails
> the build instead of silently emptying the entire anonymous path. Telemetry: `cart.price` declares
> `otel.status_code` and records ERROR on the unresolvable branch (without it the contract's
> `technical_error: any_span_errors` could never fire and every failure exported as a SUCCESS), the
> empty-cart read emits its span + histogram like any other success, and one `RequestCorrelationId`
> is minted per request and shared by every read-path span.
>
> **What does NOT work — the customer still sees no total.** Two independent reasons, both filed:
> the cart screen's summary bindings name `cart.subtotal|deliveryFee|serviceFee|total` while the API
> exposes `totalAmount` + `breakdown.{...}`, so the screen cannot render a price at all
> ([#468 "The cart screen cannot render a price: every summary binding names a field the API does not have"](https://github.com/TheCaptainCompany/captain-food/issues/468));
> and leg 1 cannot fire on the web client, because the public path never reads `captain_auth`
> ([#469 "`current` leg 1 is dead on the web AND is not tenant-scoped — fix both together or neither"](https://github.com/TheCaptainCompany/captain-food/issues/469)).
> The CHECKOUT shell's `cart_summary_mini` is a different block and does render a live total (proven
> by `web::router::tests::the_checkout_shell_carries_the_cart_it_is_about_to_charge_for`). Also open:
> [#470 "Contract migration: drop the four Cart money columns once the money-free binary is stable"](https://github.com/TheCaptainCompany/captain-food/issues/470)
> (this change ships the EXPAND half only — the columns stay so a failed deploy on the single free-tier
> instance can roll back to a binary that still selects them),
> [#471 "Extend the observability test suite to the `cart-price` contract (span status, empty-cart span, unresolvable counter)"](https://github.com/TheCaptainCompany/captain-food/issues/471)
> (the durable pin for the metrics; a bespoke spy binary was deliberately NOT built),
> [#472 "A dead control stays live: the SDUI renderer evaluates no `visible_when`/`disabled_when` and swallows resolver errors"](https://github.com/TheCaptainCompany/captain-food/issues/472),
> [#473 "Rewinding a projection checkpoint stalls the GDPR deletion engine's scan bound"](https://github.com/TheCaptainCompany/captain-food/issues/473),
> plus [#465](https://github.com/TheCaptainCompany/captain-food/issues/465) (the CartLocked lifecycle)
> and [#466](https://github.com/TheCaptainCompany/captain-food/issues/466) (the screen-roles ⊆
> resolver-roles gate hole). Three open product-owner decisions ride in
> [DECISIONS.md](proposals/DECISIONS.md) as rows **451-A** (the cart-screen bindings), **451-B** (the
> `currency_mismatch` reason folded into `offer_gone`) and **451-C** (whether #451 keeps its now-stale
> title). The prod smoke's L4 asserts the priced guest cart through `current` + `X-SESSION-ID` and
> gates on the server's self-reported `requiredSchemaVersion`, so it **fails loudly against a
> pre-#451 deployment** — deploy first, then smoke.

> 🚧 **2026-08-10 — #451 PHASE 1 LANDED: THE AMBER SPEC SLICE OF THE CART-PRICING KEYSTONE**
> ([#451 "cart.current returns the authenticated customer's priced cart"](https://github.com/TheCaptainCompany/captain-food/issues/451),
> realizing [PROP-20260810-231500](proposals/PROP-20260810-231500-cart-current-priced.md) Option B /
> LIVE, recorded in [ADR-20260810-112836 "Cart priced LIVE on read"](adr/ADR-20260810-112836-cart-priced-live-on-read.md);
> branch `claude/epic-429-production-test-order-9atwb8`, mob protocol). **Spec truth now says LIVE**:
> the `Cart` projection is a money-free pure fold (`projection_tables.yaml` money columns dropped,
> `[customer_id, updated_at]` index added, migration `20260810113000_cart_money_free_fold.sql` +
> schema-version bump); the zero-arg claim-resolved `current` query exists (`specs/ordering/api.yaml`
> + `ViewCurrentCart` story step + the storefront SDUI `cart.current` resolver repointed); the
> by-id `cart` query's live IDOR is retired at spec level (`roles: [CUSTOMER, ADMIN]`,
> claim-ownership documented); the read-side pricing contract `cart-price` is in
> `specs/observability.yaml` (`cart_price_ms`, `cart_price_unresolvable_total{reason}`); the
> impure-fold wording is corrected everywhere (ADR-0028 §5 addendum, rules, entities/events
> comments). **What does NOT yet work**: the `current` resolver is the generated
> `not implemented` stub, the generated Cart→API mapping fills the degenerate unpriced shape
> (empty lines, 0 EUR — exactly what the pre-#451 stub rendered), and the projector still folds no
> lines. **Phase 2 (GREEN)** wires `price_cart` at the resolver seam, the line fold, the
> claim-ownership narrowing in the `cart`/`current` bodies, and proves the `cart-price` metrics
> firing. Phase 1 passed the fold-purity checkpoint (architect, 4/4 judgment calls sanctioned). Three
> product-owner facts then corrected the design — carts are session-keyed BEFORE identification and
> bound by `CartBindingProcess`, the cart is saved at intent as the CheckoutSnapshot, and the
> intended cart LOCK is not modelled at all ([#465](https://github.com/TheCaptainCompany/captain-food/issues/465)).
> `cart.current` is therefore TWO-LEG (claim, then session id with `customer_id IS NULL OR = claim`)
> and `[PUBLIC, CUSTOMER]` — committed as `e9704a0`, which also repaired an anonymous-cart-read
> break Phase 1 had introduced (gate hole filed as
> [#466](https://github.com/TheCaptainCompany/captain-food/issues/466)). Phase 2 followed in the same
> branch — see the entry above for what actually landed.

> ✅ **2026-08-10 — STRIPE PUBLISHABLE KEY BAKED: the #440 env-var-only follow-up is closed**
> ([#448 "Bake the Stripe TEST publishable key as a literal deploy value"](https://github.com/TheCaptainCompany/captain-food/issues/448),
> spec-only, straight to `main`). The product owner supplied the authoritative `pk_test_…` value
> (2026-08-10) and it is now a literal `deploy:` block on `STRIPE_PUBLISHABLE_KEY` in
> `specs/payments/configuration.yaml` (production + staging, TEST mode for both — matching
> STRIPE_SECRET_KEY's reality; the SUPABASE_PUBLISHABLE_KEY baked-non-secret posture, no
> `from_github_secret`). Regeneration compiles it into the per-profile `BAKED` tables of the
> generated configs (`crates/server/src/generated/config.rs` + the payments-scope consumer bins) —
> baked non-secrets ship IN the binary, not via `render-config-sync.json` (that rail syncs
> `from_secret` names only; the Supabase baked literals follow the same shape). **Remaining
> hygiene click**: the
> `STRIPE_PUBLISHABLE_KEY` env var on the Render service is now REDUNDANT and shadows the baked
> value (env > baked; the sync never deletes) — it must be deleted from the dashboard after the
> next deploy, a product-owner action recorded as the deploy-day fact. The go-live constraint is
> unchanged: the `pk_live_` swap lands with `STRIPE_SECRET_KEY_PROD` (issue #254).

> ✅ **2026-08-10 — CART-PRICING KEYSTONE APPROVED (Option B / LIVE); BUILD STARTING**
> ([PROP-20260810-231500 "cart.current: the authenticated customer's PRICED cart"](proposals/PROP-20260810-231500-cart-current-priced.md),
> tracking [#451 "cart.current returns the authenticated customer's priced cart"](https://github.com/TheCaptainCompany/captain-food/issues/451),
> epic [#429 "Production with test data"](https://github.com/TheCaptainCompany/captain-food/issues/429)).
> **Decision (product owner, 2026-08-10)**: DECISION 1 = **Option B — LIVE**. `cart.current` is priced
> fresh on every read via the existing `application::pricing::price_cart`; the `Cart` projection stays
> a **money-free fold** (drops the impure-fold price columns). DECISION 2 sub-defaults stand:
> claim-resolved **zero-arg** `cart.current` (reuses #434 `ReadScope::Customer`), and "current" = the
> **most-recently-updated OPEN cart**. This settles [DECISIONS.md §1 row G](proposals/DECISIONS.md)
> (register 8 → 7 open) and fills the two #429 blockers "the cart total never computes" +
> "/checkout carries no route params". **The one Concern — a read-side pricing observability contract
> in `specs/observability.yaml` — is NOT a PO gate; it is folded into the #451 build chunk as DoD.**
> The keystone has an AMBER (spec) half and a GREEN (code) half; the spec changes are plan-mode with
> approval. **Consumer-mediator registration DEFERRED to first real order** per the PO (against the
> team's "start now" recommendation). **Solida rebrand still PENDING** — class-42 unresolved and **no
> entity name chosen yet**, which also gates the entity-path/rebrand work; [#411](https://github.com/TheCaptainCompany/captain-food/issues/411) stays blocked.

> 🚧 **2026-08-10 — `orders_placed_total{status="PLACED"}` EMIT WIRED ON THE PM-MAILBOX PATH —
> ARMS WITH THE `PM_MAILBOX_DELIVERY` FLIP, DOES NOT FIRE IN THE CURRENT DEFAULT POSTURE**
> ([#456 "Emit orders_placed_total so the un-told-order alarm can fire"](https://github.com/TheCaptainCompany/captain-food/issues/456),
> PR [#457](https://github.com/TheCaptainCompany/captain-food/pull/457), epic
> [#429](https://github.com/TheCaptainCompany/captain-food/issues/429), mob protocol).
> **The gap**: the counter `ORDERS_PLACED_TOTAL` and its emitter `telemetry::meters::place_order::placed`
> existed since #191, but had **ZERO call sites** — the success side of the place-order BAM contract
> (`specs/observability.yaml`) was declared and never wired, so no alert on "orders placed" could ever
> trip. **The wiring (counter only)**: one emit at the mailbox handler seam
> (`crates/infrastructure/src/mailbox/handler.rs:612`, in the `Outcome::Completed` arm AFTER
> `flush_staged_in_tx` succeeds), keyed on a pure predicate `staged_contains_order_placed(&[StagedAppend])`
> — emit IFF this delivery's staged appends carry a `DomainEvent::OrderPlaced`. **HONEST POSTURE — the
> emit does NOT fire by default yet**: `record_order_placements` is called ONLY on the PM-mailbox
> delivery path (`handler.rs`), which runs only when the `PM_MAILBOX_DELIVERY` runtime posture is ON.
> That posture is **seeded FALSE** (`specs/database/tables/referential.yaml:111`; `RuntimePosture` DB
> row, #318/ADR-20260803-104819) and its default flip **stays gated pending staging smoke** (see the
> #275 D1 entry below, ~line 1256). With it OFF, the **legacy tick runner** processes
> `PaymentCaptured` (`runner.rs` `dispatch` → `place_order::on_payment_captured` appends `OrderPlaced`
> directly) and its completion arm (`runner.rs` `Ok(Outcome::Completed) => {}`) emits **nothing** — so
> a real placement in today's default posture increments **no** counter. Therefore
> `orders_placed_total` **ARMS with the flip**, it does not fire now. This is **deliberately
> gate-then-stabilize-consistent**: the emit lives with the surviving seam it belongs to (the mailbox
> — final-vision-first; the legacy runner is being retired, not instrumented), and the counter goes
> live as a consequence of the separately-recorded `PM_MAILBOX_DELIVERY` default-flip decision, not as
> a second hidden toggle. **Until that flip, the "a stranger paid us" alarm on this counter cannot
> trip** — the un-told-order safety signal is not yet armed in production. **Why the staged set, not
> the outcome**: `OrderPlaced` is appended only when the place-order guard `should_deliver_order_placed`
> (= `domain::order::fold(stream).is_none()`) is true; a re-delivery or partial-reaction replay finds
> it false, stages nothing, and the predicate stays false — so the staged set IS the guard's output
> transitively. Keying on `Outcome::Completed` (returned even on replays that append nothing) would
> double-count a monotonic counter into a permanent lie — proved by a planted-red spy reading
> `("PLACED", 4)` vs the correct `("PLACED", 1)` over four delivery shapes. **Replay-safe,
> durable-first**: the count moves only once the append is in the completion transaction. **SDK stays
> at the infra boundary** (c4-l3 `instrumented`); domain/application untouched. **Tests**: a
> pure-predicate unit test (present/absent, no DB) plus a metric-spy binary
> `crates/infrastructure/tests/orders_placed_metric.rs` — its OWN binary because `telemetry::meters`
> binds the process meter once via `OnceLock` (the shared `main` integration binary cannot host a spy;
> same reason as `checkout_degraded_metric.rs`), no DB (the emit is pure over the staged Vec; the
> guard's replay staging is proved against real Postgres by `tests/main/pm_prepare_delivery.rs`).
> **DEFERRED** (recorded, not built): the `Outcome{placed:bool}` flag refactor (a larger
> equivalent-correctness change) and the place-order success-status SPAN CHAIN (coupled to the RED
> pricing keystone [#451](https://github.com/TheCaptainCompany/captain-food/issues/451)). No new
> status values beyond `PLACED` (the contract's `status` label is unbounded; `PLACED` is the success
> value). No PENDING/enqueue-path emission. **NAMED RESIDUAL GAP**: if the `PM_MAILBOX_DELIVERY` flip
> is deferred long-term, the legacy runner's completion arm carries no `orders_placed_total` emit and
> the alarm stays disarmed — the mob's final-vision call was to NOT instrument the retiring runner, so
> arming the alarm is bound to the flip landing.

> ✅ **2026-08-10 — SECRET-GATE EXTRACTED TO ITS OWN LEAN CRATE: the deploy-path cold-compile tail
> risk is gone** ([#453 "Extract secret-gate to a lean crate (fix #444 deploy-path cold-compile tail risk)"](https://github.com/TheCaptainCompany/captain-food/issues/453),
> PR [#454](https://github.com/TheCaptainCompany/captain-food/pull/454), epic
> [#429](https://github.com/TheCaptainCompany/captain-food/issues/429), mob protocol).
> **The regression**: #444 wired `cargo build ... --bin secret-gate` as the FIRST step of
> `deploy.yml`, but the gate lived in `tools/codegen-rs`, so that build dragged the
> guppy/determinator/regex/sha2/camino/serde_yaml tree — a COLD compile of MINUTES on a cache miss,
> inside a `timeout-minutes: 10` job that an incident rollback also runs. **The fix**: a pure move
> (no logic change) to a new top-level workspace member `tools/secret-gate` depending on
> serde/serde_json + std ONLY; `compare_secrets` stays the one unit-tested source of truth, the 6
> unit + 2 `CARGO_BIN_EXE` integration tests move with it and stay green, and the bin name
> `secret-gate` is preserved verbatim (deploy.yml invocation + test env-var key on it). `deploy.yml`
> now builds `cargo build -p captain-food-secret-gate`; the binary still lands at
> `./target/debug/secret-gate` so the invocation line is unchanged. **Before/after**: OLD path
> cold-compiled the codegen-rs guppy/determinator tree (minutes); NEW `cargo build -p
> captain-food-secret-gate` cold = ~7s — the durable verdict is `cargo tree -p
> captain-food-secret-gate` = serde/serde_json + their tiny direct deps ONLY (no guppy/determinator/
> cargo-metadata/regex/camino/sha2), NOT a warm-cache wall-clock. **`timeout-minutes: 10` left
> unchanged** (farley's belt call): the budget is now comfortably sufficient and a bump would signal
> a fragility that no longer exists. Process lesson recorded in
> [docs/claude/sessions.md §18](claude/sessions.md): a mob briefing for a CI-workflow change must ask
> whether the step fits the job's existing timeout and whether it regresses the rollback path — #444
> asked neither and the review caught it post-merge.

> ✅ **2026-08-10 — PRE-DEPLOY SECRET-PRESENCE GATE: a declared secret missing/mis-named in the
> deploy target now FAILS the deploy before Render is told to pull**
> ([#444 "CI gate: declared secrets must exist as repo secrets before deploy"](https://github.com/TheCaptainCompany/captain-food/issues/444),
> PR [#450](https://github.com/TheCaptainCompany/captain-food/pull/450), epic
> [#429](https://github.com/TheCaptainCompany/captain-food/issues/429), mob protocol). New binary
> `secret-gate` (`tools/secret-gate/src/main.rs` since #453; originally
> `tools/codegen-rs/src/secret_gate/main.rs`): a PURE comparison
> `compare_secrets(declared, present)` of the repo secrets the configuration DSL DECLARES as
> deployed-key sources — `deploy/generated/secret-keys.json` `from_github_secret` names, itself a
> deterministic fold of `specs/**/configuration.yaml` (the **superset-by-construction** declared
> source; farley-verified) — against the ones the reachable deploy target holds. Declared-but-absent
> **or present-but-empty** (never-written-empty doctrine) ⇒ FATAL, names each repo secret;
> present-but-undeclared ⇒ NON-FATAL `::warning::` (`RENDER_API_KEY`/`GITHUB_TOKEN`/
> `RENDER_DEPLOY_HOOK_URL`/`MKS_KUBECONFIG` are legitimately undeclared). Wired into `deploy.yml`
> (the authoritative Render path) as the FIRST step, present-set from `${{ toJSON(secrets) }}` piped
> on stdin. Unit-tested (comparison + a mutation-kill decoy proving it keys on `from_github_secret`,
> not the env key) plus a `CARGO_BIN_EXE` integration test running the real binary against the real
> artifact. **WHAT IT DOES NOT COVER — stated loudly in the tool's own output, the workflow comment,
> and here**: (1) **VALUES** — a name present but GARBAGE (a `pk_test` where prod needs `sk_live`,
> an expired token) PASSES; mis-NAMING and ABSENCE only, the value-level verdict stays
> `prod-smoke.sh`. (2) The **K8s `captain-secrets` sealed store** — populated out of band (#358), not
> from Actions; a name here does not prove it was sealed into the cluster. This gate proves the
> Actions → Render/declared-source NAMING boundary; the K8s store remains a NAMED RESIDUAL GAP
> (checkable once an Actions-reachable apply path exists), tracked by
> [#452 "Secret gate: extend to K8s captain-secrets name-presence + front the deploy-bins/#366 Argo path"](https://github.com/TheCaptainCompany/captain-food/issues/452). **`toJSON(secrets)` fidelity**:
> an UNSET Actions secret is ABSENT from the object (→ reported Absent), and GitHub's UI forbids
> empty secret VALUES, so the `Empty` branch's guaranteed reach is the declared-side/defensive case
> and any future present-set that can hold blanks (e.g. a kubectl-read cluster secret), not a routine
> Actions secret-side empty. **Scope fences held** (#329 trap): NOT asserting
> `secret-keys.json` vs `render-config-sync.json` production-set equality (compiler-owned, same
> emitter run, false-fails when worker consumers widen the set), NOT checking the cluster-side store.

> ✅ **2026-08-10 — THE STRIPE PUBLISHABLE KEY REACHES /checkout AND THE PAYMENT ELEMENT CAN
> MOUNT** ([#440 "Stripe publishable key: StripePublishableKeyTest scalar + payments configuration key, SSR-delivered to /checkout so the payment element can mount"](https://github.com/TheCaptainCompany/captain-food/issues/440),
> PR [#441](https://github.com/TheCaptainCompany/captain-food/pull/441), mob protocol; decisions in
> [ADR-20260810-015941](adr/ADR-20260810-015941-stripe-publishable-key-delivery.md)). The first
> #429 blocker ("no publishable key exists anywhere") is closed at the code level:
> `StripePublishableKeyTest` (`^pk_test_` — a live or secret key is unspellable in the slot),
> `STRIPE_PUBLISHABLE_KEY` declared in `specs/payments/configuration.yaml` (NOT secret,
> presence-gated: absent ⇒ boot never fails, `/checkout` degrades honestly), and the delivery seam
> server config → `SsrExec` → `RenderContext` → `data-pk` on the mount div → hydrate →
> `PaymentElement::mount` in Stripe's DEFERRED posture (no intent can exist at landing —
> acceptance-first). stripe.js ships in the checkout shell ONLY and only when the key exists.
> Key-less/invalid ⇒ `payment_unavailable_state` (fr/en) + DISABLED pay button + zero Stripe
> requests, counted by `checkout_degraded_render_total{reason=stripe_key_absent}` — emitted at the
> SSR boundary and **proved firing** by `crates/server/tests/checkout_degraded_metric.rs` (the
> repo's first spy-observed metric emission). Smoke gains L3b (/checkout must carry
> `data-pk="pk_test_…"`, outage-honest). **Shipped ENV-VAR-ONLY at the time**:
> `STRIPE_PUBLISHABLE_KEY` was declared non-secret with no `deploy:` block, production served by
> the Render env var alone — **CLOSED 2026-08-10 by
> [#448 "Bake the Stripe TEST publishable key as a literal deploy value"](https://github.com/TheCaptainCompany/captain-food/issues/448)**
> (PO-provided value baked; see the #448 entry above — the Render env-var deletion is the remaining
> hygiene click). The one extraction attempt (a CI step base64-defeating log masking) was correctly
> blocked by the security classifier and is ABANDONED — no masking-bypass retry (ADR §3).
> **Still open**: the surface-bins config-closure follow-up
> (#385 track); and the recorded activation constraint (first real-restaurant activation
> mechanically impossible while checkout serves `pk_test_` — ADR §4, binds future activation work).
>

> ✅ **2026-08-09 (morning) — THE EIGHT-DECISION BRIEF IS ANSWERED; the demo is deferred and the
> target is now production-with-test-data** ([ADR-20260809-050000](adr/ADR-20260809-050000-morning-brief-eight-decisions.md)).
> The open-decision register went **21 → 8** in one sitting, by answering rather than appending.
> **The demo epic is DEFERRED** and the ~80% of it that was production correctness wearing a
> marketing label is **re-filed on its own** as [#429 "Production with test data: a test customer places a real order against a test restaurant, paid with Stripe test mode"](https://github.com/TheCaptainCompany/captain-food/issues/429). The replacement target, in the product owner's words:
> *"test customers making test orders on test restaurants with test payment on stripe"* — on the
> **production deployment**, not a staging rehearsal (D1 → nothing hosted yet; one environment, so
> the two-namespaces-over-one-database contradiction never arises). Also decided: the named Uber
> comparison **stays and its substantiation is funded**, with the restaurant's own numbers published
> beside it (it must be COMPUTED first — the cart projector's `uber_comparison` is always `None` and
> the total is `0`); the demo session is **pre-identified, no SMS** (still blocked by the unscoped
> order reads on [#144](https://github.com/TheCaptainCompany/captain-food/issues/144)); **one
> deployment picks Stripe keys per order mode** — safe while everything is test mode, and **due a
> type-level form before any live key exists**; the neutral checkout-failure copy is **approved**,
> under a standing principle of *"as precise as possible"*; the login-to-domain bridge lives in
> **JWT claims**, with **per-person accounts for every rider and every member of restaurant staff**
> (this unblocks [#415](https://github.com/TheCaptainCompany/captain-food/issues/415)); and the
> step-DSL branching set **D1–D7 is confirmed as recommended** (PROP-20260809-003000 → `Approved`).
>
> **What stands between here and that target**, all recorded and none of it speculative: no Stripe
> publishable key exists anywhere; `/checkout` carries no route params while both its resolvers take
> required inputs; no customer bearer token exists in `crates/web` while the order reads are
> CUSTOMER-guarded; `orders`/`order`/`carts` apply no ownership filter (fix ~80% written in a draft
> PR parked since 26 July); the cart total never computes; and nobody is told when a paid order
> arrives.
>
> Last updated: 2026-08-09. Legend: ✅ done & verified · 🚧 in progress · ⏳ blocked/waiting · 📋 planned.

> 🚧 **2026-08-09 (late night) — #437: VERIFYPHONE STAMPS THE CUSTOMER CLAIM BEFORE THE TOKEN IS PARKED
> ([#437 "verifyPhone stamps captain_customer_id before token issue; customer bearer token rides the session (#429 blocking precondition)"](https://github.com/TheCaptainCompany/captain-food/issues/437),
> [PR #438](https://github.com/TheCaptainCompany/captain-food/pull/438),
> [ADR-20260809-212810](adr/ADR-20260809-212810-verify-phone-claim-stamp-posture.md)).**
> The #429 blocking precondition: `verify_phone` now resolves the Customer, STAMPS
> `captain_customer_id` + `captain_role` via a new Supabase admin ACL call
> (`identity.stamp_customer_claim`, spec-declared with `SUPABASE_SECRET_KEY`), refresh-ROTATES the
> session, and parks ONLY the rotated (claim-bearing) token — so the `captain_auth` cookie minted
> at `/auth/session` pickup already satisfies `ReadScope::Customer`. Failure posture: verification
> stands, an unstamped token is never parked, `claim_conflict` never retried (`claims.stamp` span
> + `customer_claim_stamp_failed_total{reason}` at the ACL). Idempotent re-stamp with the
> role-exactness rule (no-op ONLY on `captain_role == "CUSTOMER"`; wrong role repaired by the PUT).
> **Red-first chain, each seen red verbatim**: parked-token decode (`InvalidLastSymbol` on the
> pre-rotation token) → ordering; `stamp_decision` wrong-role (`left: Noop right: Put`) → role
> exactness; planted claim→`rider_id` transposition in `authorize()` (`left: None right:
> Some(…437)`) → the new end-to-end test (real seeded ES256 JWKS, JWT delivered cookie-ONLY,
> claim → `Principal` → `ReadScope`, tamper-rejection arm). **No client bearer plumbing** — ratified
> deviation: the httpOnly cookie IS the transport on both legs (#112 shipped design; same-origin
> fetch + WS upgrade; `ws_auth_headers` extracted pure + pinned so no-payload-token keeps the
> upgrade cookie untouched). Host-only cookie consequence recorded: storefront sign-in ≠ marketplace
> sign-in. **Deploy facts (verified)**: GitHub Actions secret `SUPABASE_SECRET_KEY` exists
> (render-config-sync run 31335187939) and Render already holds the value; presence is the gate —
> boot never fails without it, the stamp fails closed. **Known gap (pre-existing, observability
> lens)**: the customer-identification contract's `otp.verify` span is implemented nowhere — issue
> to be filed by the coordinator. DB suite untouched (no schema change).

> 🚧 **2026-08-09 (night) — #435: ScopeMembership `principal_type`/`principal_id` → `member_type`/`member_id`
> ([#435 "ScopeMembership: rename principal_type/principal_id to member_type/member_id (product-owner naming directive)"](https://github.com/TheCaptainCompany/captain-food/issues/435),
> [PR #436](https://github.com/TheCaptainCompany/captain-food/pull/436),
> [ADR-20260809-200826](adr/ADR-20260809-200826-scope-membership-member-naming.md)).**
> The membership columns hold DOMAIN ids, so they are `member_*` now; the server's `Principal`
> struct keeps its name — it IS the technical caller (the meaning the product owner reserves for
> the word). Spec table + regeneration, a separate ALTER migration (`20260809190000`, the CREATE
> is checksummed) + `REQUIRED_SCHEMA_VERSION` bump, and a rename-only code mirror
> (`ReadScope::principal()` → `member()`; the worker's revoke-failure log field key is
> `member_type` now). Proven red-then-green against throwaway Postgres: 4 scope_membership DB
> tests failed on `column "principal_type" does not exist` with the migration in and the code
> unmirrored, 60/60 green after. **Key stability**: `membership_id` (UUIDv5) hashes enum wire
> values, not column names — the pinned-literal test passed byte-identical throughout. Deploy
> fact: production has no serving binary and never ran `20260809140000`, so CREATE + ALTER land
> in one `sqlx migrate run`; pre-#436 images are not deployable against a migrated DB (no down
> migration). The `actors.yaml` `principals:` role-mapping vocabulary is a DIFFERENT concept,
> consciously deferred.

> 🚧 **2026-08-09 (evening) — #433: READSCOPE RESOLVES FROM JWT CLAIMS FOR ALL ROLES (product-owner
> correction on the merged #430, in their words: "This information is provided in the jwt") —
> [PR #434](https://github.com/TheCaptainCompany/captain-food/pull/434),
> [ADR-20260809-160000 addendum 2](adr/ADR-20260809-160000-read-authorization-lands-ported-from-152.md).**
> `read_scope` is now a PURE function of the token's verified claims (`captain_customer_id` /
> `captain_rider_id` join the two restaurant claims): the per-request `by_auth_ref` bridge and the
> rider sub-parse placeholder are DELETED from scope resolution ("sub is never an identity" — pinned
> with distinct-uuid tests, seen RED under a planted sub-fallback), `ScopeResolver` is gone entirely
> (no dependency left to be missing; the Friday-peak auth path no longer shares fate with the
> database), and the four generated resolvers that still authorized via `by_auth_ref`
> (`paymentStatus`, `paymentStatusChanged`, `myReclamations`, `customerCredit` — the mob's graphql +
> architect lenses) read the same claim-derived ReadScope, killing the order-visible-but-payment-dead
> split-brain. prod-smoke now mints the claims it needs (unconditional stamp BEFORE link generation,
> both keys, token-decoded assertion): the L4 order poll is the customer-POSITIVE production proof,
> and the negative probe is a BRIDGED stranger (the membership EXISTS path), outage-honest.
> **Honestly scoped**: `by_auth_ref` REMAINS at the write-side seams (mailbox `resolve_actor`,
> mutation edge bridges) and `myDeliveries` keeps its rider sub-parse until #415 — recorded, not
> overclaimed. **BLOCKING precondition recorded on #429's bearer-token item** (three lenses
> independently): verifyPhone must stamp the claim BEFORE the client's token is issued, or the first
> paid session is the one denied its tracking screen. **Erasure obligation on #194**: Supabase
> `app_metadata` now stores domain ids and a claim outlives erasure until expiry — the sequence must
> scrub app_metadata + revoke refresh tokens.

> ✅ **2026-08-09 (afternoon) — #429's REBASE-AND-LAND ITEM: READ-SIDE PER-INSTANCE AUTHORIZATION
> LANDED, ported from the parked PR #152
> ([#144 "Read-side per-instance authorization: ReadScope on the read ports + RESTAURANT/RIDER identity bridges"](https://github.com/TheCaptainCompany/captain-food/issues/144),
> [PR #430](https://github.com/TheCaptainCompany/captain-food/pull/430),
> [ADR-20260809-160000](adr/ADR-20260809-160000-read-authorization-lands-ported-from-152.md);
> PROP-20260725-185140 → `Approved`).** The pre-#144 hole — `orders` with no arguments dumped the
> ENTIRE tracking table to any authenticated customer; `order(id:)`/`carts(customerId:)` read
> anyone's rows — is closed by the `ScopeMembership` ACL index (grants narrow, revokes broad, ONE
> checkpoint over `Order-`/`DeliveryJob-`/`Restaurant-` so a revoke can never fold before the grant
> it supersedes) and a `&ReadScope` parameter that makes an unscoped order read UNSPELLABLE.
> Ten-lens mob briefing (ADR-20260809-013142) reshaped the port before code: **no Rider bridge
> table** (CARD-11: bridge lives in JWT claims; sub-as-RiderId placeholder until
> [#415](https://github.com/TheCaptainCompany/captain-food/issues/415)); TEXT enum storage (the
> branch predated ADR-20260728); `myDeliveries` hydrates as SYSTEM (caller-scoped hydration would
> blank the PENDING offer pool = a self-sealing dispatch outage, ux lens); `delivery` degrades
> out-of-scope hydration to null (no oracle); the subscription reads through ReadScope, closing its
> "RESTAURANT paths are trusted" gap; `customerId` REQUIRED through the checkout chain (narrowing
> legal solely on the empty log — recorded exception); prod-smoke L4 reworked in the same PR
> (placeOrder carries customerId, captured-order poll as ADMIN, and a NEGATIVE assertion proving in
> production that a non-member reads nothing — the only executable proof, #212 keeps rules.yaml
> blind here). Gates: `make rust` green (0 errors, warning histogram 37 → 37 byte-identical,
> baseline re-measured on pristine main), full infra DB suite 59/59 on a throwaway Postgres with
> `DB_TESTS_REQUIRED=1` (the money test seen RED under both forced mutations: EXISTS clause deleted →
> stranger list dumped; by_id check deleted → stranger read leaked), application 315/315.
> **Honest limits**: restaurant back-office order reads are EMPTY until minted tokens carry
> `captain_restaurant_id` (#429's restaurant leg runs on ADMIN until #415 — no such token exists
> today, nothing that works stopped working); ACL-index projection lag = a user-visible denial
> (dedicated `scope_membership_lag_positions` gauge, worker-emitted); the smoke customer has no
> domain Customer (verifyPhone needs real SMS), so its own order read is refused BY DESIGN and the
> negative assertion rides exactly that. POST-MERGE CORRECTIONS (review comments that landed as
> auto-merge fired; ADR addendum + [#432](https://github.com/TheCaptainCompany/captain-food/issues/432)):
> pre-#144 `Order-*` streams are FROZEN, not "Admin-only" — the write-side loader hard-errors on
> them, so no command can touch them (fine for smoke data; a named landmine for any future payload
> narrowing on a live log); and the smoke's outage-honesty check is incomplete (`gql()` swallows
> transport status — `{}` passes both jq probes), fix tracked on #432. Remaining tenant read surfaces + LIMIT/pagination +
> the ownership-declared validator rule = one follow-up issue.

> 🚧 **2026-08-09 (night) — G5/G6 UNBLOCKED (not closed), G7 CLOSED: the customer path is
> wired, and still unreachable in a browser
> ([#420 "Customer delivery reassurance: tracking shows the rider path, checkout FAILED state, orphan binding fix (#348 slice 8)"](https://github.com/TheCaptainCompany/captain-food/issues/420),
> the code-only half of PROP-20260809-021351 §6 item 1).** `hydrate()` no longer returns above the
> crate's only `mount_to_body`: checkout and order_tracking MOUNT, install the delegated action
> layer, resolve their declared `data_requirements`, and tracking folds `orderStatusChanged` with a
> pull re-sync on every (re)connect. `render_path_with` resolves `data_requirements` for EVERY
> matched screen — the `sdui` conjunct never had a reason, and the `requires_auth` one was a fact
> about the TRANSPORT (which the renderer cannot know), so it now asks and lets a refusal degrade the
> binding exactly as before. The checkout shell is built from `cart.current`/`me.profile`/
> `paymentStatus.byOrder` instead of `""`/`0`/`""`/`false`, and tracking from `order.byId` instead of
> `TrackingState::new(id)`; the status hero renders the resolved SENTENCE (it used to emit `data-i18n`
> on EMPTY elements, so the page a customer landed on after paying was blank above the fold).
> **G7, in the EMITTER**: `orderStatusChanged` filtered only `Order-<id>` AND deduped on
> `OrderStatus`, so the #424 delivery mirror was swallowed twice over; it now also matches THIS
> order's delivery job (bound lazily via `DeliveryReadRepository::by_order`, so a foreign envelope
> costs nothing once bound) and dedupes on the row's own `updated_at` fold clock.
> **The gate hole is closed COMPILER-FIRST** (ADR-20260803-234035): `crates/web/src/handwritten.rs`
> carries a closed `HandWrittenScreen` enum, exhaustive dispatch with no `_` arm on either entry, and
> two `const` proofs walking the generated screen tables at COMPILE TIME in both directions — a new
> `sdui: false` screen without a mount is now `E0080`, not a page that silently renders nothing.
> `every_sdui_screen_of_every_surface_renders` → `every_screen_of_every_surface_renders`, skip
> removed. Both named tests seen RED first (`left: 0` reads; `Elapsed(())` on the delivery hop).
> Gates: `make rust` green, 0 errors, warning histogram **37 → 37, same kinds**;
> `cargo test -p server --test graphql_subscriptions` **10/10** (NOT covered by `make rust`).
> The delivery-hop test was **renamed to what it proves** after `beck` established by mutation that
> it proved the DEDUPE, not the filter: reverting the filter while keeping the dedupe left it green,
> because the helper pumps 50 copies of the order's own envelope and each opens a ~3 s re-poll
> window, so a lingering `Order-` envelope delivered the second frame.
> `a_delivery_job_envelope_alone_reaches_the_confirmation_page` now isolates the delivery branch —
> verified RED under that exact mutation (`Elapsed`), green restored.
>
> **WHY G5/G6 ARE NOT CLOSED — the mounts are wired and every read they feed is REFUSED.** Three
> review lenses converged on this; it must not be mistaken for done.
> **(a)** `/checkout` has **no route params**, and `cart.current` / `paymentStatus.byOrder` both take
> REQUIRED inputs, so both documents are dropped before they are sent — the shell receives an empty
> map and renders the old hardcoded state plus the host slug. `payment_failed` **cannot become true**.
> **(b)** `order.byId`, `me.profile` and `orderStatusChanged` are all `CUSTOMER`-guarded, while the
> customer surface talks to `/public` and `web_ssr.rs` renders as anonymous `RequestRole::Public`
> with no session — and **no bearer token exists anywhere in `crates/web`**. SSR, hydrate, the
> reconnect re-sync and the socket subscribe are each refused.
> So a customer who pays today still lands on a page carrying no order. What #427 changed is that
> the page **no longer lies about it**: a refused read renders `data-status="PENDING"` and makes no
> claim, where it previously rendered "Commande introuvable" for every order, forever — `OrderRead`
> makes "the transport refused" and "no such order" unrepresentable-if-confused.
>
> **GAP(copy), on #420**: the right content for the unresolved state is the acceptance-first
> reassurance ("Reçu ✓ — confirmation en cours…"). It needs a translation key, and customer copy is
> approved verbatim by the product owner, so it rides the spec half rather than being invented here.
> **Still open on this path, each needing a DSL change and reported on #420**: no Stripe
> **publishable** key exists anywhere (`specs/payments/configuration.yaml`); a way for the checkout
> route to supply a cart/order id; a customer bearer token on the web transport; `cart.current`
> carries no restaurant NAME (the shell falls back to the host's tenant slug); and the `order.byId`
> selection carries no restaurant name either, so the hero's BODY copy is withheld rather than
> shipped with an unfilled `{restaurant}` — which is **every status in the twenty-minute pre-food
> window**, so the customer gets a title alone for the whole anxiety curve.
> **And one gate that does not exist**: `beck` re-planted the exact `if !screen.sdui { return; }` bug
> this work fixes and **both CI gates stayed green** (96 native tests, `make wasm`). `make wasm` is a
> COMPILE check and the regression class is a semantic early return, so it cannot help; the `const`
> proofs prove an arm EXISTS, never that `hydrate()` reaches it. The honest gate is
> `wasm-bindgen-test` + a headless DOM — filed on #420, not assumed.
>
> 🔴 **2026-08-09 (night) — THE CUSTOMER PATH IS INERT ON `main`, and a paid order tells nobody.**
> Four lenses briefed in parallel on [#410 "Epic: public try-before-committing demo"](https://github.com/TheCaptainCompany/captain-food/issues/410)
> (farley lead · ux-designer · beck · dba) converged independently on the same root cause, recorded
> in [PROP-20260809-021351](proposals/PROP-20260809-021351-public-demo-one-continuous-walk.md) §2:
> `renderer.rs::hydrate()` **returns early for every `sdui: false` screen** and the crate's only
> `mount_to_body` sits after that guard, so **checkout mounts no Stripe element and its place-order
> button dispatches nothing** — and its SSR shell is data-less (`router.rs:236-241` hardcodes
> `restaurant_name: ""`, `cart_line_count: 0`, `formatted_total: ""`). The same guard makes
> `/orders/:id/confirmation` render the **not-found hero for every order, forever**
> (`TrackingState::new(order_id)`, `order: None`). Separately, **no notification port exists
> anywhere** (`crates/application/src/ports.rs`) and `orderStatusChanged` is keyed per `orderId`, so
> the kitchen queue only learns about a paid order on page reload — the domain lens's named worst
> failure mode, live. **Why green gates missed all of it**: `prod-smoke.sh` never opens a browser,
> and `every_sdui_screen_of_every_surface_renders()` deliberately SKIPS `!screen.sdui` screens, so
> checkout and tracking are excluded from the one test that would have caught it — 22 web tests pass
> in 10 ms over the entire broken half. beck: *"not one test in this repo would go red if a stranger
> could not order."* #410 is therefore **not blocked on hosting**; the zero-console work is
> PROP-20260809-021351 §6, and the customer owns D1/D3/D4 in
> [DECISIONS.md §24](proposals/DECISIONS.md).
>
> ✅ **2026-08-09 — [#335 "Decide whether to consolidate integration test binaries (~3.5G of link products)"](https://github.com/TheCaptainCompany/captain-food/issues/335): `crates/infrastructure`'s 27 integration binaries consolidated into ONE (`tests/main/`, 1.4G → 70M of link products) behind a compiler-enforced `common::TestDb` witness (binary-wide lock + ONE migration-derived `reset_schema`), per ADR-20260808-224500 item 5 — which immediately surfaced and fixed a real spec↔migration drift: `catalog.slug` was still NOT NULL in production migrations while the generated schema and the projector have it nullable (`migrations/20260809000000_catalog_slug_nullable.sql`).**

> ✅ **2026-08-09 — #348 CUSTOMER-ANXIETY QUICK WINS APPLIED
> ([#424 "Customer-anxiety quick wins: DeliveryPickedUp reaches order tracking, checkout shows a FAILED state (approved spec diff, option b)"](https://github.com/TheCaptainCompany/captain-food/issues/424)),
> per the exact-text approval in [ADR-20260809-002500](adr/ADR-20260809-002500-quick-wins-approved-d6-dsl-extension-chosen.md)
> realizing [PROP-20260808-233000](proposals/PROP-20260808-233000-customer-anxiety-quick-wins-spec-diff.md).**
> **QW1 — the pickup fact now REACHES the customer's order row (the screen still has to be taught
> to say it — slice 8, [#420](https://github.com/TheCaptainCompany/captain-food/issues/420)).**
> `orderId` joins four delivery payloads (D-QW1
> option b: REQUIRED on `DeliveryAcceptedByRider`/`DeliveryPickedUp`/`DeliveryCompleted`, NULLABLE on
> the inbound `DeliveryStatusUpdated` — the orphan doctrine, where a birthless stream has no order id
> anywhere in the system); `DeliveryPickedUp` joins OrderTracking's `fedBy` + the `delivery_status`
> lineage. Application: `DeliveryJobState` folds `order_id` from the birth fact, the 3 rider command
> handlers stamp it from state, the 3 partner ACLs emit `None` and the inbound recorder ENRICHES from
> the fold before append (null therefore marks exactly the orphan anomaly), and the handlers emitter
> gained a **state-sourced field seam** so the GENERATED `update_delivery_status` supplies `orderId`
> the command deliberately does not carry. **The wiring that made it real**: the Order projector group
> now slices the full `DeliveryJob-%` family (`worker.rs`) — the `docs/sagas.md` open item since
> ADR-0031; without it the whole change was spec theater. First-ever runtime proof of the mirror:
> `order_projection.rs::delivery_facts_move_the_customers_delivery_mirror` (verified to FAIL when the
> stream prefix is removed). **Two honest limits** (independent review + ux-designer pass): the
> PARTNER path stays unfed (`DeliveryAcceptedByPartner` is in the fedBy for `courier`/
> `estimated_dropoff_at` but carries no `orderId`, so a partner delivery's courier and ETA never
> reach the customer — slice 8); and the `DeliveryJob-%` slice joined the EXISTING `Order`
> checkpoint, so any delivery event already below that position is never folded (moot on an empty
> log; on a non-empty one the `Order` checkpoint must be reset at deploy).
> **QW2 — the checkout FAILED state EXISTS but is NOT REACHABLE yet.** The screen declares
> `paymentStatus.byOrder` (a read it already performed undeclared) and a `payment_failed_state`
> section + 4 translation keys, and a FAILED status now SHORT-CIRCUITS the intent poll instead of
> spinning to the bound and reporting `IntentUnavailable` (verified: `PaymentStatus::FAILED` is
> written only by the `PaymentFailed` leg, so no timeout or transient error can produce a false
> FAILED). But production sets `payment_failed: false` unconditionally and `sdui: false` screens
> never hydrate, so **no customer can reach the state today** — and a card refused AFTER checkout
> lands on the tracking screen, which renders "Commande introuvable". Wiring the checkout page and
> that tracking twin are the blocking items, scoped on #420; a copy rewrite ("Paiement refusé" is
> untrue when the failure is technical) waits on customer approval.
> Gates: 0 errors, warning histogram 37 → 37 byte-identical (the diff clears nothing by design —
> its value is the read-model wiring), 916 workspace tests + the full infrastructure DB suite green.
>
> ✅ **2026-08-08 (night, follow-up) — #348 SLICES 1–2 APPROVED BY THE CUSTOMER LIVE; SLICE 1
> APPLIED TO MAIN BY THE RUN ([ADR-20260808-230800](adr/ADR-20260808-230800-rider-delivery-slices-1-2-approved-and-applied.md)).**
> All five answers were the recommended options: §2 as written (applied, full `make rust` gate,
> expected 43 → 37 warnings), §3.2 `sends:` approved (lands with the D6 validator mechanism),
> both customer-anxiety quick wins pulled forward (diff being prepared), slices 3–8 filed in the
> parent proposal's value order, apply-now vehicle chosen explicitly over §6 plan-mode.
>
> ⏳ **2026-08-08 (night) — #348 SLICES 1–2 SPEC DIFF PREPARED, AWAITING CUSTOMER APPROVAL —
> [PROP-20260808-221424](proposals/PROP-20260808-221424-rider-delivery-slices-1-2-spec-diff.md)
> (autonomous run; `specs/**` untouched).** The exact per-file diff realizing the approved
> [PROP-20260808-141817](proposals/PROP-20260808-141817-rider-delivery-write-surface.md) slices 1–2:
> retire the `AssignDeliveryToPartner`/`DeliveryAssignedToPartner` and
> `UpdateDeliveryPartnerStatus`/`DeliveryPartnerStatusUpdated` families (6 source files, incl. the
> forced `TestDeliveryUnassignedFromPartner` rewire + 2 prose rewords), declare
> `PaymentFailed`/`CustomerIdentified` `nonProjectedEvents` (category a), and the D6 `sends:` YAML
> (applies only WITH its validator mechanism, after
> [#399 "Validator gap: a tombstone event absent from the view's fedBy silently never dispatches"](https://github.com/TheCaptainCompany/captain-food/issues/399)).
> Expected delta 43 → 35 warnings, 0 errors, residue mapped one-to-one onto slices 3–7/D5/D6.
> **The retirement window closes when production events exist** — flagged in every status until the
> customer approves. Application = plan-mode session, per the document's §6.
> REVIEW — [#393 "Cross-cutting worker hosting: one bin per worker"](https://github.com/TheCaptainCompany/captain-food/issues/393)
> via [ADR-20260808-062933](adr/ADR-20260808-062933-one-bin-per-worker.md) (product-owner
> decision; the FINAL repo work item of the ADR-20260807-183024 program).** c4-l2 replaces
> `sync-worker` with `worker-sirene-sync` + `worker-retention` / `worker-journal-sweep` /
> `worker-erasure`, each with a DECLARED 5-field cron cadence (`schedule:`; validator rules
> `c4-worker-*`); shape follows cadence — the emitter renders CronJobs (Forbid, restartPolicy
> Never, UTC, `suspend:` from spec) for periodic workers, `bam` stays the always-on Deployment.
> Worker mains are run-to-completion passes over the EXISTING implementation crates (shared:
> `sirene_ingest::sweep` + `infrastructure::integrations::journal_sweep` extracted so monolith
> and bins run ONE implementation). MINIMAL GRANTS: periodic workers keep only the
> DATABASE_URL + HONEYCOMB_API_KEY secret floor from `common` (the GDPR erasure pod is the
> auditable case — asserted against the FULL secret catalog in the deploy test); the
> `sirene_ingest`-consumer keys route to `worker-sirene-sync` alone (still without a deploy
> source until cutover — GitHub Actions injects them). `worker-sirene-sync` lands SUSPENDED:
> sirene-sync.yml stays the authoritative cron until the #358 cutover records the handover, and
> the pass honours RUN_SIRENE_WORKER (#220 pause) besides. c4-l3: `sirene-google-acl` split —
> enrichment stays with the sync worker; Google ownership verification is its own component
> homed on `actor-restaurant` (where the `GoogleOwnershipVerifier` port executes). Bin count
> 53 → 57. GATE-THEN-STABILIZE: nothing applies manifests; the monolith's in-process loops stay
> the running instances.
>
> ✅ **2026-08-08 — ONE BIN PER ADAPTER: THE COMPOSED `adapters` POD IS SPLIT PER PARTNER, MERGED
> (PR #395) — [#391 "One bin per adapter"](https://github.com/TheCaptainCompany/captain-food/issues/391)
> via [ADR-20260808-062432](adr/ADR-20260808-062432-one-bin-per-adapter.md) (product-owner
> decision).** c4-l2 replaces `adapters` with `adapter-stripe`/`adapter-hubrise`/
> `adapter-uber-direct`/`adapter-coopcycle`/`adapter-avelo37`; the emitter derives the family from
> the adapter-crate list scanned at model load (`crates/adapters/*` — a sixth crate produces a
> sixth bin, §15 then requires its container, both directions checked). Each pod env + generated
> Config narrows to the partner's OWN env prefix within its declared `integration_scopes`
> (UBER_DIRECT_* no longer reaches the Avelo37 pod; pairwise-asserted in the deploy completeness
> test, per-partner closure sharpness in the determinator tests). `hooks.captain.food` carries one
> `/adapters/{partner}` path per Service (no surface at `/`); marketplace-host per-partner
> transition aliases kept, dead `/webhooks`|`/services` aliases dropped. Bin count 49 → 53.
> STRUCTURALLY DISSOLVES the cross-partner half of the #385 secret-grant cutover precondition;
> the remainder (bam's `domain_scopes` path + per-key consumer metadata, incl. boot-required
> `STRIPE_SECRET_KEY` in `adapter-stripe` whose webhook code reads only the webhook secret) stays
> recorded on #385. `adapter-avelo37` exists but stays unprovisioned/undeployed BY DESIGN
> (pre-milestone; its keys still declare no `deploy:` source). GATE-THEN-STABILIZE: nothing
> applies manifests; the monolith stays authoritative.
>
> 🚧 **2026-08-08 — ADR-183024 STEP (6) PREP: CNPG PLATFORM TREE IN REVIEW —
> [#360 "CNPG: operator + 3-instance cluster, WAL archiving to Object Storage, weekly executed
> restore drill"](https://github.com/TheCaptainCompany/captain-food/issues/360) repo-only slice via
> [PR #392](https://github.com/TheCaptainCompany/captain-food/pull/392),
> [ADR-20260808-063951](adr/ADR-20260808-063951-cnpg-platform-source-tree.md) (hand-written
> platform SOURCE under `deploy/platform/`, invariants pinned by `platform_*` codegen tests —
> CNPG derives from no spec, so no emitter).** Pinned operator 1.27.4 (vendored byte-identical,
> sha256 in PIN.json); `captain-db` Cluster at the ADR-20260807-114122 ENTRY shape
> (`instances: 1`, required anti-affinity, superuser disabled, postgres 17.10 digest-pinned,
> `captain-db-retain` StorageClass, barman WAL archiving to the OVH bucket by NAME —
> `cnpg-object-storage`/`claude-ro-credentials`/`restore-drill-github-token` secrets referenced,
> never provisioned, missing = visibly-failing pod); the 3-instance quorum-sync D2 shape is the
> GATED `cnpg/ha/` overlay (flip = its own one-line ADR). Weekly restore drill (Mon 04:30 UTC,
> standing scratch ns `captain-restore-drill`, least-privilege RBAC): restores latest backup,
> verifies domain_events count+md5 vs production OVER THE SAME position RANGE as SELECT-only
> claude_ro, files a deduplicated GitHub issue on failure; hourly WAL-archiving/backup-age
> check alongside (§2b practice 4). `claude_ro` grants ship as ordinary migration
> 20260808070000 (practice 5; role lifecycle stays with CNPG `managed.roles`). `db-migrate.yml`
> gains the GATED `target: cnpg-port-forward` dispatch input (default supabase unchanged;
> flip of the default is a separate ADR). NOTHING IS APPLIED: bucket, secrets, first apply,
> executed drill = the product-owner console checklist in `deploy/platform/README.md`; #360
> stays open for the EXECUTED drill.
>
> 🚧 **2026-08-08 — API TIER WIRED: ALL 49 BINS ARE BUSINESS RUNTIMES, PR #389 IN MERGE —
> [#385](https://github.com/TheCaptainCompany/captain-food/issues/385) remainder delivered by
> [PR #389](https://github.com/TheCaptainCompany/captain-food/pull/389),
> [ADR-20260808-060309](adr/ADR-20260808-060309-bare-apex-owner.md) (apex → marketplace,
> hooks → adapters).** graphql-{scope} ×8 serve their SCOPE SLICE via `server::bin_support`
> (same DI, same AuthContext/ACL as the monolith — auth stays at the schema boundary);
> gateway-{role} ×7 are the pure `gateway_runtime` (no domain/db; routing and subgraph
> acceptance share ONE `root_fields` walk, so they cannot disagree; forwards BOTH auth
> carriers — httpOnly cookie + authorization — and `x-external-api-key`, pinned by test);
> surfaces ×6 via `surface_runtime` (wasm assets a real image input; adapters composes the 5
> webhook ingestors on `hooks.captain.food`); bam projects honestly. Config emitter now applies
> deploy.rs's `needs_db` exclusion (db-less bins no longer boot-require `DATABASE_URL`).
> COMPILER-SHARPNESS DEVIATION (conscious, tested): only gateways keep a sharp compile-time
> closure — subgraphs link `server` (whole facade), surfaces reach domain via `web→core`; the
> wall is the runtime scope slice + #360's GRANT wall until the per-scope infrastructure split.
> Third coupling direction, same disclosure: `server` depends on `gateway_runtime` (shared
> `root_fields` walk — the price of routing and acceptance provably agreeing) and on
> `surface_runtime` (relocated `hosts`), so the thin runtimes ride transitively into every
> subgraph bin via `server`.
> Review: 3 independent lenses + required `claude-review`, 8 findings — 5 fixed on-branch,
> CUTOVER PRECONDITIONS recorded on #385 (auth-session mint has no bin home; per-key secret
> consumer metadata BEFORE #358 — `STRIPE_SECRET_KEY` must not reach the adapters pod;
> Avelo37 `deploy:` block ships with partner-milestone secrets; apex TLS SAN; replicas
> differentiation; WS reconnect backoff; `integration_scopes` validator rule).
> GATE-THEN-STABILIZE: the monolith `server` remains the deployed runtime until steps (6)–(7).

> 🚧 **2026-08-07 — ADR-183024 BIN RUNTIME WIRING, CQRS SPINE IMPLEMENTED, PR IN REVIEW —
> [#385](https://github.com/TheCaptainCompany/captain-food/issues/385) "Bin runtime wiring:
> business runtimes inside the 49 shells",
> [ADR-20260807-231754](adr/ADR-20260807-231754-bin-runtimes-composition-kit-scoped-config.md).**
> The 27 CQRS-spine bins (15 actor-* + 5 pm-* + 7 projector-*) are BUSINESS RUNTIMES: generated
> mains (config gate → telemetry → declared-size pool → family spawn → probe server, readiness
> 503 until the hosted runtime runs, `wired:true`) over the new hand-written
> `crates/bin_runtime` composition kit — actor/pm fleets ride the SAME
> `infrastructure::mailbox::standalone` runtime the adapters use (posture-gated money lanes,
> flip-time backfill sequenced before the restricted saga runner, monolith parity); projector
> bins drain the shared registry scope-filtered on SHARED checkpoints (handover needs no
> re-projection; `delivery` owns no group and idles honestly). Per-bin generated Config =
> scope-filtered key subset (#374 Q4 closed); `DATABASE_POOL_MAX_CONNECTIONS` declared (monolith
> + bins); adapter links derive from spec `ports:` (which EXPOSED ReclamationProcess's
> undeclared-but-used payment port — now declared). Registry scope labels are tied to the
> generated `ACTOR_SCOPES` placement table by test. RECORDED COSTS: wired bins couple to the
> full domain facade through `infrastructure` (blast radius honest in the determinator tests;
> re-sharpening = the per-scope infrastructure split, follow-up on #385) and the in-process
> status/event buses mean cross-process push subscribers go dark for bin-delivered completions
> (poll paths unaffected). REMAINDER on #385 (issue stays open): graphql-* schema slices,
> gateway-* composition + addresses, surface wasm/SDUI assets, bam aggregation, spec homes for
> the bare-domain owner + integration host, sirene/retention/deletion/journal-sweep worker
> hosting. GATE-THEN-STABILIZE: the monolith `server` remains the deployed runtime until steps
> (6)–(7).
>
> 🚧 **2026-08-07 — ADR-183024 REALIZATION STEP (5) IMPLEMENTED, PR IN REVIEW — build matrix +
> determinator gate ([#363](https://github.com/TheCaptainCompany/captain-food/issues/363)
> "deploy.yml targets the GitOps path" realized as the build matrix per the settled protocol,
> [PR #386](https://github.com/TheCaptainCompany/captain-food/pull/386),
> [ADR-20260807-223428](adr/ADR-20260807-223428-build-matrix-determinator-gate.md)).** CI learns
> to build/test/publish PER BIN with change-driven selection, fail-open to rebuilding: a second
> `tools/codegen-rs` binary (`determinator`) wraps the guppy `determinator` library + repo path
> rules for the PR-time affected set (spec-derived crate graph, never a hand list; 16 property
> tests assert the bias — unknown file → all 49 bins, pin bump → nothing, one domain scope → its
> linked bins only) and computes the per-bin SOURCE-CLOSURE hash (git blob shas of the crate
> closure + global inputs + image name, `v1:`) that `deploy/pins/{bin}.json` records.
> `build-bins.yml` (new, additive, non-required): PRs build+test exactly the affected bins; main
> (after green ci) builds+pushes per-bin images ONLY where hash ≠ pin, one shared chef cook
> (`Dockerfile.bin`'s `ARG BIN` moved AFTER the cook — the old placement keyed the cook cache
> per-BIN = 49 cold cooks; `SOURCE_HASH` baked as the `food.captain.source-hash` forensic
> label). `deploy-bins.yml` (manual dispatch, GATED — nothing applies manifests until Argo
> #366): writes only hash-changed pins `{digest, source_hash}` + regenerated manifests as ONE
> commit after verifying the published label matches; refuses missing tags/mismatches loudly.
> Monolith `build-image.yml`/`deploy.yml` byte-identical and authoritative until cutover; Render
> retirement + prod-smoke retarget move with steps (6)–(7) (#358/#366). Validate 0 errors / 43
> warnings (kinds identical to baseline).
>
> ✅ **2026-08-07 — ADR-183024 REALIZATION STEP (4) MERGED — codegen emits the
> deployment ([#349](https://github.com/TheCaptainCompany/captain-food/issues/349) "Derive
> deployment artifacts from the existing specs", [PR #384](https://github.com/TheCaptainCompany/captain-food/pull/384),
> [ADR-20260807-220528](adr/ADR-20260807-220528-deploy-emitter-pins-are-input.md)).** The emitter
> derives `deploy/generated/` from the SAME topology as the bin crates: per-bin Deployments
> (`Recreate` + `replicas: 1` pinned with #193/#242 cited in place, /health + /ping probes,
> resources, env = production secret-sourced keys of the bin's scopes + common as secretKeyRef
> into the sealed `captain-secrets`; DATABASE_URL withheld from gateway/surface families per D8,
> except bins with a DECLARED c4 edge to the stores — `adapters` records inbound facts),
> Services for the HTTP families, an Ingress derived from the screens specs' `base_url` +
> per-screen roles, `Dockerfile.bin` (ARG BIN, one shared chef cook), `images.json` (#363's
> matrix input) and `secret-keys.json` (#358's sealing contract). **`deploy/pins/{bin}.json` is
> the CI-owned deploy ledger** (`{digest, source_hash}`): the emitter reads it, bakes digests
> into Deployments, seeds nulls, never overwrites — a null pin renders `:unpinned` (visibly
> undeployable). The 49 bins upgraded to PROBE-SERVING SHELLS (bind $PORT, serve the probes,
> drain on SIGTERM, report `wired:false`). Completeness tests: bin ↔ image ↔ pin ↔ manifest both
> ways + safety-pin assertions per manifest. GATE-THEN-STABILIZE: NOTHING applies the tree (no
> Argo yet, #366); the monolith `server` deployment remains the runtime, and the bins' BUSINESS
> wiring (mailbox hosting, per-scope projection filtering, subgraph slices, gateway composition)
> is recorded on #349 as the remainder that blocks the steps (6)–(7) flip. Recorded gaps: bare
> `captain.food` host unrouted (screens specs disagree on its owner); integration paths ride the
> marketplace host pending a spec home; per-key env narrowing waits on #374 Q4 (per-bin Config).
> Validate 0 errors / 43 warnings (kinds identical to baseline).
>
> ✅ **2026-08-07 — ADR-183024 REALIZATION STEP (3) MERGED — the bin crates
> ([#382](https://github.com/TheCaptainCompany/captain-food/issues/382) "Bin crates:
> per-actor/per-PM/per-projector/per-subgraph/per-gateway/per-surface binaries from the c4-l2
> topology", [PR #383](https://github.com/TheCaptainCompany/captain-food/pull/383)).** The codegen
> emits ONE BINARY CRATE PER DEPLOYABLE under `crates/bins/` (workspace glob member, stale bins
> pruned): 15 `actor-*` + 5 `pm-*` (deps = the crate-graph's spec-declared reach), 7
> `projector-{scope}` + 8 `graphql-{scope}` (deps = their scope's crate; the kernel gets a
> subgraph but no projector), 7 `gateway-{role}` and 6 surface bins (`fo-*`/`bo-*`/`adapters`) with
> NO domain crates, and `bam` linking every scope (cross-scope consumer by design) — 49 bins, each
> manifest the bin's SCOPE ASSERTION (`use … as _;` makes every declared link compile-checked;
> machete-clean). `specs/generated/crate-graph.generated.json` now carries the FULL bin topology
> (+ `path` per bin) — the #349 input contract; validator §15 (`c4-bin-name-mismatch` /
> `c4-bin-missing` / `c4-bin-unknown`) keeps derived bins ↔ c4-l2 containers drift-free both ways.
> GATE-THEN-STABILIZE: all 49 are SKELETONS (identity + exit); the monolith `server` bin remains
> the deployed runtime until [#349](https://github.com/TheCaptainCompany/captain-food/issues/349)
> (manifests emitter) / [#358](https://github.com/TheCaptainCompany/captain-food/issues/358) (MKS
> cutover) flip deployment. Step-2's recorded facade limit is now closed FOR THE BINS (each links
> only its scopes) *(corrected 2026-08-11 — true of the SOURCE, never of the IMAGE: 50 of the 57
> bins reach the `domain` facade behind their own scope list, so nothing about the deployables was
> closed here; see the 2026-08-11 entry above)*; the monolith consumers stay facade-coupled until
> they retire. Validate 0 errors / 43 warnings (kinds identical to baseline).
>
> ✅ **2026-08-07 — ADR-183024 REALIZATION STEP (2) MERGED — per-scope domain
> crates + kernel ([#373](https://github.com/TheCaptainCompany/captain-food/issues/373) "Domain
> splits into per-scope GENERATED crates; crate graph derived from spec $refs",
> [PR #381](https://github.com/TheCaptainCompany/captain-food/pull/381)).** The codegen emits one
> `domain-{scope}` crate per `specs/{scope}/` under `crates/domains/` (manifests GENERATED,
> `[dependencies]` DERIVED from the fragments' cross-scope $ref edges — currently a clean star:
> every scope → `domain-common`, the kernel, which depends on nothing); `crates/domain` became a
> re-exporting FACADE (same `domain::generated::*` paths, same type identity — zero downstream
> churn) keeping the cross-scope artifacts (DomainEvent union over the single log, global error
> catalog, states/lifecycles folds); `specs/generated/crate-graph.generated.json` commits the
> derived topology incl. each actor/PM bin's domain-crate links (PM bridges load-bearing:
> `pm-place-order` → ordering+payments+common) — step (3)'s bin-emitter input contract. HONEST
> LIMITS: kernel changes ripple every scope (correctly); cross-scope PMs rebuild on all their
> linked scopes; and until step (3) splits the bins, the facade still couples the monolith
> consumers to every scope — the pod-level blast-radius win lands with the bin crates + #363's
> determinator. Validate 0 errors / 43 warnings (kinds identical to baseline).
>
> ✅ **2026-08-07 — ADR-183024 REALIZATION STEP (1) MERGED — the spec reorg
> ([#375](https://github.com/TheCaptainCompany/captain-food/issues/375) "Spec reorg: specs/{scope}/
> folders + common, api/config fragments, scope validator rules, c4-l2 container split",
> [PR #376](https://github.com/TheCaptainCompany/captain-food/pull/376)).**
> The loader merges `specs/{scope}/{kind}.yaml` fragments into the logical catalogs (refs stay
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
