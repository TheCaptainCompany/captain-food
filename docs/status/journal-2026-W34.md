# Status journal — 2026-W34

Journal entries for ISO week 2026-W34, newest first, in the order they were written.
Current state: [`../STATUS.md`](../STATUS.md).

> 🗝️ **2026-08-21 — THE REGISTER ROW GETS MACHINE IDENTITY: REG-2/REG-4 land, the index is
> generated, and the ask gate reads the rows**
> ([ADR-20260821-095957](../adr/ADR-20260821-095957-the-register-row-gets-machine-identity-reg2-reg4-and-the-ask-gate-reads-it.md),
> deciding [DECISIONS §48](../proposals/DECISIONS.md) REG-2 + REG-4 (vocabulary half) + REG-3's
> index + REG-SEQ per founder directive, verbatim in the ADR; whole roster consulted, one line per
> lens). `docs/decisions/<KEY>.yaml` — **19 rows migrated** (the 2026-08-19 live set, the §48
> family, the sitting's closed money rows), each carrying its `register` anchor and verbatim
> `evidence`; closed vocabulary `open·decided·deferred·superseded·withdrawn` with **biconditional**
> status↔field couplings, resolvable `decided_by`, a supersession **DAG**, `until` on deferred,
> `note` on withdrawn — validator **§22** (`validate/decisions.rs`), every rejection rule proven
> red on a planted defect. The DECISIONS.md index is now a **GENERATED region** (deterministic,
> `opened`-date not computed age, pipes escaped, §13b-checked before splicing, missing markers a
> hard error); **any `docs/decisions/**` edit is a generating edit — `make generate` in the same
> commit, straight-to-main path included**. The register-check hook now reads the row FILES at the
> point of need (never the index): a question referencing a non-open key is refused with the
> status-specific citation; open counsel-owned rows take only the external-action question; the
> firing log carries a closed reason taxonomy. **Legacy is a declaration**: the 103 unmigrated
> keys are enumerated in `_legacy.yaml` (pass, logged); next-touch migration, decided at dispatch
> time. New open row `KEY-NAMESPACE` carries REG-4's namespacing residue (split-at-close).
> `holub`'s #556 position recorded; the founder's directive is the sequencing override.

> 🔒 **2026-08-21 — AGENTS NEVER ASK AN ANSWERED QUESTION: the register check binds every agent and
> the ask surface is gated**
> ([ADR-20260821-010543](../adr/ADR-20260821-010543-agents-never-ask-an-answered-question-the-register-check-binds-every-agent.md),
> deciding [DECISIONS §48](../proposals/DECISIONS.md) REG-1's direction — enforcement on the ASK —
> per founder directive, verbatim: *"I want to ensure that the agents will no longer ask questions
> already answered. Use the best practices known for that."*; whole roster consulted, 16 lines in
> the ADR). The canonical `Register check:` trail format is declared once in
> [workflow.md](../claude/sessions/workflow.md) ("The trail rides the question") with the alias
> table; all 16 `.claude/agents/*.md` carry a thin citation block; `AskUserQuestion` is gated by
> a fail-closed PreToolUse hook (`.claude/hooks/register-check.sh`, one greppable log line per
> firing, log gitignored); and `.claude/hooks/register-check-selftest.sh` — seen RED before the
> wiring existed — proves verdicts, wiring and block presence on every turn (stop gate) and via
> `make hooks-test`. Honest scope recorded in the ADR: the hook proves trail presence and shape on
> the tool path only; prose surfaces are bound by the agent blocks; honesty stays with mob and
> review. REG-2/REG-3/REG-4 stay open and founder-owned.

> 🗂️ **2026-08-20 — THE JOURNAL WRITE PATH SWITCHES: `STATUS.md` is durable state plus an index,
> dated entries go to the ISO-week file** ([#665](https://github.com/TheCaptainCompany/captain-food/pull/665)).
> `docs/STATUS.md` is now **durable state and the journal index** — deployment, read/write side,
> authorization, architecture decisions, and links to the week files. It is **not** where a dated
> entry goes. **Write a dated entry at the TOP of the applicable `docs/status/journal-YYYY-Www.md`**:
> the journal is newest-first, so appending at the end makes the file's own header false on the first
> write. **If the week has no file yet**, create it from the header the existing week files carry —
> `# Status journal — YYYY-Www`, the newest-first sentence, and the `../STATUS.md` back-link — then
> add it to the index at the bottom of `STATUS.md` and place the entry at the top of the new file.
> **The active writer instructions were aligned in this branch**, so no standing path still points a
> dated entry at `STATUS.md`: `.claude/agents/executor.md`, `.claude/agents/architect.md`,
> `.claude/skills/architecture-review/SKILL.md`, `.github/workflows/dev-loop.yml`,
> `docs/claude/autonomous-run.md`, `docs/BACKLOG.md` and the PR template.
> **Still owed, approval-gated**: `CLAUDE.md` carries three analogous `STATUS.md` instructions and is
> deliberately untouched here — `:309` still says a reversing re-ranking gets a `STATUS.md` line,
> which this branch now contradicts. That reconciliation is a separate approved change, not a silent
> edit around it.

> 📄 **2026-08-19 — THE DECISION REGISTER IS THE UNIT OF DECISION: proposal filed, `Proposed`, NOT
> dispatchable** ([PROP-20260819-110442](../proposals/PROP-20260819-110442-the-decision-register-is-the-unit-of-decision.md),
> [DECISIONS §48](../proposals/DECISIONS.md), tracking
> [#658 "The decision register cannot say what is still open…"](https://github.com/TheCaptainCompany/captain-food/issues/658)).
> Answers the founder's *"do the agents ask questions the ADRs already answered?"* — **yes**, but not
> because the corpus is unreadable: the record re-litigated on 2026-08-18 was one `grep` away and
> nothing required the grep. Rows `REG-1`…`REG-4` (all OPEN), `ADR-VOLUME` (architect-ruled: **do not**
> write fewer ADRs — stop making them the decision index) and `REG-SEQ` (🔴 does not displace
> [#556 "Local acceptance harness"](https://github.com/TheCaptainCompany/captain-food/issues/556)).
> ⚠️ **The founder's 2026-08-18 deferral of
> [PROP-20260818-013222](../proposals/PROP-20260818-013222-graph-engineering-for-the-team-workflow.md)**
> ([#643 "DEFERRED — Graph engineering for the team workflow"](https://github.com/TheCaptainCompany/captain-food/issues/643),
> verbatim: *"we will not apply it yet we will finish what we have started first"*) **plausibly covers
> this proposal too** — same class, one day apart. The architect flagged it rather than routing around
> it; **it is the founder's to confirm**, and nothing here proceeds meanwhile.
>
> 🔧 Same change: **`docs/adr/README.md` was still 13 entries stale** after the `bfe6694` sweep, which
> indexed the twelve ADRs dated 2026-08-18 and left thirteen dated 2026-08-11 → 2026-08-16 unindexed —
> including [ADR-20260815-115220](../adr/ADR-20260815-115220-auto-merge-on-green-by-default-hold-human-for-the-named-class.md)
> and [ADR-20260816-134352](../adr/ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md),
> **both cited by CLAUDE.md**. Now current: **189 date-time ADR files, 189 index rows**, sorted, no
> duplicates, every link target verified to exist. That a competent, correctly-scoped staleness sweep
> still left thirteen rows unindexed is `REG-3`'s own evidence: **prose discipline does not converge.**

> ✅ **2026-08-19 — THE SIX QUEUE ANSWERS LANDED: four register rows close, two open**
> ([ADR-20260819-103112](../adr/ADR-20260819-103112-the-six-queue-answers-a-fiscal-host-in-the-money-path-and-a-refund-bearer-with-no-field.md),
> thirteen lenses; [DECISIONS §47](../proposals/DECISIONS.md)). **Q1** do nothing (the association→company
> boundary will not be reconstructible — a dated knowing acceptance) · **Q2** Open Collective, *"but not
> yet configured"* · **Q3** ship the pre-filled contribution · **Q4** restaurant bears by default, admin
> refunds on platform issue · **Q5** a **free-delivery threshold** replaces the margin mechanism outright
> · **Q6** the team answers the money-line question itself. Closed: `CONTROLLER-HANDOVER`,
> `CONTRIB-DEFAULT`, `REFUND-BEARER`, `MARGIN-MECHANISM`. Opened: `DELIV-THRESHOLD`, `OC-LEDGER`,
> `CONFLICTS-20260819`. ⚠️ **Q3 × Q2 collide** — Open Collective's ToS (FETCHED 2026-08-19) makes the
> **Host** solely responsible for refunds, so a fiscal host leaves Captain owing an Art. 22 remedy it
> cannot execute
> ([BRIEF-20260819](../legal/BRIEF-20260819-open-collective-and-the-self-answered-position.md)).
>
> **The walk still stands as next** — none of the six beats it; Q3 and Q5 are slice **content** for the
> walk, not chunks queued behind it (`holub`). **`CAPTAINNET-ZERO` is now the highest-leverage blocked
> row** (it absorbed `REFUND-BEARER`'s residue and gained Q5's subsidy term) — **founder-owned, RED, and
> ranking it first does not make it dispatchable.**
>
> ⚠️ **Two live defects a concurrent session must know about**: **both refund paths debit the platform
> silently** — the approval leg calls `payment.refund(intent, amount)` with **no transfer reversal**
> (`crates/application/src/process_managers/refund.rs:122-141`), so with `captainNet` zero the balance
> drawn down is other orders' unsettled restaurant and rider money, and the first symptom is a negative
> platform balance on a Friday night, not an error. And **a zeroed contribution is unrepresentable** —
> `specs/ordering/events.yaml:388` sets `tips: minItems: 1`, so a customer who zeroes the pre-fill emits
> **no event at all**, which is exactly the observation that would exonerate the mechanic.

> 🗓️ **2026-08-19 — COST-OF-DELAY ORDER REVERSED: the Stripe answer moves two windows**
> ([DECISIONS §47](../proposals/DECISIONS.md)). The founder clarified that the association holds a
> **test-mode** Stripe account and the company will open a **separate new** one, so **no real money ever
> passes through the association**. Consequence for the register: the first real order is gated behind
> the company's live account, so **BREAKDOWN-ZERO**'s window is **later** than previously recorded, and
> **CONTROLLER-HANDOVER**'s — the incorporation date — is **earlier and outside the team's control**.
> That reverses the stated order of the two. §47 files the rows the ten answers owe; **none is
> decided**.
>
> ⚠️ [PROP-20260819-021500](../proposals/PROP-20260819-021500-checkout-stops-as-a-published-fact-and-an-appended-refusal.md)
> (Q3, checkout stops as a published fact and an appended refusal) is **`Proposed`, not Approved** —
> nothing may be implemented from it yet.

> 💶 **2026-08-18 (evening) — THE TEN ANSWERS LANDED: per head, monthly invoice, stop checkout**
> ([ADR-20260818-233000](../adr/ADR-20260818-233000-the-ten-answers-per-head-monthly-invoice-and-a-cagnotte-that-exists-only-in-prose.md),
> thirteen lenses). **Settled**: processes are the point (Q2, costs accepted); the fallback never
> touches the customer (Q5); **per head**, deleting the margin-proportional fee outright (Q6); a
> **monthly** shortfall invoice, never per order (Q7); the cagnotte bears refunds (Q8) and is judged
> monthly against four months of covered costs (Q10). This **answers** the three money-model conflicts
> flagged in the round-1 entry below — they now owe **register rows**, not analysis.
>
> **Do not build on any of these yet.** (1) **Q3's GOAL is accepted, its MECHANISM is not**: he decided
> STOP CHECKOUT, and six lenses reject the four-hop synchronous availability chain on six independent
> grounds — readiness is a **published fact** (`crates/server/src/lib.rs:259` is already push-shaped),
> the refusal must be an **appended fact**, a closed gate is invisible to the observability contract,
> probes make **slow read as down**, four hops manufacture a 99.6 % ceiling, and the endpoint they would
> read is a boot-time constant. (2) **Ten register rows are owed and none is decided** — Q9 reverses
> the `"Aucun"` default (ADR-20260808-203443) and Q8 contradicts ADR-20260818-150000 *the same day*.
> (3) **CRD 2011/83 Art. 22 is open on the pre-filled contribution**: the prohibited shape is the
> **default**, and the remedy is **reimbursement of every contribution ever collected** — grade (b),
> leaning inside, with the French transposition number NOT statable today
> ([BRIEF-20260818-pre-filled-contribution-and-the-monthly-invoice](../legal/BRIEF-20260818-pre-filled-contribution-and-the-monthly-invoice.md),
> G1–G7). (4) **`cagnotte` has zero hits outside `docs/`** — three of the ten answers rest on a
> concept the system does not model.

> 🧭 **2026-08-18 — THE TEAM ASKED, THE FOUNDER ANSWERED: no human maintains this Rust**
> ([ADR-20260818-210000](../adr/ADR-20260818-210000-the-ai-maintained-codebase-premise-prose-is-a-convention.md)).
> He invited the asks himself, disagreed with six of seven **and gave reasons**; the reasons went back
> to all thirteen lenses. The durable one: *"layers was just conventions now it will be compilation and
> controlled… I'm not able to maintain rust, only the ai will be able to do it"*. Compilation-level
> enforcement, the crate split and DB-level rejection are the deliberate **substitute for a human
> maintainer**, chosen after the conventional layered approach shipped and produced AI errors it could
> not catch. Working rule adopted from it: **a rule that lives only in prose is a convention, and this
> repo has decided conventions are not enough.** Eight lenses conceded published doctrine to it; the
> bounds they held are on the record (enforcement replaces a missing **reviewer**, never a missing
> **user**; the database rejects **unrepresentable** states, never **wrong** ones; safety and quiet
> arrive at the same rate; the split is drawn on **layers**, not aggregates, so it enforces the
> dependency rule and not the rule that lost him money; and an enforcement boundary the enforced party
> can move in the same commit is a convention with extra steps, made real only because moving it is
> LOUD).
>
> **⚠️ Open money-model conflicts surfaced, and DECIDED NOWHERE — do not build on either side of them
> yet.** (1) **`captainNet` zero versus contribution**: he says contributions arrive AS the Stripe
> service fee, and by `specs/common/entities.yaml:22` that makes `captainNet` exactly the contribution
> and NON-ZERO, against ADR-20260818-134500 and the "captainNet is zero at V0" line below. (2) **The
> published margin formula**: `specs/network/scalars.yaml:81-87` scales the restaurant contribution
> with the restaurant's margin and that description is emitted into the shipped GraphQL schema, while
> `join.captain.food/tarifs` promises *"jamais un pourcentage sur ta marge"*. (3) **Per-head versus
> per-order**: the public fallback is a per-head split falling as restaurants join; the repo models a
> per-order margin-proportional deduction, and a per-period quotient is not knowable when
> `OrderPlaced.breakdown` is appended. Each needs its own register row; the stored-shape and legal
> surfaces are `HOLD: human`. **Ask #1 (one order end to end) is recorded as UNANSWERED** — a
> coordinator card defect, being re-put corrected, not a founder disagreement.

> ↩️ **2026-08-18 — CAPTURE ON DELIVERED DISSOLVES THE REFUND GAP**
> ([ADR-20260818-161500](../adr/ADR-20260818-161500-capture-on-delivered-dissolves-the-refund-gap.md)).
> Asked who absorbs a refund larger than the restaurant's share, the founder answered **neither**:
> *"We capture the payment on order delivered… we will just not capture."* Applying his own 2026-08-08
> ruling, **a failed delivery never captures, so there is no refund, no gap and no receivable** — the
> ledger aggregate the mob had scoped as a proposal is not needed for it, and legal's set-off clause
> narrows to the reclamation path. *"The restaurant carries it"* becomes **it loses the food it cooked
> and receives nothing** — no debit, no surprise on a bank statement.
>
> **Also**: there is **no customer service fee at V0** (the voluntary contribution is the whole
> customer money surface), which resolves the fee-versus-contribution naming contradiction by removing
> the fee. And rider pay on a failed run **depends on the cause** — but `DeliveryCancelled.reason` is
> **nullable free text** today, and free text cannot drive a payment rule, so a typed
> `DeliveryFailureCause` is owed first.
>
> **Narrowed, not closed**: post-delivery reclamations still capture first, so the over-cap question
> survives there; and a rider owed for a run on an uncaptured order is paid from money that exists
> nowhere in the flow — the second bill is still unpaid.

> 📸 **2026-08-18 — TWO PHOTOS, and the labels are OPTIONAL**
> ([ADR-20260818-174500](../adr/ADR-20260818-174500-two-photos-the-packing-check-and-the-drop-off-record.md),
> amended). The founder wants the restaurant to photograph the order and the rider to photograph the
> drop-off. Twelve lenses reviewed it. **The constraint that rewrites it**: the drop-off photo is
> necessary only for UNATTENDED delivery — where the customer takes the bag, their own act proves it,
> and a photo there fails minimisation. Two modes, two proofs. **The photo never gates the transition**
> (completion triggers capture, so a blocked photo blocks payment for food already delivered) — the
> shape is a closed sum `Captured(ref) | Waived(reason)`, and the waiver reason is the typed
> delivery-failure cause ADR-20260818-161500 already owes.
>
> **The reference image clarified it**: per-item **labels are OPTIONAL** (app-generated if the
> restaurant wants), the **photo is mandatory**. Coordinator correction on the record — the labels
> were overstated as answering the objections; they answer them only for adopters. The **bare photo's
> real value is a completion/handoff proof** — against a paid order nobody acted on and against the
> wrong bag handed over — independent of labels. **The privacy floor does not improve and slightly
> worsens** (a non-adopter photographs its own POS ticket with customer name/phone/address); the six
> preconditions stand, and the balancing test and DPIA must be written to that worst input.
>
> **Team recommendation (founder may overrule)**: land a nullable attachment ref on the two existing
> command/event pairs as content on a scheduled chunk; reach "avoid errors" today with a zero-byte
> per-line packing confirmation; let the drop-off photo wait on the #134 upload framework. **First
> fix**: `PROP-20260725-120055` still says Supabase Storage against a decision for OVH — rewrite
> before any dispatch.

> 🧾 **2026-08-18 — THE INVOICE CHAIN IS RULED: restaurant → customer, rider → RESTAURANT, Captain
> self-bills both**
> ([ADR-20260818-134500](../adr/ADR-20260818-134500-the-invoice-chain-restaurant-to-customer-rider-to-restaurant.md)).
> Six answers on the decision form. **The load-bearing half was not on the form**: the founder chose
> *"neither exactly"* and wrote that the **rider invoices the RESTAURANT** — so delivery is a supply
> to the restaurant, the restaurant sells a delivered meal, and **Captain is a party to neither
> supply**.
>
> **This resolves the contradiction** BRIEF-20260818 §2 found: the adopted proposal describes the
> **sale** (restaurant is its own merchant of record) and the five other records describe the
> **payment mechanism** — they were never talking about the same thing. Still open and NOT resolved:
> ADR-0017's *"merchant of record → no PSP licence"* clause, which remains a non-sequitur needing a
> real instrument.
>
> **Money keeps resting on Captain's Stripe balance, knowingly.** Captain collects on the
> restaurant's behalf and pays the rider on the restaurant's behalf — a payment-agent posture whose
> characterisation no research retires. **Recorded as a decision taken with the exposure in front of
> him, not as a gap.**
>
> **Also**: rider self-billing is a **separate** decision — V0 self-bills partner companies only,
> never an individual rider. The team is **authorised to draft the self-billing mandate and the terms
> structure** for founder review (no contract artifact exists in the repo at all today). The customer
> receipt carries the **restaurant's** name. `captainNet` is **zero at V0** — the company is funded by
> voluntary contributions per ADR-20260808-203443, and **two new elements** landed with that answer:
> a public **open expense-and-income platform** (a product surface nothing carries), and a
> **shortfall split across all restaurants** — a contingent liability that must be in the terms
> before the first restaurant signs.
>
> **Reusable now**: [`docs/templates/decision-form.html`](../templates/decision-form.html) — founder
> directive, *"make this format a template for the next times"*. Rule and the register-check lesson in
> [docs/claude/sessions.md](../claude/sessions.md).

> ✅ **2026-08-18 — DECISION QUEUE CLEARED: the restaurant signs in by EMAIL LINK, and #638 FREEZES
> at chunk 1**
> ([ADR-20260818-101500](../adr/ADR-20260818-101500-the-restaurant-signs-in-by-email-link-and-638-freezes-at-chunk-1.md)).
> Both answers *"Agreed"*, to the two items put with a recommendation.
>
> **1. Email link, not phone OTP, for the restaurant.** The deciding argument is that
> `SMS_MAX_SENDS_PER_DAY_GLOBAL` is platform-wide and is described in its own declaration as the only
> ceiling on the bill: the rider's working tool is already on that bucket, and putting the restaurant
> there too makes a restaurant-side surge and a **rider lockout at Friday peak** the same number. The
> rider population is bounded; the restaurant one is not. This does **not** license cloning
> `verifyPhone` — that command is register-or-identify and creates the Customer; staff sign-in is
> **identify-only against a pre-provisioned roster**, whatever the factor.
>
> **2. `#638` freezes at chunk 1 (merged, PR #644); chunk 2 is not dispatched.** Ordering, not
> correctness — the founder's own "avoid AI errors" rationale is untouched. A second authorization
> layer under a first that does not exist defends nothing: every restaurant caller is
> `Identity::Unbound` today, so `ReadScope::Restaurant` is unreachable and there is no bound identity
> for a policy to resolve against. Row security also **cannot** close the refund hole —
> `approveRefund` is a participant check against folded state, the UNBINDABLE class in §39.
> Recorded so a concurrent session cannot read the frozen chunk as available work.
>
> **What starts instead — the slice, one sentence**: *one real Tours restaurateur finds their own
> restaurant on their phone browser, proves it, signs in, and can see and act on only their own
> orders and only their own refunds.* Three operations (`approveRefund`, `denyRefund`,
> `pendingRefunds`), not the 83 in the §39 scope. It discharges both rulings of
> ADR-20260818-094500, the V0 sequencing of ADR-20260818-004646, and §39 IDOR trigger (i) — which
> ruling A is itself the event that trips.

> ⚖️ **2026-08-18 — RULINGS: staff sign-in has a mechanism, refund approval stays with the
> restaurant, and the executor now refuses a stale base**
> ([ADR-20260818-094500](../adr/ADR-20260818-094500-staff-auth-mechanism-and-refund-approval-stays-with-the-restaurant.md)).
> Three founder rulings in one message; the whole roster was consulted before the answer, eleven
> lenses replied, and the `Consulted:` block records what each caught.
>
> **A — STAFF-AUTH mechanism ([#639](https://github.com/TheCaptainCompany/captain-food/issues/639)).**
> The rider signs in by phone, Supabase-handled, **OVH SMS** as the sender, required for V0 because
> the phone is the rider's working tool. The restaurant starts on the **web** and self-registers by
> finding their restaurant. Restaurant onboarding is named open by the founder; **account managers
> were not mentioned and are therefore not ruled** — and the mob's reading is that the
> `RESTAURANT_ACCOUNT`/`RESTAURANT` split is a story-map persona with no command, event or
> projection behind its assignment relationship, so V0 models one person bound to one location.
>
> **B — `approveRefund` is NOT narrowed to `[ADMIN]`.** *"The approval of the refund must be done by
> the restaurant by default"*; admin is the intervention. `roles: [RESTAURANT, ADMIN]` stands and the
> back-office widget stays. The consequence is the point: the write-side hole must now close by
> **binding**, which puts [#178](https://github.com/TheCaptainCompany/captain-food/issues/178) on the
> critical path rather than beside it.
>
> **C — the executor refuses a base it was not given** (landed, `.claude/agents/executor.md`):
> `git rev-parse HEAD` is a precondition, and a mismatch is a refusal. Six consecutive dispatch cards
> carried a stale base, including the one whose own text warned about it.
>
> **What the mob found that changes the work**: restaurant onboarding is not undesigned — it is
> designed and **anonymous** (`claimRestaurantListing` is `roles: [PUBLIC, RESTAURANT_ACCOUNT]`) and
> it **grants a `ScopeMembership` row**, so it is the write path into the trust anchor; the model has
> no word for the *person* (`principals.RESTAURANT` is an organisation id, so a credential against it
> is a shared login); the rider's OTP would draw on the customer's platform-wide SMS kill switch, so a
> flood against the anonymous endpoint grounds the fleet at peak; and the ownership fact ruling B
> needs is **already folded on the approve path** — no projection read, no new table required.
> A and B are **one slice of three operations**, not two programmes.

> 🔐 **2026-08-18 — GENERATED SECURITY SQL EXISTS, APPLIED TO NO DATABASE, SINCE 2026-08-18**
> ([#638](https://github.com/TheCaptainCompany/captain-food/issues/638) chunk 1). This line has a
> **visible age on purpose**: if it is still here in six weeks the chunk failed, and it will be
> legible. The flip event is the **local-acceptance walk**
> ([#556](https://github.com/TheCaptainCompany/captain-food/issues/556), ADR-20260817-105844) — a
> dated, local, near thing somebody is already building — **not** the production cutover, which is a
> cluster that does not exist and a suspension nobody has scheduled lifting.
>
> `specs/generated/security.{,permissive.}generated.sql` — two artifacts off one emitter
> (`tools/codegen-rs/src/emit/security.rs`), one guarded table (`OrderConversation`, picked because
> it has **no member-bearing column**, so the predicate cannot be short-circuited past
> `ScopeMembership`) plus the policy-bearing membership index. **Nothing entered `migrations/`**, and
> that is mechanical, not prose: `tools/codegen-rs/src/tests.rs::security_ddl_fence` refuses
> `ROW LEVEL SECURITY` / `CREATE POLICY` / `CREATE ROLE` in the deployed chain and converts at the
> walk with no edit — no tripwire the flipper has to delete.
> `crates/infrastructure/tests/rls_matrix.rs` applies the artifacts to its own
> throwaway databases (one per mode) and proves the matrix: 2 policied personas × 5 probes, a rider
> default-deny arm, the projector's negative-then-positive write arms, and the two modes compared as
> result sets. **Seven semantic mutations seen red** (M1, M2, M3a, M4, M5, M6, M7; M3b was not
> applicable — chunk 1 emits no persona views, so there is no join-first path to degrade), and two
> are kept as permanent arms — the
> GUC-shaped policy that lets a rider read the customer's thread, and the inherited persona grant
> that hands one login role the union. Decision:
> [ADR-20260818-171500](../adr/ADR-20260818-171500-mode-gates-the-whole-per-table-subtractive-surface-including-the-owners-write-policy.md)
> — `mode:` gates the whole per-table subtractive surface, **including the owner's write policy**.
> No `specs/**` source changed (`mode:` is an emitter parameter, not yet a DSL key), so no SPEC-LOG row.

> 🗂️ **2026-08-18 — RECORDS: the founder's own rationale for database-level security, plus the two
> register rows owed since the corrupted run** (Records only: `docs/**` — no `specs/**`, no
> `crates/**`, so no SPEC-LOG row and no regeneration.)
>
> **1. `PROP-20260818-010343` §1.1 now carries the argument no lens produced.** The founder,
> 2026-08-18: *"This will help us to avoid AI errors and unauthorised access."* Every lens in the mob
> pass argued row security as defence in depth against an **attacker**. The first clause names a
> different failure, and for this repository a likelier one — **a resolver written by an agent forgets
> its filter, and the policy holds anyway**. In a largely agent-authored codebase an omitted `WHERE`
> is the failure application-layer review is worst at catching, **because the code looks correct**.
> Recorded as the rationale that best justifies **building and proving** the layer early while
> production is suspended — and explicitly **not** as a reason to apply a policy to a database that
> does not exist yet (ADR-20260818-004647's cutover sequencing is untouched), nor as a property that
> survives the proposal's §2 defects: it holds only while the member type is bound to the **database
> role** (C-1) and the persona view stays **join-first** (C-2).
>
> **2. New register row `STAFF-AUTH` ([DECISIONS §46](../proposals/DECISIONS.md)), founder-owned, OPEN.**
> Restaurant staff, account managers and riders **have no way to sign in at all**. Verified at HEAD:
> the only authentication operations in the whole DSL are `specs/customer/api.yaml`'s
> `requestPhoneVerification` (:38) and `verifyPhone` (:43), both `roles: [PUBLIC, CUSTOMER]`, plus the
> V1 email pair (:50, :55), both `roles: [CUSTOMER]`; and the sole claim writer, `stamp_put_body`,
> hardcodes `role: "CUSTOMER"`, so nothing writes a RESTAURANT, RESTAURANT_ACCOUNT or RIDER claim
> anywhere. Until it is answered, every pilot credential for a non-CUSTOMER role is a hand-stamp in a
> third-party console — and per ADR-20260818-004646 Correction 3 such a token approves **any** pending
> refund. Tracked as [#639](https://github.com/TheCaptainCompany/captain-food/issues/639) (the number
> the ADR records; its live title and state are UNVERIFIED — this run had no GitHub read).
>
> **3. `IDENT-1` premise corrected, ruling untouched; `AUTHZ-GRAMMAR` updated.** *"Two of four roles
> have no mapping fact at all"* understated it — **three of four have no authentication path**, and
> only **one** business identifier is actually stored in the provider. The row now defers to
> ADR-20260818-004646 rather than restating the correction. And
> [#636](https://github.com/TheCaptainCompany/captain-food/issues/636) has been re-pointed to *"finish
> the `requires:` emitter"*, so AUTHZ-GRAMMAR's owed re-pointing is no longer outstanding.

> 🕸️ **2026-08-18 — FOUNDER RULING: BUILD THE GRAPH ENGINEERING. Plan committed as
> [PROP-20260818-013222](../proposals/PROP-20260818-013222-graph-engineering-for-the-team-workflow.md)**
> (Records only: `docs/**` — no `specs/**`, no `crates/**`, so no SPEC-LOG row and no regeneration.
> Nothing is implemented; this is the brief.)
>
> *"If we put in place the graph engineering we will improve the efficiency so make the plan now."*
> **Headline: a template and a validator, with the document as the gate's doc comment** — not a
> workflow engine, not a state file, not prose. **The evidence, re-derived at `8494e67`**:
> `CLAUDE.md:120` tells every session *"gates are hooks in `.claude/settings.json`"*, and that file
> contains **zero** hooks — `grep -rln '"hooks"' .claude/` matches nothing, anywhere. A load-bearing
> claim in the resident index, false, and **nothing ever went red**. A prose graph fails open; this
> repo has the proof, so every claim the graph makes is the doc comment on an executable rule.
> **Change set**: `docs/dispatch/TEMPLATE.md` (the 9 cards spell the base-SHA field **three** ways —
> `**Read at**` ×4, `- **Base**` ×5, `- **Card SHA stamp**` ×1) with a **13-row briefing table where
> silence is one token**, so "briefed 7 of 13" becomes a missing row in a diff ·
> `tools/codegen-rs/src/validate/dispatch.rs`, five rules modelled on `proposals.rs`, inside the
> already-blocking `make validate`, **no new runtime dependency** · a dead-man's-switch workflow
> shaped like `stale-claim-reaper.yml`, firing on **absence** · an executor preflight line
> (**proposed only** — `.claude/agents/*.md` commits need in-conversation approval,
> `docs/claude/sessions/workflow.md` §"A commit touching `CLAUDE.md` or `.claude/agents/*.md` needs
> in-conversation user approval") ·
> `docs/claude/team-graph.md` as the derivation, held to three
> constraints so it is not a fifth authority. **State model: 13 collapse to 8** —
> `intake → briefing/dispatch → execution-checkpoint* → independent-review → ci-gate → merged`, plus
> `blocked`, `founder-decision-required` (renamed: in this codebase a *customer* orders food),
> `stopped`; `repair`/`replan` are re-entries a fold counts, `split` is an edge,
> `merge-supervision` is the attribute `auto_merge_enabled`. **The finding to read**: of 11 edges,
> **5 are machine-decidable and 6 are agent assertions** — drawn differently, always.
> ⚠️ **`done` must not mean merged** (`farley`): the terminal state is `merged`, and `deployed` is
> declared **out of the graph with a named trigger** — `deploy.yml` is `workflow_dispatch`-only and
> production is suspended (§45 PROD-1), so a `deployed` state today would be one that never fires.
> ⚠️ **Two candidate gates were killed by mutation-testing them** (`beck`): *"the base SHA exists"* is
> green on all 9 cards **including the ones that were wrong** (verified — all 16 SHA refs resolve,
> including `4077188`, which card #623 labels stale in its own header), and line-bounds checking is
> green with an off-by-eight planted. What goes red is an **anchor token** beside each citation.
> Rule earned: **a gate that cannot go red is worse than none, because it reads as coverage** — every
> rule ships with a planted-defect test. ⚠️ **The routing matrix is REFUSED as a matrix**: it already
> exists as three lines in [ADR-20260816-134352](../adr/ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md);
> lenses have no GitHub identity so CODEOWNERS cannot apply, and a check over a PR body promotes that
> body to state. Only a `category → roster-size` floor is encoded. ⚠️ **The authored state is
> append-only, one file per transition**, like `.claude/loop-budget/<ISO-week>/` — never a
> `graph-state.json`: that shape already cost this repo **seven failures in one day**
> ([ADR-20260812-011057](../adr/ADR-20260812-011057-loop-budget-is-an-append-only-ledger-and-the-timer-is-never-committed.md)).
> **The graph never drives anything** — auto-labelling, auto-merging or re-ranking would make it a
> controller over state it cannot see, and is a recorded reversal.
> 🗣️ **Dissent preserved, not relitigated** (`holub`): wait until one order flows end to end —
> process artifacts outrun code **2.4:1** (102 commits in 14 days, 19 touching `crates/**`, 46
> docs-only). The founder decided otherwise; the sequencing answers it by making phases 3–5
> independently abandonable.
> ❓ **Open, founder decision required — GRAPH-SPEC-1**: the brief carried *"specs/ remains read-only
> in autonomous mode"*, which is verbatim the rule
> [ADR-20260810-221840](../adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md)
> **supersedes** (Accepted; lifted after eight issues were measured blocked). **The plan proceeds with
> `specs/**` NOT re-frozen** and raises it as a register row instead — a re-freeze is a decision
> reversal deserving its own ADR, not a constraint inherited through an implementation brief.

> 🔐 **2026-08-18 — THREE FOUNDER RULINGS: THE TOKEN CARRIES NO BUSINESS IDENTIFIER, RLS LANDS AT THE
> CUTOVER ON THE EMPTY DATABASE, AND THE SETTLEMENT READ IS BACK IN SCOPE**
> (Records only: `docs/**` — no `specs/**`, no `crates/**`, so no SPEC-LOG row and no regeneration.
> Records: [ADR-20260818-004646](../adr/ADR-20260818-004646-no-business-identifier-lives-in-the-identity-provider.md) ·
> [ADR-20260818-004647](../adr/ADR-20260818-004647-database-level-security-lands-at-the-cutover-and-the-settlement-read-returns-to-scope.md) ·
> [DECISIONS §46](../proposals/DECISIONS.md) rows **IDENT-1**, **AUTHZ-LOCUS**, **AUTHZ-GRAMMAR**, **RLS-SEQ**.
> Whole roster consulted first; both ADRs carry a per-lens `Consulted:` block.)
>
> **1. No business info is stored inside the identity provider** — *"the mapping with business
> identifiers will be done in the OVH Postgres."* Asked V0 or post-first-order: **"v0"**, so it
> sequences **before** the write-side enforcement seam ([#178](https://github.com/TheCaptainCompany/captain-food/issues/178)
> slice 1). **This is a change, not a confirmation of the posture**: `crates/server/src/auth.rs:194-220`
> binds `claims.customer_id`/`restaurant_id`/`restaurant_account_id`/`rider_id` today, and
> `crates/infrastructure/src/integrations/supabase_auth.rs:424-433` writes
> `app_metadata.captain_food = { role, customer_id }` **back into** the provider. The bridge that
> replaces them already exists for one role — `by_auth_ref` (`crates/application/src/queries.rs:341`),
> called only from the mailbox worker and only for CUSTOMER
> (`crates/infrastructure/src/mailbox/handler.rs:244-258`) — and is promoted to the request seam and
> extended to the other three. **The price, not softened**: `resolve_read_scope` (`auth.rs:1833`) is
> synchronous today and runs once per request (`crates/server/src/graphql/routes.rs:166`, plus :285 per
> WS connection); it becomes a lookup, and the enforcement slice's *zero I/O at peak* claim dies with
> it — peak being Friday/Saturday 19:00–21:30. **Cheapest moment there will ever be**: production is
> suspended (§45 PROD-1) and Q-L3 = no real end user, so dropping the claims strands **no issued
> credential**; after a pilot the same change forces a re-auth of every credential. It is a
> **MIGRATION** — resolve-and-ignore first (no token invalidated), then stop stamping, then erase the
> stored identifier; `domain_events.user_id` is the auth subject (ADR-0041) and does not move.
> ⚠️ **Measured residue**: only CUSTOMER has the mapping end to end. RIDER has the fact in the event
> (`specs/delivery/events.yaml:343-351`) but **no projection column**; **RESTAURANT and
> RESTAURANT_ACCOUNT have no `authRef` anywhere in `specs/**`**. Those facts must be authored, and the
> `specs/**` changes this implies are **owed and unapproved** — nothing in `specs/**` was touched.
>
> **2. RLS lands at the CloudNativePG cutover, on the empty database — starting at
> `OrderConversation`, not `OrderTracking`.** Three of the four drafted tables do not survive, each
> for a reason measured against the tree: a policy on `OrderTracking` breaks the settlement read
> **silently** (`payment_settlement.rs:83-84` reads it under `ReadScope::System` on all four legs
> before every capture and release, and **RLS filters rows rather than raising**, so zero rows lands
> in the existing `HookOutcome::Skip` arm — *"nothing to settle"*: food delivered, money never
> collected, reported as a green log line, strictly worse than the wall STO-9 describes);
> `View_DeliveryJob` is a **VIEW** (`specs/generated/views.generated.sql:6`) and Postgres has no
> `CREATE POLICY` for views, with `security_barrier` an optimizer fence rather than
> `security_invoker`; `CustomerCreditBalance` is per-customer and **`ScopeType` has no member for it**
> (`specs/common/scalars.yaml:721-729` is `ORDER`/`RESTAURANT`); and `FORCE` as drafted leaves the
> `projector_{scope}` writer **no policy slot** (RLS is default-deny for non-owner roles — the
> projection stops on the first event after cutover) while a `WITH CHECK` over `ScopeMembership`, a
> separate projection with its own checkpoint, makes a read-model rebuild **order-dependent**.
> `OrderConversation` is the right first table: a TABLE, identity = its `orderId`, and the Art. 9
> free-text surface named in §45 IDOR-DEADLINE-GAP. **Gate-then-stabilize is untouched** —
> PROP-20260811-093000 §6.3 still governs RLS on `domain_events`.
>
> **3. The `OrderTracking` settlement read (§32 STO-9) is back in scope** — *"now that we know how to
> deal with security at the database level we can integrate it now."* The row stays **OPEN**, options
> (a)–(e) and the 2026-08-15 lean on (e) unchanged; what changed is that it is a **precondition** of
> any policy on that table rather than a separable row.
>
> **The DESIGN those rulings sequence is now on the table** (2026-08-18):
> [PROP-20260818-010343](../proposals/PROP-20260818-010343-database-level-security-the-measured-design.md),
> tracking issue
> [#638 "Database-level security: the measured RLS design"](https://github.com/TheCaptainCompany/captain-food/issues/638).
> It is `dba`'s work, **built and measured on a throwaway PostgreSQL 16.13 cluster** against the real
> generated identifiers, and almost none of the held draft survived it. Two blocking findings: the
> persona split is **decorative** while the member type comes from `current_setting('app.member_type')`
> (a rider connection read 2 customer orders — the fix binds the member type to the **database role**
> as a policy literal), and an **RLS predicate cannot drive a scan** (200k orders: 180.569 ms `Seq Scan`
> through an `EXISTS`-policy view vs 0.263 ms through a view that **joins from `scopemembership`
> first** — RLS is the backstop, the query carries its own selective predicate). Plus the **zero-row
> family** (four measured ways this design silently returns nothing), the counterfactual that `FORCE`
> is the single flag between an empty screen and a **total cross-order leak**, silent projector
> `UPDATE 0` under a `FOR SELECT`-only policy set, and the `identity_binding` **placement** correction
> (projected into every `recovery: replay` database, not parked in `captain_write`). Rollout adopts
> [#637](https://github.com/TheCaptainCompany/captain-food/issues/637): every policy ships
> `USING (true)` first, one `mode:` key, `permissive` → `enforcing` per table. **Two open FOUNDER
> decisions** are named in it and block approval — the rider's own payout/tip/rating columns, and
> `View_DeliveryJob`'s placement.
>
> **The three externally-authored ADRs are HELD, not deposited.** `ADR-20260817-232744/232745/232746`
> are not in `docs/adr/`; their surviving content is carried, corrected, by
> [#635](https://github.com/TheCaptainCompany/captain-food/issues/635),
> [#636](https://github.com/TheCaptainCompany/captain-food/issues/636) and ADR-20260818-004647.
> **AUTHZ-GRAMMAR**: the `authorization:` block is **declined as new grammar** — `requires.acting`
> already exists and is validated (`specs/comms/actors.yaml:65-72`,
> `tools/codegen-rs/src/refs.rs:453-455`), so the corrected design is to finish its emitter with
> completeness keyed on `actors.yaml receives[]` (the receiving actor — the only join that sees the
> three PM-received commands, i.e. the refund door). **Owed to the `architect`**: #636's body still
> describes the grammar it was filed to correct, and `PROP-20260726-171500` still reads as if §D1 were
> open — both need re-pointing in the same change as the first implementing slice.

> 🛒 **2026-08-17 — THE SMOKE'S CART READ IS A PAIR ON TWO HOSTS, NOT ONE READ ON THE WRONG ONE**
> ([#622](https://github.com/TheCaptainCompany/captain-food/issues/622), PR
> [#633](https://github.com/TheCaptainCompany/captain-food/pull/633). Tooling + workflow only: no
> `specs/**`, no `crates/**`, so no SPEC-LOG row and no regeneration.)
>
> `prod-smoke.sh` L4 wrote the guest cart on the marketplace host and read it back there too, then
> placed and tracked the order there — a journey no client walks. `current` resolves its tenant from
> the `Host` ([#469](https://github.com/TheCaptainCompany/captain-food/issues/469)) and correctly
> refuses to answer unbounded, so the read returned `{"current":null}` **with no error**:
> byte-identical to "the cart never projected". Measured both ways on the same cart and session —
> `null` on the marketplace host, `OPEN` at 1200 EUR cents on the tenant host. **The server is
> right**; only the smoke was wrong. This is *not* recorded as the cause of the 2026-08-04
> `{"cart":null}` red: the defect is confirmed and deterministic, the attribution is strong but
> **unconfirmed**, and settling it needs the run log against #469's landing date — which nobody has
> done and nobody needs done to fix this.
>
> **The fix is a PAIR, because `null` is a legal answer for two different reasons.** Positive on the
> storefront (exact cart id, `restaurantId`, `OPEN`, **`== 1200`**, `EUR`); negative control once,
> after the positive is green, same query and session, on the marketplace host, where `current` must
> be `null`. Three outcomes, all attributable: tenant non-null + marketplace null = correct; **both
> null = the cart is genuinely broken**, delivered in seconds instead of never; marketplace non-null
> = a cross-tenant leak, an incident rather than a smoke bug. The total is `==` and stays `==` — if a
> fee component ever enters cart pricing, that red is the intended alarm, and the failure message
> says so, so the predicate cannot be quietly weakened to `> 0`.
>
> **Three structural changes rather than three comments.** The whole L4 leg moved to the storefront
> (L3's restaurants-by-slug and catalog reads stay on the marketplace — that is genuine browse); call
> sites **cannot name a base** any more (`marketplace*` / `storefront*` / `admin*` helpers hardcode
> theirs), because prose survives until the next refactor and a removed choice does not; and L4 fails
> loudly up front if `SMOKE_BASE_DOMAIN` is not the apex — `surface_runtime::hosts::APEX` is a
> compile-time constant, so under any other domain every host classifies as `Default` and the
> identical undiagnosable null returns, one config flip away, where it would be blamed on this fix.
> Also: the read guard's "a stranger's `orders` is empty" now runs **after** a positive control that
> the owner's own list contains the order (real today, vacuous the day that query becomes host-bound),
> and the cart wait carries a diagnosis arm — one ADMIN by-id read separating a read-path/host defect
> from a projection defect. That arm's absence is why a null went undiagnosed for twenty nights.
>
> **Discharges the "Owed, unfiled" half recorded below**: the nightly's schedule is disabled here,
> with the reason and the re-enable condition in the workflow header — carried **verbatim** from the
> unmerged [#556](https://github.com/TheCaptainCompany/captain-food/issues/556) branch so the two
> merge without conflict. Without it, `main` would keep a nightly that runs a *fixed* smoke against a
> deliberately-suspended production every morning: a red per night carrying no information.
>
> **The review found the same class a THIRD time, and that is the entry's real lesson.** Every round
> of this change had one defect where a failure of the DIAGNOSIS was reported as a finding ABOUT THE
> SYSTEM: the original `null` (host refusal read as a broken cart); then an EMPTY reading read as
> "row absent" (caught by the rehearsal); then a **`null` reading** read as "row absent" — the
> tolerant helper collapses a transport failure to `{}`, and `{} | .data.cart // .errors` prints a
> bare `null`, which is non-empty, so an emptiness guard waves it through: **a failed ADMIN request
> reported as a broken projection**. Both lenses found that fourth state independently. The rule that
> survives it: **check the envelope before interpreting, and have the arm state its conclusion rather
> than print a value plus a legend** — a legend is one more place to map the wrong row. Two more:
> the order-timeout arm sat at function top level, so a failed mint **exited the script** and the
> 90-second money-path wait ended with **no verdict at all**; and `TENANT BINDING BREACH`, the
> loudest string in the file, asserted a cross-tenant leak for a symptom that a mis-pointed
> `SMOKE_PUBLIC_BASE` produces identically — which is exactly how the red was produced. It now tests
> the two bases for coincidence and names the configuration cause when they match.
>
> **Executed, not asserted** — a fix to a gate that is never run is a hypothesis. Against a local
> single-database stack (bare server, real migration chain, the walk's JWKS stub): L1→L3b PASS and
> the cart pair green (`1200` on the storefront, `null` on the marketplace host); the negative
> control **seen red** with its binding-breach message when the public base is pointed at a tenant
> host; and the pre-fix state **seen red attributably** — `ADMIN sees cart …: {"status":"OPEN",
> "totalAmount":{"amountCents":1200}} (row PRESENT = the STOREFRONT READ or its host binding is the
> defect)` instead of a bare `{"cart":null}` timeout. **The rehearsal caught a defect in the fix
> itself**: with the ADMIN mint unavailable the diagnosis arm emitted an EMPTY reading into a
> sentence offering two interpretations — indistinguishable from "row absent", the same
> mis-attribution class being fixed. Unusable token, unparseable body and a real answer are now three
> different outputs.
>
> **Scope honesty**: nothing downstream of the cart leg has run green in production since 2026-07-29.
> The rehearsal stops at `placeOrder` → `FAILED/Internal`, whose proximate cause is the placeholder
> Stripe key (intent-create precedes any append). Further reds there are new findings, not this fix
> failing, and this PR does not claim "L4 fixed".

> 🔎 **2026-08-17 — A FAILED CHECKOUT IS ATTRIBUTABLE AGAIN, AND THE JOURNAL ROW CAN NO LONGER
> CARRY A STRIPE KEY** ([#623](https://github.com/TheCaptainCompany/captain-food/issues/623), PR
> [#626](https://github.com/TheCaptainCompany/captain-food/pull/626); floor of
> [#625](https://github.com/TheCaptainCompany/captain-food/issues/625) and part 1 of
> [#624](https://github.com/TheCaptainCompany/captain-food/issues/624). SPEC-LOG row landed.)
>
> The walk's failed `PlaceOrder` recorded `{"code":"Internal","context":{}}` with nothing at ERROR or
> WARN. **The trap was that the obvious fix leaks**: ten handler sites already do
> `context: { detail: e.to_string() }`, and Stripe's message for a bad key is literally
> `"Invalid API Key provided: sk_test_…"` — so widening `detail` would have written a key into a
> jsonb column kept for the retention window and served as `Operation.errorCode`. The leak canary
> landed RED first and proved the catalogued (`PaymentDeclined`) arm was **already** copying provider
> prose verbatim; the empty `{}` #623 reports was the only branch on that path that was not leaking.
>
> The row now carries a GENERATED bounded shape (`CommandFailureAttribution`: seam, reason, optional
> gateway status) in which a provider body is unspellable, and the free text goes to the log. **No
> verdict, outcome, status, retry behaviour or customer-facing message moved**, and **no
> `errors.yaml` code was declared** — catalogue membership is the REJECTED-vs-FAILED discriminator,
> so declaring one flips a verdict silently.
>
> Two things measured that the dispatch card had wrong or open. **`verdict_of_error`'s
> `DomainError::Repository` arm is dead on every live path** (intercepted above all three call sites,
> the `PlaceOrder` one inside `pm_delivery::prepare`), so the card's "cart read" second seam is
> undrivable and the two-seam test uses `COMMAND_PAYLOAD` instead. **`PaymentGatewayRefused` was
> minted at five sites and matched at ZERO** — renaming it left all 363 `application` unit tests
> green — so it carried no machine meaning at all until this change gave it some.
>
> New validator **§21 `obs-technical-error-unreachable`**: `place-order` declared
> `technical_error.any_span_errors` while not one of its spans could carry an error status, so the
> class was structurally unreachable and its dashboard permanently empty. 12 findings before the
> instrumentation, 11 after; the remainder are the frozen ratchet population, same posture as §20.
>
> **The review changed three things and they are the better half of the chunk.** The catalogued arm
> no longer records `{}` either: a declined card now carries the same bounded shape with the
> gateway's `402`, because `PaymentDeclined` and nothing else answers *was it declined* but not
> *declined how*, on the money path. It is claimed on EVIDENCE — a real gateway answer — so the
> fail-closed stand-in, which mints the same catalogued code with no gateway behind it, still says
> nothing rather than blaming a customer's card. `Seam::EVENT_APPEND` was **withdrawn from the
> scalar**: nothing in the tree produced it, its producers are #628's, and shipping a
> declared-but-unemitted member inside the chunk about that defect class would have been the joke
> writing itself — an exhaustive test now makes adding one fail the build. And the secret predicate
> is a shared function (`stripe_adapter::secrets`) widened to `sk_` / `rk_` / `whsec_` / `pk_live_` /
> `sk_live_`, so [#627](https://github.com/TheCaptainCompany/captain-food/issues/627)'s canary calls
> it instead of re-remembering the list, narrower.
>
> Two claims were **corrected** by the review rather than confirmed. The second seam's mutation is
> not "undiminished": gateway-vs-read would have crossed two `DomainError` variants, gateway-vs-
> payload is two `Invariant`s discriminated by prefix, so the test exercises prefix discrimination
> twice and variant discrimination never. And the validator's red was **not** seen before its fix —
> rule and fix are one commit — which is accepted only because its planted-input test is red-on-
> mutant permanently rather than once at authoring time.
>
> **Deferred and NOT in that PR** (`HOLD: human` or their own chunk): re-classifying gateway failures
> out of `business_rejected` (#624 part 2), the typed gateway-failure enum and declaring
> `PaymentGatewayRefused` in the catalogue (#625), and the eleven frozen
> `obs-technical-error-unreachable` contracts, now enumerated with an owner in
> [#631](https://github.com/TheCaptainCompany/captain-food/issues/631) so the baseline is not their
> permanent home.

> 🗳️ **2026-08-17 — THE FOUNDER ANSWERED THE WHOLE DECISION QUEUE: THE WALK GOES FIRST ON ONE
> DATABASE, PRODUCTION STAYS DOWN ON PURPOSE, AND THE ROSTER REVERSION IS STRUCK**
> (records-only run, straight to `main`; no `specs/**`, no `crates/**`, so no SPEC-LOG row and no
> regeneration. New [DECISIONS §45](../proposals/DECISIONS.md) ·
> [ADR-20260817-105844](../adr/ADR-20260817-105844-the-walk-goes-first-on-one-database-and-production-stays-suspended.md) ·
> [ADR-20260817-105845](../adr/ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md).)
>
> Six rows went to him as one queue and came back answered. **Two went against the team's own
> recommendation** (PROD-1, REV-1) and are recorded with the reasoning that supports what he chose.
>
> **(1) PROD-1 — production STAYS SUSPENDED, as a decided state.** He declined restoring-with-signup-
> closed and chose to walk locally. **The defect underneath was never the 503**: the nightly
> `prod-smoke` has been RED for **19 consecutive scheduled runs** (last green **2026-07-29**;
> 2026-07-30 → 2026-08-17, no gaps), of which the suspension explains only **13** — **six earlier red
> nights have an unrecorded cause** — and no record in this repository called it a broken gate. The
> "Open incident" section at the foot of this file is retitled accordingly. **Owed, unfiled**:
> re-point the nightly at the local walk target or disable its schedule with the reason in the
> workflow — the **disable half is now landed** (see the #622 entry at the top of this file; the
> re-point stays owed and is deliberately conditional on the walk being green end to end).
>
> **(2) SEQ-1 — the walk goes FIRST, on ONE database.** The acceptance criterion is **unchanged as
> what certifies** (local, eleven databases, full enforcement, the six clauses, the auth posture, the
> D2 semantics, the honesty sentence); it simply stops **gating** the first end-to-end reading. The
> target is the single-node k3s stack that already stood up on **2026-08-11**. This resolves the
> 2026-08-13 ↔ 2026-08-14 contradiction **in favour of the 2026-08-13 sequence**, and the two
> `STATUS.md` markers placed earlier today are re-cut: the 2026-08-14 entry is now labelled **the
> sequence that CERTIFIES**, the older program of record **the sequence that RUNS NEXT**. They no
> longer compete. **It does not overturn final-vision-first**: that directive forbids an intermediate
> *where the final step can be built*, and it cannot be — the split band is blocked on **STO-7**,
> **STO-8** and **STO-9** (each independently), with **STO-10** parked and **RDR-1** open upstream of
> #513's grant emitter. **Mandatory label**: what the one-database walk produces is a **reading**,
> never *accepted*.
>
> **(3) MOB-ANTECEDENT (§44 MOB-COST-1a, CLOSED) — the roster reversion is STRUCK.** HIGH-CONSEQUENCE
> returns to (b)+(c) as originally ruled. In its place: **a dispatch card may not state a derived
> number without naming its antecedents, and any bare number it does state is marked `UNVERIFIED
> input`** — the cause both banked misses share (#608's briefing was `WHOLE ROSTER` on the committed
> card; #609's clean attribution `vernon` rejected as his own depth miss). **Banking and holub's
> verification condition are untouched**; only what a MISS *triggers* changed. **Residue, named**: a
> genuine roster-width MISS now has no automatic consequence — it is banked with an attribution and
> returns to the founder. Spec-side half already executable (PR #610's `derived_from` → `ConfigKey`);
> dispatch-side half filed as [#619](https://github.com/TheCaptainCompany/captain-food/issues/619).
> CLAUDE.md's mob bullet is amended in this change.
>
> **(4) REV-1 — `claude-review` comes OUT of the required checks.** A knowingly-given-up mechanical
> guarantee: the team's own reviewer is the gate that finds things (it failed
> [PR #610](https://github.com/TheCaptainCompany/captain-food/pull/610)'s first head on three
> blockers, one on the money path), while the bot check's live failure mode
> ([#593](https://github.com/TheCaptainCompany/captain-food/issues/593)) is red-in-25-seconds with an
> empty key and **no diff evaluated**, blocking every PR. **Compensating control: the independent
> reviewer pass before ready-for-review stays MANDATORY.** ⚠️ **NOT EXECUTED** — the ruleset PATCH
> returned **403 from this session's agent proxy** (*"Write access to this GitHub API path is not
> permitted through this proxy"*), an egress block rather than a GitHub denial, and the proxy's README
> forbids routing around it. **`claude-review` is still required on `main` today**; it is an open
> action on #593. ADR-20260807-235930 carries an amendment box.
>
> **(5) STO-10 — PARKED until the walk lands**, reported blocked, never re-ranked; #513 still must not
> emit the CONNECT that would decide it by default.
>
> **(6) IDOR-DEADLINE — the deadline is now the EARLIEST OF** a second **restaurant** credential
> issued outside the team *including demos and pilots* · a **rider** credential to a non-team person ·
> the **first real customer order**. Enacted as the two lenses proposed, and strictly **tighter** than
> the wording it replaces. §39, [#178](https://github.com/TheCaptainCompany/captain-food/issues/178)
> (a new `## Deadline` block) and [#618](https://github.com/TheCaptainCompany/captain-food/issues/618)
> (which already carried it) all match. The condition travels with it: **an open item past its own
> published deadline is the worst state available**, so it is met or publicly re-dated *before* it
> passes.
>
> ⚠️ **ONE GAP FOUND WHILE LANDING THE SIX, raised not smoothed — new register row
> `IDOR-DEADLINE-GAP`.** All three deadline triggers are things the **team** does. For `CUSTOMER`,
> **nobody issues anything**: signup is self-service, and two CUSTOMER-reachable reads
> (`orderConversation`, `reclamation`) take a caller-supplied id with no ownership check. So
> *production restored with signup open* issues credentials outside the team, reaches other customers'
> free-text prose, and **trips none of (i), (ii) or (iii)** — carried today **only** by PROD-1 keeping
> production down. It was **not** treated as a contradiction that stops the answer: the new trigger set
> is strictly tighter than the one it replaces and the gap existed identically under the old wording,
> so landing it improved the record and the honest output is the open row plus this line.

> 🧾 **2026-08-17 — FOUR RECORDS PUT RIGHT: THE IDOR COVERS 83 OF 118 OPERATIONS, NOT THE ORDER
> LIFECYCLE'S WRITES, AND THE #608 CHECKPOINT MISS WAS NEVER A ROSTER MISS** (records-only run, straight to `main`; no `specs/**`, no
> `crates/**`, so no SPEC-LOG row and no regeneration).
>
> **Why a whole session on records**: on 2026-08-16 a contradicting `STATUS.md` line propagated a false
> claim into a founder-facing brief. That is the cost that earned this run — a wrong record is not
> inert, it is an input.
>
> **(1) [DECISIONS §39](../proposals/DECISIONS.md) IDOR-1 — scope corrected, verdict and deadline
> untouched.** The row described a cross-tenant **write** IDOR on the order lifecycle. The real surface
> is **83 of 118 operations on both sides**: 76 of 86 mutations with no proven domain binding — split
> **37 bindable** (a payload field is the caller's own scope; prove field == verified claim) and **39
> unbindable** (*no payload field corresponds to the caller at all*, so stripping ids from payloads does
> nothing; needs a participant check against folded state) — plus **7 read surfaces with no
> `ReadScope`**, filed as [#618 "Read surfaces missing `ReadScope` — the read half of the write-path authorization gap (#178) — and two return the whole
> platform when called with no arguments"](https://github.com/TheCaptainCompany/captain-food/issues/618), **two of
> which return other tenants' rows when called with NO ARGUMENTS**. `approveRefund`/`denyRefund` are the
> worked unbindable case: role `RESTAURANT`, payload `{orderId, amount, reason}`, **no identity
> consulted anywhere** — money movement decided by a caller nothing proves is a party to the order.
> **Also corrected: the fix is not simply cheaper-because-claims.** The read side resolves identity from
> JWT claims, but the **write** side does a **database lookup in the mailbox worker**
> (`mailbox/handler.rs:244-257`) **and only for `CUSTOMER`**; every other role gets `None`. Any claim of
> the form *"we already have the identity at the handler"* is **false today**. And `external_tokens` is
> a **flat shared list with no per-partner identity** (`auth.rs:442,480-483`) — a partner action cannot
> be attributed to a partner, **present tense**, not gated on the first order. ~~Recorded but **not
> enacted**~~ ✅ **ENACTED 2026-08-17 ON A FOUNDER ANSWER**: two lenses argued the trigger should be
> the **earliest of** a second restaurant credential outside the team *including demos and pilots*, a
> rider credential to a non-team person, or the first real customer order — an IDOR needs two
> principals and the second credential exists at **onboarding** — and the founder took it. §39,
> [#178](https://github.com/TheCaptainCompany/captain-food/issues/178) and
> [#618](https://github.com/TheCaptainCompany/captain-food/issues/618) all carry the new wording.
> ⚠️ **Gap raised while landing it** (new register row **IDOR-DEADLINE-GAP**): all three triggers are
> things the TEAM does, while customer signup is **self-service** — so *production restored with
> signup open* trips none of them, and is carried today only by production being deliberately down.
>
> **(2) [DECISIONS §44](../proposals/DECISIONS.md) MOB-COST-1a — the ATTRIBUTION was wrong; the RULING is
> untouched.** The row blamed the [#608 "Nothing detects an authorized payment with no order birth"](https://github.com/TheCaptainCompany/captain-food/issues/608)
> miss on the checkpoint no longer inviting a lens. The committed
> claim-time card (`6d00cb3`) says **`Briefing roster: WHOLE ROSTER`** — only the *checkpoint* was
> narrowed, so the wrong arithmetic was in front of **every** lens and none challenged it. The card's
> *"originated in THIS CARD"* is also imprecise: the committed card contains **no 50 s figure**, only
> *"a threshold justified against the ~7-day Stripe hold expiry"*. **n=2**, and the second points the
> same way: [#609 "Lane addressing residue after #596"](https://github.com/TheCaptainCompany/captain-food/issues/609)
> banked a MISS (its card's §"Checkpoint verification", now on `main` via PR #613) whose clean
> attribution `vernon` **rejected** — his own briefing finding
> named the literals and read their coupling to the declaration as a liability without taking the step
> to reading it as a **pin** — banked shared, weighted to `vernon`, only the *escalation* to the absent
> `young`. **The defect exposed is not roster width**: *a coordinator-authored derived number is
> consumed by every lens as established fact and nothing verifies it.* §44 is the founder's own ruling,
> so the **recorded reversion of HIGH-CONSEQUENCE to whole-roster STANDS** and no class is declared
> un-reverted; the replacement — **a dispatch card may not state a derived number without naming its
> antecedents, and any bare number it does state is marked UNVERIFIED input** (a gate, whose spec-side
> half PR [#610 "Detect an authorized payment with no order birth"](https://github.com/TheCaptainCompany/captain-food/pull/610) already built) — is
> **recommended and PENDING FOUNDER**.
>
> **(3) Three stale readings in this file reconciled** (see the marked entries): the split-first
> keystone tail is now labelled **the live sequence**, and the older *"harness before L5"* program of
> record is labelled **superseded** in place with its still-true content named, so a reader landing on
> either one knows which wins; and the Stripe-webhook line that still framed an ingress as an open
> founder-owned blocker is corrected against
> [ADR-20260813-004634](../adr/ADR-20260813-004634-supabase-auth-is-retained-for-v0-and-the-window-closes-at-the-first-real-order.md)
> — **no inbound ingress is required**, the CLI's outbound tunnel plus its own signing secret reaches a
> local stack. The true residue is kept: nothing **wires** it yet.
>
> **(4) New [BRIEF-20260816-idor-obligation-map](../legal/BRIEF-20260816-idor-obligation-map.md)** — the
> `legal-specialist` obligation map and counsel packet (IDOR-L1…IDOR-L9) landed in the house format.
> **Not legal advice, not clearance.** Two findings **survive the code fix**: (i) **free-text
> special-category data** — reclamation descriptions and order conversations are unbounded customer
> prose in a *food* business, so they predictably carry allergy, illness and dietary-religious
> statements (Art. 9(1)), which makes individual notification close to automatic, is a mandatory-DPIA
> trigger in its own right, and needs an **Art. 9(2) basis for the ORDINARY case** — unsolved design
> work that scoping the reads does not answer; (ii) **blast-radius unboundability** — with no tenant
> predicate, no pagination and no returned-row-count logging, a breach's scope could not be bounded
> after the fact, so **notification would have to assume the maximum**. Cheap fix, on the artifact list.
> The brief also records the **publication split** the team followed. ~~Its condition: if production is
> restored before the fix lands, the public posture page comes down until it does.~~ **WITHDRAWN
> 2026-08-17 — see the entry below.**
>
> **Two adjacent findings, named not fixed** (both owe a `specs/**` edit + SPEC-LOG row, out of scope
> here): `specs/ordering/api.yaml:207` claims *"Restaurant/ownership scoping is enforced server-side"*
> and `specs/comms/api.yaml:58` claims *"Ownership enforced server-side"* — **both false**, and the
> ordering one contradicts its own next sentence. A false control claim in the source of truth is how a
> reviewer stops looking. **✅ BOTH CORRECTED 2026-08-17, along with four more the sweep found.** And:
> **this repository is public**, so §39 and this file publish a `file:line` recipe for a live
> unremediated cross-tenant IDOR — carried today only by there being no live instance with real users.

> 🔒 **2026-08-17 — THE PUBLIC-REPO CORRECTION: SIX FALSE CONTROL CLAIMS FIXED, ONE THEATRE CONTROL
> REPLACED, AND THE CREDENTIAL GATE HAS A HOLE**
> (spec+docs direct-to-`main`; [DECISIONS §39](../proposals/DECISIONS.md),
> [SPEC-LOG](../SPEC-LOG.md), [SECURITY.md](../../SECURITY.md),
> [BRIEF-20260816-idor-obligation-map](../legal/BRIEF-20260816-idor-obligation-map.md).)
>
> Arising from a **corrected premise — this repository is public**, which nobody had established when
> the 2026-08-16/17 security records were written. **Nothing was reverted**: `legal-specialist` was
> explicit that narrowing the §39 widening would be strictly worse, since the diff stays in public git
> history and the code says everything anyway — a concealment signal bought for zero secrecy.
>
> **(1) Six false control claims in the DSL, corrected.** `specs/**` descriptions asserted
> *"ownership enforced server-side"* on surfaces that enforce nothing beyond a role guard:
> `restaurantReclamations`, `restaurantDeliverySatisfaction`, `orderConversation`,
> `orderConversationInternalNotes`, **`pendingRefunds`** (money path — the `restaurantId` filter is
> OPTIONAL, so omitting it returns every restaurant's refund queue) and `restaurantLocationsByAccount`.
> The register named two; **the sweep found four more**. **Nine sibling sites with the same phrasing
> were verified TRUE and left alone** — the phrasing is not uniformly false, so each site had to be
> read against its resolver.
>
> **(2) The publication-split control was theatre — withdrawn and replaced.** *"The posture page comes
> down"* protects nothing when the source is public. Replaced by: **production may be restored, but no
> credential is issued outside the team until the write-binding lands**, enforceable at the auth
> provider rather than by promise.
>
> **(3) ⚠️ THAT GATE HAS A HOLE — customer signup is SELF-SERVICE.** Verified: `verifyPhone` /
> `requestPhoneVerification` are `roles: [PUBLIC, CUSTOMER]` and a first verified phone CREATES the
> Customer. **Any stranger self-issues a CUSTOMER credential the moment the surface answers**, so the
> gate must be **signup**, not onboarding. **TWO** surfaces are CUSTOMER-reachable, take a
> caller-supplied id and apply no ownership check — `orderConversation` **and `reclamation`** — and they
> are exactly the two free-text stores the Art. 9(1) finding is about, so **a stranger who registers
> with a phone number reads other customers' complaint text and message threads**. Carried today
> **only** by the 503. ~~**Decision owed** (recorded, not taken): keep signup closed at the auth provider
> while production runs, or close #618 first.~~ **ANSWERED BY CIRCUMSTANCE, NOT BY RULE, 2026-08-17**:
> the founder chose to leave **production suspended** ([DECISIONS §45 PROD-1](../proposals/DECISIONS.md)),
> so neither arm is exercised and no surface answers. **It is not closed** — it becomes owed again,
> unchanged, the day restoration is on the table, and the same fact is why the new IDOR deadline's
> trigger set has a hole (**IDOR-DEADLINE-GAP**).
>
> **(3b) `reclamation` was NOT part of the prose fix and needs its own work.** It asserts no control, so
> there was no false claim to correct — but it is the sharpest unscoped read on the platform. Recorded
> in §39 for whoever works #618. **Related record defects found and fixed the same run**: four records
> cited #618 by a title it does not have (a *proposed* title never applied), and §39 claimed *"#618 owns
> the authoritative list"* of unscoped surfaces when the issue body contains **no enumeration at all**.
> Same shape as the defect this entry is about — **a record asserting that a control or an artifact
> exists somewhere else, which nobody re-checked.** The verified enumeration now lives in §39.
>
> **(4) The strongest argument is not the legal one: `domain_events` is immutable.** A hostile write is
> not deleted, it is upcast — it stays in the log forever. The empty-log window is what makes this
> defect free today, and **one stranger's write ends it permanently.**
>
> **(5) Public-tree sweep for personal data and secrets: CLEAN** — a useful recorded negative. Scanned
> the tracked tree and **618 issues/PRs**. Every phone number is synthetic (`+336123456xx`,
> `+33611223344`, `+33600000000`); the only non-example emails are `noreply@anthropic.com` and
> `ops@uberdirect.example`; commit authors are all `users.noreply`/bot addresses. Stripe-key hits are
> **prefix validators** (`sk_live_` comparisons), not keys, and `deploy/generated/secret-keys.json` is
> names-only by construction. Tours street names in fixtures are real streets but identify no person.
> **Nothing was deleted, because nothing genuine was found** — and a deletion in public git history
> would not be a deletion anyway. **Standing rule recorded: no production data ever enters this
> repository**, including a pasted log line in an issue.
