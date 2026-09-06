# ADR-20260906-192007 — Slice 3b and the command change land as expand/contract behind an interlocked write door, with the refusal set enumerated

<!-- Filename: docs/adr/ADR-20260906-192007-slice-3b-and-the-command-change-land-as-expand-contract-behind-an-interlocked-write-door-with-the-refusal-set-enumerated.md -->

## Status

Accepted — a **team decision by consent** under
[TEAM-DECIDES-OPTION-SPACES](../decisions/TEAM-DECIDES-OPTION-SPACES.yaml)
([ADR-20260904-013834](ADR-20260904-013834-the-team-decides-option-spaces-and-spec-diffs-external-legal-and-admin-gated-actions-stay-with-the-founder.md)):
the whole roster was briefed before any code (`briefing-816-s3b4.md`, dispatched 2026-09-06 19:08Z —
`HOLD: human`, money movement, a non-additive GraphQL input change on a shipped money mutation, a
legal surface), fourteen lenses answered in full
(`briefing-816-s3b4-answers.md`) — **every one declared CONCERN** — the splits are resolved below by
the recorded rules, and the founder reads this record. This record is written **before the card**
that dispatches it, per the founder's own WIP-one condition (holub, journal 2026-09-06): nothing
else opens in this lane until it is demonstrable behind the door on the walk. Realizes **slice 3b +
the command change** of [PROP-20260831-134539](../proposals/PROP-20260831-134539-priced-quote-token.md)
§11 item 4, as **ONE deliverable**, on
[#816 "Display/charge divergence is undetected: the expectedTotal equality check never runs in
production"](https://github.com/TheCaptainCompany/captain-food/issues/816) — reopened at this
briefing after a stale linked-issue association had closed it (architect, 2026-09-06 19:01:43Z;
`commands.rs:2855` still reads `expected_total`, `commands.yaml:141-149` still declares
`expectedTotal`, so the launch-blocker count reading 0 was the artifact of a squash body carrying no
closing keyword, not of the work being done).

## Enforced by

No code lands with this record — it is written to exist **before the card**, so nothing here is
"enforced" yet. What the card that dispatches this deliverable must write, and this record commits
it to: `rules.yaml#/ServerPriceAuthority` rewritten in the same change to state the expand/contract
door and the enumerated refusal set (D-A, D-D) rather than the `expectedTotal` equality claim it
carries today; a new door-interlock test pinning that `RUN_QUOTE_REQUIRED_ON_PLACE_ORDER` may only
be `true` when `RUN_FOLD_PRICED_CART_READ` is `true`, refused at startup otherwise (D-B); the
DB-gated `crates/server/tests/quote_walk.rs` suite — the walk itself plus beck's (a)–(j) — run in the
lab with the write door forced ON (D-F, D-J); `crates/telemetry/src/contract.rs`'s alternation test
for `quote.verify` (D-I); the `deprecated:` emitter's own codegen test (D-C); the CatalogVersion
zero-input-blocks-and-zero-field-arguments test in its own `tools/codegen-rs/src/validate/` module
(D-L). Each is named here so the card cannot narrow scope without reopening this record.

## Context

[#816](https://github.com/TheCaptainCompany/captain-food/issues/816) is the display/charge
divergence: a paid order's charge and its displayed price can diverge with nothing detecting it.
Slice 3a ([ADR-20260906-154419](ADR-20260906-154419-the-priced-read-is-served-from-the-fold-and-carries-its-coordinate-behind-a-door.md),
merged [PR #922](https://github.com/TheCaptainCompany/captain-food/pull/922)) gave the cart READ a
fold-priced arm behind a door and minted a coordinate that is used and immediately dropped — "3a
alone is inventory" (holub): it closes nothing on #816 by itself. PROP §11 item 4 is the next chunk:
the opaque `quote` word, the signing mechanism, and the command change (`quote` required,
`expectedTotal` removed) that together make the coordinate a **held commitment** rather than a value
computed and discarded. The 2026-09-06 briefing put questions to fourteen lenses; every one declared
CONCERN, and eleven distinct **card defects** were banked before this record closes them (listed at
the end of the Decision section, each attributed `card defect`).

The central tension the briefing surfaced and this record resolves: PROP §11 item 4 as written says
`quote` required, `expectedTotal` removed, **in one change** — but an env-gated door cannot un-remove
an already-shipped GraphQL SDL field (farley, graphql, reviewer, beck, business — independently, the
same finding). Removing `expectedTotal` while an already-loaded client bundle still sends it, and
before every client sends `quote`, breaks checkout for that client outright — not gated by anything a
door can flip. The record below is the reconciliation: expand/contract (ADR-0043's discipline,
extended here to a GraphQL input for the first time), never a same-change field swap.

## Decision

### D-A — Expand/contract on the shipped money input

[ADR-0043](0043-db-migration-release-strategy.md)'s expand/contract discipline is **extended to a
shipped GraphQL input** — a new decision, recorded here, not a citation of ADR-0043 having already
said this about GraphQL. `quote: QuoteToken` is added **nullable** to the SDL now (`PlaceOrderInput`
and `Cart`); `expectedTotal` is **kept**, nullable, marked `deprecated:` (D-C) and **ignored** by the
server. The **write door is CLOSED** by default: `quote` is structurally accepted and **never read**
— not "verified when present" (farley: that reading would make rollback restore a mixture keyed on
the client's own version, never a clean revert) — every order is priced and charged **at HEAD**,
which is exactly today's behaviour. A quote present while the door is closed is **ignored**
(business, beck). The **write door OPEN**: an absent `quote` is refused at the **handler**, typed,
never as a GraphQL non-null violation; a present `quote` is verified; a mismatch produces exactly
**one** refusal (D-D). Requiredness is enforced by the handler behind the door, **never by SDL
`!`**: the non-null flip is its own separate, later recorded step, gated on a cache-drain condition
and the checkpoint metric `place_order_quote_present_total{present}` reaching 100% (farley). Deleting
`expectedTotal` is likewise a **separate, later recorded change** (the contract half) — not this
deliverable's.

Mint runs in **both** read arms regardless of the write door's position (business's original framing
was wrong, corrected at the briefing): minting requires the fold coordinate, and with the read door
(`RUN_FOLD_PRICED_CART_READ`) OFF there is no coordinate to mint from, so `quote` is `null` on the
read in that state. The write door therefore **cannot** be open while the read door is closed — D-B.

### D-B — The two doors are interlocked, not merged into one key

Two keys, because their flip positions differ in time (the read door may open before the write door;
the write door must never open before the read door): `RUN_FOLD_PRICED_CART_READ` (3a, unchanged) and
the new write-side door `RUN_QUOTE_REQUIRED_ON_PLACE_ORDER` (`type: bool`, `runKind: door`,
`decisionRow: QUOTE-MINT-PRECONDITIONS`, `deploy.production`/`staging` `false`), declared at both
composition roots per the standing clause of
[ADR-20260904-081527](ADR-20260904-081527-rider-standing-is-a-grant-on-the-identity-row-the-doors-are-human-only-and-step-4-lands-in-three-slices.md)
§8. **A stated interlock, not a shared key** (young's card defect, resolved): the write door may only
be `true` when the read door is also `true`; startup **refuses** the mixed state (compiler-first per
ADR-20260803-234035 — the write door's witness is *constructed from* the read door's witness, so the
mixed state is unrepresentable rather than merely checked), pinned by a test. Without this, the four
fleet states include one — mint from the fold, verify from the projection — that is a mismatch storm
under ordinary projector lag: the mutant shipped as configuration, in young's words.

### D-C — The `deprecated:` emitter key

Async-graphql's `InputObjectField` already carries a `deprecation` attribute; `api.yaml` has never had
a way to say so. This deliverable adds the emitter key `deprecated:` (a string reason, optional),
consumed by `emit_server_inputs`/`emit_sdl`, applied to `PlaceOrderInput.expectedTotal`. One emitter
branch, one codegen test (D-C's test: a field marked `deprecated:` in the source YAML must appear
`@deprecated` in the generated SDL; a field not so marked must not). This closes graphql's blocker on
D-C: keeping `expectedTotal` in the SDL, deprecated, was previously unwritable.

### D-D — The refusal set, enumerated

Evans's and young's finding (independently, and the load-bearing one of the whole briefing): "ONE
refusal when the quote does not verify OR the fold disagrees" was fusing **three different facts**
that must not share one word. Enumerated, so the card cannot re-collapse them:

1. **Structural rejections** — forged signature, a quote minted for a foreign cart used on this one,
   a retired `keyId`, or a coordinate `V` beyond the stream's own head (not forgery: it means OUR
   history moved backwards — PITR, re-seed, or a read routed to a lagging replica; no replica routing
   exists today, `deploy/platform/README.md:87`, so this is recorded as a FENCE, not a live risk).
   These reject on the ADR-20260810-112836 §6 path, exactly as an unresolvable fold does today:
   **loud in telemetry** (an attack signal, its own bucket), and the customer sees the **same
   cause-neutral screen** as every other refusal here — **no consumer price copy**, ever, on this
   arm.
2. **`QuoteNoLongerHonoured`** — the ONE business error, `context: { cartId }` only, for exactly two
   causes: the cart was edited after the quote was minted (the token binds `cartId` **and** the
   cart's own stream version, so a cart edit is a **structural** mismatch the moment the versions
   differ — young: "cheapest fix: bind the cart version into the token, and the business branch
   collapses to a structural check"), and expiry (`QUOTE-STALENESS`, N = 30 min). Quiet in
   telemetry (`DomainError::Rejected`), same cause-neutral screen.
3. **Fold failure** (the coordinate the quote names cannot be resolved at verify time) —
   `technical_error`, exactly the existing unresolvable-at-read classification
   (ADR-20260810-112836 §6), never HEAD, never the projection.

`PriceMismatch` (`specs/ordering/errors.yaml:250-262`) is **deleted, with its `tests.yaml`
coverage** — not repurposed (evans: repurposing it would leave a fossil of the exact #816 kind, a
name whose copy ("Prices have changed…") is false for the structural causes above). The **one**
customer-facing screen is ux's cause-neutral draft, quoted here as a **DRAFT for counsel** (never
clearance, CQ-7 below): en — *"We couldn't confirm your total. Your card was not charged and no
authorization was taken. Your basket is intact — open it to see current prices and order again."*;
fr — *"Nous n'avons pas pu confirmer votre total. Votre carte n'a pas été débitée et aucune
autorisation n'a été prise. Votre panier est intact — ouvrez-le pour voir les prix actuels et
commander à nouveau."* MUST NOT: "the price changed", any amount, an old→new pair, a delta, a
currency figure, a countdown, refund/bank-date language. The CTA reuses
`checkout.payment_failed.back_to_cart` — no new CTA, no retry control.

### D-E — `totalCents` scope: the token signs the catalog-lines total, never the delta

The token signs the catalog-lines total (D1's `totalCents`), not the full CTA number the customer
sees (lines + live fees). The write side **refuses** when the displayed CTA total does not equal
lines-total (from the token) plus live fees (recomputed at verify time) — it **never** charges the
delta. This is the temporal-coherence half of legal's finding: the guarantee (Code de la
consommation L112-1/L221-5 posture) binds the **total** shown before the consumer is bound, and this
deliverable's card must state the sentence above **before** any test is written against it
(legal's red-first: `the_charge_never_exceeds_the_total_the_customer_was_shown`).

### D-F — Where verify runs

Inside `application::commands::place_order`'s pre-payment guard block, **strictly before** the Stripe
`PaymentIntent` creation call — the same placement reasoning as the existing `ENFORCE_SERVICE_HOURS_GUARD`
guard (`specs/ordering/processmanager.yaml:57-58,78-79`: a refusal after intent creation strands a
Stripe hold). Verify folds `as_of(catalogId, V)` — **never** `at_head` (vernon, young: `at_head`
would reopen the exact mixed-authority mutant this whole design exists to close). The projection may
only **subtract** (veto via `PriceUnresolvable`-shaped refusal), **never add** a price, a tax rate or
an acceptance the fold itself did not produce — the same "projection is a veto, never a source"
sentence D2 of ADR-20260906-154419 already states for the read side, restated here for the write
side (evans, young).

### D-G — The seventh carve-out, cross-referenced

Recorded in the companion amendment below (item 2 of this dispatch), in
[ADR-20260904-081527](ADR-20260904-081527-rider-standing-is-a-grant-on-the-identity-row-the-doors-are-human-only-and-step-4-lands-in-three-slices.md)
§8 — **on `main`, before the branch** (reviewer, vernon: the fourth and sixth carve-outs were both
recorded *after* card-directed touches; the seventh is recorded *before*, closing that recurring
gap). It licenses exactly: one additive `steps:`/`guard:` entry in `processmanager.yaml`'s
`PlaceOrder` chain (the `:93` `PriceMismatch` guard is REPLACED by the quote-verify guard plus its
`throws:` `$ref`, strictly before the payment call — structural, not prose); one added argument at
the `PlaceOrder` arm in `crates/infrastructure/src/mailbox/pm_delivery.rs` (the only production call
site of `application::commands::place_order`), in the `enforce_service_hours_guard` shape (the second
carve-out's own precedent, PR #885); and — because D-H makes the key a **configuration key**, not a
port — the matching `CommandDeps` field in `crates/infrastructure/src/inbox.rs` and its construction
in `crates/infrastructure/src/mailbox/standalone.rs` reduce to that same second carve-out's shape
rather than needing a new one. The verify logic itself lives in `commands.rs`, **unfenced**
(vernon, reviewer, architect — independently, the same finding): the fence's `process_managers/**`
path holds only the `PaymentAuthorized`/`PaymentFailed` **event** legs, never the `PlaceOrder`
**command** leg, so the brief's claim that this deliverable "will need
`crates/application/src/process_managers/**`" is itself the first banked card defect (vernon).

### D-H — The key is a declared secret configuration key

The signing key copies `EMAIL_QUOTA_KEY_HMAC_SECRET` (`specs/common/configuration.yaml:1102-1124`)
**key-for-key**: `type: string`, `secret: true`, `required: [staging, production]` (no default — a
defaulted signing key is a forgeable price, dba), boot exits 78 on MISSING in staging/production via
`Config::resolve()`/`must_stop_on_problems()`. A fixed `DEV_ONLY` value is used in dev/test, exactly
the `DEV_ONLY_HMAC_KEY` precedent (`application::email_guard`). A `SigningKey` newtype with a
**module-private constructor** (the `ActingRole` shape, ADR-20260803-234035 level 4) makes the dev
value uncallable from the production construction path; the write door **refuses at boot** when the
resolved key is the dev value and the door is open in staging/production — never a per-request check.
`QuoteVerifier` holds a **key SET** (plural — it must accept a quote signed under a key that has since
been retired but is still inside the overlap window); `QuoteMinter` holds **one current key** (two
distinct types, so a minter cannot accidentally verify and a verifier cannot accidentally mint). A
two-key overlap of **60 minutes** (`N` + skew, `N` = the existing 30-minute `QUOTE-STALENESS`
backstop) is a **precondition of this deliverable**, not of a later rotation: without it, a hard
key-rotation cutover rejects every quote minted in the preceding 30 minutes platform-wide (business:
UNVERIFIED-Little's-law estimate ~15–25 live tokens/restaurant, ~500–750 across ~30 restaurants at
20:30 — the shape, not a costed figure). Key **custody** is admin-gated, the founder's external list
(not a crypto-shredding key — nothing personal is encrypted by it); **rotation policy** is the
team's, recorded here as: single key + one live `keyId` is acceptable at V0 provided the overlap
above is observed on any rotation.

### D-I — Observability

`quote.verify` is an **INTERNAL** span **inside** `place-order.spans`, never a sibling contract,
constructed at the `QuoteVerifier` adapter / command's guard boundary, beside
`command.validate`/`cart.read`; late-bound attributes only: `business.quote_outcome`,
`business.version`, `business.quote_key_id`, `business.quoted_total_cents`,
`business.failure_reason`, `otel.status_code` — **never** token bytes or the signature. The **door
alternation** lands in the same change: door OFF ⇒ `quote.verify` never fires; door ON ⇒
`required: true` — a `required: false` verify span with `place-order.status_rules.success` unchanged
is a **STOP** (observability: a run that never verified would score `success`, the #588 lie
repeated). `quote_mint_total{outcome}` (on the read, `minted|key_unavailable|sign_failed`) and
`quote_verify_total{outcome}` (`accepted|cart_changed|expired|tampered|unresolvable` — young's five
outcomes) are **CLOSED** outcome sets, each mapped in `status_rules` **before** the flip. A `surface`
dimension (`read|verify`) is added to `catalog_as_of_fold_ms` and `catalog_as_of_reads_total` — a
contract-row change on an already-landed row, a SPEC-LOG sentence, no migration. `business.correlation_id`
is recorded on `quote.verify` (the only join from mint to charge). `keyId` is a **span attribute**,
never a metric label (no bounded population exists until rotation policy is decided, D-H). The mint
is an **attribute** on `catalog.as_of.fold` (`business.quote_key_id`), never its own span. **Dead-man**:
`quote_verify_total` alone cannot fire on silence (silence is ambiguous); the pair that starves
together is `quote_mint_total{outcome}` on the read plus the per-run invariant "every `place_order`
success carries exactly one `quote.verify`" — a HEAD-fallback regression shows up as
successes-without-verifies, the structural detector D4 needs. **No alert-routing surface exists in
the repo today** (one prose mention, `delivery/configuration.yaml:75`) — recorded plainly: the
counter **existing** is not the same fact as someone being **paged**; a routing spec is a separate
option space, not opened here.

### D-J — The client renders the refusal; the walk's home; the /cart→/checkout binding

**The client renders the refusal, or the sentence is only half-built** (holub's stop condition, ux's
GAP). `order_tracking` gains `operationStatus.byMessage` plus one refusal `conditional_section`; the
refusal is **post-enqueue** and today's client navigates on acceptance regardless
(`crates/web/src/tracking.rs:414` shows "Reçu ✓ — confirmation en cours…" with **no terminal path on
`REJECTED`**) — a refused order shows false reassurance forever without this. The terminal path lands
in `crates/web/src/tracking.rs`. Copy keys: the message in `errors.yaml` (D-D), the screen sidecar
in `restaurant_frontoffice.translations.yaml` — **two strings, two jobs** (ux's corrected card
defect: the brief's cited `specs/screens/customer.yaml` does not exist; the customer surface is
`specs/screens/restaurant_frontoffice.yaml`, `cart` at `:399`, `checkout` at `:466`).

**The walk's home is the DB-gated `crates/server/tests/quote_walk.rs`**, beside
`rider_standing_walk.rs` / `platform_admin_walk.rs` — **`tools/walk/` is NOT on main** (0 paths at
`origin/main`; it exists only on the unmerged `origin/556-local-walk-harness`, tip 2026-08-17). The
walk runs **inside this PR**, door forced ON in the lab: one cart, one signed quote, one tampered
token, one refusal screen — a transcript in the PR body is the evidence bar (farley, holub); a
process restart proves the same quote still verifies (beck (d)). Register item (3) is corrected in
the same change (item 4 of this dispatch).

**The /cart→/checkout binding**: the quote submitted by `place_order` must be the quote of the read
that **painted the recap total on `/checkout`** — never a quote stashed at `/cart`. One binding
sentence at `restaurant_frontoffice.yaml:72` plus one test (ux). The /cart→/checkout **seam** itself
(two reads, two mints, two moments) is **recorded OPEN**, never claimed closed by this deliverable —
this is the antecedent [#930 "Priced quote token slice 3a follow-ups"](https://github.com/TheCaptainCompany/captain-food/issues/930)'s
item 17 names, carried into register item (17) below.

### D-K — What is cut, what stays, what lands here on the pool

The sampled `payload_bytes` re-measure is **CUT** from this deliverable (holub: it prices a closed
door, and belongs with the flip card) — but the `fetch_all_rows_with_byte_total` benchmark **arm** is
**kept and measured here** (dba: it has never been timed, and it is the query production actually
performs). The deadline arm — a `statement_timeout` on a **dedicated fold connection pool**, never
pool-wide `after_connect`, never a transaction wrapping the fold — **lands in this deliverable**
(register item (8), amended below): dba wants it here because 3b **doubles** the unbounded reads
(mint AND verify both fold to head with no bound today); young agrees — there is no closed arm left
on the verify leg once `expectedTotal` is gone, so an unbounded fold on the write path is a checkout
outage waiting on a slow Catalog stream at 20:30, not merely a latency number.

### D-L — Compiler-first shapes for this deliverable

Tuple collapse **first** (vernon, holub, level 4): `AsOfPriceAuthority::at_head(...) -> Result<AsOfCatalog, DomainError>`, callers use `.coordinate()` rather than carrying `(AsOfCatalog, CatalogVersion)` as two values that can silently disagree. The `carts` fan-out narrows via a **witness the fan-out cannot construct** (graphql, level 4) — `priced(.., door_open: bool, ..)`'s `bool` is replaced by a typed witness at the two single-cart-read entry points; one emitter branch chooses the entry point per operation; the dead `ctx.data::<RunFoldPricedCartRead>()` at `query.rs:557` is removed. `CatalogVersion` must appear in **zero** `input` blocks **and zero field arguments** of the generated SDL (graphql: forbids D1 option 3 — a bare catalog version on the wire — from re-entering through a back door). 3b's own codegen test for this lands in its **own** new `tools/codegen-rs/src/validate/` module, with its own `#[cfg(test)]` (holub: #925 rewrites `tests.rs`, a write-set collision named at the architect's briefing answer — the two cards must not touch the same file). `QuoteMinter` lives in `crates/application` (the right layer, `hmac`/`sha2` are already dependencies there and in `crates/infrastructure`) — `crates/server` (the mint's call site) carries neither dependency and must not gain one.

### D-M — Numbers, and the fold count per checkout

No Tours arrival rate exists anywhere in the repo (dba, business — independently) — the card this
record precedes states **saturation shapes**, never invented figures, and every rate-like number
anywhere in this record is `UNVERIFIED input` unless an antecedent is named beside it (as done
above). The fold count per checkout is **≥ 3** (observability: `/cart` mint, `/checkout` mint,
`place_order` verify — the phase-0 budget row of `QUOTE-MINT-PRECONDITIONS` models **one** leg per
read and is therefore now known to model **one leg too few** for the checkout-wide picture; corrected
as register item (16) below). [#925 "Emit the citation graph"](https://github.com/TheCaptainCompany/captain-food/issues/925)
is **sequenced after** this deliverable (architect: `warning-baseline.json` collision — 3b moves the
`obs-metric-no-emitter`/`command-no-mutation` warning surface that #925 also touches; running them in
either order without sequencing risks a baseline race between two concurrent lanes).

### Card defects banked (attribution `card defect`, all found and closed by the decisions above)

1. The brief's claim that this deliverable needs `crates/application/src/process_managers/**` — it
   does not; the command leg is unfenced `commands.rs` (vernon) — closed by D-G.
2. `CQ-7` cited in the brief but existing in no record — authored here and in the companion legal
   amendment (item 5 of this dispatch) (legal) — closed by D-D/D-E and the legal amendment.
3. "ONE refusal message" stated over a set that was never enumerated before the briefing (evans) —
   closed by D-D.
4. `specs/screens/customer.yaml` cited in the brief; it does not exist — the customer surface is
   `restaurant_frontoffice.yaml` (ux) — closed by D-J.
5. "Paid TWICE" stated as a bare derived number with no antecedent (observability) — closed by D-M
   (≥ 3 folds per checkout, with its own antecedent named).
6. A second write-side door named with no interlock against the read door (young) — closed by D-B.
7. The brief named one rollout ordering (client sends `quote` before the server requires it) and was
   silent on the opposite one it also contains (client stops sending `expectedTotal` before the
   server removes it) (graphql) — closed by D-A (both orderings are now sequenced: expand, then the
   requiredness flip, then the field's own later deletion).
8. D4 as originally written ("required, non-null") contradicted "the write door decides whether null
   is refused" with no reconciliation (beck, reviewer) — closed by D-A.
9. The write-side door's CLOSED arm was left undefined — "a switch with one position" (holub) —
   closed by D-A (closed = priced and charged at HEAD, today's behaviour, stated explicitly).
10. `tools/walk/` cited in the brief and in register item (3) as the walk's home; it is not on `main`
    (farley) — closed by D-J, and by the register amendment (item 4 of this dispatch).
11. Several rate-like figures (peak fold cost, key-rotation blast radius, Little's-law token counts)
    were at risk of being recorded as decided numbers rather than shapes with named antecedents (dba,
    business) — closed by D-H and D-M, every figure above carrying its antecedent or `UNVERIFIED input`.

Eleven distinct items are banked above; the dispatch that commissioned this record named ten — the
discrepancy is stated rather than silently resolved (ADR-20260817-105845: a bare number is marked,
never asserted past its antecedent). The source list is `card-816-s3b4-decisions.md`'s own
semicolon-separated enumeration, reproduced in full.

## Alternatives considered

- **`quote` required, `expectedTotal` removed, in one change** (PROP §11 item 4 as originally
  written) — refused (D-A): an env-gated door cannot un-remove an already-shipped SDL field; this is
  not staging, it is a category error the briefing surfaced independently across five lenses.
- **One key for both doors** (young's original framing) — refused (D-B): the two doors flip at
  different times in the rollout, so a shared key would force them to move together when the whole
  point of 3a's door was to move first.
- **Mixed-authority verify (mint from the fold, verify from the projection, or vice versa)** —
  refused everywhere (D-F, D-B): the exact mutant the interlock exists to make unrepresentable.
  Killed in slice 3a already by a planted test; re-affirmed here for the write side.
  A HEAD/projection fallback on any fold failure at verify time — refused (D-D, D-F): every lens's
  STOP, restated from ADR-20260906-154419 D4/D7 for the write side.
- **Repurposing `PriceMismatch` for the new refusal** — refused (D-D): its copy ("Prices have
  changed…") is false for the structural causes in the enumerated set; a fossil kept alive is worse
  than one deleted.
- **A single customer-facing message per cause** (three or more strings) — refused (D-D): ux's
  cause-neutral draft is deliberately the same screen for every refusal in the set, because naming
  the cause on a consumer surface risks legal's "the price changed" exposure for causes that are not
  price changes at all (a forged token, a stale cart).

## Consequences

### Positive

- The command change makes "charged = quoted" enforceable rather than aspirational: today's
  `expectedTotal` equality check never runs in production (#816's own finding); this deliverable
  makes the check load-bearing and typed, behind a door that can be rolled back to today's behaviour
  with zero code change.
- The refusal set, enumerated once, retires a whole class of future "which error do I throw here"
  questions and removes a legal-copy hazard (a mismatched refusal string implying a price change that
  did not happen).
- The interlock (D-B) makes an entire fleet-state class — mixed mint/verify authority — structurally
  unrepresentable rather than merely tested against.

### Negative

- The write door still cannot be flipped after this deliverable alone: `QUOTE-MINT-PRECONDITIONS`'s
  items must still discharge (the phase-0 budget, the peak-shape drill, the `carts` fan-out bound),
  now amended with four more (items 14–17 below) that this deliverable itself surfaces.
- **What stays OPEN, named rather than glossed:** the flip under a rolling deploy (half the fleet
  verifying, half not, is a fleet-parity hazard the door's own `declare_flag` makes observable, never
  invisible — it does not make it safe by itself); the key's actual provisioning (admin-gated,
  founder's external list — not landed by this record); the assumption that "every client sends the
  field" before the requiredness flip (the checkpoint metric `place_order_quote_present_total{present}`
  is the instrument, not yet a measured 100%); the peak fold cost with a real signing key and a real
  verify leg on the write path (the phase-0 budget models one leg per read, corrected to ≥ 3 folds
  per checkout by D-M, but still lab-measured, `UNVERIFIED input`); and counsel's own CQ-5, CQ-6 and
  the new CQ-7 (authored below) remain open questions this deliverable carries forward, never
  answers.

### Follow-up actions

- The card dispatching this deliverable names the branch/PR and proceeds under this record's D-A
  through D-M without re-opening any of them.
- The non-null flip on `quote` and the deletion of `expectedTotal` are each their own later recorded
  step (the contract half of D-A), gated on the checkpoint metric and a cache-drain condition
  respectively.
- `QUOTE-MINT-PRECONDITIONS` items (3), (2) and (8) are corrected, and items (14)–(17) are opened, in
  the companion amendment (item 4 of this dispatch).

## Reversal check

Run against the terms *expectedTotal*, *PriceMismatch*, *quote required*, *write door*, *one
refusal*, across `docs/adr/`, `docs/decisions/` and `specs/ordering/**`.

No cited record is reversed. **[ADR-0043](0043-db-migration-release-strategy.md)** is *extended*,
not amended — its discipline is applied to a new surface (a GraphQL input) for the first time, which
this record states plainly rather than presenting as a re-citation. **[ADR-20260906-154419](ADR-20260906-154419-the-priced-read-is-served-from-the-fold-and-carries-its-coordinate-behind-a-door.md)**
is consistent throughout: D2's "the projection may only veto" sentence is restated for the write side
(D-F) rather than contradicted, and D4's `RUN_FOLD_PRICED_CART_READ` is unchanged, only interlocked
with a new sibling key (D-B). **[ADR-20260904-081527](ADR-20260904-081527-rider-standing-is-a-grant-on-the-identity-row-the-doors-are-human-only-and-step-4-lands-in-three-slices.md)
§8** is *consumed* (the seventh carve-out, recorded in the companion amendment), not reversed — the
standing clause for `runKind: door` keys is exercised for the second time since it was recorded.
**`PROP-20260831-134539` §6 D4 and §11 item 4** are *rewritten*, in the companion amendment (item 3 of
this dispatch), to match this record — a living-proposal refinement (ADR-20260801-020000), never an
appended "superseded" block. **`docs/decisions/QUOTE-MINT-PRECONDITIONS.yaml`** is *amended*, in the
companion amendment (item 4), correcting two items and opening four — an open register row is
expected to accrue exactly this way.

## Consulted (ADR-20260812-143619 — one line per lens)

Briefing before any code (`briefing-816-s3b4-answers.md`, 2026-09-06, 19:08Z dispatch); **no lens
output is legal advice or clearance**.

- **young** — a mismatch is a business rejection only for a cart edited after minting or expiry, and
  binding the cart's own stream version into the token collapses that branch to a structural check;
  verify must fold `as_of(catalogId, V)`, never `at_head`; the second door needs a stated interlock
  with `RUN_FOLD_PRICED_CART_READ`, not a shared key.
- **vernon** — the verify step belongs in the existing, unfenced `application::commands::place_order`
  pre-payment guard block, never in `process_managers/**`, which holds only the
  `PaymentAuthorized`/`PaymentFailed` event legs; the carve-out shape is one additive guard entry plus
  one threaded argument, nothing else.
- **evans** — "ONE refusal" collapsed three unrelated facts (forged/foreign/retired-key, a cart edited
  after minting, a genuine fold disagreement) into one word, and `PriceMismatch` must be deleted with
  its coverage rather than repurposed, because its copy is false for the structural causes.
- **graphql-architect** — a door cannot rescue a non-null input, so `quote` stays nullable in the SDL
  with requiredness enforced by the handler; the SDL diff is additive-only this PR, and
  `expectedTotal`'s removal is the *opposite* rollout ordering from `quote`'s addition, both of which
  must be named and sequenced.
- **ux-designer** — one cause-neutral refusal screen for the whole enumerated set, reusing the
  existing `back_to_cart` CTA with no new control; the client must render a terminal `REJECTED` path
  on `order_tracking` or the refusal is invisible to the very customer it is meant to protect.
- **legal-specialist** — the token signs the catalog-lines total, not the CTA total, so the write side
  must refuse rather than charge a delta between the two; the refusal copy must carry no cause, no
  amount and no blame; `CQ-7` (discharge of the information duty on a cause-neutral refusal, and
  whether "no amount charged" needs more when an intent was created and not captured) is authored
  here for counsel, never answered.
- **business-specialist** — the write door must not flip before slice 4's absorb-band lands, because a
  hard refusal at 20:15 costs the restaurant the whole order while Captain's own share of that loss is
  zero (`captainNet` is zero in code today); the two-key rotation overlap is a precondition of this
  deliverable, not of a later rotation event.
- **dba** — the `fetch_all_rows_with_byte_total` benchmark arm has never been timed against the query
  production actually performs and must be measured here even though the sampling lever is cut; the
  deadline arm (a dedicated fold pool plus `statement_timeout`) lands here because 3b doubles the
  number of unbounded folds on the same request.
- **observability-agent** — a `quote.verify` span with `required: false` and an unchanged
  `place-order` success rule is a STOP, because a run that never verified would silently score
  success; the outcome sets for both new counters must be closed and mapped before the flip, and no
  alert-routing surface exists anywhere in the repo today.
- **farley** — the write door has exactly two arms (closed = quote accepted and never read, priced at
  HEAD; open = absent quote refused, present quote verified), and an env flag cannot un-remove an SDL
  field, which is why this is expand/contract and not a same-change swap; the walk's home is the
  DB-gated `*_walk.rs` suite, because `tools/walk/` was never merged to `main`.
- **holub** — the write-side door's closed arm was left undefined in the brief, which makes it a
  switch with one position rather than a rollback path, and the deliverable is inventory again unless
  the refusal actually renders on the client inside this same PR.
- **beck** — the load-bearing red-first test is `the_customer_is_charged_the_price_they_were_shown`
  (mint at V, move the catalog, place with the quote, assert the payment spy captured the quoted
  amount, not HEAD's); D4 as originally written could not be reconciled with "the door decides
  whether null is refused" without picking one, which this record does.
- **reviewer** — the seventh carve-out must be recorded on `main` before the branch, not after a
  card-directed touch as the fourth and sixth carve-outs were; D4's nullability contradiction needed
  resolving in the record, not deferred to the diff.
- **architect** — #816 was closed by a stale linked-issue association and must be reopened before the
  card claims it; the brief's `process_managers/**` claim and the `enforce_service_hours_guard`-shape
  claim for hunk (b) were themselves unverified against the current fence text, both corrected in
  D-G.
