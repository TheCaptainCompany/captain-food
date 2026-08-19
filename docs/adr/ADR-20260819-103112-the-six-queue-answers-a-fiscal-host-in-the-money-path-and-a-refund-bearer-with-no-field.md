# ADR-20260819-103112 — The six queue answers: a fiscal host in the money path, a refund bearer with no field, and a margin rate that never had a consumer

<!-- Filename: docs/adr/ADR-20260819-103112-the-six-queue-answers-a-fiscal-host-in-the-money-path-and-a-refund-bearer-with-no-field.md -->

## Status

Accepted — as a **record of what he decided** and of what the roster returned on it. Recording is
not authorisation to build. Four register rows close on these answers (§10); the rest stay owed, and
the two new rows this round opens (`DELIV-THRESHOLD`, `OC-LEDGER`) are filed, not ranked for work.

## Enforced by

n/a — no behavioral guarantee

The behavioural guarantees these answers *imply* — a refund bearer carried end to end, a
contribution offer recorded alongside its outcome, a free-delivery threshold with a waived state —
are **owed**, not created here. Each is named as a candidate card in §11 and each carries its own
`rules.yaml` entry + ADR-0032 test when it lands.

## Consulted

Required by [ADR-20260812-143619](ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md):
a lens never asked is indistinguishable from a lens with nothing to say. All thirteen were asked on
the six answers; all thirteen returned.

- **architect** — on Q1, *what he accepted is SMALLER than what he was shown*: his Stripe answer had
  already emptied the money-attribution half, since no real money reaches the association. On Q5,
  **no insertion point takes the threshold's inputs** — `price_cart(catalogs, cart_id,
  restaurant_id, lines)` (`crates/application/src/pricing.rs:45-50`) receives no `ServiceType`, no
  restaurant record, no threshold. And `marginRate`'s clamp ships through introspection on the
  **restaurant's own** `/restaurant/graphql` schema. Owns the ranked-list delta (§10).
- **business-specialist** — Q4 resolved the gross misallocation; the residual defect is that **the
  discretion is unbudgeted** — no debited pot, no per-period ceiling, so incidence varies between
  restaurants. On Q5, Captain may **suggest, never cap**; the one legitimate control is a **floor**;
  and **if the threshold sits at or below the minimum order, free delivery is always on and the
  mechanism does nothing.**
- **legal-specialist** — **FETCHED** the Open Collective ToS this session: the **Host** receives and
  holds funds, and under §4(e) the Collective Admin or Host are **solely responsible for providing
  any refunds** — so Q2 routes contributions to a place where Captain does not hold the lever that
  Q3's Art. 22 exposure requires it to pull. Also authored the `WORKING POSITION` labelling spec for
  Q6. Full return:
  [BRIEF-20260819-open-collective-and-the-self-answered-position](../legal/BRIEF-20260819-open-collective-and-the-self-answered-position.md).
- **dba** — **nothing of its own.** Erasure and retention are unaffected by Q1: both are keyed to a
  row's own timestamps. Free residual: **record the handover date boundary out-of-band** in a
  register row. On the margin retirement: the **projection column is free** (`View_*` restores by
  replay); the **event-shape half is a migration** — split the card or the whole thing is
  `HOLD: human`.
- **ux-designer** — the stored shape for Q3 (offer on `OrderPlaced`, choice on the contribution
  event), the four constraints restated under a non-zero default, and the finding that **the
  discovery card will lie**: `specs/screens/restaurant_frontoffice.yaml:324` binds the delivery slot
  to `empty_ref: common.delivery.free`, so zeroing the fee makes every basketless card read
  *"Livraison offerte"* unconditionally.
- **graphql-architect** — `marginRate` is **input-only** (on `RegisterRestaurantInput` and
  `UpdateRestaurantInput`, on no output type, no query, no `View_*`), so **deleting it costs zero
  read-side clients**. And the structural finding: **`@deprecated` appears zero times in the
  generated SDL** and no `deprecated` key exists anywhere in `tools/codegen-rs/src/` — every schema
  change in this repo is either additive or breaking, with no third option.
- **holub** — **the walk still stands.** None of the six beats it; Q3 and Q5 are slice **content**
  the walk should carry, not chunks queued behind it. Names the waste: six answers landing at once
  tempts six work items against **seven already-live local branches** — *"that is a queue wearing a
  status report. Stop starting."*
- **farley** — **nothing in lens**, one caveat: if Open Collective lands as a **console setup**
  rather than a key in `specs/payments/configuration.yaml` flowing through the same GitOps pipeline,
  it is a production money path **no gate can see and no environment can reproduce**. Same test for
  the Q4 admin refund — it ships dark behind a toggle with a demo seed exercising it, or the flip is
  its first execution.
- **beck** — the bearer test **cannot be written to fail today**: there is no field to bind an
  assertion to, which makes it a **design finding, not a testing one**. First production failure: a
  restaurant invoiced a service contribution on an order the platform refunded **for its own fault**,
  discovered by reading its own invoice. Wants historical rows to read **bearer-unknown** (§4).
- **young** — no bearer on `RefundOpened` (`specs/ordering/events.yaml:422-441`), `RefundApproved`
  (`specs/common/events.yaml:57-72`) or `PaymentRefunded` (`specs/payments/events.yaml:134-158`),
  and `user_type = ADMIN` is **not** a proxy because both roles are legal approvers on the same
  mutation. On Q1: it creates **no obligation on stored events** — the operating legal person was
  never a business fact any command produced. Wants absence to resolve to `RESTAURANT` (§4).
- **vernon** — **the live defect**: the approval leg calls `payment.refund(intent, amount)` with **no
  reversal instruction** (`crates/application/src/process_managers/refund.rs:122-141`), so Stripe
  refunds from the platform balance and Captain eats it. Plus the boundary rulings: one process
  manager, not two; bearer in the PM's own state; resolved **before** the Stripe call, which precedes
  the state write (`crates/actor_runtime/src/completion.rs:69`).
- **evans** — *"A field no code interrogates is not a model element, it is a rumour."* The
  `minimumOrder`-vs-threshold **vocabulary trap** (same shape, same origin, same screen, opposite
  semantics). And the finding that **the published language moved before the model did** (§8).
- **observability-agent** — the blocking shape defect: `specs/ordering/events.yaml:388` —
  `tips: minItems: 1`, so **a customer who zeroes the pre-fill emits no event at all**. Under a
  non-zero default, absence is exactly the observation that exonerates us. On `OC-LEDGER`:
  joinability dies, completeness belongs to the fiscal host, and OC contributions are structurally
  outside the Q3 classification.

## Context

On 2026-08-19 the founder answered the six-question decision queue that
[ADR-20260818-233000](ADR-20260818-233000-the-ten-answers-per-head-monthly-invoice-and-a-cagnotte-that-exists-only-in-prose.md)
and [DECISIONS §47](../proposals/DECISIONS.md) had put to him. Under ADR-20260812-143619 the answers
were relayed **verbatim** to the whole roster before any record was composed; all thirteen lenses
returned.

Three of the six settle a register row outright. One (Q3) is a **reaffirmed** decision — he chose the
pre-fill a second time, with CRD Art. 22's verbatim text in front of him — so the roster was
instructed not to re-argue its merits and did not. One (Q6) hands the team ownership of a question
the `legal` lens had graded irreducible. And one (Q2) turned out not to be a new door at all.

## Decision

### §1. His six answers, verbatim

**Q1 — the handover instant.** → **DO NOTHING — accept that the boundary will not be reconstructible.**

> (No comment. He chose this with the trade-off stated in front of him, including "cannot be bought
> back later at any price". It is a decision, not a misreading. Do not re-litigate it; the register
> records it as a dated knowing acceptance.)

**Q2 — any non-Stripe door?** → **YES.**

> "Open collective but not yet configured"

**Q3 — contribution default.** → **SHIP THE PRE-FILL NOW.**

> "Same approach than hellloasso. People will understand the situation because we will explain them
> the product like helloasso."

**Q4 — refund bearer.** → **SOMETHING ELSE.**

> "Restaurant by default and in case of platform issue, the admin has the possibility to refund."

**Q5 — delivery subsidy.** → **FREE-DELIVERY THRESHOLD — "livraison offerte dès 25 €".**

> (No comment. This replaces the margin-derived mechanism outright.)

**Q6 — what a customer-facing money line legally is.** → **ANSWER IT OURSELVES from the lenses'
analysis and proceed.**

### §2. SETTLED / OPEN

| Q | Verdict |
|---|---|
| **Q1** do nothing | **SETTLED.** `young`: creates no obligation on stored events — the operating legal person was never a business fact any command produced. `architect`: **what he accepted is SMALLER than what he was shown** — his Stripe answer had already emptied the money-attribution half, since no real money reaches the association. `dba`: erasure and retention unaffected; both keyed to a row's own timestamps. |
| **Q2** Open Collective | **Fact settled, architecture question OPENED.** And it is **not a new door** — see conflict 1 in §9. |
| **Q3** pre-fill | **SETTLED as his choice, executed.** No lens re-argued it. Design and evidence consequences only (§5). |
| **Q4** refund bearer | **Direction settled, incidence NOT.** Three lenses converge on one card (§4). |
| **Q5** free-delivery threshold | **SETTLED as the mechanism.** Commercially and structurally incomplete (§6). |
| **Q6** answer it ourselves | **SETTLED as ownership, not as content.** `legal`: a question's bucket is a property of the question; his answer changes who carries the residual risk, not the grade. |

### §3. Q3 × Q2 COLLIDE — the highest-value new finding (`legal`, FETCHED this session)

Open Collective ToS, retrieved **2026-08-19**, verbatim:

- §1(a): *"**'Host'** — an Organization that **receives and holds funds** contributed by Financial
  Contributors via the Platform **on behalf of** one or more Collectives."*
- §4(b): *"Each contribution you make is paid directly to an Organization that receives and holds
  funds…"*
- §4(e): *"**all contributions are final, and there are no refunds**… the Collective Admin or
  **Host** are solely responsible for providing any refunds."*

**The collision**: Q3's Art. 22 exposure is *reimbursement of every contribution ever collected*.
Q2 routes contributions through an entity where **Captain does not hold the refund lever**. He would
owe a remedy he cannot execute.

**The fork that avoids it** (FETCHED, "Choosing a Fiscal Host"): four options — *No one* ·
**Organization** (*"manage money using your own bank account and legal status"*) · *Our Own Fiscal
Host* · **Apply to a Fiscal Host**. Only the last two put a **third legal person** in the money path.
**Organization keeps OC as a public ledger only** — but requires a legal entity that does not yet
exist. **One configuration screen decides it.**

Other fetched facts: platform operator **OFi Technologies LLC** (Delaware, owned by a 501(c)(6));
host fee class **6 % of incoming contributions**, taken before anything reaches the collective;
public contributor recognition by default (§4(f)); changing host later requires a **zero balance**,
both hosts' cooperation, and **cancels non-Stripe recurring contributions**.

**Coordinator correction, banked** (§12.1): the relay told the founder OC's ledger partially rescues
Q1. `legal`: half true and the wrong half — the ledger evidences **receipt**, not **attribution**,
because the counterparty on every row is the Host, before and after incorporation alike. Pre- and
post-incorporation contributions look identical.

Full transcription, with the labelling spec and the reversibility analysis:
[BRIEF-20260819-open-collective-and-the-self-answered-position](../legal/BRIEF-20260819-open-collective-and-the-self-answered-position.md).

### §4. Q4 — three lenses, one card, and the current behaviour is already wrong

- **`vernon` (the live defect)**: the approval leg calls `payment.refund(intent, amount)` with **no
  reversal instruction** (`crates/application/src/process_managers/refund.rs:122-141`). Stripe
  refunds from the platform balance; absent an explicit reversal, Captain eats it. **Today BOTH paths
  already debit the platform** — the admin path is the silent default. With `captainNet` zero, the
  balance drawn down is other orders' unsettled restaurant and rider money. **First symptom: a
  negative platform balance on a Friday night, not an error.** Under separate charges and transfers,
  *the debit IS the choice of which transfer reversals to issue*, and nobody has built the field that
  decides.
- **`young`**: no bearer on `RefundOpened` (`specs/ordering/events.yaml:422-441`), `RefundApproved`
  (`specs/common/events.yaml:57-72`) or `PaymentRefunded` (`specs/payments/events.yaml:134-158`). And
  `user_type = ADMIN` is **not** a proxy — both roles are legal approvers on the same mutation.
  *"If `bearer` is not on the event, the ledger is unbuildable forever for that period — Q1's
  accepted loss repeated once per refund instead of once in the company's life."*
- **`beck`**: the test cannot be written to fail today — no field to bind an assertion to; a **design
  finding, not a testing one**. First production failure: **a restaurant invoiced a service
  contribution on an order the platform refunded for its own fault**, discovered by reading its own
  invoice, with no ledger to reconcile against and (per Q1) no reconstructible boundary to settle the
  dispute from.
- **`business`**: resolved the gross misallocation; the remaining defect is that **the discretion is
  unbudgeted** — no debited pot, no per-period ceiling, so incidence varies between restaurants, and
  inconsistency kills a cooperative faster than a fee. *"Today 'admin refunded it' and 'restaurant
  bore it' are indistinguishable in code. The first restaurant told Captain absorbed a platform
  failure, that then sees its payout reduced anyway, has been lied to by accident."*

**UNRESOLVED DISAGREEMENT — recorded, NOT merged.** Whoever writes the card decides it explicitly and
says so in the card:

| Lens | Position | Its ground |
|---|---|---|
| **`young`** | absence resolves to **`RESTAURANT`** | it is the founder's stated default, so the historical rows read as the policy that in fact governed them |
| **`beck`** | historical rows read **bearer-unknown** | unknown is truthful; defaulting **asserts something false** about events written before the concept existed |

Both are defensible and they produce **different data**. This ADR does not pick one.

**`vernon`'s boundary rulings**: NOT two process managers — one run, one state arc; only who may send
the command differs, and that is already role-scoped. Bearer lives in the PM's **own** state, never
re-derived from `OrderTracking` (whose lag `crates/application/src/process_managers/refund.rs:6-8`
documents). No cross-aggregate transaction today, because the debited party is an external Stripe
account — but the moment a bearer path debits something internal, that is a second aggregate and a
separate leg. **Ordering hazard**: the Stripe call precedes the state write
(`crates/actor_runtime/src/completion.rs:69`), so the bearer must be resolved BEFORE the call.

### §5. Q3 — what the pre-fill requires

- **`observability` (the blocking shape defect)**: `specs/ordering/events.yaml:388` —
  `tips: minItems: 1`. **A customer who zeroes the pre-fill emits no event at all.** Under a zero
  default, absence ≈ nobody cared. Under a non-zero default, **absence is exactly the observation
  that exonerates us**, and it is indistinguishable from: the module never rendered, the customer
  never reached checkout, or a bug dropped it. *"There is no counterfactual unless the shape lands
  before the flip. Capturing the zero-default baseline is free if the event exists first, and
  unbuyable after. Same class as Q1's handover instant, which he has just knowingly accepted losing
  once. **Do not lose it twice.**"*
- **The field nobody else named**: `affirmativeAct` / `interaction` is **not** derivable from
  `chosen == presented` — a customer may deliberately re-select the same figure. Accepted-by-inaction
  and confirmed-deliberately are legally different facts and only the first is the Art. 22
  population. Every other field is a lookup; this one is destroyed if not captured at the instant.
- **`ux`'s shape**: `presentedAmount` (server-resolved, stored, never re-derived) · `suggestionBasis`
  · `suggestionRuleVersion` · `disclosureVersion` (the exact copy + layout shown) · `surface` ·
  `zeroOptionPresented` · `chosenAmount` (zero representable) · `interaction`. Classification is a
  **read over the fold, never a stored label**: `accepted_default` · `confirmed_default` · `lowered`
  · `zeroed` · `raised` · **`absent`** (rendered, no outcome) — its own bucket, alertable, never
  folded into `zeroed`.
- **The offer rides `OrderPlaced` (always); the choice rides the contribution event.** Numerator and
  denominator, no orphan event, no zero-valued money event.
- **`ux` — the sentence that changes the legal posture**: *"An **undisclosed** default is the Art. 22
  shape; a **disclosed, one-tap-reversible** default is the arguable one."* So the copy must say,
  inline in the component and never behind *"en savoir plus"*: *« Nous pré-remplissons 2 € ; vous
  pouvez le mettre à 0 € »*.
- **`ux` — the four constraints under a non-zero default**:
  - **tap parity SURVIVES, RESTATED** — literal parity is unachievable (accepting costs zero taps),
    so the rule is *zero reachable in exactly ONE tap*: same depth, same visual weight, no
    confirmation, no keyboard, no nesting.
  - **sticky zero SURVIVES AND INVERTS** — the most important change: *a customer who chose zero is
    never re-presented with a re-armed non-zero default*. Re-asking a decliner is the
    "required to reject" shape repeated per order, which is where the remedy multiplies.
  - **no second ask UNCHANGED** — a customer-initiated standalone page is pull, not a second ask.
  - **one word on the receipt NO LONGER SUFFICIENT** — the receipt must make reversal actionable,
    because *a self-service remedy is the only remedy cheaper than the Art. 22 one*.
- **`ux` — a framing correction that is a commercial-practices point, not a taste one**: the recorded
  framing (*"helps the restaurant keep a commission-free service"*) is true as an *outcome*, but the
  money goes to the platform. The copy must say *« Cette contribution va à Captain »* first, then the
  why.
- **`ux` — one subtraction with its cost**: a single default, **not** a tier ladder with a
  highlighted *"le plus choisi"*, and no free-amount keyboard at checkout. A pre-selected recommended
  tier is a distinct dark-pattern signature stacked on a pre-fill. Cost: the generous customer uses
  the receipt control or the standalone page.
- **`legal` — what the artifact must additionally carry under a non-zero default**: provenance of the
  default (config key + version + tenant); the presented default **server-issued, echoed by the
  client, re-resolved server-side**, never client-asserted; a zero-affordance property gate-checked
  per version; **a population query + a contribution-only refund route**, because the remedy is
  per-order across the whole population and must not touch the meal payment or the restaurant's
  payout; and a **retention carve-out** so the artifact survives customer erasure in pseudonymised
  form (Art. 17(3)) — an erasure that deletes the only proof of consent is self-harm.
- **Home**: not `serviceFee`, not `TipRecipient: CAPTAIN`. Shipping it as a CAPTAIN tip inherits a
  **third** characterisation (*pourboire*) with its own regime, and destroys the evidentiary
  separation in our own records.

### §6. Q5 — what the threshold breaks and needs

- **THREE lenses independently: `riderPayout = delivery` is falsified**
  (`specs/common/entities.yaml:21-22,47-48`). *"Livraison offerte"* means offered by *someone*; as
  written the invariant says it is offered by the rider. The register settles the **funder** —
  [ADR-20260818-150000](ADR-20260818-150000-captain-is-the-tool-the-restaurant-carries-the-delivery.md),
  the restaurant buys the delivery — but **the model cannot express it**. The third money term
  (customer-paid delivery vs restaurant-owed delivery cost) survives Q5 intact.
  `grep "Money\|Cents" specs/delivery/*.yaml` → **zero hits**, re-verified at HEAD.
- **`architect`**: no insertion point takes the inputs —
  `price_cart(catalogs, cart_id, restaurant_id, lines)` (`crates/application/src/pricing.rs:45-50`)
  receives no `ServiceType`, no restaurant record, no threshold. Adding one is either a second read
  on the checkout hot path at peak, or a value carried on the cart snapshot. **Proposal material, not
  a card.**
- **`evans` — the vocabulary trap that would cause a real bug**: `minimumOrder` is a floor **below
  which you may not order**; the threshold is a level **above which something becomes free**. Same
  shape, same origin, same screen, **opposite semantics**. Share a scalar and someone disables
  checkout at the free-delivery line. Also needed: a word for the **waived state**
  (`common.delivery.free` is currently keyed on `deliveryFee == 0`, conflating
  waived-by-threshold / never-charges / not-yet-modelled), and a word for the **shortfall** distinct
  from the minimum-order shortfall already in the copy.
- **`ux` — the card will LIE**: `specs/screens/restaurant_frontoffice.yaml:324` renders the delivery
  slot with `empty_ref: common.delivery.free`, so zeroing the fee makes **every basketless card read
  "Livraison offerte" unconditionally**. It needs a conditional string, not the free/paid binary.
  Plus **three** cart states, and the third is the one that gets skipped: crossed-then-**UN**crossed
  when an item is removed — a fee silently reappearing between cart and payment is a price change
  after the pre-contractual display. Plus: one definition of the measured total (subtotal TTC before
  fees, after discounts?), and **service-type gating** so the badge does not render on COLLECTION.
- **`business`**: Captain may **suggest, never cap** — a Captain-set threshold is Captain writing
  cheques on a restaurant's account; a city-wide one is a promotion funded from `captainNet`,
  recorded zero. The one legitimate control is a **floor**: refuse a threshold below the point where
  the incremental basket cannot cover the drop. And an inertness risk: **if the threshold sits at or
  below the minimum order, free delivery is always on and the mechanism does nothing.** Break-even
  stated as **UNVERIFIED** — no basket distribution and no partner tariff card exist in the repo.
- **`architect` — already tracked, do not re-file**: the cart ships `minimum_order_warning` and
  `disabled_when` bound to a declared gap, and **the renderer evaluates no `visible_when`/
  `disabled_when` at all** —
  [#472 "SDUI renderer evaluates no `visible_when`/`disabled_when`"](https://github.com/TheCaptainCompany/captain-food/issues/472),
  open, unchanged. A threshold banner today would be a second inert expression on the same cart.

### §7. `marginRate` never had a consumer (`evans` and `architect`, independently)

- `price_cart` is the sole price authority and **never resolves the restaurant row at all**
  (`crates/application/src/pricing.rs:45-50`); lines 104-113 write zeros into all eight legs. Its own
  header says *"The real ADR-0016/0017 fee/split policy plugs in here"* — **future tense, since V0**.
- The clamp formula exists **only as prose**. Its other half (`PricingPolicy.marginLow/marginHigh`)
  sits in a different scope and **has never been joined to it**. *"The two halves of one formula sit
  in two scopes and have never been introduced."*
- **`evans`**: *"A field no code interrogates is not a model element, it is a rumour."* We collect a
  restaurateur's food margin, replicate it, and flag it a commercial secret in three places — in
  exchange for nothing, ever.
- **`graphql`**: it is **input-only** — on `RegisterRestaurantInput` and `UpdateRestaurantInput`, on
  no output type, no query, no `View_*`. **Deleting it costs zero read-side clients.**
- **`architect`, sharper than round 2 recorded it**: the clamp ships through introspection on the
  **restaurant's own** `/restaurant/graphql` schema (`updateRestaurant` is
  `[ADMIN, RESTAURANT_ACCOUNT]`) — to precisely the audience promised *"jamais un pourcentage sur ta
  marge"*.
- **`evans` — the honest sequencing**: stop **collecting** (command inputs) and stop **projecting**,
  and leave the event property as a **tombstoned optional the deserializer ignores**. Banks 100 % of
  the trust benefit at zero migration risk. `RestaurantUpdated` is the sharp case: `*Updated` carries
  the full entity with **replace semantics**, so dropping a member changes what a *replay of an old
  event* means.
- **`architect`'s bonus**: `specs/database/projection_tables.yaml:74-75` records the secret as the
  reason for a deferral (#513) and `:617` as the reason `Restaurant` is deliberately not replicated
  into the uber fold. Deleting the column **dissolves a recorded deferral and reopens a
  database-split option**.
- **Sequencing constraint**:
  [#571](https://github.com/TheCaptainCompany/captain-food/issues/571) is open and touches the same
  fold. **Never dispatch both at once.**

### §8. The published language moved before the model did (`evans`)

`specs/screens/captain_frontoffice.translations.yaml:47-48` ships *"Flat monthly fee. No per-order
commission."* **to customers today**, against `/tarifs`'s **0 € d'abonnement**. A third pricing
model, live in the product.

**Coordinator correction, banked** (§12.3): the relay earlier called "flat subscription" its own
invention with no repo antecedent, and `architect` agreed. Both wrong — the phrase is in a shipped
translations file.

### §9. Structural gap — no deprecation path exists (`graphql`)

`@deprecated` appears **zero** times in the generated SDL and no `deprecated` key exists anywhere in
`tools/codegen-rs/src/`. Every schema change in this repo is therefore either **additive or
breaking** — there is no third option to offer when review asks for a deprecation path. That is the
real blocker on removing the two `marginRate` input fields cleanly, and it is **structural, not
`marginRate`-specific**.

### §10. Q6 — how a self-answered position must be labelled (`legal`)

Summarised here; the full spec with its four hard lines, two carve-outs and seven review triggers is
in
[BRIEF-20260819-open-collective-and-the-self-answered-position](../legal/BRIEF-20260819-open-collective-and-the-self-answered-position.md).

A self-answered position **MAY** describe the mechanism factually, state which characterisation the
team will **build for** (choosing the **strictest plausible** reading as the build target), record
analysis / alternatives / date / sources / grades and the **quantified downside if wrong**, and drive
internal reversible artifacts. It **MAY NOT** state a conclusion of law as settled, **leave the
repo**, close a counsel question or count as diligence — and it may not be used at all on the two
carve-outs (**authorisation questions** and **fiscal receipts**).

The label is **fixed and greppable**, because *a label does not travel by reference*:
`WORKING POSITION (self-answered) — NOT legal advice, NOT clearance` + `Id: WP-<YYYYMMDD>-<slug>`.
The register row stays **counsel-gated**, never ✅ DECIDED.

### §11. Register conflicts named — rows owed, no lens argued merits

1. **Q2 is not a new door — it is a recorded, active, never-executed decision.** `docs/adr/HISTORY.md:106`
   marks ADR-009 (Open Collective) **"✅ Active"**, and `ADR-20260808-195315:79` records *"Accounting
   will be publicly visible on Open Collective"*. **The roster treated it as an option space when the
   register already had it decided — the third register-reading failure this session.** It also
   collides with the erasure epic (#194) via a permanent public contributor ledger on a US-operated
   platform, and with `docs/adr/HISTORY.md:207`, which records Open Collective as post-PMF (M18+).
2. **Q3 vs `ADR-20260808-203443:64`** (the `"Aucun"` default) — a reversal; the ADR must be amended in
   the same change as the row, or two live records state opposite defaults. **`ux`**: the founder
   reversed **one clause of four**; the other three (educational, dark-pattern-free, never blocking
   the pay path) are untouched and are now the entire constraint set — say so explicitly, or a reader
   takes the whole sentence as superseded.
3. **Q4 vs `specs/architecture/c4-l3.yaml:114`** — *"refunds reverse the transfers"*, unconditional
   and plural. Restaurant-bears semantics written into the C4 description; must become conditional in
   the same change.
4. **Q4 vs `ADR-20260818-150000:78`** (`beck`) — that ADR **rejected** a stored bearer enum *on the
   grounds it was storing the policy*. Q4 makes bearer a **fact about one order**, not a policy, so
   the stated reason no longer holds. Someone must say whether Q4 reverses it.
5. **Q4 vs `ADR-20260818-150000`, "What the restaurant must see"** (`business`) — *"No automatic debit
   of restaurant money on delivery failure without either restaurant approval or a per-order-capped
   auto-approve the restaurant switched on itself."* **"Restaurant by default" must not be implemented
   as "debit by default."**
6. **Q5 vs `specs/common/entities.yaml:21-22,48`** — `riderPayout = delivery`, falsified by the
   threshold.
7. **Q5 vs `ADR-20260818-233000:230-232`** (`evans`) — the row *"KEPT, meaning changed — `marginRate`…
   evans withdraws round-1 fact 5"*. Q5 voids that row's premise; the withdrawal was conditioned on a
   purpose the founder has now replaced. **The row is now wrong and will be read as live.**
8. **`specs/stories.yaml:279-282`** — the activity *"set each restaurant's margin (ADR-0016/0017)"*
   cites a **superseded** ADR. **The gate hole**: ADR-0032 enforces that every mutation *has* a story
   step; nothing checks that a story describes a mechanism that still exists. `make validate` stays
   0 errors over it.
9. **Q6 vs `BRIEF-20260818-counsel-packet-and-self-answer-triage.md §3(c)`** — MONEY-LINE stays
   **IRREDUCIBLE**. Recording it as *"Q6 closed the question"* would make the record wrong about its
   own subject.
10. **Q1 forward constraint, not a row** (`architect`): there is no receipt or invoice engine, so the
    projected-issuer option is not a thing that exists to fix. What Q1 forecloses is that it can ever
    be *derived* — when Q7's invoice engine is built, the issuing entity must be **stamped on the
    document at issue time**. Belongs in that engine's spec.

### §12. The architect's delta to the ranked list

**Closed**: `CONTROLLER-HANDOVER` (Q1) · `MARGIN-MECHANISM` (Q5) · `CONTRIB-DEFAULT` (Q3) ·
`REFUND-BEARER` (Q4 closes the default; its residue **merges into `CAPTAINNET-ZERO`**).

**The vacated rank-1 slot**: nothing moves up automatically — Q1 was rank 1 on an **external clock**
and nothing else has one. By leverage it is **`CAPTAINNET-ZERO`**, which gained two dependents in one
sitting (Q4's admin debit, Q5's subsidy term) on top of blocking every cagnotte fold. **Founder-owned,
stays RED; ranking it first does not make it dispatchable and it is not proposed for work.**

**Moved**:

- **`BREAKDOWN-ZERO` splits, and half moves DOWN.** Half A (strip the published clamp) folds into the
  margin-retirement card; **half B (pin `restaurantContribution` at zero) is newly BLOCKED by Q5**,
  because a threshold creates the first genuine per-order restaurant-borne amount and
  `restaurantContribution` is described as exactly *"the restaurant's variable service part, deducted
  from its payout"* — `young`'s one-field-two-meanings trap with a **concrete** second meaning.
  Reported blocked, not re-ranked. It also stops being *"the row that gets strictly more expensive by
  waiting"* — the Stripe answer moved that window.
- **`MARGIN-WRITE` re-parents** to
  [#636](https://github.com/TheCaptainCompany/captain-food/issues/636) /
  [#178](https://github.com/TheCaptainCompany/captain-food/issues/178): its question dissolves with
  the field, its finding survives — `updateRestaurant` has **no field-level authorization anywhere**,
  only a screen comment claiming one.
- **`MONEY-LINE-LEGAL` rises on ownership, not merit** — two open rows wait on it.

**New rows**:

- **`DELIV-THRESHOLD`** — the threshold field, where it is computed, the ninth money term. Ranked
  below `CAPTAINNET-ZERO`, above `BREAKDOWN-ZERO` half B; *`architect` states plainly it is the child
  of its own recommendation and does not rank it top*.
- **`OC-LEDGER`** — a money door with no ACL, no inbound event, no observability contract, and a
  business quantity authoritative **outside `domain_events`**, against
  [ADR-20260811-014129](ADR-20260811-014129-a-business-metric-is-a-projection-and-every-reference-is-a-ref.md).
  Two designs exist and neither is recorded: an ACL ingesting OC's ledger as inbound integration
  events, or declaring the cagnotte out of the system and never folding it.

**`observability` on `OC-LEDGER`**: an ACL keeps ADR-20260811-014129 intact, but three things do not
survive — **joinability dies** (no `correlation_id`, no order, no tenant, so per-request
explainability is unavailable for that money, and every metric must state which inflow it mixes);
**completeness belongs to the fiscal host**, learned by pull on their schedule with no push and **no
path back**, so it is a permanent declared poll needing an ingest-freshness gauge and a heartbeat
alerting on the **absence** of a sweep; and OC contributions are **structurally outside the Q3
classification** (no presented amount, no checkout) and must be excluded from the coercion-evidence
denominator, never merged.

### §13. The roster's proposed cards — candidates for the architect to rank, none dispatched here

| Lens | Card | Class |
|---|---|---|
| `young` / `beck` / `vernon` | **Carry the refund bearer end to end** — closed-set scalar, required on `ApproveRefund` and `RefundApproved`, a column on `refund_process_manager`, a `bearer` input on the `payment.refund` port that the ACL turns into explicit transfer reversals. Model the **cause**, never the approver (ADR-0041: the person is envelope, the cause is business data). | `HOLD: human`, full mob |
| `ux` / `legal` / `observability` | **The contribution offer is recorded, not just its outcome** — the offer/choice VO pair, the four constraints as executable assertions, the disclosure block as one versioned component, the population query and contribution-only refund route, the retention carve-out. | `HOLD: human`, full mob |
| `architect` | **Retire the margin mechanism outright**, with the superseding ADR as **phase 0 of the same PR**. Scope verified across 14 sites. | `HOLD: human`, full mob |
| `evans` | **A — name the threshold's four concepts** (two dedicated scalars, the waived-reason term, the shortfall term), specs only. | REVERSIBLE INTERNAL |
| `evans` | **B — stop collecting `marginRate`**: command inputs + projection column; event property **tombstoned, not deleted**. | `HOLD: human` |
| `graphql` | **`@deprecated` support in the SDL emitter** + the validator rule that earns it: a field present in the previous generation and absent from this one must have shipped `deprecated` first. | REVERSIBLE INTERNAL |
| `business` | **A proposal (not an ADR) — the free-delivery threshold's commercial parameters**: who sets it, Captain's suggest-not-cap posture, the floor guard, the below-threshold money shape, threshold-vs-minimum-order, activation disclosure in euros with a worked example, and the two folds that price it. | `HOLD: human`, full mob |
| `legal` | **A gate that fails if a `WP-` id appears in any declared external artifact.** | REVERSIBLE INTERNAL |
| `observability` | **The OC ingest declared-poll contract** — separate card, not on the screen's critical path. | `HOLD: human` |
| `holub` | **Nothing.** The walk stands. |
| `farley` | **Nothing** in lens; one caveat (see `Consulted`). |
| `dba` | **Nothing of its own.** Free residual: record the date boundary out-of-band. On the margin deletion, split the card — the projection column is free, the event-shape half is a migration. |

### §14. `holub` — the walk still stands

None of the six beats it. **Q3 and Q5 are slice CONTENT the walk should carry, not chunks queued
behind it** — a threshold and a pre-filled contribution are two lines on the same checkout screen the
harness already has to render, and neither is evidenced until someone walks it. Q4 and Q6 are records.

**The named waste**: six answers landing at once tempts six work items against **seven already-live
local branches** (`556-local-walk-harness` last touched 2026-08-17, plus 618, 638, 623, 622, 609,
608 — antecedent: `git log -1` per branch). *"That is a queue wearing a status report. Stop
starting."* And two dated commitments already point at #556: `docs/STATUS.md:209` makes the walk the
**flip event** for the generated-but-unapplied security SQL, `:1989` the first end-to-end reading
under [ADR-20260817-105844](ADR-20260817-105844-the-walk-goes-first-on-one-database-and-production-stays-suspended.md).
Re-ordering around fresh decisions would silently slip both.

> *"Can one real person in Tours place one paid order end to end, see the ETA, and have somebody told
> about it?"*

### §15. Coordinator defects banked (ADR-20260816-134352)

1. **Card defect** — the relay told the founder Open Collective's ledger partially mitigates Q1. It
   evidences receipt, not attribution; the counterparty on every row is the Host both sides of the
   handover. Caught by `legal`.
2. **Register defect** — the relay presented Q2's Open Collective as a newly-opened door.
   `docs/adr/HISTORY.md:106` marks ADR-009 **Active** and `ADR-20260808-195315:79` already records it.
   Caught by `legal`. **Third register-reading failure this session**; the prior two are recorded in
   [ADR-20260818-210000](ADR-20260818-210000-the-ai-maintained-codebase-premise-prose-is-a-convention.md).
3. **Correction of a correction** — the relay told the founder "flat subscription" was its invention
   with no repo antecedent, and `architect` confirmed it. `evans` found it shipping to customers at
   `specs/screens/captain_frontoffice.translations.yaml:47-48`. Both were wrong; the phrase is real.

All three are **card defects**, none roster width, so **none reverts a review class**
(ADR-20260816-134352 as amended by ADR-20260817-105845).

## Alternatives considered

- **Re-argue Q3 on the Art. 22 exposure.** Rejected: he has now chosen the pre-fill **twice**, the
  second time with the instrument's verbatim text in front of him. Under ADR-20260812-143619 that is
  a reaffirmed decision; the team executes it and records the design and evidence consequences. The
  exposure is not softened by this ADR — it is carried, unchanged, into the legal brief and the
  register row.
- **Record Q6 as closing MONEY-LINE-LEGAL.** Rejected on `legal`'s own ground (conflict 9): a
  question's bucket is a property of the question. Q6 changes **who carries the residual risk**, not
  the grade. The row stays counsel-gated.
- **Merge `young`'s and `beck`'s bearer defaults into one recommendation.** Rejected: they produce
  different stored data and both grounds are sound. A merged recommendation would hide the choice
  from whoever writes the card.
- **Dispatch one or more of the nine candidate cards now.** Rejected on `holub`'s ground: seven local
  branches are already live and two dated commitments point at #556. The cards are filed as
  candidates for the architect to rank, not queued.

## Consequences

### Positive

- Four register rows close on founder answers, and the two that vacate rank do so **with their
  residue named** rather than silently absorbed.
- The Q3 × Q2 collision is caught **before** a configuration screen is touched — the fiscal-host
  choice is a one-screen, hard-to-reverse decision and it is now recorded as such.
- `marginRate`'s retirement is now supported by an independently-verified finding that it never had a
  consumer, with the read-side cost measured at **zero** and the migration half isolated.
- The `absent` bucket and the `interaction` field are named **before** the pre-fill ships, which is
  the only window in which the zero-default baseline can be captured at all.

### Negative

- Q1's accepted loss is real and dated: the association→company boundary will not be reconstructible
  from `domain_events`, and `young` names the shape in which it **repeats** — once per refund — if
  the bearer field does not land.
- **Both refund paths debit the platform today, silently.** That is a live defect recorded, not fixed,
  by this ADR (`crates/application/src/process_managers/refund.rs:122-141`).
- **A zeroed contribution is unrepresentable today** (`specs/ordering/events.yaml:388`,
  `tips: minItems: 1`), so the evidence that would exonerate the mechanic cannot yet be written down.
- Ten register conflicts are named and none is resolved here; two live records state opposite
  defaults until conflict 2 is closed.
- No deprecation path exists in the SDL emitter, so the cleanest form of the `marginRate` retirement
  cannot be offered at review until that gap is filled.

### Follow-up actions

- Register rows: [DECISIONS §47](../proposals/DECISIONS.md) is updated in this same change — four
  closed, `BREAKDOWN-ZERO` split, `MARGIN-WRITE` re-parented, `DELIV-THRESHOLD` and `OC-LEDGER`
  opened, the ten conflicts filed.
- Legal transcription:
  [BRIEF-20260819-open-collective-and-the-self-answered-position](../legal/BRIEF-20260819-open-collective-and-the-self-answered-position.md),
  same change. ⚠️ Its counsel-question slot **G8–G11 is empty** — the dispatch named those questions
  but the aggregation did not carry them, and they are **not** composed by the executor (the same
  defect class banked in ADR-20260818-233000 §10). They are owed from the `legal` lens directly.
- Conflict 2 (`ADR-20260808-203443:64`) must be amended **in the same change as** whichever card
  ships the pre-fill — not before, not after.
- The nine candidate cards in §13 go to the architect to rank against the walk; none is dispatched by
  this ADR.
