# Status journal — 2026-W32

Journal entries for ISO week 2026-W32, newest first, in the order they were written.
Current state: [`../STATUS.md`](../STATUS.md).

> ✅ **2026-08-09 (morning) — THE EIGHT-DECISION BRIEF IS ANSWERED; the demo is deferred and the
> target is now production-with-test-data** ([ADR-20260809-050000](../adr/ADR-20260809-050000-morning-brief-eight-decisions.md)).
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
> [ADR-20260809-212810](../adr/ADR-20260809-212810-verify-phone-claim-stamp-posture.md)).**
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
> [ADR-20260809-200826](../adr/ADR-20260809-200826-scope-membership-member-naming.md)).**
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
> [ADR-20260809-160000 addendum 2](../adr/ADR-20260809-160000-read-authorization-lands-ported-from-152.md).**
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
> [ADR-20260809-160000](../adr/ADR-20260809-160000-read-authorization-lands-ported-from-152.md);
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
> in [PROP-20260809-021351](../proposals/PROP-20260809-021351-public-demo-one-continuous-walk.md) §2:
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
> [DECISIONS.md §24](../proposals/DECISIONS.md).
>

> ✅ **2026-08-09 — [#335 "Decide whether to consolidate integration test binaries (~3.5G of link products)"](https://github.com/TheCaptainCompany/captain-food/issues/335): `crates/infrastructure`'s 27 integration binaries consolidated into ONE (`tests/main/`, 1.4G → 70M of link products) behind a compiler-enforced `common::TestDb` witness (binary-wide lock + ONE migration-derived `reset_schema`), per ADR-20260808-224500 item 5 — which immediately surfaced and fixed a real spec↔migration drift: `catalog.slug` was still NOT NULL in production migrations while the generated schema and the projector have it nullable (`migrations/20260809000000_catalog_slug_nullable.sql`).**

> ✅ **2026-08-09 — #348 CUSTOMER-ANXIETY QUICK WINS APPLIED
> ([#424 "Customer-anxiety quick wins: DeliveryPickedUp reaches order tracking, checkout shows a FAILED state (approved spec diff, option b)"](https://github.com/TheCaptainCompany/captain-food/issues/424)),
> per the exact-text approval in [ADR-20260809-002500](../adr/ADR-20260809-002500-quick-wins-approved-d6-dsl-extension-chosen.md)
> realizing [PROP-20260808-233000](../proposals/PROP-20260808-233000-customer-anxiety-quick-wins-spec-diff.md).**
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
> APPLIED TO MAIN BY THE RUN ([ADR-20260808-230800](../adr/ADR-20260808-230800-rider-delivery-slices-1-2-approved-and-applied.md)).**
> All five answers were the recommended options: §2 as written (applied, full `make rust` gate,
> expected 43 → 37 warnings), §3.2 `sends:` approved (lands with the D6 validator mechanism),
> both customer-anxiety quick wins pulled forward (diff being prepared), slices 3–8 filed in the
> parent proposal's value order, apply-now vehicle chosen explicitly over §6 plan-mode.
>

> ⏳ **2026-08-08 (night) — #348 SLICES 1–2 SPEC DIFF PREPARED, AWAITING CUSTOMER APPROVAL —
> [PROP-20260808-221424](../proposals/PROP-20260808-221424-rider-delivery-slices-1-2-spec-diff.md)
> (autonomous run; `specs/**` untouched).** The exact per-file diff realizing the approved
> [PROP-20260808-141817](../proposals/PROP-20260808-141817-rider-delivery-write-surface.md) slices 1–2:
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
> via [ADR-20260808-062933](../adr/ADR-20260808-062933-one-bin-per-worker.md) (product-owner
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
> via [ADR-20260808-062432](../adr/ADR-20260808-062432-one-bin-per-adapter.md) (product-owner
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
> [ADR-20260808-063951](../adr/ADR-20260808-063951-cnpg-platform-source-tree.md) (hand-written
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
> [ADR-20260808-060309](../adr/ADR-20260808-060309-bare-apex-owner.md) (apex → marketplace,
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
> [ADR-20260807-231754](../adr/ADR-20260807-231754-bin-runtimes-composition-kit-scoped-config.md).**
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
> [ADR-20260807-223428](../adr/ADR-20260807-223428-build-matrix-determinator-gate.md)).** CI learns
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
> [ADR-20260807-220528](../adr/ADR-20260807-220528-deploy-emitter-pins-are-input.md)).** The emitter
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
> ([ADR-20260807-183024](../adr/ADR-20260807-183024-one-decomposition-axis.md), D1–D8 with D2/D8 in
> their product-owner-revised forms; critical-path-growth accepted knowingly).** Final shape:
> `specs/{scope}/` folders + common (8 scopes) · **`captain-core`** (log+mailbox, ALL backup budget)
> / **`captain-views`** (per-scope projection schemas, NO backups — restore is replay) ·
> per-scope projectors over the single log · **`graphql-{scope}` services + a boring generated
> gateway per role** (top-level routing from a codegen composition table; nested types intra-scope
> by validator rule) · per-scope configuration. Three standing reviewers now exist: `architect`
> (microservice/actor lens), `dba` (Postgres/food-service), `graphql-architect` (API composition).
> **Realization order** (ADR consequences): spec reorg → #373 crates → bin crates → #349 emitter →
> #363 build matrix → core/views in #360 → #358+#361 with the product owner live. Was:
> ([PROP-20260807-174246](../proposals/PROP-20260807-174246-one-decomposition-axis-specs-schemas-projectors.md),
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
> decisions + a critical-path-growth concern open in [DECISIONS.md §18](../proposals/DECISIONS.md).

> ⏳ **2026-08-06 (later) — THE DESTINATION IS REOPENED FOR KUBERNETES
> ([PROP-20260806-223656](../proposals/PROP-20260806-223656-kubernetes-as-the-deployment-substrate.md),
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
> ([ADR-20260807-002705](../adr/ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md), superseding the
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
> ([ADR-20260806-151122](../adr/ADR-20260806-151122-hosting-destination-is-clever-cloud-not-ovh.md),
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
> **Consequence**: [PROP-20260805-181926](../proposals/PROP-20260805-181926-host-provisioning-and-configuration-ownership.md)
> is **mostly moot** — D1–D6 have no subject without a host we own, **only D7 survives**, and D3
> (SaltStack) is settled by construction. **One blocking precondition before any spend**: whether
> Clever Cloud meters **egress** the way Render did. Render's outbound-bandwidth exhaustion is one of
> the incidents that started this migration, and repeating it on a new PaaS is the single way this
> decision fails. **✅ That blocker CLEARED 2026-08-06: Clever Cloud includes 10 TB/month egress at no
> charge** — orders of magnitude above what the WASM bundle plus GraphQL can produce at V0 peak (get it
> in writing before it is load-bearing). **But object storage is a separate meter**: Cellar egress is
> **EUR 0.09/GB**, so the planned file-attachment framework
> ([PROP-20260725-120055](../proposals/PROP-20260725-120055-generic-file-attachment-framework.md)) —
> restaurant and menu **photographs**, in an image-heavy marketplace — is the Render bandwidth failure
> returning through a different door unless where images are served from is decided deliberately.
> **Remaining before purchase** (all on the ADR): the estimator's cheap selection is **under-specced**
> (`pico` = 256 MiB, `XXS Small Space` = 1 GiB disk / 512 MiB / 45 connections — the latter barely
> above the Supabase free tier being escaped); pick the **Docker runtime, not `Rust`**, or the platform
> compiles the workspace on every deploy and digest pinning dies with it; and declare the sqlx pool
> ceiling against the 45-connection limit. Prices/specs come from the vendor estimator only — a
> third-party spec table already produced wrong VPS-2 figures once (corrected 2026-08-05).

> 📋 **2026-08-05 — Who owns the OVH host: provisioning IaC + host configuration
> ([PROP-20260805-181926](../proposals/PROP-20260805-181926-host-provisioning-and-configuration-ownership.md),
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
> blocks `Approved`. **Seven** open decisions in [DECISIONS.md §16](../proposals/DECISIONS.md).
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
> ([ADR-20260804-154700](../adr/ADR-20260804-154700-screen-actions-are-checked-against-their-command-inputs.md))**.
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
> ([ADR-20260804-041227](../adr/ADR-20260804-041227-populate-the-two-dead-columns-and-address-refund-facts.md))**.
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
> ([ADR-20260804-032640](../adr/ADR-20260804-032640-delete-unread-read-models.md))**, product-owner
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
> ([ADR-20260804-014546](../adr/ADR-20260804-014546-read-models-declare-their-readers.md))**: the READ-side
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
> (PROP-20260802-130500 phase 2, [ADR-20260803-214500](../adr/ADR-20260803-214500-actor-door-contains-the-phase-2-widening.md))**:
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
> ([ADR-20260803-203455](../adr/ADR-20260803-203455-mailbox-doors-are-declared-by-reachability.md))**:
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
> (PROP-20260802-130500 §5 directive, [ADR-20260803-172654](../adr/ADR-20260803-172654-mailbox-port-demands-a-capability-witness.md))**:
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
> ([ADR-20260803-143216](../adr/20260803-143216-admin-requeue-rides-the-mailbox.md))**: operator
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
> [ADR-20260802-170059](../adr/ADR-20260802-170059-client-surface-is-spec-gated.md)); measurement
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
> [ADR-20260803-104819](../adr/20260803-104819-db-persisted-pm-mailbox-delivery-posture.md))**: the
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
