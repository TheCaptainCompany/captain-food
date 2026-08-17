# Dispatch — the walk: one order, end to end, on one database

- **Issues**: [#556 "Local acceptance harness"](https://github.com/TheCaptainCompany/captain-food/issues/556) then [#554 "Smoke L5 — acceptance lifecycle legs"](https://github.com/TheCaptainCompany/captain-food/issues/554)
- **Base**: `main` @ `7180274` (the card commit is the only diff from `c570cbd`)
- **Reversibility class**: **REVERSIBLE INTERNAL** for the artifact — a harness and a smoke script, no stored event shape, no production money movement, Stripe TEST only. The chunk's *verdict* is what the next fortnight is bet on, so four lenses briefed rather than two.
- **Merge posture**: auto-merge-on-green. Not `HOLD: human`.
- **Briefed**: `farley`, `beck`, `holub`, `ux-designer` — **all four declared a concern**, so all four are at the checkpoint.

> **Antecedent rule** ([ADR-20260817-105845](../adr/ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)). The first version of this card **broke the rule it opened by quoting** — `holub` caught it. Every figure below now names its source or is marked `UNVERIFIED input`.
>
> **This card was substantially wrong and has been rewritten.** Its target was wrong, its auth design was wrong, and its leg list could not have run. Read §0 first.

## §0 — What the briefing changed

| The card said | Reality, measured by a lens |
|---|---|
| Stand up the local k3s stack | **Impossible here.** Disk 4.7 GB free against a 9.6 GB DiskPressure threshold; `kubectl`/`k3s`/`sqlx` absent; `dockerd` not running; `kubectl logs` broken on that stack; and its runbook forbids `cargo build` while it is up — which is exactly the red-first loop. (`holub`, measured; `farley` concurs independently.) |
| A JWKS stub is the auth harness | **Wrong shape.** `SUPABASE_URL` has two consumers — the verifier's issuer string *and* the identity ACL that POSTs OTP/verify/admin. A JWKS-only stub verifies tokens and leaves registration unwalkable. (`farley`, `holub`.) |
| `accept delivery → delivered` | **Cannot run.** `DeliveryCompleted` transitions only from `PICKED_UP`/`OUT_FOR_DELIVERY`; the rider's real machine is accept → **confirm pickup** → delivered. As carded the walk throws `InvalidDeliveryStatus`. (`ux-designer`.) |
| "Delivered" | **Two different mutations.** Capture hangs off `OrderDelivered`, produced by a PM bridge from `DeliveryCompleted`. Calling `markOrderDelivered` directly green-lights capture while skipping the hop where it can silently never fire. (`ux-designer`.) |
| Six red smoke nights, cause unknown | **Resolved.** `farley` read the run logs: five distinct causes, and 2026-07-28 is not among the 19 (it precedes the last green and was already recorded). The unexplained set is now **zero**. |
| Replay assertion "if cheap" | **Cheap and stronger than assumed** — checkpoint reset, no tooling. Priced in §5. (`holub`, `beck`.) |

## Why this chunk

Founder decision, 2026-08-17 ([DECISIONS §45](../proposals/DECISIONS.md), [ADR-20260817-105844](../adr/ADR-20260817-105844-the-walk-goes-first-on-one-database-and-production-stays-suspended.md)): **walk first on one database; production stays deliberately suspended.** The acceptance criterion is unchanged as what *certifies*; it stops gating the first reading.

**No order has ever gone through this system end to end.** (`docs/STATUS.md` 2026-08-17 / PROD-1: zero real customer orders.) Last green nightly smoke **2026-07-29**, 19 consecutive red since — 13 attributable to the suspension, the rest now explained.

**The most useful thing the briefing produced**: the 2026-08-04 red died at `{"cart":null}` — *the cart never projected*, topology-independent, never diagnosed. That is this walk's **leg 2**. Expect it as the first red; do not read it as a harness bug.

## §1 — Target: a bare local process, not a cluster

`pg_ctlcluster 16 main start` (PostgreSQL 16 is installed, cluster `16/main` exists, down) → apply the migration chain → `cargo run -p server` → `/etc/hosts` mapping the literal `captain.food` names to `127.0.0.1` (required: host classification hard-codes the apex). No image, no cluster, logs on stdout, rebuild is re-run.

**Two settings carry over or the cheap target loses a real catch** (`farley`):
- **`APP_PROFILE=production`.** Development merely *prints* the config report and continues.
- **The full `deploy/generated/secret-keys.json` key set.** Note `SUPABASE_URL` is declared for production/staging only, deliberately — so a `development` walk silently gets the fail-closed identity service and 503s every role path. That is the "auth down vs harness misconfigured" ambiguity arriving by default.

**Hard constraint:** the same script must stay runnable against this target, k3s, and later MKS by changing the base-domain/scheme variables only. That is true today. Do not break it.

Honest non-proofs to record: local PG **16** vs CNPG **17.10** (`holub` grepped all 47 migrations for 17-only syntax — none), and the generated manifests are not exercised. The k3s rehearsal keeps its own separate job as the deploy-path reading, off this walk's critical path.

## §2 — Auth, and the divergence resolved

`farley` proposed keeping the real `SUPABASE_URL` (issuer and identity ACL stay real) and pointing only `SUPABASE_JWKS_URL` at a local stub — separate keys, `^https?://` accepted. Cleaner, and avoids a fake.

`holub` measured that **Supabase is unreachable from this environment**: `CONNECT tunnel failed, 502`, confirmed as a policy denial in the proxy's own failure list. Stripe and GitHub are reachable; Supabase is not.

**Deciding fact: the measurement wins.** `farley`'s design is the better one and it cannot run here. **But do not build a GoTrue fake either** — both lenses called that a shim, and a fake that mints convincing customers is precisely the thing that would make the walk's first leg *look* real while proving nothing.

**So:**
- Stub **only** the JWKS endpoint. The verifier stays unmodified and fail-closed. Tokens are locally RS256-signed with a `kid`, carrying `aud: "authenticated"`, `iss = {SUPABASE_URL}/auth/v1`, and the whole `app_metadata.captain_food` object — role plus domain id. That object is the only thing that yields a grant, and role matching is strict equality, so an ADMIN token cannot walk a `/restaurant` path.
- **`mint_token` gains a claims argument** so it stamps `restaurant_id`/`rider_id` as it already stamps `customer_id`, and **extends the decode-and-assert discipline to them**. Today only `customer_id` is asserted — so a `restaurant_id` that fails to reach the token surfaces as a mysterious accept failure rather than a named one. That assert exists because it already happened once.
- **The real-registration leg stays a declared RED.** Both `beck` and `farley` independently found it may be unwalkable regardless of the stub: phone-OTP has no admin API that hands you the code, and the honest mechanism is a Supabase test phone number, which is project configuration and may be admin-gated. **Attempt it, and if it cannot run, leave it red and named** — that is this card's own rule and it applies to its most important leg. The rest of the walk proceeds on a minted customer token.

`ux-designer` found what that red would otherwise hide: `CartBindingProcess` reacts to **`CustomerIdentified` only**, while a first-time phone emits `CustomerRegistered` — so *the first order a customer ever places may be the only one whose cart is never bound to them.* **Assert `CartBoundToCustomer` in `domain_events` after registration.** If it is absent, that is the walk earning its cost on leg two.

## §3 — Stripe

`stripe` is not installed; the binary is a GitHub release (reachable). Ordering is load-bearing and the first card omitted it (`farley`):

1. `stripe listen --print-secret --api-key sk_test_…` → the `whsec_`. **This must precede boot** — the webhook secret is a fail-closed boot gate with a shape check, so scraping it off the listener banner means booting with a placeholder and restarting.
2. Boot with that secret.
3. `stripe listen --api-key … --forward-to http://<host>:8080/adapters/stripe/webhooks` — host-independent route, no ingress.

Unattended works with `--api-key` (`stripe login` is browser-interactive). Make it fail loudly if the CLI falls back to a stored login rather than blocking on a prompt.

**Before the walk, fire a `stripe trigger` and assert the server saw it.** A positive liveness proof on the push path — the same discipline `mailbox_wake.rs` already applies. Without it, a tunnel that dies mid-run makes the walk hang at the authorization poll and report *"authorization never happened"*, sending the executor at the saga instead of the tunnel.

## §4 — The walk

Corrected sequence (`ux-designer`; the founder's sentence puts customer creation before the cart, but the cart is guest/session-keyed and binds at checkout, so the card's order is the right one):

`browse → add to cart (guest) → verifyPhone → [CustomerRegistered + CartBoundToCustomer] → placeOrder → Stripe confirm → PaymentAuthorized → OrderPlaced → restaurant reads its own orders → accept → ready → DeliveryJob → acceptDelivery → confirmPickup → completeDelivery → DeliveryCompleted → OrderDelivered → PaymentCaptured`

**`DELIVERY`, never `COLLECTION`.** Three lenses reached this independently. Capture-at-READY for collection is decided but unimplemented, so a COLLECTION walk cannot reach capture honestly and would pin the decided-against behaviour. `holub` adds the reason it is *under*-argued: `specs/payments/processmanager.yaml:12-14` still says `OrderDelivered` is the handover fact for **both** service types, contradicting the recorded decision — so an executor reading the source of truth would implement the wrong thing believing the spec backed them. **Treat that paragraph as known-wrong; file the SPEC-LOG correction separately, do not fix it here.**

**Call `completeDelivery`, not `markOrderDelivered`**, and assert both `DeliveryCompleted` and `OrderDelivered` before `PaymentCaptured`.

### The two non-negotiable assertions

1. **Assert on `domain_events` rows, and keep the read-model assertion alongside — not instead.** `beck` found this is a live defect, not a preference: the projector's `OrderPlaced` arm *writes* `payment_status = 'AUTHORIZED'` at birth, with no payment fact anywhere. So today's `paymentStatus == AUTHORIZED` proves *the order was born*, not *the authorization was recorded* — one safe indirection today, fully vacuous and silent the moment anything else births an order.
2. **The accept leg is performed by the OWNING restaurant's identity.** Never ADMIN, never the path role alone — otherwise the walk passes identically before and after the authorization fix.

### One addition both `holub` and `ux-designer` reached independently

**Immediately after birth, the owning restaurant reads its own `orders` and the new order is there.** That query *is* the kitchen reloading — the shipped product's real notification mechanism, degraded but real. Without it the walk skips the domain lens's own worst failure mode. It may also expose `orders` as one of #618's unscoped reads for free.

**And the accept leg is labelled `SIMULATED — no notification channel exists` in the transcript**, not footnoted. There is no new-order push; every subscription requires the identifier of the thing you are waiting to learn about. The walk's accept happens at T+0 because the script holds the order id, so its time-to-accept number is fiction — and that is the distribution the acceptance-timeout TTL is meant to be chosen from.

## §5 — Replay, as a separate final leg

Snapshot `to_jsonb(t)` for the row, delete it, delete the `Order` projection checkpoint, let the live projector re-drain, compare. **Byte equality on the whole row — a curated column list is the vacuous version.** Safe because every projector timestamp comes from the envelope, never the clock (`beck` verified).

Three conditions, all part of the leg rather than comments: **refuse to run if the table holds more than one row or the target is not local** (those statements against a populated database are a read-model wipe); it runs **after** the money legs; and its own red is deleting an `OrderAcceptedByRestaurant` row between snapshot and reset — `after` must differ.

States only that the Order projector is a pure replayable fold over this stream set. Not process-manager replay, not upcasting.

## §6 — Red-first, and what a fake red looks like

**A legitimate red is a semantic edit applied while the rest of the walk is green.** "The previous leg is not written yet" is a cascade, not evidence. Each leg's transcript shows the mutation, the preceding legs' PASS lines **in the same run**, and the FAIL line containing that leg's own assertion text.

> **A timeout is not a leg's red.** A timeout is compatible with three worlds — mutation detected, stack slow, previous leg never fired. Every new wait must carry a terminal three-way diagnostic naming which. The existing L4 timeout branch already does this; copy it rather than treating it as a special case.

Second fake red: the wait helper swallows the checker's stderr, so a broken `jq` filter is byte-identical to "state not arrived". New checkers must distinguish *query errored* from *state not yet*.

**The completeness check, and it is compiler-first-in-shell:** the run block is a flat list at the bottom of the file. Add a leg and forget one line, and the script prints ALL PASS. **Replace the flat list with a declared array driving the run loop, plus a final assertion that every declared leg reported a verdict.** Declaring a leg must *be* running it.

`beck` specified ten mutations, M1–M10, with expected failure shapes — take them from the briefing verbatim. Two notes: **M5** (mint the accept token as `RESTAURANT_ACCOUNT`) is the sharpest single mutation on the card. **M6** (a restaurant token whose claim disagrees with the payload) is **expected to PASS today** — record it as an observation naming the authorization gap; do **not** make it a gating assertion in either direction, because a green assertion there pins the decided-against behaviour.

## §7 — Fences

- **No flag flips.** All three default OFF and the chain still completes: birth-through-lane false means the saga appends `OrderPlaced` itself, the acceptance timeout is shadow-mode, the service-hours guard accepts in both positions. Stated so a careful reader does not fear leg 7 is fenced off.
- **Print the resolved configuration in the transcript.** One line; it makes the reading self-describing rather than a green tick nobody can later interpret.
- **Do not build the notification subscription**, do not fix the backoffice board (its five order buttons sit outside the iteration scope, all live regardless of status — record the finding with the ids the walk used), do not fix the impossible-stream behaviour test, do not fix the stale spec paragraph, do not touch `specs/**`. **Every defect the walk finds becomes an issue, not this chunk's diff.**
- **No browser. Do not restore production.**

## §8 — What a green walk does NOT prove

Put this in the report verbatim; the ADR's label is **reading, not certificate**.

No deploy path, image, TLS, DNS or ingress. Stripe TEST with one card: **no 3DS/SCA** — in France the common path, not an edge case — no decline, no partial capture, no dispute. **Nobody was told**: the accept leg is script-issued. One instance, no WAL archiving, no base backup, restore drill never executed against this stack. One order, one user: nothing about Friday peak. One database, so the boundary rows pass because the wall is not physical. Flags OFF, so this certifies the pre-flip configuration and the evidence does not transfer the day a default flips.

## §9 — Sequencing, WIP 1

**A.** Target up: Postgres, migrations, `cargo run`, hosts file. `/health`, introspection, **browse the catalog** — public, no token. *First walked leg, before any harness exists.*
**B.** JWKS stub; one role token verified by the unmodified verifier. **`holub`'s checkpoint is here** — the target and stub decisions are expensive to unwind and he wants to stop a wrong one at hour one, not leg three.
**C.** The chain, strict order, one leg at a time, red first.
**D.** Restaurant-visibility assertion, then replay. Stop.

**Finish line, testable:** one `DELIVERY` order walked browse-to-captured against one local Postgres, every leg asserted on its `domain_events` row with accept performed as the owning restaurant, and a pasted leg-by-leg transcript stating what the red looked like first — with any leg that cannot be walked left red and named, not routed around.

**In the same change**: the nightly smoke stops lying — re-pointed or its schedule disabled with the reason in the workflow file ([#620](https://github.com/TheCaptainCompany/captain-food/issues/620)). Landing a new reading while a 20th red night fires is the train-people-to-ignore-the-gate failure.

## §10 — Environment hazards, all paid for today

`check-drift` is a whole-tree `git diff --quiet` — it reports generated drift when your tree is merely dirty. Two worktrees sharing one `CARGO_TARGET_DIR` produce compile errors naming symbols that exist in your source; `cargo clean -p` the affected packages. After any `os error 28`, `cargo clean -p` whatever was mid-compile. Never pipe a gate into `head`. `gh` is absent; `GH_TOKEN` + `curl` on repository-scoped REST paths works, but some paths are proxy-blocked — after a 403, test one adjacent write before recording the scope, and name the path rather than "the API". **Verify any new test by name in the log, never by the suite total.**

## Findings

_(Lenses and the executor append here.)_

**Briefing, `7180274`** — all four declared a concern; all four are at the checkpoint.

- **`holub`** — target and stub shape, both decided in hour one and expensive to unwind. Checkpoint at end of phase B.
- **`farley`** — target choice, the auth wiring as actually built, the `stripe listen` liveness proof, the "does not prove" section, and the nightly fixed in the same change.
- **`beck`** — whether money-path assertions are on `domain_events` **and** kept alongside the read-model one; whether new timeouts carry the three-way diagnostic; whether the run block became declaration-driven. Found **no card errors** in the first version, which no other lens managed.
- **`ux-designer`** — the corrected leg sequence, `CartBoundToCustomer`, the SIMULATED label, and the backoffice board finding recorded rather than fixed.
