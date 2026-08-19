# ADR-20260818-233000 — The founder's ten answers: per head, monthly invoice, stop checkout — and a cagnotte that exists only in prose

<!-- Filename: docs/adr/ADR-20260818-233000-the-ten-answers-per-head-monthly-invoice-and-a-cagnotte-that-exists-only-in-prose.md -->

## Status

Accepted — as a **record of what he decided**. Recording is not authorisation to build: three of the
ten answers reverse or refine an existing record, and those wait on their register rows (§8).

## Enforced by

n/a — no behavioral guarantee

The behavioural guarantees these answers *imply* — `restaurantContribution` pinned to zero, the
`"Aucun"` default, a refusal classified `technical_error` — are **owed**, not created here; each is
named as a candidate card in §9 and each carries its own `rules.yaml` entry + ADR-0032 test when it
lands.

## Consulted

Required by [ADR-20260812-143619](ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md):
a lens never asked is indistinguishable from a lens with nothing to say. All thirteen were asked on
the ten answers; all thirteen returned.

- **architect** — the compounding finding: *"deploy just what changed" × `Recreate`/1-replica × STOP
  CHECKOUT = deploying `actor-payment` at any hour takes checkout down*; and `cagnotte` has **zero**
  hits outside `docs/`.
- **business-specialist** — Q6's new mechanism is **self-reported by the party it prices**, so it
  converges to inert; and the first cover test fires by construction, because the pot starts at zero.
- **legal-specialist** — CRD 2011/83 **Art. 22** fetched verbatim: the prohibited shape is the
  DEFAULT, and the remedy is **reimbursement of every contribution ever collected**. Full return in
  [BRIEF-20260818-pre-filled-contribution-and-the-monthly-invoice](../legal/BRIEF-20260818-pre-filled-contribution-and-the-monthly-invoice.md).
- **dba** — four hops at 0.999 = **99.6 %** availability manufactured by the check itself; better
  signal `MAX(now() - lease_heartbeat_at)` per lane; and corrects the coordinator on LOGIN roles (§10).
- **ux-designer** — the participation must be disclosed on the **discovery card** beside the fee and
  the ETA, and must **vanish** rather than render `0,00 EUR` when the cagnotte covers costs.
- **graphql-architect** — probes contend with the load they report on, so **slow reads as down**; and
  the gate has no legal home in the composed schema. Endorses Q2 unreservedly (static composition).
- **holub** — sequences his five commitments by cost of delay (§11); *"the records are excellent, they
  are also the only thing shipping"*.
- **farley** — change-detection yield today is **zero**, the split build is **slower**, and the
  readiness endpoint the Q3 chain would read is a **boot-time constant**.
- **beck** — the harness must never require all 57; version skew is the first bug class types cannot
  reach; and no test asserts the `"Aucun"` default, which is why a ten-day-old decision could be
  reversed in conversation with nothing going red.
- **young** — the Q6 × Q7 **versioning trap** (one stored field, two meanings, identical shape); and
  if checkout stops, **the stop must be an appended fact**.
- **vernon** — the founder's own `/health` precedent is already **push-shaped**; readiness is a
  published fact, not a question; the system already fails closed — the real defect is no customer
  **verdict**.
- **evans** — **withdraws round-1 fact 5**: `marginRate`'s purpose is legitimate. "Cagnotte" is one
  word doing three jobs (inflow · balance · account).
- **observability-agent** — a closed gate produces zero runs, so **green contract and dead checkout**;
  and the cost side of Q10 can never be a fold.

## Context

On 2026-08-18, the same evening as
[ADR-20260818-210000](ADR-20260818-210000-the-ai-maintained-codebase-premise-prose-is-a-convention.md)
(round 1 — the AI-maintained-codebase premise; not restated here), the founder answered the ten open
questions the team had put to him: one on what actually broke, one on process topology, one on the
peak-hour failure posture, and seven on the money model. Under ADR-20260812-143619 the answers were
relayed verbatim to the whole roster before any record was composed.

The ten answers are **decisions**. What follows records them, then records what the roster returned:
what each choice now requires that nobody has built, what conflicts with something already on the
record, and one mechanism the roster does not accept as described.

## Decision

### The ten answers, verbatim

**Q1 — what broke first?**

> "The smoke test was working on production (render + Supabase)"

**Q2 — processes or crates? → PROCESSES ARE THE POINT**, explicitly accepting the release-path and
connection costs.

> "We will improve the compilation duration with parallelism
> We will deploy just what has been changed instead of delivering the big monolithic for a small change.
> We will be able to ensure that the apps have the rights rights.
> And the most important the split process will ensure that they are really isolated.
> I used to work in this context in more comfortable and easier to understand the system with these separations."

**Q3 — payments worker down at 20:00 Friday? → STOP CHECKOUT.**

> "Like what we did on health check on the database we will do the same between services they depends on.
> The customer graphql app will check the availability of the actors it depends on like the place order process manager
> The place order process manager will check the availability of order actor payment actor and payment worker
> Payment worker will check the availability of stripe adapter"

**Q4 — whose account receives money?**

> "For now no cagnotte created but it will be created once the company will be created"

**Q5 — two lines or one? → THE FALLBACK NEVER TOUCHES THE CUSTOMER**; only restaurants share the
shortfall. (No comment given.)

**Q6 — per head or per order? → PER HEAD**, deleting the margin-proportional mechanism outright.

> "The margin rate was used to compute the participation on the delivery costs to be paid by the customer
> The restaurant will indicate the margin rate and the minimum margin rate the restaurant is ready to have for the contribution on delivery costs to be paid by the customer"

**Q7 — shortfall carrier? → PERIODIC INVOICE ONLY** — monthly, computed after the period closes,
never per order. (No comment given.)

**Q8 — who bears a refund? → THE CAGNOTTE bears all of it.**

> "The refund done by the admin of Captain food will impact the cagnotte. But this case is very exceptional."

**Q9 — HelloAsso framing or mechanic? → FRAMING AND MECHANIC**: a suggested amount is pre-filled and
can be lowered to zero.

> "We will apply the same logic, same mechanism.
> The customers are contributing for the interests of the restaurants and the riders conditions to make the platform free or affordable."

**Q10 — cagnotte sufficiency cadence and figure?**

> "It will be decided monthly with 4 months costs covered in advanced"

### What this ADR decides beyond recording them

1. **Q3's GOAL is accepted; Q3's MECHANISM is not adopted as described.** He decided STOP CHECKOUT
   and that decision stands. Six lenses reject the *four-hop synchronous availability chain* on six
   independent grounds (§4). The mechanism returns for design; the posture does not.
2. **Both Q3 remedies are taken, not blended.** `young` asked explicitly that his remedy not be merged
   with `vernon`'s. They are different fixes to different defects and both are recorded (§4).
3. **Nothing else is resolved here.** The ten register rows in §8 are **named, not decided** — several
   are `HOLD: human` and one (Q9) touches a regulated shape.

## SETTLED / NOT SETTLED

| Q | Lenses | Verdict |
|---|---|---|
| Q1 what broke | holub, farley, beck | **SETTLED** — nothing regressed: production was deliberately suspended and the gate left pointing at it. Scope correction: "working" tops out at L4 (`cart -> placeOrder`, PENDING). `beck` holds that the leg in dispute stays UNANSWERED |
| Q2 processes | farley, dba, graphql, architect, beck | **SETTLED as a decision**; every lens then priced it (§5). `graphql` endorses unreservedly — static composition, no planner |
| Q3 stop checkout | vernon, young, observability, graphql, dba, farley, architect | **GOAL SETTLED, MECHANISM NOT** — rejected by six lenses on six independent grounds (§4) |
| Q4 whose account | legal | **SETTLED** — one funds posture, not two. Narrows the irreducible question; does not close it |
| Q5 no customer fallback | evans, graphql, legal | **SETTLED.** `legal`: *"the cleanest answer of the ten"* |
| Q6 per head | architect, evans, business, ux, young, graphql | **SETTLED on the fee**; his comment opens a NEW undesigned mechanism (§6) |
| Q7 periodic invoice | young, business, legal, beck | **SETTLED** — the only buildable shape. Creates the first Captain-issued B2B invoice in the model |
| Q8 cagnotte bears refunds | business, evans, beck, legal, architect | **NOT SETTLED** — he answered the admin path; the restaurant-approved default is silent, and the bearer has no representation (§7) |
| Q9 pre-fill | ux, legal, beck, business, observability | **SETTLED as his choice**; it is a decision reversal AND a regulated shape |
| Q10 monthly / 4 months | observability, business | **Cadence SETTLED, figure not** — the cost side can never be a fold (§7) |

## 4. Q3 — six independent grounds against the mechanism, and two remedies

His goal is not contested: at 20:00 on a Friday with payments down, checkout must not take money it
cannot process. What the roster rejects is *a chain of synchronous availability questions in front of
`placeOrder`*. The six grounds are set out separately because they are independent — fixing any one
leaves the other five.

1. **`vernon` — readiness is a published FACT, not a question.** His own `/health` precedent is
   already push-shaped: `crates/server/src/lib.rs:259` is a **30 s cached snapshot refreshed by a
   background heartbeat**; the handler reads a mutex and returns. It reports on its OWN dependency and
   never calls another service. All four Ask conditions fail. And the system **already fails closed** —
   no `OrderPlaced` unless the fenced transaction commits. The real defect is that there is no customer
   **verdict**: checkout sits PENDING forever.
2. **`young` — the refusal is not in the fold.** Replay a Friday and you cannot reconstruct why
   checkout refused. **If checkout stops, the stop must be an appended fact.** (Asked explicitly that
   this not be blended with `vernon`'s remedy.)
3. **`observability` — a closed gate is invisible to the contract.** Zero `place-order` runs means zero
   errors and zero budget burn: **green contract, dead checkout**. And if the refusal rides the
   validate span it classifies as `business_rejected`, hiding every payments outage. It must be
   `technical_error`, **never** `business_rejected`.
4. **`graphql` — under load, slow reads as down.** Probes contend with the traffic they report on, so a
   timeout converts a **slow** Friday into a **zero-revenue** one — it stops checkout under load rather
   than under failure. Separately, the gate has **no legal home in the composed schema**: the chain
   spans scopes, nested-cross-scope is forbidden, the gateway holds no state and 400s cross-scope
   documents.
5. **`dba` — the check manufactures the outage it reports.** Four hops at 0.999 = **99.6 %**, the entire
   loss created by the check; **+80 ms** on the ETA-bearing path; and a 10 s cache reports the worker up
   for up to 9 s after it died, so it does not even deliver the STOP. **Better signal**:
   `MAX(now() - lease_heartbeat_at)` per lane from the mailbox — it measures the worker's last
   **durable act**, catching a worker that answers 200 while its lane has not advanced.
6. **`farley` + `architect` — the endpoint does not exist.** What the chain would read is a boot-time
   constant (`let db_ready = !config.database_url.is_empty();`, 42 of 57 bins), and **32 of 57 bins
   have no `kind: Service`** — four of the six components in the chain are **unaddressable**.

**Both remedies are taken.** `vernon`'s: readiness becomes a *published* fact each component asserts
about itself, consumed without asking. `young`'s: the refusal is *appended*, so a replay can say why.
They are complementary and neither substitutes for the other.

**Register**: six lenses independently name
[ADR-20260810-231300](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md).
Its monitoring carve-out does not cover this: the carve-out's own limit is *"the observer is outside
the system it observes and has no durable record to reconcile against"*, and a checkout gate is inside
the system and has the mailbox to reconcile against.

## 5. What Q2 (processes) requires — measured, with antecedents

Each line names its antecedent and the lens that derived it. He accepted the release-path and
connection costs; these are the costs, stated so they are paid deliberately.

| Finding | Antecedent | Lens |
|---|---|---|
| **Change-detection yield is zero** — 20 of the last 20 commits touching `crates/**` in 30 days touched a shared core crate; 45 of 57 bins depend on `bin_runtime`, and the 8 `graphql-*` link the monolith as a library (their own Cargo comment). "Deploy only what changed" is delivered by cutting the shared-core edge (#475, #423), not by the topology | `git log`, Cargo manifests | farley |
| **The split is SLOWER today** — `build-bins.yml:122` `timeout-minutes: 350`, one runner, sequential loop, against `build-image.yml:51` `timeout-minutes: 40` for the monolith. The matrix fan-out is a **precondition** of his reason #1, not a follow-up | `build-bins.yml:122`, `build-image.yml:51` | farley |
| **The rights and the isolation are not in the emitted topology** — `serviceAccountName` on **0 / 57**; NetworkPolicy resources under `deploy/`: **0**; all 45 `DATABASE_URL` entries resolve to the **same** secret key. `specs/database/databases.yaml` declares owner roles; nothing emits a per-bin DSN | `deploy/**`, `specs/database/databases.yaml` | farley |
| **The only silent cutover-day failure** — the per-bin readiness probe is a boot-time bool, while the monolith's real gate (503 `schema_behind` when `applied_version < REQUIRED_SCHEMA_VERSION`) is referenced by **zero** of the 57 bin crates | `crates/server/src/lib.rs:1549` | farley |
| **The compounding chain**: all 54 Deployments are `replicas: 1`, `strategy: Recreate`. **"Deploy just what changed" × Recreate/1-replica × STOP CHECKOUT = deploying `actor-payment` at any hour takes checkout down.** Each decision is defensible alone; together they compose an outage. The fix is leases/fencing maturing enough to lift `Recreate` (#193, #242) | `deploy/**` manifests | architect |
| **The mailbox payload becomes a published contract** — `inbound_messages` has **no version story** (`grep -rn "upcast"` returns one prose hit). N processes make it durable in a table, so a rollback does not undo it | `inbound_messages` schema | architect |
| **Version skew is the first bug class types genuinely cannot reach** — one release-time pairwise gate between each actor's declared inbox and its deployed peers, not a wall of tests | — | beck |
| **The connection arithmetic on the record is wrong by two factors** — **45** db-needing bins (42 always-on), not 37, and **LISTEN connections sit outside the pool** (`PgListener::connect`, not `pool.acquire()`). Redone: **~330 steady vs `max_connections: 220`**. Fix: split connection classes — LISTEN (55, fixed) direct; everything else through one CNPG Pooler in **transaction** mode (179 client → ~76 server), needing a new `DATABASE_LISTEN_URL` key, because LISTEN through a transaction pooler silently delivers nothing. `PROP-20260811-093000` §8.3's recorded recommendation (session mode) **multiplexes nothing** in front of long-lived sqlx pools | `PROP-20260811-093000` §8.3 | dba |
| **The node shape this needs** — 1 db + 2 app + LB = **EUR 67.80/mo ex-VAT** (LB S = EUR 6.00, d2-8 = EUR 20.60, from ADR-20260807-114122's confirmed catalog prices). Numerically the figure he rejected on 2026-08-07 — but that was **HA redundancy for a single-instance app**; this is **capacity for 57 processes he has just chosen**. HA is the EUR 109 rung and is not being asked for | ADR-20260807-114122 | dba |
| **"Really isolated" is not a property the 57 have today** — `specs/generated/apps.generated.md` measures **49 of 57 as FAT**: they link domain crates their manifests never declare. The process boundary is not what would give it to them; the crate boundary already delivers the compile-time half | `specs/generated/apps.generated.md` | holub |
| **The harness must never require all 57** — a named subset only: an all-or-nothing harness gets skipped, and a skipped harness reports `ok`. **Upside nobody else named**: process boundaries are fault-injection seams for free — the harness can kill `actor-payment` and watch checkout refuse, which is Q3's own acceptance test and close to unwriteable in a monolith | — | beck |

## 6. Q6 — the re-ruling, and the Q6 × Q7 versioning trap

### The versioning trap (`young`)

Q7 makes `restaurantContribution` **permanently zero**. Q6 creates a **new per-order money line**. A
future implementer looking for a per-order `Money` field with "contribution" in the name will find the
zeroed one and fill it — **one stored field, two meanings, identical shape**. No upcaster can
distinguish them, and every fold over the field becomes silently wrong for all time. The same trap sits
one level up on `marginRate`: same name, same shape, new meaning.

Compiler-first fix: a `ZeroMoney`-typed member, so the field **cannot be spelled** non-zero — in which
case the test becomes redundant, which is the correct outcome (CLAUDE.md, ADR-20260803-234035).

### What dies, what lives, what is new undesigned surface

- **DELETED** — the Captain service-fee machinery: the clamp formula in `specs/network/scalars.yaml:81-87`
  and its shipped SDL description; `PricingPolicy`'s calibration surface (`fee_rate`, `buyer_share`,
  `margin_low`, `margin_high`) with its seed, admin query and read repo; ADR-0016 superseded. **Not
  free**: `PricingPolicy` is `replicated: read-databases` and #509's restore drill asserts its seeded
  row counts.
- **KEPT, meaning changed** — `marginRate` and its collection path. **`evans` withdraws round-1 fact 5**:
  the purpose is legitimate, it is not a fee input. But the write policy flips (see §7 live defect 3),
  and a meaning change on a stored shape is CLAUDE.md **question (2)**.
- **NEW, undesigned** — a second scalar for the minimum (named for what it **governs**, not its unit —
  `evans`); a **delivery cost the domain does not model**; a **third money term**, because
  `riderPayout = delivery` becomes false under a partial participation; and **when** it is computed — a
  pre-order quote on the checkout hot path, or a tariff table read at pricing time. That last is a
  genuine option space → **a proposal, not an ADR**.
- **`business` prices it**: the mechanism is **self-reported by the party it prices**, so it converges
  to inert — a restaurant declares `minimum_margin = current_margin`, absorbs zero, and the customer
  pays 100 % of delivery. Strictly better instruments already exist in the trade: a flat per-order
  delivery subsidy, or a **free-delivery threshold**, which also lifts the basket — same economics, no
  secret disclosed, explainable on the pedagogical receipt. And the variance is **unexplainable by
  construction**: the same customer at the same address sees a different participation per restaurant,
  for a reason we have promised never to publish.
- **`ux`**: it must be disclosed on the **discovery card**, beside the delivery fee and the ETA, not
  first seen at checkout (`/tarifs` promises *"0 frais caché"*); and when the cagnotte covers costs the
  line must **vanish**, not render `0,00 EUR` — the switch-on month needs its own disclosure or it
  reads as a stealth fee.

## 7. Q8, Q10, and the live defects the round surfaced

### Q8 — the bearer has no representation

- **`beck`**: the only candidate figure is `captainNet`, recorded **zero at V0**
  (ADR-20260818-134500:114, `STATUS.md:70`). A refund cannot be borne by a quantity defined to be zero.
  The test cannot be written — a design finding, not a testing one.
- **`evans`**: "cagnotte" is one word doing three jobs — the **inflow** (already modelled as
  `TipRecipient: CAPTAIN` / `OrderTipped` / `captain_tip_cents`, and a *tip* is gratuity for a person
  while a *contribution* funds a common good, a different VAT question), the **balance** (genuinely new:
  a two-sided fund that can go negative, not a sum-of-events fold) and the **account** (Q4's meaning).
- **`business`**: the pair keys the bearer on **who pressed the button** rather than **what happened**,
  which makes not-deciding the cheapest restaurant strategy — stall until the customer escalates and the
  cost moves to the mutual pot. Recommends **bearer-follows-CAUSE**, reusing the typed
  `DeliveryFailureCause` already owed by ADR-20260818-161500.
- **`legal`**: it **removes** the set-off problem entirely (nothing is debited from the restaurant, so
  no set-off clause is needed — a real reduction in the terms). But a pooled fund compensating the
  public for a risk has the silhouette of an insurance operation **the moment the CGU promise it**. Free
  design consequence today: keep the refund **discretionary** in the terms, never a promised indemnity.
  And a public refund line naming WHICH restaurant caused it is a reputational disclosure and, for a
  sole trader, personal data about a named natural person — **aggregate it**.

### Q10 — what the cover test needs

- **The cost side can never be a fold** (`observability`): OVH, Stripe fees, Honeycomb and insurance
  arrive as external invoices, and *"four months in advance"* is a claim about the FUTURE while a fold
  only sees the past. The cost must be an **appended domain event** — a platform-cost declaration, its
  declarer on `domain_events.user_id` per ADR-0041 — **not a config key**, so the runway the platform
  BELIEVED it had when it priced a cascade is reconstructible. Plus a second measure: declared-vs-realised
  drift.
- Gauges must be **re-asserted every export cycle**, with an evaluation heartbeat alerting on the
  ABSENCE of increment, and a `cagnotte_cost_declaration_age_days` staleness gauge. In-repo precedents:
  `otp_send_guard_enforcing`, `payment_birth_gap_sweep_heartbeat_total`.
- **`make validate` will block**: a business metric's `activity:` is a mandatory `$ref` into
  `specs/stories.yaml`, and there is no admin funding activity and no `public_user` activity for a
  public cagnotte page. Every business metric today reads through a **tenant-scoped** GraphQL query; the
  cagnotte is platform-wide and anonymous — a read shape BAM does not have.
- **The first test fires, guaranteed** (`business`): the pot starts at zero (Q4), so cover at month 1 is
  0 < 4 → cascade; and per head means the invoice is **largest when the restaurant count is smallest**.
  Two cheap fixes, and they must be **PUBLISHED, not recorded**: seed the cagnotte with four months up
  front, and/or a declared pilot grace window with a public end date. Plus a **published EUR ceiling per
  restaurant per month** — without a cap, a departure raises everyone else's share, which raises the
  next departure's probability.
- **The Q8 × Q10 coupling** (`business`): the cagnotte bears refunds **and** the cagnotte balance fires
  the cascade ⇒ **every refund brings forward the date restaurants start paying**, and each
  restaurant's bill is partly a function of other restaurants' failures.
- **The upside** (`business`): a live per-restaurant forecast — *"if the month closed today your share
  is 14 EUR; it was 19 EUR before Chez Marco joined"* — turns the cascade into restaurant-led
  acquisition. Same number, shown continuously instead of as a month-end invoice.

### Live defects surfaced, independent of tonight's answers

1. **`tip_amount_selector` has no renderer** — registered at `crates/web/src/generated/registry.rs:122`,
   emitted by both screens, matched nowhere in `crates/web/src/renderer.rs`; it falls through to the
   catch-all at `:581` producing a bare div. Used for `courier_tip` **and** for `refund_amount`, the
   partial-refund money picker (`specs/screens/restaurant_backoffice.yaml:437-444`). CLAUDE.md: *"a
   control that renders but does nothing is worse than no control."* (`beck`)
2. **The customer reads the restaurant's payout** — `Cart.breakdown` / `Order.breakdown` are
   CUSTOMER-readable and return the eight-field type including `restaurantContribution`,
   `restaurantPayout`, `captainNet` (`specs/generated/schema.generated.graphql:482-491`). `roles` is
   operation-level and `navRoles` nav-edge only, so **only a type split can stop it**. (`graphql`)
3. **The ADMIN-only guarantee on `marginRate` was never enforced** — `specs/network/api.yaml:207-209`
   gives `updateRestaurant` to `RESTAURANT_ACCOUNT` with **no field-level restriction**; only the screen
   omits the widget, while `restaurant_backoffice.yaml:502` asserts the control. (`architect`)
4. **No test asserts the `"Aucun"` default** — which is why a ten-day-old decision could be reversed in
   conversation with nothing going red. The founder's own round-1 thesis, demonstrated on his own
   decision. (`beck`)
5. **Delivery money does not exist in the domain** — `grep "Money\|Cents" specs/delivery/*.yaml` returns
   zero hits; there is no `DeliveryFee` scalar or entity field. The only term is the kernel leg
   `PaymentBreakdown.delivery`, pinned by `riderPayout = delivery`. (`architect`, `evans`)
6. **The published promise lives where no gate can see it** — *"jamais un pourcentage sur ta marge"*
   appears only in `docs/STATUS.md` and the round-1 ADR, nowhere in `specs/**`. The formula it forbids
   **is** in the specs and **is** shipped through introspection. (`evans`)

## 8. The convergent closing finding — the published promise has no artifact in this repo

Three lenses reached this independently, in their own final checks, without seeing each other's returns.

- **`architect`**: `cagnotte` appears in **six files, all under `docs/`** — three ADRs, two briefs,
  `DECISIONS.md`. **Zero hits in `specs/**`, `crates/**` or `tools/**`.** So the thing Q8 says bears
  every refund, Q10 judges monthly against four months of covered costs, and Q5/Q7 make the sole funding
  source before restaurants are billed **is not a modelled concept anywhere in the system** — no
  aggregate, no event, no projection, no scalar, no metric. Under the round-1 premise it is a
  **convention** carrying three of his ten answers.
- **`ux`**: *"sans jugement"* — and the whole `/tarifs` text — exists nowhere in the tree except as a
  quotation inside ADR-20260818-210000. So when the reversal lands and the module ships, **no validator,
  test or grep can detect that the shipped French copy and the public page have diverged**; the only
  mechanism is a human remembering to open a website. The checkout copy will live in
  `specs/screens/restaurant_frontoffice.translations.yaml`; the sentence it must not contradict lives in
  a marketing CMS, edited by someone else on a different clock.
- **`legal`**: there is **no repo-held copy of the published page** — it is GitHub Pages via CNAME
  (ADR-0036), outside this tree. Three consequences: (1) the Q5 and Q7 page corrections **cannot be
  closed by a diff in this repo**, and `make validate` stays green while promise and model diverge, so
  each register row must name the **page** as the artifact and someone must confirm it changed; (2) the
  evidence artifact for a pre-contractual statement is a **dated capture**, and an ADR's verbatim block
  is an agent's transcription, not a capture; (3) it sharpens the Q9 card — the per-order consent
  artifact is the only one of the three surfaces that would sit **inside** this repo's gates.

## 9. The ten register rows owed — named, none decided

Each row below names the record it conflicts with and quotes it. **No lens argued the merits and this
ADR decides none of them.** Several are `HOLD: human`; Q9 also touches a regulated shape.

1. **Q9 vs [ADR-20260808-203443](ADR-20260808-203443-tips-voluntary-contributions-funding-model.md):64**
   — the recorded default is `"Aucun"`. And vs the published page: *"0 € possible, sans jugement"*, with
   *"ajouter … s'il le souhaite"* — opt-in wording, where a pre-fill is opt-out. **Two records, not one.**
2. **Q6's second field vs `specs/screens/restaurant_backoffice.yaml:502`** — *"an UpdateRestaurant field
   the owner must NOT set … back-office/ADMIN only"* — **and vs ADR-0028 §4** — *"back-office only — not
   exposed on the public Restaurant type"*. Note for the row: the claim was **never enforced**
   (`specs/network/api.yaml:207-209`).
3. **Q8 vs [ADR-20260818-150000](ADR-20260818-150000-captain-is-the-tool-the-restaurant-carries-the-delivery.md)**,
   verbatim: *"the restaurants will carry the refund in case of problem **not the rider neither the
   platform**"*. The cagnotte is the platform. Same day, same decider.
4. **Q3 vs [ADR-20260810-231300](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)**
   (no polling, only pushing), **and vs ADR-20260720-015500 acceptance-first** — `young`: a readiness
   precondition in front of `placeOrder` narrows *"accepted onto the mailbox, answered PENDING"*.
5. **Q10 vs ADR-20260808-203443 §1.2**, which fixes the trigger as *"contributions fall short"*; a
   four-month **runway** test is a stricter and different trigger. Refinement or reversal is the
   register's call.
6. **Q5 vs `/tarifs`** — *"Côté client — une petite participation aux frais de fonctionnement à la
   commande"* is deleted by his answer. A change in the consumer's favour, but the page must be
   corrected: leaving it **reserves a charge the company has decided never to make**.
7. **Q7 vs the `/tarifs` headline** — *"0 € d'abonnement"* beside a recurring monthly invoice. The
   fallback paragraph reconciles the substance; the headline does not.
8. **Standing** — `/tarifs` says *"association à but non lucratif"* while ADR-20260808-195315 records a
   **SASU**. Q4's *"once the company will be created"* is the moment this must resolve. And
   `docs/STATUS.md:2165` records that the site **already publishes an association and an RNA number** —
   if that association publishes a page soliciting contributions, part of this is **live, not future**.
9. **Age flag** (`architect`) — *what a customer-facing money line on a Captain surface legally is* has
   been open across **three** founder exchanges (08-08, the 08-18 invoice chain, tonight) with **no
   register row of its own**.
10. **The LOGIN-roles correction** (`dba`) — per-persona LOGIN roles are a **recorded rejection**
    (PROP-20260818-010343 §13 D-A) on the connection ceiling; nothing in the founder's answers reopened
    them, and the chosen design (NOLOGIN + `SET LOCAL ROLE`) costs **zero** additional connections,
    because the role change is transaction-scoped and co-terminous with a transaction-pooler lease.
    Related and worth a row of its own: `rls_matrix.rs:551` uses `set_config('app.member_id', $1, true)`
    — transaction-local. Had that flag been `false`, a transaction pooler would leak one caller's
    identity to the next transaction on the same server connection, silently.

## 10. Coordinator defects banked (ADR-20260816-134352)

**First defect, attribution `card defect` — not roster width.** The relay card asserted that per-persona
LOGIN roles worsen the connection budget, carrying a round-1 remark forward as though the roles were on
the table. They are a **recorded rejection** (PROP-20260818-010343 §13 D-A) that no founder answer
reopened, and the chosen NOLOGIN design costs zero extra connections. Caught by `dba`.

Because the attribution is a card defect, this does not go to the founder: only a miss attributed to
**roster width** would, and under
[ADR-20260817-105845](ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)
a MISS no longer reverts a class automatically. Round 1's defects are banked in
[ADR-20260818-210000 §*Coordinator defects banked*](ADR-20260818-210000-the-ai-maintained-codebase-premise-prose-is-a-convention.md#coordinator-defects-banked)
— every one of them attributed to the **card-defect class**, none to roster width, and the last of
them **self-reported by the `ux-designer` lens** rather than found by the coordinator. That section is
the list; it is deliberately not restated as a count here, and none of it is re-banked here.

**Second defect this round, attribution `card defect`.** The card that commissioned
[BRIEF-20260818-pre-filled-contribution-and-the-monthly-invoice](../legal/BRIEF-20260818-pre-filled-contribution-and-the-monthly-invoice.md)
named an **aggregation** (the coordinator's summary of `legal-specialist`'s return) as the source for
material it described as **transcription**, so the executor **authored** a legal artifact's counsel
questions and obligation map and attributed them to the lens. Caught by the executor, who flagged it
rather than shipping it silently; corrected in revision 2 of that brief, which now carries the lens's
own words. Executable fix the executor proposed: **a card that says "carry lens X's return" must point
at the lens's own return, or say plainly that the executor is composing** — the two are different
deliverables, and only one of them may be attributed.

## 11. The roster's proposed cards — candidates for the architect to rank

**Not decisions, not approvals, not dispatched.** Ranking is the architect's under
ADR-20260810-215503; a `Priority` is not an approval.

| Lens | Card | Class |
|---|---|---|
| farley | make the per-bin readiness probe a real verdict once in `bin_runtime` (pool round-trip + `REQUIRED_SCHEMA_VERSION`, 503 `schema_behind`) so all 57 inherit it | REVERSIBLE INTERNAL |
| architect | pin `serviceFee` and `restaurantContribution` to zero as a `rules.yaml` invariant + ADR-0032 test, stripping the clamp formula from `MarginPercent`'s description in the same change | REVERSIBLE INTERNAL |
| young | pin `restaurantContribution` to zero as a RULE with its test; better, a `ZeroMoney` type making it unspellable | HOLD: human |
| dba | make the connection budget derived and executable — `DATABASE_LISTEN_URL`, a CNPG transaction Pooler, per-family pool ceilings, and a codegen test asserting the derived total under `max_connections` — in ONE commit | HOLD: human |
| vernon | give `PaymentSettlementProcess` its own `state_table:`, opened at authorization by the leg that creates the hold, and repoint all four settlement legs off `View_OrderTracking`. Closes STO-9's head-of-row BEFORE the split creates it | HOLD: human |
| observability | pre-author the `checkout-readiness` contract before the gate is designed — `checkout.readiness` span, `checkout_blocked_total{dependency,reason}`, `checkout_gate_open` re-asserted every cycle, and the rule that a refusal is `technical_error`, never `business_rejected` | HOLD: human |
| beck | the gate ships shadow-first, default OFF, with its typed error, its counter and a FOUR-ARM test (all-ready accepts; unrelated dependency down still accepts; probe timeout distinguished from NOT_READY; stale snapshot forces the fail-open/closed decision) | HOLD: human |
| legal | encode the contribution's **consent artifact** before any contribution screen: its own home (not `serviceFee`, not `TipRecipient: CAPTAIN`), carrying the presented default, the chosen amount and an affirmative-act flag; a declared default-selection property on the selector; a contribution-specific reimbursement route | HOLD: human — blocked on the Q9 register row |
| ux | the contribution-module journey spec as design input to the reversal record, with tap parity, sticky zero, no second ask and one word on the receipt as testable rules | HOLD: human |
| ux | mirror the binding `/tarifs` clauses into the repo beside the surfaces they constrain — *"0 € possible, sans jugement"* · *"0 frais caché"* · *"jamais un pourcentage sur ta marge"* · *"0 € d'abonnement"* — so a change to either side is visible in a diff. Checked artifact or merely recorded is the register's call | REVERSIBLE INTERNAL |
| business | declare the cover contract: a platform cost run-rate configuration key + a `bam` fold answering *"how many months does the cagnotte cover?"* and *"contribution run-rate / cost run-rate?"* | REVERSIBLE INTERNAL (the public half is HOLD: human) |
| evans | one word per concept in the money model, with the explicit prohibition that `marginRate` is not re-pointed at the delivery meaning | REVERSIBLE INTERNAL (what it constrains is HOLD: human) |
| graphql | split `PaymentBreakdown` into a buyer-facing `CheckoutBreakdown` and a settlement type, additively (add + `@deprecated`), retiring the live exposure of a restaurant's payout to its customers | HOLD: human |
| holub | walk clauses 4-5-6 (accepted → delivered → captured) as an L5 leg against the one-database monolith target, RED first; label the output a reading, never acceptance | REVERSIBLE INTERNAL |
| beck | resolve `tools/walk/` — it exists, or `prod-smoke.yml`'s comment is corrected in the same change | REVERSIBLE INTERNAL |
| beck | `tip_amount_selector` renderer — independent of tonight, and it covers the partial-refund money picker | REVERSIBLE INTERNAL |

## 12. The roster's recommended sequencing (`holub`, by cost of delay)

Presented as **the roster's recommendation**, not a decision:

1. **Q6's breakdown shape first** — the only one that gets more expensive by waiting; stored event
   shapes are immutable and the free window closes at the **first real order**, not at the walk.
2. **Q9's register row** — cheap now, and its legal instrument has external lead time. The build waits
   on the row; the walk runs on the recorded `"Aucun"` default. Flipping a default later is textbook
   gate-then-stabilize.
3. **Q7's splits** — the shortfall CLAUSE must be in restaurant terms before the first signature (a legal
   precondition, and slice content, not waste). The invoicing RUN is the safest defer on the whole list.
4. **57 processes after the walk reads**, behind `bin_runtime` decomposition — otherwise the split is
   nominal.
5. **The availability chain last** — not a decision but a consequence of one; it cannot be designed
   before the split exists.

Flow, with antecedents (`holub`, from `git log`): **121 commits since 2026-08-11 — 20 touching
`crates/`, 101 touching only `docs/` and `specs/`**; eleven ADRs dated 18 August alone. *"The records
are excellent. They are also the only thing shipping."*

## Alternatives considered

- **Record the ten answers only, and leave the roster's returns in the session.** Rejected: GitHub and
  the session are never the record, and three of the findings (the Q6 × Q7 trap, the compounding deploy
  chain, the Art. 22 exposure) are exactly the kind that is expensive to rediscover.
- **Resolve the ten conflicts inside this ADR.** Rejected: each is a decision reversal or refinement and
  belongs in the register (CLAUDE.md question (1)); several are `HOLD: human` and one touches a
  regulated shape.
- **Record Q3 as rejected.** Rejected as a misstatement of what happened: he decided STOP CHECKOUT, and
  that posture stands. Only the four-hop synchronous mechanism is contested.
- **Dispatch the roster's cards now.** Rejected: ranking is the architect's, and an agent must never
  re-rank to make its own recommendation legitimate.

## Consequences

### Positive

- Seven of the ten questions are closed enough to design against, and the money model has a single
  funding source, a single fallback carrier and a single refund bearer.
- Q5 and Q7 are strict improvements for the two parties who cannot negotiate: the customer never carries
  the fallback, and no per-order surprise can appear on a restaurant's statement.
- The Q6 deletion removes a shipped formula that contradicted a published promise — the promise wins.
- The `/health` precedent `vernon` found means the push-shaped answer to Q3 already exists in this repo
  and does not have to be invented.

### Negative

- **The cagnotte is a convention.** Three of the ten answers rest on a concept with zero presence in
  `specs/**`, `crates/**` or `tools/**`.
- **Q9 reverses a ten-day-old recorded decision and lands on a regulated shape**, with no test guarding
  the default it reverses.
- **The Q2 costs are real and now owed**: a slower build, an unenforced isolation story, a connection
  budget over the ceiling, and a compounding deploy/outage chain.
- **The Q8 × Q10 coupling** makes each restaurant's bill partly a function of other restaurants'
  failures — a fairness property nobody has decided to accept.

### Follow-up actions

- File the **ten register rows** in [DECISIONS.md](../proposals/DECISIONS.md) — named in §9, decided
  nowhere. Q9, Q8, Q3 and Q6's stored-shape half are `HOLD: human`.
- Rank the §11 cards under [BACKLOG.md](../BACKLOG.md)'s method; no bucket moves to make an AMBER item
  dispatchable.
- Counsel questions **G1–G7** and the Art. 22 exposure:
  [BRIEF-20260818-pre-filled-contribution-and-the-monthly-invoice](../legal/BRIEF-20260818-pre-filled-contribution-and-the-monthly-invoice.md).
- The Q6 *when-is-it-computed* option space (pre-order quote vs tariff table) → a **proposal**, with its
  tracking issue, not an ADR.
