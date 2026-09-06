# ADR-20260906-154419 — The priced read is served from the fold and carries its coordinate, behind a door

<!-- Filename: docs/adr/ADR-20260906-154419-the-priced-read-is-served-from-the-fold-and-carries-its-coordinate-behind-a-door.md -->

## Status

Accepted — a **team decision by consent** under
[TEAM-DECIDES-OPTION-SPACES](../decisions/TEAM-DECIDES-OPTION-SPACES.yaml)
([ADR-20260904-013834](ADR-20260904-013834-the-team-decides-option-spaces-and-spec-diffs-external-legal-and-admin-gated-actions-stay-with-the-founder.md)):
the whole roster was briefed before any code (ADR-20260809-013142, full mob — `HOLD: human`, the
money path and a spec change to `specs/ordering/processmanager.yaml`), twelve lenses answered
(`briefing-816-s3-answers.md`, 2026-09-06), the two genuine splits are resolved below by the
recorded rules, and the founder reads this record. Realizes **slice 3a** of
[PROP-20260831-134539](../proposals/PROP-20260831-134539-priced-quote-token.md) §11 (row rewritten
as **3a / 3b / 4** in this change), landed by [PR #922](https://github.com/TheCaptainCompany/captain-food/pull/922)
on [#816 "Display/charge divergence is undetected: the expectedTotal equality check never runs in
production"](https://github.com/TheCaptainCompany/captain-food/issues/816). Ships in two runs on the
same branch/PR: the first landed deliverable 1 (structural) and half of deliverable 5, then stopped
correctly on a fence gap (see the amendment banner on
[ADR-20260904-081527](ADR-20260904-081527-rider-standing-is-a-grant-on-the-identity-row-the-doors-are-human-only-and-step-4-lands-in-three-slices.md)
§8, which recorded the fifth carve-out this run's deliverable 2 consumes); this record is completed
by the continuation.

## Enforced by

`rules.yaml#/CartPricedFromLiveCatalog` and `rules.yaml#/ServerPriceAuthority` (unchanged — both
pricers, live-projection and fold, satisfy the same invariant); the four server-side behaviour tests
of Deliverable 3 below pin D2/D3/D4 red-first; `tools/codegen-rs/src/tests.rs::run_flag_parity`
pins D4's fleet-parity requirement; `crates/telemetry/src/spans.rs::catalog_as_of_fold_declares_every_recorded_field`
pins D5's correlation-id requirement.

## Context

[#816](https://github.com/TheCaptainCompany/captain-food/issues/816) is that a paid order's charge
and its displayed price can diverge with nothing detecting it — CLAUDE.md's worst failure mode
("a paid order nobody is told about"), here inverted: a price shown that the charge does not honor.
PROP-20260831-134539 answers it with a **signed quote token**: the customer is shown a price at a
named catalog coordinate, and checkout is held to that exact coordinate or refused. Slice 2
(#920, merged, dark) built the capability — `AsOfCatalog`/`AsOfPriceAuthority::as_of` fold a fixed
past coordinate — with no caller. Slice 3a, this record, gives the capability its FIRST caller: the
cart READ (`cart.current`/`cart`/`carts`), behind a door, still with no signing and no
customer-facing coordinate — the mint, not yet the quote.

The 2026-09-06 briefing put nine questions to twelve lenses (young, vernon, evans, business, dba,
holub, observability, ux, graphql, beck, farley, legal); business, young, vernon and holub initially
argued for minting per-checkout as an aggregate concept distinct from the read seam, until ux
corrected the premise: `cart.current` is a `data_requirement` of exactly two screens (`/cart`,
`restaurant_frontoffice.yaml:406`, and `/checkout`, `:467`) — the menu screen reads
`restaurant.bySlug`/`catalog.byRestaurant` only (`:349`) and the cart FAB binds `cart_item_count`
(`:181`), never the priced read. So minting ON THE PRICED READ is already once-per-checkout-side
render, on both screens, with one coordinate — the "per-checkout" the business/young/vernon/holub
position wanted was already available at the seam that exists, once its actual screen population was
read correctly, not one that needed inventing.

## Decision

**D1 — WHERE the mint happens.** `cart.current` (and `cart`/`carts`, the same seam) is the ONLY
priced read the mint attaches to. It is the `data_requirement` of exactly `/cart`
(`specs/screens/restaurant_frontoffice.yaml:406`) and `/checkout` (`:467`); the menu screen and the
cart FAB never call it. Minting there is once per checkout-side render, never per menu paint, never
on the ETA surface: one coordinate per priced read, on both screens — the /cart→/checkout seam (two
reads, two mints, two moments) is recorded OPEN for 3b/4, never a claim that one mint pins both. The
door gates all THREE resolvers (`cart.current`, `cart`, `carts`) through the one `priced()` seam;
`carts` multiplies the mint N× (up to 50 serial unbounded fold reads per render on a pool of 5,
`crates/infrastructure/src/persistence/cart.rs:43`'s `LIMIT 50`) — the register row's item (7) is the
flip precondition that fan-out must clear before the door opens on `carts`.

**D2 — ONE authority, no mixed sourcing.** With the door OPEN, the priced read prices FROM THE
FOLD: one unbounded range read of `Catalog-{id}` to head, through the dedicated port op
`AsOfPriceAuthority::at_head(catalog_id, correlation_id) -> Result<(AsOfCatalog, CatalogVersion), DomainError>`
(never `latest_version()`, never `Option<CatalogVersion>`, never a relaxed `load_range` check —
landed in the first run, deliverable 1). The coordinate carried is the CEILING the fold was bounded
at (= the highest raw row version the adapter verified, non-`Option`); `AsOfCatalog` has no
constructor that omits it. `price_cart_at` prices from that ONE fold; the projection read
(`CatalogSnapshot::load` + `price_cart`) is the CLOSED arm — prices and coordinate NEVER come from
two reads or two authorities. `specs/ordering/processmanager.yaml:68` (whose prose asserted
projection-pricing is DELIBERATE "so display and charge cannot disagree") is rewritten in the SAME
commit that lands the open arm, because the leading option here mixes coordinate-from-stream with
prices-from-projection nowhere — the fold supplies BOTH, so the sentence's premise (one shared
authority) still holds, restated for the door's two arms.

Option (a) — a projected `catalog.version` column read alongside the projection, with a
two-folds-agree gate (dba's recommendation) — is REJECTED for this door position (see D9): a
checkpoint-reset rebuild-in-place replay serves a PARTIAL version while the row serves partial
state, so a quote minted mid-rebuild is charged at a price the rebuild chose (young) — #816
inverted, the exact failure this whole proposal exists to close.

**D3 — NO customer-facing coordinate in 3a.** The coordinate is server-carried only: it travels
inside the read (as part of the `AsOfCatalog` value the resolver already holds) and is DROPPED at
the reply boundary — no `catalogVersion` field, no `quote` field, no `entities.yaml` property, no
input, no command. Slice 3b (the very next card in this lane; holub's WIP-one condition: nothing
else opens between) introduces the ONE published word `quote`, an opaque, nullable, ordering-owned
value on `api.yaml` `types:` only, with graphql's codegen test (`CatalogVersion` in zero `input`
blocks) and ux's binding-table prose line. 3a stores nothing and decides nothing (vernon): the
coordinate is never used in this slice to accept, refuse, or freeze anything — it is carried and
then discarded, purely so D2's ONE-READ discipline has somewhere to put the value it necessarily
produces.

**D4 — the door.** New key `RUN_FOLD_PRICED_CART_READ` (`type: bool`, `runKind: door`,
`decisionRow: QUOTE-MINT-PRECONDITIONS`, `deploy.production`/`staging` `false`), declared at BOTH
composition roots (`crates/server/src/lib.rs`, `crates/infrastructure/src/mailbox/standalone.rs` —
the standalone declaration is the PRE-RECORDED carve-out of ADR-20260904-081527 §8's standing
clause, consumed here for the first time since it was recorded). CLOSED = today's projection-priced
read, unchanged, explicitly stated as the closed arm rather than a fallback. OPEN + fold `Err`
(coordinate absent, or stream unreadable) = `technical_error` through the
EXISTING unresolvable-at-read path (`ADR-20260810-112836` §6; the same `PriceUnresolvable`-adjacent
classification `price_cart` already uses) — NEVER a HEAD/projection fallback (every lens's STOP).
**No timeout, `LIMIT` or `statement_timeout` bounds `at_head` today** (the pool sets only
`acquire_timeout`) — a slow fold is unbounded; the deadline arm (a bounded L or a statement timeout
on the read) is a named flip precondition, not a shipped behaviour (register row item (8)).
New register row `docs/decisions/QUOTE-MINT-PRECONDITIONS.yaml` (open, team) names what must hold
before the flip: the phase-0 budget (D6), the observability contract rows (D5), the walk drill
(farley), 3b's signed quote (D4/holub), and dba's preconditions (D9).

**D5 — observability.** `catalog.as_of.fold` joins `cart-price`'s `spans:` (`required: false` — only
the OPEN arm's mint opens it) and `catalog_as_of_fold_ms` joins its `metrics:` — INSIDE the
`cart-price` contract, never a sibling (observability's gap-between-two-contracts hazard). New
histograms `catalog_as_of_stream_length` and `catalog_as_of_payload_bytes`, `attributes: []` (no
bounded population exists for catalog/tenant). New dead-man counter
`catalog_as_of_reads_total{outcome}`, `outcome` ∈ `{applied, refused}` (closed set), so a
HEAD-fallback regression cannot hide as silence. `business.correlation_id` is RECORDED on
`catalog.as_of.fold` from the priced read's own request-scoped id (the observability HARD STOP —
previously declared `Empty` and left that way with no caller; this is the first caller). The span
attribute `business.stream_length` is RENAMED `business.rows_read` (beck: it is rows at or below the
coordinate, and the true head-derived L is what the new histogram carries — the two must not share a
name that implies they are the same measurement). `cart-price`'s existing 300/600 ms budget is
untouched at phase 0, its INITIAL comment amended to say the fold now sits inside it. Status stays
`technical_error` (decided, ADR-20260810-112836 §6 — never re-litigated here).

**D6 — the budget as a design target, not a verdict.** Phase 0, recorded on the register row with
its antecedents: the as-of leg is a headroom carve INSIDE `cart-price`'s existing p95 300 / p99 600
(`specs/observability.yaml`, INITIAL) — leg ≤ 150 ms p95, ≤ 250 ms p99 (150 = the recorded #920
escalate line; ALL `UNVERIFIED input` — lab, one container, peak-unverified; production is
deliberately suspended, ADR-20260817-105844, so no distribution exists to compare against).
"Observed production L" is NOT a blocking precondition (holub: unsatisfiable while suspended) — the
CONTRACT ROW is; the max-L NUMBER is deferred, the BEHAVIOUR above it is decided now: refuse
(`technical_error`), never HEAD. Arm (c)'s native `at_head` median measured 113.8 ms (max of 10 =
116.6 ms) BEFORE this lever, and measures 89.6 ms AFTER it (executor hand-back, round 2, same
L=2,000 fixture, 10 iterations, lab, one container, `UNVERIFIED input`, not re-run in CI). The
head-of-list lever — `payload_bytes` read from Postgres (`sum(octet_length(payload::text))` in the
same SELECT) rather than re-serialized per row — is now LANDED, in slice 3a's own round-2 commit;
the next lever, now at the head, is **sampled** (business: `payload::text` still renders jsonb per
row INSIDE Postgres, on the pool-of-5 tier, so the ~28 ms this lever removed may have MOVED rather
than gone — sampled is the preferred end state, re-measure owed at 3b with the contract rows live) >
content-hash import suppression (#921 item 2) > narrow fold > refuse; SNAP-1 last. Against 89.6 ms
rather than the superseded 113.8 ms, PROP §12's crossing claim is +5% over arm (b)'s 85.6 ms
end-to-end median (not +33%), the max-drift-to-140-ms antecedent (#920) unchanged, and the claim is
about the p95 of a distribution that does not exist yet — production suspended,
ADR-20260817-105844 — never about the median.

**D7 — refusal states are the screen's business, not 3a's.** 3a changes NO reply shape: a fold
failure surfaces through the existing `technical_error` path, exactly like today's
`PriceUnresolvable`. ux's three no-price states (no number on the total row/CTA; the pay button not
pressable; a timeout with explicit no-charge reassurance; never a stale total, never a zero, never
price-change wording for a defect) are recorded in PROP §4/§11, owed at item 5 (slice 4).

**D8 — legal (never clearance).** 3a does NOT discharge B1 and does NOT close #816: the coordinate
is unsigned, uncarried by the customer, and unstored. `TaxRate` stays the whole per-mode object; no
shape makes one-rate-per-line an invariant; no prose calls the pinned rate "the applicable/legal
rate" — it is the catalog-declared rate at a coordinate. Counsel questions CQ-5/CQ-6 (below) are
appended verbatim to the counsel packet. Telemetry hygiene: no customer/contact attribute rides
beside the coordinate on any span — pseudonymous by construction.

**D9 — the (a)/(b) split, resolved by the recorded rule.** dba recommended option (a) (a projected
`catalog.version` column plus a two-folds-agree gate); young, farley, vernon and evans rejected or
conditioned it (the rebuild-window failure of D2, the missing Catalog rebuild recipe, no
down-migration, `Catalog`'s read used by the WRITE path at every add-to-cart/checkout — STO-7). The
recorded rule (ADR-20260904-013834: a split takes the reversible option behind a gate) resolves it:
**(b)**, behind `RUN_FOLD_PRICED_CART_READ`, CLOSED in production until the register row's
preconditions hold — which now INCLUDE dba's: `#921` item 2 (content-hash import suppression)
landed BEFORE the door opens on the `/cart` render, `payload_bytes` on the contract row (dba: L's
cost is BYTES, not length — one 500-product `CatalogImported` ≈ 200 KB, 26% of the payload in 0.05%
of L), and the buffer-cache instrument named (`pg_statio_user_tables` hit ratio on `domain_events`).
dba's (a) stays on the record as the alternative with its own gate test, re-weighed at 3b/4 on
MEASURED cost if the fold-priced read's wall crossing proves too dear. The read-path →
write-database crossing (b) introduces (dba) is named here as the cost of this choice: STO-7/STO-8
(both open) describe the write path already reading the `Catalog` projection at every
add-to-cart/checkout; this door adds a SECOND read path (the event-stream range read) behind a gate
that is the containment — a rolling deploy in which half the fleet folds and half projects is made
observable, not invisible, by the door's own `declare_flag` fleet-parity evidence at both
composition roots (ADR-20260905-223957 §5, ADR-20260906-113444).

**The projection stays a VETO on the open arm (young NB9).** `price_cart_at` needs HEAD metadata per
line/option (product/option names, images, presentation — the fold carries no labels, by slice 2's
own design), so under projector lag or a rebuild the fold-priced read REFUSES rather than mis-charges
— the coordinate does not yet make this read rebuild-neutral, and 3b inherits the veto unchanged.

## Reversal check

Run against the terms *cart priced live on read*, *fallback*, *live catalog*, *projection*, across
`docs/adr/`, `docs/decisions/` and `specs/ordering/processmanager.yaml`.

**[ADR-20260810-112836](ADR-20260810-112836-cart-priced-live-on-read.md) §1/§3 is AMENDED IN PART**
by this record, with a banner added to that ADR: §1/§3 said the cart is priced LIVE, on every read,
from the catalog projection, with no cached/frozen price anywhere on the read side. That remains
true with the door CLOSED (the default everywhere, today) and remains true in spirit with the door
OPEN — the read is still priced FRESH, at request time, from an authoritative source; what changes
is WHICH authoritative source a request reads from, gated by a door bound to an open precondition
row. §6 (unresolvable-at-read is `technical_error`, never `business_rejected`) is **NOT amended** —
this record's D4 explicitly reuses it, unchanged, for the fold's own refusal path. No other cited
record is reversed: `QUOTE-TOKEN`, `QUOTE-STALENESS`, `ADR-20260831-121957` §4d,
`ADR-20260831-165146`, `PROP-20260831-134539` §11/§12, `ADR-20260808-171056`,
`ADR-20260815-030206`, `STO-7`, `CHK-1`, `SNAP-1 (open AMBER, untouched)`, `ADR-20260906-113444`,
`ADR-20260817-105845`, `ADR-20260904-081527` §8 (consumed, not reversed — the fifth carve-out
recorded there is exercised here for the first time), `ADR-20260904-013834` are all consistent with
and cited by the decision above.

## Pinned claims (ADR-20260906-152024 §2)

- **`cart.current`/`cart`/`carts` are the only priced reads, and `/cart`+`/checkout` are their only
  two screen consumers** (D1, ux's premise correction): `UNVERIFIED input` this run — a codegen
  test asserting the emitted screen tree's `data_requirements` for `/cart` and `/checkout` include
  `cart.current` and the menu screen does not was not written in this run (time-boxed out); the
  claim is citable as `specs/screens/restaurant_frontoffice.yaml:349,406,467`'s current text, not as
  a gate-enforced fact.
- **`domain_events.version` is 1-based** (D2/D4's coordinate semantics): PINNED by
  `tools/codegen-rs/src/tests.rs::eventstore_version_note_matches_the_writer` and the append-into-an-
  empty-stream assertion landed under [#921 "Priced quote token slice 2 follow-ups"](https://github.com/TheCaptainCompany/captain-food/issues/921)
  item 1 (ADR-20260808-171056).
- **Unresolvable-at-read is `technical_error`, never `business_rejected`** (D4): PINNED by
  `specs/observability.yaml`'s `cart-price.status_rules.technical_error` rule plus the existing
  `cart_price_unresolvable_total`/span-ERROR behaviour `price_cart` already exercises
  (ADR-20260810-112836 §6).
- **`specs/ordering/processmanager.yaml:68`'s prior prose ("display and charge cannot disagree
  because both price from the projection")** (D2): this is a PROSE claim about DESIGN INTENT, not an
  enforced invariant — no test pins it beyond the equivalence test (`price_cart`/`price_cart_at`
  agree at HEAD, `crates/application/src/pricing.rs`), which is the enforcement this record relies
  on going forward; the prior sentence is rewritten, not narrowed to fit a test (legal/evans:
  narrowing a claim to fit a test would be a decision reversal — this is a rewrite of a now-outdated
  premise instead).

## Counsel packet additions (never advice or clearance)

Appended verbatim from `briefing-816-s3-answers.md`'s `## legal` section to
`docs/legal/BRIEF-20260831-repricing-and-price-quote-counsel-packet.md`:

- **CQ-5**: Does the burden accept a reproducible server recomputation, or require an artifact of
  what the SCREEN rendered; does signing change it?
- **CQ-6**: Must delivery fee and platform fee freeze inside the same coordinate as catalog lines,
  or is a per-component freeze defensible as the total displayed?

## Alternatives considered

- **Option (a) — a projected `catalog.version` column plus a two-folds-agree gate** (dba) — refused
  for THIS door position (D2/D9): the checkpoint-reset rebuild window makes a quote a rebuild's
  choice (young), no Catalog rebuild recipe or down-migration exists (farley), and the write path
  already reads this same projection at every add-to-cart/checkout (STO-7) — making the two-pricer
  equivalence load-bearing for money forever rather than for a reversible gate's lifetime. Stays on
  the record, re-weighable at 3b/4 on measured cost.
- **Minting per-checkout as a distinct capability from the read seam** (business/young/vernon/holub's
  opening position) — superseded by ux's premise correction: the capability already exists exactly
  where needed (`cart.current`, the only priced read, feeding exactly the two post-decision
  screens), so inventing a second mint site would have produced the SPLIT MINT every lens's STOP
  forbids.
- **A customer-facing `catalogVersion` field in 3a** — refused (D3); deferred to 3b as the opaque
  `quote` word, never a bare integer, never a catalog-scope type inside an ordering-owned reply.
- **A HEAD/projection fallback on fold failure** — refused everywhere (D4/D7); every lens's STOP.

## Consequences

### Positive

- The write-path/read-path pricing divergence #816 names becomes structurally closer once 3b/4 land
  a signed, checkout-held coordinate — this slice supplies the read-side half with no behaviour
  change while the door is closed (the default everywhere today).
- The fold-priced path retires half of STO-7's pricing leg in the LONG run (young): once the write
  path also folds instead of projecting, the `Catalog` projection stops being a money-path read.
  Not done in 3a.
- The door's fleet-parity `declare_flag` at both composition roots makes a rolling-deploy split
  OBSERVABLE (`runtime_flag_state`) rather than a silent, per-process pricing disagreement.

### Negative

- A second read path (the event-stream range read to head) now exists behind the door, alongside
  the existing projection read the write path uses (STO-7) — a wall crossing dba names explicitly;
  the door is the containment until the preconditions on `QUOTE-MINT-PRECONDITIONS` are discharged.
- 3a is INVENTORY on its own (holub): a coordinate minted and immediately dropped commits nobody to
  anything. It closes nothing on #816 until 3b (the signed quote) lands, in the same lane, next.

### Follow-up actions

- Slice 3b: the opaque `quote` word (api.yaml `types:` only), the zero-input-blocks codegen test,
  ux's binding-table prose line, the signing mechanism.
- Slice 4: `rules.yaml#/ServerPriceAuthority`'s enforcement of the signed quote at checkout
  (`rules.yaml:60-65`, untouched by 3a).
- The screen `data_requirements` pin named above (`Pinned claims`), if a future card has budget for
  it.

## Consulted (ADR-20260812-143619 — one line per lens)

Briefing before any code (`briefing-816-s3-answers.md`, 2026-09-06); **no lens output is legal
advice or clearance**.

- **business** — the budget as a headroom carve inside `cart-price`, not `place-order`; CHECKOUT-ONLY
  framing superseded by ux's premise (the mint IS the checkout-side read); the cheapest-lever order
  (content-hash suppression > narrow fold > refuse, SNAP-1 last); refuse-with-retry, never HEAD.
- **young** — the coordinate is a fact about the LOG, minted only by a component that read at that
  position; option (a)'s rebuild-window failure (a checkpoint-reset replay serves a partial version
  while the row serves partial state); (b) retires half of STO-7's pricing leg in the long run; a
  paint is not a quote — mint once at the binding read.
- **vernon** — the range read is a legitimate immutable-prefix read, not an Ask; `AsOfCatalog`
  carrying its coordinate strengthens the value (one indivisible thing); 3a stores nothing, decides
  nothing; the `latest_version()`/`Option<CatalogVersion>` back door stays absent from the port.
- **evans** — the coordinate is ENVELOPE in 3a, never payload, never a bare `catalogVersion`; the
  published word `quote` is 3b's, opaque and ordering-owned; `processmanager.yaml:68` rewritten in
  the same commit that makes its old premise incomplete; catalog→ordering stays Customer/Supplier
  with a translation boundary.
- **dba** — L's cost is bytes, not length (the `payload_bytes` contract row); option (a) costs the
  projector nothing but crosses no wall the write side doesn't already cross, while (b) opens a NEW
  read-path→write-database crossing dba names as this decision's real cost; the duplicate serving
  index on `domain_events` is a separate, already-dispatched follow-up.
- **holub** — 3a alone is inventory; the smallest thing touching a user is 3a→3b→4 as one lane with
  no other work between; "observed production L" is unsatisfiable while suspended and is not a
  blocking precondition — the contract row is.
- **observability** — `catalog.as_of.fold` joins `cart-price`, never a sibling contract;
  `catalog_as_of_reads_total{outcome}` as the dead-man against a silent HEAD-fallback regression;
  `business.correlation_id` recording is the HARD STOP; the one-directional emitted-contract gate is
  a known, un-widened gap this slice does not fix.
- **ux** — the premise correction that reprices the whole per-paint debate: `cart.current` is
  exactly `/cart`+`/checkout`'s data requirement, never the menu or the ETA surface; never a split
  mint; the three no-price states are 3b/4's, not 3a's; one prose line at the binding table records
  the coordinate is server-carried and deliberately unrendered.
- **graphql** — response-only is guaranteed by SHAPE (`emit_server_inputs` never walks `types:`); the
  coordinate must be nullable wherever it appears in 3b, never non-null on `Cart`; zero new `input`
  blocks in this slice's SDL diff; no touch to `commands.yaml#/PlaceOrder`.
- **beck** — the mint's ceiling is the value `from_stream` was bounded at, not "the highest applied
  business version" (an empty fold's `Option` trap); the load-bearing property is the RELATION
  (price, coordinate) in one reply, never a coordinate-only or price-only assertion; the four
  red-first server tests and their expected reds, named on the dispatch card.
- **farley** — the gate proves order-of-magnitude non-regression, nothing about ms/p95/p99 in
  isolation; the door's form (`runKind: door`, `decisionRow` bound to an open row); rollback is a
  flip, complete, because nothing durable is written by 3a; the walk drill before any production
  flip.
- **legal** — 3a does not discharge B1 or close #816; `TaxRate` stays the whole object; no shape
  makes one-rate-per-line an invariant; CQ-5/CQ-6 appended as questions, not answers; the pseudonymous-
  by-construction telemetry hygiene fence.
