# Dispatch — the walk: one order, end to end, on one database

- **Issues**: [#556](https://github.com/TheCaptainCompany/captain-food/issues/556) (harness) then [#554](https://github.com/TheCaptainCompany/captain-food/issues/554) (L5 lifecycle legs)
- **Base**: `main` @ `c570cbd`
- **Card SHA stamp**: `c570cbd`. Load this card **at this SHA plus the diff since**. If the tree has moved in a way this card does not describe, **discard the card and read the tree**.
- **Reversibility class**: **REVERSIBLE INTERNAL** for the artifact — a harness and a smoke script, no stored event shape, no production money movement, Stripe TEST only. But the chunk's *verdict* is what the founder is betting the next fortnight on, so the briefing roster is four rather than two.
- **Merge posture**: auto-merge-on-green. Not `HOLD: human`.
- **Briefing roster**: `farley` (the release path is his standing question), `beck` (what the walk proves and what would make it theatre), `holub` (scope — this is the chunk he has been asking for, so his job is to stop it sprawling), `ux-designer` (the journey the walk must actually follow).
- **Checkpoint**: to lenses that declare a concern. Bank explicitly whether the narrow set missed anything.

> **Antecedent rule, now binding** ([ADR-20260817-105845](../adr/ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)): this card may not state a derived number without naming where it comes from, and any bare number it does state is **UNVERIFIED input**. It earned itself on the run that landed it — the previous dispatch said "~12 nights" of red smoke; the verified figure was **19 consecutive**, of which only 13 are attributable to the suspension. Treat every figure below accordingly.

## Why this chunk, now

Founder decision, 2026-08-17 ([DECISIONS §45](../proposals/DECISIONS.md), [ADR-20260817-105844](../adr/ADR-20260817-105844-the-walk-goes-first-on-one-database-and-production-stays-suspended.md)): **walk first, on one database; production stays deliberately suspended.** The acceptance criterion is unchanged as what *certifies* — local, eleven databases, full enforcement — it simply stops gating the first end-to-end reading.

The fact that makes this the right chunk and not a nice-to-have: **no order has ever gone through this system end to end.** Four legs were walked against a deployed stack until 2026-07-29; accept and delivered have never run above a handler; capture has never run through the product at all. Nine green lifecycle behaviour tests over nine hand-seeded streams do not compose into one walked order — one of them seeds a stream the real lifecycle cannot produce.

**Production staying down removes work rather than adding it.** No credential gate to build, no self-service signup to close, no live surface to protect. The walk needs a stack, not a hostname.

## What exists already — do not rebuild it

- **The stack ran.** A single-node k3s with the CNPG operator, the full migration chain, the generated monolith overlay, `/health` 200 and smoke L1+L2 passing, on 2026-08-11. Overlay: `deploy/platform/local-rehearsal/`. Runbook: `docs/runbooks/cutover-local-rehearsal.md`. Its own runbook warns against a concurrent `cargo build` — heed that.
- **The smoke script is re-pointable** via its base-domain / public-base variables. **One script, one pipeline** — do not grow a second walker.
- **`mint_token` already works** and already stamps and asserts a `customer_id` claim on the issued token, decoding the JWT to check it. That discipline is the model for the rest.
- **The webhook question is settled and the records disagree with each other** — one 2026-08-11 line still calls it an open blocker; two records two days later resolved it: the Stripe CLI's outbound tunnel plus its own signing secret reaches a local stack, so **no inbound ingress is required**. Nothing in the repo wires it yet. That is scripting, not infrastructure.

## Scope

### Part 1 — the harness (#556)

1. **`mint_token` gains a claims argument** so it stamps `restaurant_id` / `rider_id` exactly as it already stamps `customer_id`. Keep the decode-and-assert discipline for the new claims.
2. **A local issuer + JWKS stub**, so the fail-closed verifier is exercised unmodified. The verifier is env-configured, accepts an `http://localhost` issuer, and restricts algorithms to asymmetric families — so serve RS256 with a `kid`. **Zero verifier changes; no gate weakened.** One trap: the development profile does not *require* the keys, so an unset stub 503s every role path and reads as "auth is down" rather than "the harness is misconfigured" — make that failure say which it is.
3. **`sk_test` / `pk_test` wiring** and the `stripe listen` invocation with its `whsec_` handling.

### Part 2 — the walk (#554), red first

Every leg seen **red before green** — that is the recorded method for this work, not a preference.

`browse → add to cart → register a real customer → place order → Stripe confirm → authorized → order born → accept → ready → delivery job exists → accept delivery → delivered → captured`

**Use `DELIVERY`, not `COLLECTION`.** Three lenses reached this independently and it is the single most important instruction on this card. The current L4 smoke places a COLLECTION order, and capture-at-READY for collection is **decided but unimplemented** — so a COLLECTION walk cannot reach capture honestly, and whoever writes it first will capture on delivered and **pin the exact behaviour the register decided against.** A test that certifies a decided-against behaviour is worse than no test.

**Customer creation must be real.** Today the smoke fabricates a UUID and stamps it as a claim; no genuine `CustomerRegistered` is produced anywhere in the repo, at any level. Register through the real phone-verification path so the first leg stops being fiction.

## The two assertions that are not negotiable

Both from `beck`. Without them the walk can go green while the product is broken.

1. **Assert on `domain_events` rows, not read-model rows.** A projection that fabricates a default satisfies a read-model assertion — that exact class has already been caught once in this repo. The walk asserts the *fact was recorded*, then separately that the projection reflects it.
2. **The accept leg is performed by the OWNING restaurant's identity — never ADMIN, never the path role alone.** If the walk accepts as ADMIN it passes identically before and after the authorization fix, which makes it a green end-to-end test that is blind to the largest known defect in the system.

Add, if it is cheap: **replay the stream at the end and assert the read model converges.** That is the CQRS claim the whole architecture rests on and nothing currently proves it end to end.

## Fences

- **Do not flip any flag.** Order-birth-through-lane, the acceptance timeout and the service-hours guard are all OFF by default. Walk the default configuration — the walk's job is to report what the shipped default does, and flipping a default is a separate recorded decision.
- **Do not build the missing leg.** There is **no** "a new order arrived for this restaurant" subscription anywhere in the repo — the kitchen learns by reloading. Name it in the report as a missing leg, do not fill it here; it is a product decision with a design attached.
- **No browser.** The founder's chain does not mention one, and the four recorded browser walls are their own chunk with their own unknowns.
- **Do not restore production**, or point anything at `live.captain.food`. Its suspension is now a recorded decision, not an outage.
- **Do not touch `specs/**`** unless the walk proves a spec is wrong — in which case stop and say so rather than editing.

## Gates and evidence

- `make rust` 0 errors; `make test-crates` no new failures.
- **The walk itself is the evidence.** A pasted transcript of the run, leg by leg, with the assertion that fired at each — plus, for every leg, whether it was seen RED first and what the red looked like.
- **Verify any new test by name in the log, never by the suite total.** Recorded twice today at real cost: a total dropping by one reads as noise, and a stale cached test binary can silently not run a new test while the suite reports green.
- If a leg cannot be walked, **say which and why** and leave it red rather than routing around it. A walk with an honest gap is worth more than a complete one with a stub in it.

## Known environment hazards, all paid for today

- **`check-drift` is a whole-tree `git diff --quiet`** — it reports "generated artifacts drifted" when your own tree is merely dirty. Read the stat, not the exit code.
- **Two worktrees sharing one `CARGO_TARGET_DIR` produce compile errors naming symbols that exist in your source.** Fix: `cargo clean -p` the affected packages.
- **After any `os error 28` (disk full), `cargo clean -p` the packages that were mid-compile** — a truncated artifact is judged fresh and fails semantically later.
- **Never pipe a gate into `head`** — it can truncate the run and report a partial pass.
- **`gh` is not installed**; `GH_TOKEN` + `curl` against repository-scoped REST paths works. Some paths are blocked by the agent proxy — after a 403, test one adjacent write before recording the scope of the block, and name the path rather than "the API".
- Disk is the binding constraint. `CARGO_INCREMENTAL=0`, and sweep `target/debug/incremental`.

## Findings

_(Lenses and the executor append here. "Nothing in my lens" is a complete answer.)_
