# ADR-20260818-150000 — Captain is the tool: the restaurant buys the delivery, and carries its failure

**Status**: Accepted, with two consequences the code does not yet honour · **Date**: 2026-08-18 ·
**Decider**: the **FOUNDER / Tech CEO**, answering the "Who Buys The Delivery" decision form ·
**Builds on**:
[ADR-20260818-134500](ADR-20260818-134500-the-invoice-chain-restaurant-to-customer-rider-to-restaurant.md)
(the invoice chain) ·
[ADR-20260808-203443](ADR-20260808-203443-tips-voluntary-contributions-funding-model.md) (the
funding model) · **Reframes**: [ADR-0017](0017-3way-stripe-connect-split.md) ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

## The three answers

**1. The customer keeps a visible delivery line — as a breakdown of the restaurant's price**, not as
a second thing bought.

**2. The restaurant carries a failed delivery**, verbatim:

> *"Captain make in relationship the restaurants the riders, they are working together and the
> restaurants will carry the refund in case of problem not the rider neither the platform that does
> not take any benefits from the transaction. For partner riders there is a deal between the
> restaurants and them not with the Captain. The Captain is just the tool."*

**3. Delivery pricing**, verbatim: *"The Captain sets the prices for their riders and passes the
prices provided by the partner rider."*

**The fact that was not on the form**: the delivery-partner contract is **restaurant ↔ partner**.
Captain operates the integration and is not a party to the delivery. This appears in no prior record.

## The finding that matters most: today the code falsifies the posture

Reached independently by **vernon**, **architect** and **evans**.

The dispatch walk is **city-ranked** — `CityDeliveryRanking` is keyed `(city_id, effective_from,
rank)` and `DeliveryChannelCatalog.enabled` is **global**. `RestaurantDispatchConfig` carries
`city_id`, `mode`, `self_dispatch_ttl_seconds` and **no partner-entitlement column anywhere**.

So Captain offers the restaurant's job to a **platform-ranked** partner — and answer 2 then lands the
refund for **that partner's** failure on the restaurant.

> **vernon**: *"'Just the tool' is falsifiable in code, and today the code falsifies it. A tool does
> not pick its user's counterparty from its own city-ranked table."*

**This is the smallest change that makes the posture defensible, and it is cheaper than any argument
about the balance.** The seam already exists (the `offer_job` input strategy hook), so it is a rules
row plus a referential column, not a process-manager rewrite. The invariant is statable today:

> **A channel may only be offered for a restaurant that has an active agreement with that channel.**

Two riders on it: `DispatchExhaustionFailsClosed` currently counts channels the restaurant was never
entitled to as attempts; and *"can this restaurant receive deliveries at all?"* is **unrepresentable**
today — the only partnership funnel in the repo is restaurant↔**Captain**, so an `ACTIVE_PARTNER`
restaurant with no signed rider deal is a fully orderable delivery restaurant with **no legal dispatch
route**, discovered at 20:00 Friday on an order already paid.

## A correction to the framing this ADR was briefed with

The briefing said the buyer's total *"lands on Captain's Stripe balance and is transferred out"*.
**beck** checked: there is **no transfer code at all** — grep across `crates/**` for
`reverse|reversal|clawback` and for `on_behalf_of|transfer_data|application_fee|destination` returns
nothing, and the refund path refunds the intent without reversing anything. So the sentence is **half
true: it lands, nothing moves out.** The transfer half is a **stated design, not an implemented
mechanism**, and it must not be quoted as fact. Everything below that depends on transfers is
therefore **cheap now and expensive after the first live transfer**.

## Consequences

### The refund mechanics do not survive contact — three defects, all inert today

- **No bearer.** `PaymentRefunded` carries `amount`, `orderId`, `restaurantId` — and that
  `restaurantId` is *the order's restaurant*, not *the bearer*. Incidence lives only in
  process-manager code, so a replay under a later policy re-derives an earlier period's answer to
  *"what did this restaurant owe"*. **young**'s settling experiment: replay a settled refund with the
  incidence rule flipped and diff the restaurant's payout total; if it moves, incidence is not in the
  fold.
- **Store the fact, not the rule.** The refund should carry the **per-party reversal decomposition**
  mirroring `PaymentBreakdown` — *"the restaurant carries it"* reads as `restaurantPayout −12.40,
  riderPayout 0, captainNet 0`. A `bearer: RESTAURANT` enum would be storing the policy, and policies
  move; numbers replay unchanged with no upcaster.
- **The default reversal contradicts the ruling.** The architecture says *"refunds reverse the
  transfers"*, and Stripe's default reversal is pro-rata across the transfer group (`VERIFY-FIRST`) —
  which makes **the rider fund part of the refund**, the exact opposite of answer 2. The reversal
  behaviour must be named explicitly rather than inherited from the gateway default.
- **No ledger for the negative.** The refund cap is the captured **total**, not the restaurant's
  payout, so a full refund on a failed delivery exceeds `restaurantPayout` whenever `delivery` and
  `serviceFee` have already left. The remainder is a **receivable from the restaurant** that no
  aggregate can hold — *"a liability with no ledger becomes a spreadsheet."* **This is a genuine
  option space and therefore a PROPOSAL, not this ADR**: cap the refund at `restaurantPayout` (cheap,
  but someone else absorbs the rest, contradicting answer 2) versus a restaurant-account ledger
  aggregate (matches the answer, costs an aggregate and a payout-adjustment path).

### The stored shape does NOT change — record that, or someone will version it anyway

**young**: `PaymentBreakdown.delivery` is the same cents; the invariant `riderPayout = delivery` still
holds numerically and has already been reframed as the restaurant's cost settled on its behalf. This
is a **description-only change — no upcaster, no `PaymentBreakdownV2`, no migration.** Every event in
`domain_events` remains a true fact. The **only** genuine shape change is a **provenance** field from
answer 3 (Captain-set versus partner-passed), which is additive, optional and tolerant-reader.

### The language debt — evans dissents from the coordinator, and is right

The framing does not merely "help": it **redefines the legal meaning of five kernel terms while
leaving the words in place** — `delivery`, `riderPayout`, `restaurantPayout`, `serviceFee`,
`restaurantContribution`. That is language debt **added**, in the highest-fan-out place in the tree.
Two on the record: **`restaurantPayout`'s arithmetic is unchanged but it is now a net settlement after
offsetting the rider invoice, not proceeds of a sale — the worst kind of drift, because the number
survives and no test fails**; and **`serviceFee` versus "takes no benefit"** — a fee is compulsory, a
contribution is not, and the funding ADR says contribution.

**"Merchant of record" is Stripe's word.** It crossed the ACL and landed in the kernel, the one
artifact where one-name-one-meaning is enforced; it belongs on the adapter side describing Stripe's
mechanism. **Captain in our own language is an AGENT**: collecting agent for the restaurant, paying
agent of the restaurant toward the rider, self-billing agent for both invoice series.

### Credentials — a second agency relationship, with no record at all

Under fact A the partner credential is **the restaurant's**, and Captain would store and use another
party's credential to incur that party's liability. Today all three adapters resolve **one platform
credential** from process env at the composition root.

- **Storage**: adapter-owned connection tables keyed by restaurant, encrypted at rest — never
  event-sourced, never in `api.yaml`, never projected. Not a `recovery: pitr` database: a revoked
  secret would survive in every WAL archive segment for the retention window. **Compiler-first**: a
  `Credential` newtype constructible only by an adapter connection repository, with a validator rule
  as the fallback.
- **Recovery**: the three adapter databases are declared `recovery: refetch`. **That becomes false** —
  only a human at that restaurant can re-connect it, and losing forty is forty phone calls, not one
  re-ingest. They flip to `backup-required`, and the claim that one adapter database is the only one
  inside a backup story is amended in the same change.
- **Deploy**: on today's path **onboarding a restaurant would be a deploy** — edit a cluster Secret
  and restart the shared adapter, **bouncing every restaurant's channel to onboard one**. Onboarding
  must be a data event on a running system. **Do not ship an intermediate "N sets of env vars."**

### The surface asserts the old posture where a partner can read it

The composed schema still says `riderPayout`, `captainNet` and "merchant of record" in text
introspectable by any partner. `EXTERNAL` is one un-narrowed role, so under fact A
`deliveryPartnerAvailabilities` shows **partner A the coverage of partner B** — a
competitor-intelligence disclosure, not an outage. And `approveDeliveryPartnerAvailability` is
`[ADMIN]`: **an admin approving a partner is Captain gatekeeping a deal it is not party to.**

### What the restaurant must see — the difference between a cost and an ambush

- **No automatic debit of restaurant money on delivery failure** without either restaurant approval
  or a per-order-capped auto-approve **the restaurant switched on itself**. *"If Captain can refund
  the restaurant's money without the restaurant, 'just the tool' is false at the moment it matters
  most."* The speed answer is the restaurant pre-authorising a ceiling, never Captain deciding by
  default.
- **The refund is decomposed or it is resented.** Today the claim panel offers a **tip preset grid**
  (`[200, 450, 500, 1000]`) as the refund control. A `NOT_DELIVERED` claim is food the restaurant
  cooked, a delivery it did not perform, and a service fee it did not set — three lines, each with a
  bearer.
- **The payout-adjustment read model precedes enabling refunds on a real payout.** Today the money
  move is downstream of the tap, so the consequence lands weeks later by bank statement, where no
  screen explains it. **A refund with no visible ledger line is a designed surprise.**
- **Disclosure at activation, in euros, with a worked example** — on the screen where the restaurant
  turns on partner delivery, not in terms. A failed delivery is not a fee event: it is roughly one
  basket plus the rider fee, and at a hypothetical 25 € basket that erases the margin on four to six
  good orders (`UNVERIFIED input` — no basket data exists).
- **The second bill is unanswered**: who pays the rider on a failed drop? If the restaurant pays both,
  say so at activation. If the partner absorbs it, that is a selling point and belongs in the pitch.

### Sequencing — the shortest path to a first real order moved

Fact A converts the delivery-channel ordering into a **gate on the first real order**, because the
restaurant's third-party sales cycle now sits in front of it and Captain cannot compress a contract it
is not party to. **The shortest path is therefore COLLECTION** — no partner contract, no passed-through
price, no failed-delivery refund, no rider invoice. `VERIFY-FIRST` that collection walks end-to-end
today: ADR-20260814-141350 says it is **modelled**, and modelled is not walked. **That check may be the
highest-value hour on the board.**

**The repo contains no contract of any kind.** Three liabilities now land on a restaurateur who has
signed nothing — the failed-delivery refund, the shortfall split, and the partner contract they must
sign themselves. **Terms are on the critical path to the first restaurant, ahead of every rider
question**, and the mandate draft authorised in ADR-20260818-134500 §4 is where they start.

**The slice in flight is untouched.** Nothing in these answers touches the #618 caller binding. One
line of slice *content*, not a new slice: **the refund screen states who is debited, before the press.**

## Consulted

Whole roster invited; all eleven replied. **business-specialist withdrew its own earlier framing** — "the fastest churn trigger
we could build" does not survive the founder's reasoning, and it restated the risk as magnitude and
disclosure, which is a better objection. **evans dissents** from the coordinator's "the framing merely
helps" and its dissent is recorded above rather than averaged away. **beck** corrected the briefing's
own premise about transfers. **vernon**, **architect** and **evans** reached the entitlement finding
independently. **dba** supplied the storage and recovery consequences, **farley** the deploy one,
**graphql-architect** the surface one, **ux-designer** the refund-screen design, **young** the
stored-shape and fold analysis, **holub** the sequencing and the instruction to stop deliberating and
land this.

**No lens output here is clearance.** The funds-posture characterisation remains open per
BRIEF-20260818 §3(c), knowingly accepted.


---

## Legal — appended, and it changes two things above

`legal-specialist`, last to report. Nothing FETCHED this round; every item below is
**PRIMARY-READABLE** (a free, named instrument) or **VERIFY-FIRST** (reasoning from training). **No
clearance.**

### The framing does real work — on one question, and none on the other

The "irreducible" item was **two questions wearing one name**.

- **Commercial / VAT / consumer characterisation — fact A STRENGTHENS it, materially**, and in a way
  nobody had named: it is the structure that keeps Captain out of **commissionnaire de transport**
  status. Under **C. com. art. L132-1** a commissionnaire acts *in its own name for the account of
  another and is personally bound to the third party*, and organising transport for others is a
  regulated, registered activity (Code des transports; `VERIFY-FIRST` on numbering and on whether
  prepared-meal delivery falls inside). *"The contract is restaurant ↔ partner"* is the cleanest
  available defence to that. **Fact A is load-bearing, and worth papering properly.**
- **Payment-services perimeter — the framing does NOTHING.** PSD2 **(EU) 2015/2366 Art. 3(b)**
  excludes agents acting *on behalf of* the payer or the payee; the perimeter turns on **acts
  performed, not margin taken**, and there is no gratuity exclusion in Art. 3. Gratuitousness is at
  least compatible with a mandate (**C. civ. art. 1986** — le mandat est gratuit sauf convention
  contraire), but **a mandate that exists only in a founder's sentence is unprovable**: the framing
  becomes legally operative only when it is **written down as a mandate, signed, before the first
  payment.** The ADR-134500 chain — one principal on both legs — is the single strongest Art. 3(b)
  argument available and it survives the money resting on the balance. **It does not close the
  question.** *"A gratuitous agent holding a third party's funds is still holding a third party's
  funds."*

### A correction to the founder's own words, which the record must carry

> *"Does not take any benefits from the transaction"* is **not accurate as stated.**

[ADR-20260808-203443](ADR-20260808-203443-tips-voluntary-contributions-funding-model.md) puts the
**platform voluntary contribution inside the order process** — Captain receives money from the
customer, on its own account, in the same checkout. The **shortfall split** is a second benefit,
contingent, from restaurants. Stated to counsel or to a regulator as *"we take nothing"*, that is an
**incomplete description — and an incomplete description is exactly what voids a rescrit guarantee**
(LPF L80 B, `VERIFY-FIRST`).

**The accurate sentence, to be used everywhere instead**: *Captain charges no commission; it solicits
a voluntary contribution from the customer at checkout and may levy a shortfall share on restaurants.*

### The restaurant carrying the refund is ORTHODOX, not a concession — and the consumer right is why

**C. conso. art. L221-15**: the distance-selling professional is *responsable de plein droit* toward
the consumer for performance **whether performed by itself or by other service providers, without
prejudice to its recourse against them.** The restaurant sells a *delivered meal*; a failed delivery
is its non-performance toward the eater whoever dropped the bag. **The founder's answer is correct on
the consumer leg.** Three binding conditions follow:

1. **The consumer may never be routed to the rider or the partner.** A term saying "delivery incidents
   are between you and the courier" is an unfair term and contradicts L221-15. Answer 1 — the delivery
   line is a breakdown of the restaurant's price, not a second purchase — is *the reason* this holds.
   **Keep the two answers welded.**
2. **It is lawful only if the restaurant actually HAS a recourse.** L221-15 gives the seller one, and
   it is worth nothing without a named counterparty, terms, a liability clause and an insurer. **If
   the restaurant has no identifiable contract with the partner, Captain is allocating a loss to a
   party with no remedy** — a *déséquilibre significatif* argument between professionals (**C. com.
   art. L442-1-I-2°**) and a P2B transparency failure.
3. **The refund mechanism is a separate legal act from the refund allocation.** Refunding from
   Captain's balance and netting it off future payouts is **set-off on the restaurant's money** and
   needs an **express written right, a cap and a contestation route**. Without that clause it is a
   debit the restaurant did not agree to.

### The new exposure fact A CREATES rather than cures

Answer 3 keeps **price-setting for Captain's own riders** with Captain, alongside ranked dispatch,
offer timeouts, escalation and availability revoke — while the **restaurant** is the rider's nominal
counterparty and bears the refund. Price-setting and sanction-shaped deactivation are the canonical
control indicia in French requalification case law (`VERIFY-FIRST` on citations), and feed the
Platform Work Directive presumption machinery.

> **Fact A moves the counterparty risk onto restaurants without moving the control off Captain.**
> Co-employment and *tiers responsable* theories live in that gap.

**This is the first euro of legal budget after the funds posture.**

### The two contracts, which must exist and must never be conflated

The repo **contradicts** fact A today: the partner self-registers **to Captain**, availability goes
live only after a **Captain admin** approves, and Captain walks a ranked list with its own timeouts.
No restaurant appears anywhere in that flow. If Captain orders transport **in its own name** it is a
commissionnaire — *"just the tool" is precisely the commissionnaire's position, and a commissionnaire
is a party.* If **in the restaurant's name**, it needs a mandate, and an act beyond the mandate is
*inopposable au représenté* (**C. civ. art. 1156**) with the partner falling back on apparent
authority — against Captain.

| | Contract | Captain's position |
|---|---|---|
| **1** | restaurant ↔ partner — the **transport contract** | **not a party** |
| **2** | Captain ↔ partner — the **platform-access contract**: credentials, admission, revocation, ranking, data transmission, self-billing on the restaurant's behalf | **necessarily a party** |

**The repo builds (2). The founder's answer describes (1). Both are true; the record has neither.**

### Four blockers — illegal or unprovable to launch without

1. **Written mandate, restaurant → Captain**: collect the price, pay the rider/partner, order delivery
   in the restaurant's name, issue invoices in its name (self-billing, **CGI art. 289-I-2** and its
   BOFiP conditions, `VERIFY-FIRST`). **Without it the "tool" posture has no artifact and the PSD2
   Art. 3(b) argument has no mandate to hang on.**
2. **Consumer terms naming the restaurant as seller** — SIREN, address, total price, the delivery line
   as a component, the withdrawal exemption, the complaint route to the restaurant (**never the
   rider**), and a referenced mediator (**C. conso. L612-1**).
3. **Refund allocation + set-off clause**, with the shortfall split in the same document.
4. **Per-line VAT on the stored order** — and **now harder**: the consumer-facing delivery line, being
   a component of the restaurant's price rather than a separate supply, must be **ventilated across
   the basket's mixed rates.**

### Two more, briefly

**The "tool" framing pulls Captain INTO P2B and the DSA — it does not push it out.** The city-ranked
walk **is a ranking** under P2B Art. 5, and `revokeDeliveryPartnerAvailability` is squarely an Art. 4
restriction needing a statement of reasons. And on **GDPR**: if Captain chooses the partner, Captain
determines a means over the address and phone sent to it — an **Art. 26 joint-controller** argument
rather than clean Art. 28 processing.

**Three questions added to the counsel packet**, each answerable in minutes with the context above: in
whose name is the delivery order placed and does it trigger transport registration; does Captain
holding price-setting and revoke while the restaurant is the counterparty create co-employment or
third-party exposure; and does the voluntary contribution plus shortfall share defeat "acting for the
payee only" under Art. 3(b). **Q10, the funds posture, is unchanged and still first in line.**
