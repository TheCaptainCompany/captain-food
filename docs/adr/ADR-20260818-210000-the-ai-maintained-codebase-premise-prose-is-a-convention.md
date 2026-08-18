# ADR-20260818-210000 — The AI-maintained-codebase premise: a rule that lives only in prose is a convention

<!-- Filename: docs/adr/ADR-20260818-210000-the-ai-maintained-codebase-premise-prose-is-a-convention.md -->

## Status

Accepted

## Enforced by

n/a — no behavioral guarantee

## Consulted

Required by [ADR-20260812-143619](ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md):
a lens never asked is indistinguishable from a lens with nothing to say. All thirteen were asked; all
thirteen returned.

- **architect** — ask #2 SURVIVES: *"service fee in Stripe"* names the money's **transport**, while the
  kernel models a **price**, so the two stories still disagree in code.
- **business-specialist** — the **naming** half of ask #4 is WITHDRAWN (the public page states the
  cascade first, plainly, with its exit condition); the **measuring** half survives, because no fold
  watches the bet.
- **legal-specialist** — ask #5 MODIFIED, the spend WITHDRAWN: replaced by acts costing zero that only
  work while the event log is still empty, and keeping the boundary *encode the facts, never the
  conclusion*.
- **dba** — WITHDRAWS its own ask #7 (it quoted the index file instead of the register); holds that the
  database rejects **unrepresentable** states, never **wrong** ones.
- **ux-designer** — ask #3 MODIFIED and partly withdrawn: [ADR-20260808-203443](ADR-20260808-203443-tips-voluntary-contributions-funding-model.md)
  settled it ten days ago, and two thirds of its own factual premise was false.
- **graphql-architect** — concedes its enforcement mechanism was *"a reviewer catches the schema
  smell"*, which is exactly the baseline the founder has rejected.
- **holub** — ask #1 SURVIVES and is **NOT ANSWERED**; enforcement is not ceremony competing with
  review, it **is** the review — but it substitutes for a missing **reviewer**, never a missing **user**.
- **farley** — ask #1 SURVIVES and is NOT ANSWERED; under an autonomous maintainer, **gate honesty**
  becomes the most load-bearing property in the system.
- **beck** — concedes structural drift entirely (tests are the wrong instrument for asserting
  structure); holds that an enforcement boundary the enforced party can move in the same commit is a
  convention with extra steps.
- **young** — ask #6 SURVIVES, orthogonal to authorship: the split converts today's latent violations
  into **peak outages**, not compile errors.
- **vernon** — ask #6 MODIFIED: the remedy must be a **type**, not a diagram; and the split is drawn on
  **layers**, not aggregates.
- **evans** — ask #2 SURVIVES; concedes knowledge crunching assumes a human carrying meaning between
  sessions and there is none, so a name whose meaning lives in an ADR and not in a type **is a
  convention**.
- **observability-agent** — nothing to withdraw; raises one new narrow ask, because every enforcement
  layer converts a runtime-visible failure into a layer-local silent rejection.

## Context

On 2026-08-18 the founder invited the team's asks himself:

> "Let them ask what they want to change and I will be able to explain them and potentially improve
> the situation"

Seven asks went to him, one per lens or lens pair. He disagreed with six and answered the seventh with
an *improve*. His **reasons**, not his verdicts, were then relayed verbatim to the whole roster under
ADR-20260812-143619, with one question each: does your position **SURVIVE** his reasoning, is it
**MODIFIED**, or is it **WITHDRAWN**? All thirteen lenses returned.

| Ask | Lens(es) | Verdict | One-line reason |
|---|---|---|---|
| 1 — one order end to end | holub, farley (+beck echo) | **SURVIVES — NOT ANSWERED** | Coordinator card defect: the ask was a green harness walk; he answered "show a real order to real users". |
| 2 — one money story | architect, evans | **SURVIVES** | "service fee in Stripe" names the TRANSPORT; the kernel models a PRICE. |
| 3 — contribution journey | ux-designer, graphql-architect | **MODIFIED / partly withdrawn** | Already decided in ADR-20260808-203443; the team re-litigated a settled record. |
| 4 — name the cascade | business-specialist | **naming half WITHDRAWN, measuring half SURVIVES** | The public page states the cascade first, plainly, with the exit condition. |
| 5 — funds posture step | legal-specialist | **MODIFIED — spend withdrawn** | Replaced by acts costing zero that only work while the log is empty. |
| 6 — money process ownership | young, vernon | **SURVIVES (young) / MODIFIED (vernon)** | Orthogonal to authorship; but the remedy must be a type, not a diagram. |
| 7 — resilience doctrine | dba | **half SURVIVES, half WITHDRAWN** | Doctrine text lags the register; the drill is not runnable — no cluster exists. |
| — | observability-agent | **new narrow ask** | The DB-rejection path needs a contract; its healthy state is silence. |

One of the seven answers is not an answer to its ask at all: it is the premise the whole exchange
turns on, and it is addressed to more lenses than the two who asked.

## Decision

### The founder's answer #6, verbatim

Asked to draw the money flow as ONE owned long-running process (`delivered → captured → settled →
self-billed`) with durable state and compensation, before building more of it:

> "In a non AI context I will agree and that's what we already did it actually and it worked on
> production with the smoke tests but we had a lot of very bad ai errors that's the reason why I
> have put in place enforcement and split and security because layers was just conventions now it
> will be compilation and controlled. We need to integrate the fact that the literature around
> software does not consider the ai coding autonomously context. I have to remind you that I'm not
> able to maintain rust, only the ai will be able to do it so we have to think differently than
> what has been done is the past in terms of coding organisation."

### The founder's answer #7, verbatim

Asked to make the resilience doctrine match reality and run the restore drill once:

> "We have tested render and Supabase and we learned that it's too expensive for the outbound
> bandwidth and the database size was too small and too expensive. We also realised that the ai
> took a lot of shortcuts to make the app works without respecting the layers. That's the reason
> why I have decided to go to OVH where the outbound bandwidth is unlimited and free, we can put in
> place Kubernetes as a service there and we will be able to deploy micro services because the
> system will be completely split with a lot of crates to ensure that no shortcuts will be taken by
> the ai, also we will put in place security at the app and database level to ensure that even with
> backend ai errors the database will reject the operations."

### The premise, stated plainly

**No human maintains this Rust.** The founder cannot, and says so. Compilation-level enforcement, the
crate split and database-level rejection are therefore the **deliberate substitute for a human
maintainer** — not belt-and-braces on top of one. They were chosen **empirically**: the conventional
layered approach was tried here, shipped to production, passed its smoke tests, and produced AI errors
that layering-as-convention could not catch. The published software literature was written for teams
whose maintainer is a person, and does not account for this context. That is not an opinion the team
may argue past; it is the standing context every lens now argues **inside**.

### The rule this repo adopts from it

**A rule that lives only in prose is a convention, and this repo has decided conventions are not
enough.** It is the founder's own argument turned into a working rule, and several lenses adopted it
against their own published doctrine.

In practice, from here:

1. A lens proposing a guarantee **names the executable form it takes** — a type, a constraint, a gate,
   a generated artifact — or states plainly that it has none and why prose is the only reachable form.
   "It is written in the ADR" is a statement about a convention.
2. **The order stays compiler-first** ([ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md)):
   a type that makes the mistake unspellable beats a check; a check beats a bullet. This ADR supplies
   the *why* that ranking now rests on.
3. **The substitution is bounded, and the bounds are part of the record** — see the next two sections.
   Enforcement replaces a reviewer's eye on **shape**. It replaces nothing about **desire**,
   **semantics**, **users** or **feedback**.

### What this ADR does NOT decide

- **Ask #1 is recorded as UNANSWERED, not as a disagreement.** Its wording turned a request for a green
  end-to-end walk in the local harness into what read as a request to demo a real order to real users,
  and he answered the second. holub, farley and beck ruled that independently. It goes back to him
  corrected; his DISAGREE on it stands against a question he was not asked.
- **No money-model item is resolved here.** The `captainNet` zero-versus-contribution conflict, the
  `serviceFee` naming, the published margin formula and the per-head/per-order mechanism are named as
  **open** below and decided nowhere in this record. Several need their own register rows and, in at
  least the stored-shape and legal-surface cases, `HOLD: human` dispatches.
- **No backlog Priority bucket or row position changes** on the strength of this record.

## What the roster conceded to answer #6

This is the part of the exchange worth keeping. Each concession is a lens giving up ground its own
published doctrine had held.

- **holub** — "crate splits, compilation enforcement and DB-level rejection are not ceremony competing
  with review; **they are the review**. I was measuring against a baseline he has deliberately
  rejected." Its objection had priced enforcement as overhead **added to** human review; there is no
  human review to add it to.
- **evans** — concedes that knowledge crunching, the core of his method, assumes a **human carrying
  meaning between sessions**, and that here there is none. He turns his own words back on his own
  deliverable: a name whose meaning lives in an ADR and not in a type **is a convention**. Ubiquitous
  language must therefore be spelled in scalars and newtypes, not in glossaries.
- **graphql-architect** — concedes its whole enforcement mechanism was "a reviewer catches the schema
  smell in review", which is the rejected baseline restated. Schema discipline has to become an
  emitted, committed, gated artifact or it is not discipline.
- **legal-specialist** — "My deliverables have been briefs. A brief is prose... I should be asking for
  a **newtype**, not a bullet." Concedes that a legal constraint delivered as a memo is precisely the
  convention class the founder rejects. **Keeps one boundary**: encode the **facts** in types, never
  the **conclusion** — and no lens output is legal advice or clearance.
- **beck** — concedes structural drift **entirely**: tests are the wrong instrument for asserting
  structure, by design, and a test that asserts a layering rule is a slower compiler with worse error
  messages. Holds only the behavioural, value-dependent class as genuinely out of reach of types.
- **vernon** — concedes that the mailbox lane is a **runtime** guarantee about **ordering**, not a
  **compile-time** guarantee about **what a handler may write**, and points at his own evidence:
  `MessageHandler::handle` hands out a raw `&mut Transaction`. His ask survives only in the form the
  premise permits — a type, not a diagram.
- **farley** — concedes his baseline was "a human reads a red gate and judges broken-versus-environment".
  With no such human, an ambiguous verdict is not a nuisance, it is a dead end.
- **dba** — **withdraws its own ask**: it quoted the resident index file instead of the register, and
  [ADR-20260807-114122](ADR-20260807-114122-mks-starts-at-one-node.md) already chose the current
  instance count deliberately, on cost.
- **young, ux-designer, business-specialist, observability-agent** — orthogonal to the premise; each
  argued its position under it rather than asserting it over the top of it.

## What the roster held, and what it costs him

Holding these costs him something concrete; that is why they are on the record rather than in a thread.

- **holub — enforcement substitutes for a missing REVIEWER, never a missing USER.** No amount of
  compile-time rejection tells anyone whether the thing is worth having. And the dependency runs his
  way: the generated security SQL is applied to no database, and the flip event named in `STATUS.md`
  **is** the acceptance walk — the enforcement answer cannot prove itself until the end-to-end walk
  runs.
- **beck — an enforcement boundary the enforced party can move in the same commit is a convention with
  extra steps.** The AI edits `specs/database/**`; the emitter regenerates the security artifact from
  it; the AI edits trait bounds and crate manifests. What makes the fence real is not that it cannot be
  moved but that **moving it is LOUD** — SPEC-LOG line, warning baseline, mob, ADR. That is a **social
  gate carrying a compile-level claim**, and it should be named as one rather than described as
  compilation.
- **beck — a test is the only executable statement of INTENT the next AI will read.** The compiler
  prevents the wrong shape; it cannot say the shape is **desired**. Deleting behavioural tests because
  "the type system covers it" removes the only artifact that says what was wanted.
- **dba — the database rejects UNREPRESENTABLE states, never WRONG ones.** A real substitute for a
  reviewer on **shape**; **no substitute at all on semantics**. A correctly-shaped row with the wrong
  amount in it passes every constraint that will ever exist.
- **dba — the premise makes the restore drill MORE load-bearing, not less.** A projector whose fold
  quietly stopped being deterministic replays wrong **deterministically** and satisfies every
  constraint. Only restore-and-replay sees it.
- **observability-agent — safety and quiet arrive at the same rate.** Every enforcement layer converts
  a runtime-visible failure into a **layer-local silent rejection**; the bill for the safety is paid in
  telemetry, and it must be paid deliberately.
- **vernon — the split is drawn on LAYERS, not aggregates.** `crates/domain/src/` holds the aggregate
  modules in one crate, so no crate contains one transaction boundary. The split enforces the
  **dependency rule** — it does not enforce the rule that actually lost him money, which is one
  aggregate per transaction. The stateless settlement process manager is the receipt.
- **young — the split converts today's latent violations into peak outages.** A cross-crate access that
  is a code smell today becomes a hard failure at the database grant level after the split, and the
  first place it lands is checkout on a Friday.
- **farley — gate honesty becomes the most load-bearing property in the system**, because an autonomous
  maintainer's only defence against an ambiguous verdict is to re-run it.

## Verified facts the roster surfaced

Each keeps its antecedent and its attributing lens, per
[ADR-20260817-105845](ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md);
this record is bound by that rule as much as a dispatch card is. Facts, not decisions — several of them
contradict each other and none is resolved here.

1. **The margin formula is PUBLISHED.** `specs/network/scalars.yaml:81-87` describes the restaurant
   contribution as scaling with `clamp((margin-55)/(70-55),0,1)`, and that description is emitted into
   the shipped schema at `crates/server/src/graphql/generated/scalars.rs:2399,2402`, readable by
   introspection. The public page says *"jamais un pourcentage sur ta marge"*. (architect)
2. **Mechanism mismatch.** The page's fallback is a PER-HEAD split falling as restaurants join; the
   repo models a PER-ORDER margin-proportional deduction. Different mechanisms. (architect)
3. **Structural impossibility.** `OrderPlaced.breakdown` is required and `restaurantContribution` is
   required per-order, but the promise defines that share as a per-period quotient over a population —
   not knowable when the event is appended, ever. (young)
4. **No upcaster needed or wanted.** `serviceFee` cents carrying a contribution keeps every stored row a
   true fact; `ADR-20260818-150000:92-98` already records this posture. **Language debt, not schema
   debt** — nobody may "fix" it by rewriting stored events. (young)
5. **`marginRate` is collected as a commercial secret** (`projection_tables.yaml:75`) on stored events
   (`specs/network/events.yaml:116,214`) as input to a fee the page promises never to charge. (evans)
6. **The contribution already exists in the API**: `tipOrder` / `TipRecipient: CAPTAIN`
   (`specs/ordering/api.yaml:310`), `TipOrder`, `OrderTipped` (`specs/ordering/events.yaml:371`),
   `captain_tip_cents` (`projection_tables.yaml:835`), story step at `specs/stories.yaml:69`.
   (ux-designer, graphql-architect)
7. **But `CAPTAIN` is selectable from no screen**: the only tip widget is `courier_tip`, post-delivery,
   RIDER/RESTAURANT (`specs/screens/restaurant_frontoffice.yaml:266-278`). The page places the ask at
   the cart moment. (ux-designer)
8. **ADR-20260808-203443 already decided ask #3** ten days ago, overriding the ux objection **with** a
   reason, and already mandates an `"Aucun"` default. The team re-litigated a settled record.
   (ux-designer)
9. **`PaymentSettlementProcess` declares no state table** — the only process manager of five without one
   (`specs/payments/processmanager.yaml`); its capture guard reads `View_OrderTracking`, another
   consumer's projection. It compiles and passes every gate. (vernon)
10. **`MessageHandler::handle` hands out a raw `&mut Transaction`**
    (`crates/actor_runtime/src/message.rs:150-155`) — nothing in the type system says which stream a
    delivery may append to. (vernon)
11. **`self-billed` has no owner**: `grep -rl "SelfBill\|self-bill" specs/` returns nothing, while
    `ADR-20260818-134500` records the two-series invoice chain as decided. (young)
12. **`tools/walk/` does not exist in the tree**, yet `.github/workflows/prod-smoke.yml:32-43` names it
    as the smoke's successor and gates the cron on it being green. No end-to-end evidence path exists
    under either name; the harness from
    [#556 "Local acceptance harness"](https://github.com/TheCaptainCompany/captain-food/issues/556) is
    on an unmerged branch. (beck, holub)
13. **The production smoke asserted checkout → AUTHORIZE only.** `tools/smoke/prod-smoke.sh:12-20`
    records that capture assertions "belong to the FUTURE L5 fulfilment leg", so
    `restaurant notified -> accept -> deliver -> capture` has never been green anywhere. Last green
    2026-07-29; red for 19 consecutive scheduled runs to 2026-08-17, of which the suspension explains
    13, six unrecorded — and no record in the repo called it a broken gate. (farley)
14. **No automated deploy path to any cluster exists**: nothing in `.github/workflows/` applies
    `deploy/generated/manifests` or `deploy/platform`; the only executable deploy is a manual dispatch
    to Render — the stack he has rejected. (farley)
15. **Connection arithmetic**: `ls crates/bins` = 57; the generated default
    `DATABASE_POOL_MAX_CONNECTIONS = 5`; `deploy/platform/cnpg/cluster.yaml:72` sets
    `max_connections: 220` on a 1 Gi cluster. 57 x 5 = 285 exceeds 220 at one replica each, before any
    scaling, and per-persona LOGIN roles worsen it because a transaction pooler cannot share a pool
    across roles. Does not bite today — the monolith is the deployed runtime until
    [#358 "MKS bootstrap"](https://github.com/TheCaptainCompany/captain-food/issues/358). (dba)
16. **The append-only gap**: the generated security artifact contains no `domain_events` or
    `inbound_messages` line, so nothing at the database level stops an app role issuing
    `UPDATE domain_events` or `DELETE`. The cheapest high-value item on the board. (dba)
17. **The GUC is set only in the test suite** — `set_config('app.member_id', ...)` appears in no runtime
    code, and its third argument must be `true` (transaction-local). Session-scoped under a pooled
    connection at peak means customer A's connection authorizes customer B's rows. Decidable now,
    before the call site exists. (dba)
18. **RLS covers 2 tables** (`orderconversation`, `scopemembership`), proved with seven seen-red
    mutations in `crates/infrastructure/tests/rls_matrix.rs`. (dba)
19. **Entity conflict**: `/tarifs` says *"association à but non lucratif (coopérative SCIC et agrément
    ESUS visés)"*; `ADR-20260808-195315` records the project as carried by a SASU for now. CLAUDE.md
    requires external artifacts to name the capacity the statutes actually confer. (legal-specialist)
20. **`serviceFee` is a VAT characterisation, not plumbing.** A voluntary payment falls outside VAT only
    where there is no direct link to a supply (CJEU *Tolsma* C-16/93; VERIFY-FIRST). Labelling it
    `serviceFee` in Stripe metadata, in `PaymentBreakdown` and in the stored event asserts that direct
    link in our own records, against our own interest. Free to fix today. (legal-specialist)
21. **`business_metrics.yaml` declares one metric family** (`OrderAcceptanceLatency`): no contribution
    fold, no cost-recovery fold, no refund-cost fold. (business-specialist)
22. **`ApproveRefund` carries no liability attribution** (recorded at `ADR-20260818-094500:178`), so a
    delivery-caused refund is silently a restaurant cost — the one thing the public page promises never
    happens. (business-specialist)
23. **`captainNet` conflict**: if contributions arrive AS `serviceFee`, then by
    `specs/common/entities.yaml:22` `captainNet` is exactly the contribution and is NON-ZERO,
    contradicting `ADR-20260818-134500:114` and `docs/STATUS.md:70` ("captainNet is zero at V0"). A
    recorded-decision conflict needing a register row. (architect)
24. **Doctrine lag, not a decision**: CLAUDE.md advertises ">=3 instances, executed restore drills";
    `deploy/platform/cnpg/cluster.yaml:26` says `instances: 1`, which ADR-20260807-114122 chose
    deliberately, and `deploy/platform/kustomization.yaml` states "NOTHING APPLIES THIS TODAY". The
    drill is not runnable because no cluster exists — it is a cutover precondition, not a now-task. (dba)

### The public promise, verbatim

The founder's answer #4 pointed at `https://join.captain.food/tarifs` (not `/pricing`). It is more
specific than anything in the repo, and it is the public promise the money model must match:

- *"Tarifs : c'est gratuit pour les restaurants"* — *"0 % de commission, 0 € d'abonnement, 0 frais
  caché."*
- *"0 % de commission sur tes plats — jamais un pourcentage sur ta marge."*
- *"La plateforme vit de la contribution volontaire de ses utilisateurs : au moment de commander, le
  client peut ajouter une contribution libre à son panier s'il le souhaite — 0 € possible, sans
  jugement. C'est un pari."*
- *"Ce ne sera jamais une grille tarifaire, ni une marge. Au pire, on partagerait à prix coûtant le
  strict nécessaire pour couvrir le fonctionnement : Côté client — une petite participation aux frais
  de fonctionnement à la commande. Tout ce qui est versé au-delà reste une contribution. Côté
  restaurateur — une part des coûts réels, répartie sur le nombre de restaurateurs embarqués : plus on
  est nombreux, plus elle est faible. Jamais une commission, jamais un pourcentage sur tes plats."*
- *"Et dès que le modèle le permet, tout ça disparaît : retour au gratuit."*
- *"Captain.Food est un bien commun numérique, open source, porté par une association à but non
  lucratif (coopérative SCIC et agrément ESUS visés)."*
- *"Rien n'est encore construit. Captain.Food démarre à Tours."*

## Coordinator defects banked

Banked with attribution as
[ADR-20260816-134352](ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md)
requires. **All four are card defects. None is a roster-width miss, so nothing here returns to the
founder as a class reversion.**

1. **Card defect (coordinator).** Ask #1's wording turned a request for a green technical walk in the
   local harness into what read as a request to demo a real order to real users. holub, farley and beck
   independently ruled that he answered a different question. His DISAGREE on ask #1 is therefore **not
   recorded as a disagreement**, and the ask is re-put corrected.
2. **Register defect (coordinator), and the blunt version:** ask #3 asked the founder to decide
   something ADR-20260808-203443 settled ten days earlier, with the ux objection already overridden and
   reasoned and an `"Aucun"` default already mandated. The "check the register before you ask" rule was
   written into `docs/claude/sessions.md` **that morning** and broken by the decision form built **that
   afternoon**. That is the premise of this ADR demonstrated on itself: the rule was prose, prose is a
   convention, and the convention did not survive six hours. **The fix must be executable, not another
   prose rule** — the form's question set has to be mechanically confronted with the register before it
   can be sent.
3. **Invented characterisation (coordinator).** The team's promise was described as "0% commission, flat
   subscription". The page says 0 € subscription, and the architect confirms no repo antecedent for a
   subscription anywhere. Corrected in the relay before the lenses reasoned from it — but it had already
   been in front of them once.
4. **Card defect (ux-designer, self-reported).** Its ask asserted "no screen, mutation or read model
   exists" for the contribution; facts 6 and 7 show two thirds of that was false.

## Alternatives considered

- **Record nothing** — the exchange was a consultation, its verdicts are in the mob returns, and the
  premise "is obvious". Rejected: the premise **changes how every lens must argue**, and eight of the
  thirteen conceded published doctrine to it. An unrecorded premise is re-litigated by the next session,
  exactly as ask #3 was.
- **Record it as an argument the team lost** — a note that the founder disagreed with six of seven asks.
  Rejected: it inverts the value. He invited the asks to explain, the reasons are the content, and the
  parts the roster **held** are the parts that cost him something and must therefore survive in writing.
- **Record the premise AND resolve the money-model conflicts in the same ADR** — they surfaced together.
  Rejected: the `captainNet` conflict, the `serviceFee` characterisation, the published margin formula
  and the per-head/per-order mechanism are separate decisions, several touching stored event shapes and
  legal surfaces (`HOLD: human`). Bundling them would smuggle four undecided things through on the
  authority of one decided one.
- **Turn the premise straight into a validator rule** — "no guarantee may be recorded in prose".
  Rejected as unspellable in that form: the rule is about how arguments are made, and the executable
  forms it demands are per-case (a type here, a constraint there, a gate where types cannot reach). The
  executable pieces are follow-ups below, each with its own owner.

## Consequences

### Positive

- **Every lens now argues inside the same premise**, and eight of them have said in writing which part
  of their own doctrine it invalidates. That is cheaper than rediscovering it per session.
- **The bounds of the substitution are on the record**: shape yes, semantics no; reviewer yes, user no;
  compilation yes, desire no. A future proposal that claims enforcement covers semantics can be refused
  by citation rather than by re-argument.
- **The prose-is-a-convention rule is self-applying**, and its first casualty is the team's own
  register-check rule (defect 2) — which makes the case for executable enforcement without needing a
  hypothetical.
- **The public promise is quoted verbatim in the repo** for the first time, so the money model has a
  fixed thing to be reconciled against instead of a remembered summary.

### Negative

- **The premise raises the cost of every guarantee.** "Write it down" is no longer an acceptable
  discharge, and some guarantees have no reachable executable form today; those now have to be declared
  as knowingly unenforced rather than quietly documented.
- **It can be over-applied.** Taken past its bounds it argues for deleting behavioural tests ("the type
  covers it"), which beck's held position forbids: the compiler cannot say a shape is desired.
- **The enforcement programme it justifies is not yet load-bearing anywhere** — the security artifact is
  applied to no database, the split is drawn on layers rather than aggregates, and the end-to-end walk
  that would prove any of it has never run.
- **Holding both the premise and the roster's reservations costs him work** he has not scheduled: the
  loud-fence naming, the semantics gap, the telemetry bill and the restore drill are all real and all
  open.

### Follow-up actions

**Open items — named here, decided nowhere.** Each needs its own register row, and the first, second and
fourth are `HOLD: human` class (stored event shapes and legal surfaces):

- **The `captainNet` conflict** — contribution-as-`serviceFee` makes `captainNet` non-zero
  (`specs/common/entities.yaml:22`) against `ADR-20260818-134500:114` and `docs/STATUS.md:70`.
- **The `serviceFee` naming**, which is a VAT characterisation carried in stored events and Stripe
  metadata, not plumbing.
- **The published margin formula** in `specs/network/scalars.yaml:81-87`, emitted into the shipped
  schema description, against *"jamais un pourcentage sur ta marge"*.
- **The per-head versus per-order mechanism**, and with it the structural impossibility of a per-period
  quotient on a per-order required field.
- **The entity conflict** between the page's SCIC/ESUS wording and the recorded SASU.

**Questions converged for the founder** — each checked against the register by the asking lens; the
first two were asked independently by two lenses:

1. **[farley AND dba]** Does the anti-shortcut fence require N deployed **processes**, or is N compiled
   **crates** behind one deployed binary enough? The compiler rejects a cross-crate shortcut identically
   either way, DB-level rejection is independent of process count, and the connection arithmetic (fact
   15) bites only in the process form.
2. **[evans AND graphql-architect]** When the cagnotte falls short and the *participation aux frais*
   switches on, does the customer see **two** lines (a cost participation plus a free contribution, per
   *"tout ce qui est versé au-delà reste une contribution"*) or **one**? Different VAT treatment; one
   field or two.
3. **[ux-designer]** "Same approach as HelloAsso" — the **framing** (with a zero default) or also the
   **mechanic** (a pre-filled suggested amount)? A pre-filled amount contradicts both the page's *"0 €
   possible, sans jugement"* and ADR-20260808-203443's `"Aucun"` default.
4. **[architect]** Per-head or per-order — delete the margin-proportional mechanism outright, or is it
   the intended shape and the page's per-head wording the thing that is wrong?
5. **[business-specialist]** Do customer refunds and credits count as a "real cost" spread across all
   onboarded restaurants, or does the restaurant whose order went wrong bear its own? The page reads as
   neither; the model today quietly does the second.
6. **[young]** Is the restaurants' shortfall share billed as a separate **periodic** invoice, or can it
   ever attach to an individual order? Only the first is buildable.
7. **[legal-specialist]** Whose account actually receives money today — the cagnotte and the checkout
   contribution, separately? Decides whether one funds posture is open or two.
8. **[observability-agent]** On what cadence and against what cost figure is the cagnotte judged
   insufficient? The only input that turns the `/tarifs` fallback clause into something observable.
9. **[vernon]** When the payments worker is down at 20:00 on a Friday, does checkout keep accepting with
   funds authorized and capture deferred, or stop? Decides whether payments can be a separate deployable.
10. **[holub]** "When you say it does not work — what is the first thing you saw break?" A fact only he
    holds.

**Cards the roster proposed — candidates for the architect to rank. Not decisions, not approvals, not
dispatches:**

- **holub** — land the [#556 "Local acceptance harness"](https://github.com/TheCaptainCompany/captain-food/issues/556)
  work red on `main`, run the walk once, record the first failing leg in `STATUS.md`.
- **beck** — resolve `tools/walk/` (it exists, or the workflow comment is corrected in the same change),
  then one money-path leg red-first: quantity 0 on a stock-tracked offer, asserting the TYPED rejection
  reaches the client — the oversell coverage gap tracked at
  [#354](https://github.com/TheCaptainCompany/captain-food/issues/354).
- **farley** — a repo-only `deploy-verify` CI job: render every overlay, stand up an ephemeral kind/k3s,
  run migrations on an empty database, execute the restore-drill CronJob once. No cluster, no bill.
- **dba** — extend `emit/security.rs` to the write path: REVOKE UPDATE/DELETE on `domain_events`; a
  `BEFORE UPDATE` monotonicity trigger on `mailbox_partitions`; a seen-red arm proving the GUC is
  transaction-local; drop the redundant duplicate index at
  `migrations/20260717120000_domain_schema.sql:125`.
- **vernon** — give `PaymentSettlementProcess` its own state table and move the capture guard off
  `View_OrderTracking`; then, as a follow-on, replace the raw `&mut Transaction` with a stream-scoped
  writer capability.
- **young** — pin `OrderPlaced.breakdown.restaurantContribution` to zero with a test, before anything
  writes.
- **evans / architect** — make the money nouns say what the pricing page says: dedicated scalars per
  concept, and strike the margin formula from `scalars.yaml` and from the shipped schema description.
- **graphql-architect** — emit per-role SDL as a committed, gated artifact, with an additive-only
  validator rule.
- **ux-designer** — a journey spec for the cart-moment contribution reusing `tipOrder`, with a validator
  assertion that no contribution widget declares a non-zero default. Merge with the business fold card.
- **business-specialist** — name the refund/credit bearer and declare the fold that watches it.
- **legal-specialist** — a dated evidence-and-fence file under `docs/legal/`: page snapshots, the funds
  fence, the cagnotte-versus-checkout table, and the structure-dependent items ranked by cost of delay.
- **observability-agent** — a `persistence-guard` contract in `specs/observability.yaml`: a `db.reject`
  span, `db_constraint_rejections_total`, a `db_guard_enforcing` gauge polled from catalog
  introspection, and a status rule classifying every `db.reject` as `technical_error`, never
  `business_rejected`.

**Process follow-up, from defect 2:** the register check before a decision form is sent must become
executable. A prose rule in `docs/claude/sessions.md` has now failed once, on the same day it was
written, which is the strongest evidence this record contains for its own premise.
