# ADR-20260831-121957 — The PM `read:` step is retired: `source:` fixed the physics and left the ownership

**Status**: Accepted (the retirement is decided; **how the two survivors are spelled is OPEN** —
register row **PMW-4**) · **Date**: 2026-08-31 ·
**Decider**: the founder / Tech CEO, verbatim below ·
**Reconsiders**: **PMW-1** (`docs/decisions/PMW-1.yaml`, CLOSED 2026-08-15 as (a) + the additive §8
grammar) — the challenge row is **PMW-4** ·
**Amends**:
[ADR-20260815-030206](ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md)
(the rule is unchanged; its *enforceable* form moves from "a validator rule over `read:`" to "there
is no `read:`") ·
**Rewrites in place** (LIVING-proposal doctrine, ADR-20260801-020000):
[PROP-20260815-142349](../proposals/PROP-20260815-142349-actor-answers-block-and-the-ask-step.md)
§18 and D2 ·
**Register**: [DECISIONS](../proposals/DECISIONS.md) §42 (**PMW-1**, **PMW-3**, **PMW-4**) ·
**Posture**: `HOLD: human` for the *build* this record authorises — see §9. **This record itself is
docs-only**: no `specs/**` edit, no code, no generated artifact beyond the register index.

## Enforced by

Nothing yet, deliberately, and the shape of the eventual enforcement is the point of §1: the
retirement's gate is the **absence of a step kind**, not the presence of a rule. The PM step matcher
in `tools/codegen-rs/src/validate/process_managers.rs` is already closed — its `"read" =>` arm sits
at `:423` inside a match whose catch-all at `:854` pushes `pm-step`: *"unknown step kind '{}' (read
| guard | call | deliver | send | state)"* — so **deleting the `"read"` arm makes the kind
unspellable by the gate that already exists**, and the step-kind list the message prints is the one
that shrinks with it. No new validator rule
is owed for the retirement itself (ADR-20260803-234035: a check is the fallback; deleting a gate the
compiler subsumes is a correct outcome).

Two things ARE owed and are recorded here rather than built:

1. **`grep -rn 'from_read' specs/` returns zero** is the countable proof of a real retirement, and
   belongs in the build item's Done-when. It returns **ten** matches at this record's HEAD
   (`specs/ordering/processmanager.yaml:178` · `specs/delivery/processmanager.yaml:56,57` ·
   `specs/payments/processmanager.yaml:144,147,153,172,175,181` · plus one doctrine-header line at
   `specs/common/processmanager.yaml:82` that documents the form). Command:
   `grep -rn 'from_read' specs/`.
2. **A locally-served `ask:` folds the answering aggregate's own stream and nothing else** — §7's
   fourth line, owed as a validator rule, not prose. Not built here.

## The directive (verbatim, founder / Tech CEO, 2026-08-31)

> "`read:` stays, exactly as PR #566 lands it with `source: PROJECTION | EVENT_STREAM` **<=== must
> be retired from the process manager**"

serving his own rule from earlier the same night (2026-08-15, the directive
ADR-20260815-030206 records):

> "The PM must not do the load too, it's the actor that will do that, it will build the state and
> give information to the PM if needed with get operations and the PM must call the actor to save
> the events or call the actor with a command."

> "The read is coming from the actor or the state of the PM, nothing else. We already have a rule
> for that."

## 1. The diagnosis — `source:` fixed the PHYSICS and left the OWNERSHIP

`source: PROJECTION | EVENT_STREAM` shipped in
[PR #566 "A process-manager read step declares its SOURCE, not only its shape (#564 PR1)"](https://github.com/TheCaptainCompany/captain-food/pull/566),
merged **2026-08-16** as `b0fd7fdf` (`git log -1 --format='%H %ci' b0fd7fdf` →
`b0fd7fdfb685837c5a14999ff5aef667f0779268 2026-08-16 01:29:50 +0000`). It answered a real question
— *where do the bytes physically come from* — and answered it well.

It did not answer the question the founder's rule actually asks. A `read:` step, in either source
mode, has the **process manager naming another aggregate's table and picking columns out of it**.
The fold is written on the PM's side of the boundary. That is forbidden whether the bytes arrive
from a projector-maintained row or from a stream fold: the PM is holding a model it does not own.
`EVENT_STREAM` moved the *storage*; it left the *ownership* exactly where it was.

**This is not the founder changing his mind.** Retirement is the **level-4 (unrepresentable state)**
form of the rule he stated on 2026-08-15 — the compiler-first floor of ADR-20260803-234035 — where
`source:` was the level-2 (declared-and-checked) form. A record that reads *"the founder reversed
himself"* misstates what happened: the rule is constant, and its enforcement moved down the ladder
from *declared* to *unspellable*. `read:` was the last place the wrong thing was still sayable.

## 2. PMW-3 (the transport) stays PARKED — and this record does not touch it

Retiring `read:` does **not** revive the transport question, and the record says so explicitly
because the two are easy to confuse: both are about "asking the actor".

- The founder personally removed the transport key
  ([PROP-20260815-142349](../proposals/PROP-20260815-142349-actor-answers-block-and-the-ask-step.md)
  §0 / D6 — *"the worry was vindicated"*), and wrote *"I don't think we should involve inbound
  messages table for queries to actors"*.
- **PMW-3 is OPEN and explicitly NOT adopted** (DECISIONS §42). Nothing here adopts it, leans toward
  it, or creates a dependency on it.

The mechanism question is settled **structurally**, not by choosing a transport (young):

> **The wall separates MODELS, not processes or address spaces.** `domain_events` is the *write*
> model's storage. A fold through the aggregate's own fold function **is** the write side. There is
> no third thing that needs a transport to be reached.

So there is no objection to a PM holding an `EventStore` port. The objection is to a PM holding an
`OrderReadRepository` — which is live today at
`crates/application/src/process_managers/payment_settlement.rs:54` and
`crates/application/src/process_managers/delivery_dispatch.rs:83` (used at `:106`).

The property this buys is **rebuild neutrality**, and it is a first-order correctness property, not
a performance note: **drop every projection, replay, and an `EVENT_STREAM` leg decides identically.
A `PROJECTION` leg does not.** Under a `PROJECTION` leg, *rebuilding a projection is a business
event* — it can change what the money path decides. That is the violation.

## 3. The headline — this is nine standing violations, not a keyword rename

Re-derived at this record's HEAD (`6b74739b`):

| Fact | Command | Result |
|---|---|---|
| `read:` steps | `grep -rn '^\s*- read:' specs/*/processmanager.yaml` | **15** — ordering 5 (`:30,41,63,71,163`), payments 8 (`:51,68,84,99,130,159,187,217`), delivery 2 (`:34,44`) |
| `source:` split | `grep -rn 'source: ' specs/*/processmanager.yaml` | **4 `EVENT_STREAM`** (`ordering:32,43,73` · `delivery:46`) / **11 `PROJECTION`** (`ordering:65,165` · `payments:53,70,86,101,132,161,189,219` · `delivery:36`) |
| Standing violations | 11 `PROJECTION` − 2 survivors (§4) | **9** |

Of the nine, **eight are on the money path** — the four settlement legs
(`specs/payments/processmanager.yaml:53,70,86,101`) and the four refund legs (`:132,161,189,219`),
every one of them reading `OrderTracking` — and the ninth is the dispatch birth leg
(`specs/delivery/processmanager.yaml:36`).

**ADR-20260815-030206 is therefore, today, a rule with nine standing violations.** The deliverable
this record authorises is **nine legs**, not a keyword. Sequencing it as a rename would understate
it by an order of magnitude and would put a "mechanical" label on eight money-path changes.

*(PMW-1's prose said "thirteen declared `read:` steps" and "three already stream folds" on
2026-08-15. Both are stale; the row is annotated with the re-derived 15 / 4 and their antecedents.)*

## 4. The two survivors are TWO CLASSES — and "exemption" is the wrong word for both

Two `PROJECTION` steps survive the rule. They are **not one class**, and the distinction is
load-bearing (evans):

### 4a. `specs/ordering/processmanager.yaml:163-169` — a session's open carts. A genuine carve-out.

The leg enumerates every OPEN cart of a session on `CustomerIdentified`. It is **set-shaped**, and
its key — `SessionId` — **belongs to no aggregate**: `grep -rn 'Session' specs/*/actors.yaml`
returns nothing. There is no actor to ask. This is exactly carve-out 2 of ADR-20260815-030206
("set-shaped / index-shaped reads have no actor to ask"), and it is lag-benign by construction: a
cart missed on this pass binds on the next identification, and the Cart's one-time bind absorbs the
duplicate.

### 4b. `specs/ordering/processmanager.yaml:63-68` — the live-catalog price authority. NOT a carve-out at all.

An addressable `Catalog` aggregate **does** exist (`specs/catalog/actors.yaml:8`, `type: aggregate` at `:9`,
`identity: $ref '#/Catalog/state/catalogId'`). So the carve-out reasoning does not apply and cannot
be borrowed.

The reason this step reads the projection is **positive**. The cart screen and the checkout leg go
through the **same `price_cart` seam** — `crates/server/src/graphql/cart_read.rs:14` imports
`application::pricing::{price_cart, CatalogSnapshot}` from `crates/application/src/pricing.rs`, and
that file's own header names it "the ONE `price_cart` authority". That coherence carries a **legal
display guarantee**: `specs/ordering/rules.yaml#/ServerPriceAuthority` (`specs/ordering/rules.yaml`,
the `LEGAL DISPLAY GUARANTEE` clause at `:61-65`) pins *Code de la consommation* L112-1 / L221-5 —
**the total displayed at the commit moment equals the total charged**. Folding the catalog stream on
the checkout leg would make display and charge disagree under projector lag, and surface
`PriceMismatch` *after* the customer has decided and is looking at the Stripe element.

In Evans's terms this is a **Published Language with a Shared Kernel implementation**: the strategic
pattern here is already the right one.

### 4c. Therefore: the word "exemption" is REJECTED, everywhere.

It is **false** for 4b — there is nothing to be exempted from; the shared read is the correct
design, not a tolerated one. And it is **dangerous**: "exemption" tells the next reader that the
ideal is to ask the aggregate and this is a lapse awaiting cleanup. That is backwards. "Cleaning it
up" would charge a price the customer never saw, on the money path, at peak.

No record, row, grammar key or comment arising from this decision may call either survivor an
exemption.

## 5. The recorded divergence — how the survivors are spelled (this is the OPEN half)

Three lenses agreed the survivors need a spelling and **disagreed on how many kinds**. The
divergence is recorded rather than blended, because the two positions have different costs and the
row (**PMW-4**) is what carries the choice.

- **vernon** — **ONE** differently-named step kind, carrying a **mandatory exemption `$ref`**. Cost:
  one fewer node kind in the walker/emitter/validator surface. Cost against: it reintroduces the
  noun §4c rejects, and it makes 4a and 4b the same thing in the grammar when they are not.
- **evans** (**recommended**) — **TWO** kinds, each `$ref`ing a *different domain artifact*, and no
  "exemption" noun anywhere:
  - `index:` with `by:` → the **unowned key scalar** (the 4a shape: `SessionId`, a key no aggregate
    owns);
  - `authority:` → the **rule declaring who is authoritative** (the 4b shape:
    `rules.yaml#/ServerPriceAuthority`).

  Each kind then *names the reason it exists*, and the reason is a `$ref` the refs walker can see.
- **young** sides with **distinct narrow kinds**, and supplies the decisive argument:

  > *"Retirement is the level-4 form only if there is no residual escape hatch; two carve-outs
  > riding a surviving `read:`, or a generic exemption `$ref`, is `source:` again wearing a new
  > name."*

**Decided here**: the retirement, and that a *generic* escape hatch is not acceptable. **Open in
PMW-4**: one kind or two, with evans's two-kind shape recommended and vernon's single kind recorded
as the considered dissent with its stated cost.

## 6. What the retirement DOES and does NOT close

**It does NOT close the #544 silent-expiry class, and this record does not claim it does.** Folding
Payment's stream before the Stripe webhook lands still observes *not-AUTHORIZED*, and `skip: true`
still says nothing. What closes the ~7-day silent authorization expiry is the **exhaustive branch**
— PROP-20260815-142349 §9 over the six members of `specs/*/scalars.yaml#/PaymentStatus`, plus
`absent: RETRY`, the `deadline:`, and the `SettlementOverdue` reminder (§11). That work is unmoved
by this decision.

**What the fold DOES buy** is undersold by the existing note on
`specs/payments/processmanager.yaml:56`, and it is worth stating precisely:

> Under `PROJECTION`, *"not yet projected"* and *"not authorized"* are **the same observation** —
> the code cannot tell them apart. **An ambiguous absence becomes an authoritative absence.**

That is the real win. It does not by itself make the leg act correctly; it makes the leg's input
*mean* something, which is the precondition for the exhaustive branch to be a decision rather than a
guess.

## 7. The discipline — four lines that belong together

Separately each of these reads as optional. Together they are the doctrine of a reply, and they are
recorded as one block for that reason.

1. **A fold gives freshness, not atomicity** (PROP §6). Reading fresher does not make the read and
   the subsequent decision one transaction.
2. **The deadline lives in exactly one place — the caller's `ask:` step** (PROP D5, founder:
   *"The caller timeout is decided on the client side"*).
3. **The reply's authority expires at send** (PROP §6). A reply is a snapshot, not a lease.
4. **Ask for values that are write-once or monotone** (young). *A mutable foreign field read into a
   guard is the thing to eliminate, not the thing to make fresher.* After PROP §10 the settlement
   decision needs only `paymentIntentId` from Order — **write-once, hence immune to staleness by
   construction**. Lines 1–3 manage staleness; line 4 removes the need to.

## 8. One condition, owed as a validator rule and NOT built here

A locally-served `ask:` must fold **the answering aggregate's own stream and nothing else** — every
reply property resolving to that actor's declared `state:`.

A "local ask" that folds two streams, or joins a projection, is **an undeclared projection living on
the write side with none of a projection's licences**: no disposability, no projector, no
checkpoint, no rebuild story. It would recreate the defect this record retires, inside the mechanism
that replaces it.

Recorded as owed. Not built here (this record is docs-only).

## 9. CLAUDE.md question (2) — NO migration is owed; and it is still `HOLD: human`

**Answered honestly, because borrowing migration language a change does not need is its own defect.**

Is the shape already **emitted, stored or promised**? No:

- `read:` steps emit **hook trait signatures and call sites** — `pm_read_infos` at
  `tools/codegen-rs/src/emit/pm_orchestrators.rs:710`, consumed into `PmEmit { … reads: … }` at
  `:2112` — **not data**.
- PM **state rows** come from a different emitter entirely (`tools/codegen-rs/src/emit/pm_state.rs`
  over `specs/database/tables/process_managers.yaml`), and **no `read:` step ever writes a state
  column**.
- **No `read:` appears in any event payload.** Nothing in `domain_events` carries this shape.
- **`source:` is consumed by NO emitter.** `grep -rn 'READ_SOURCE\|EVENT_STREAM'
  tools/codegen-rs/src/emit/` returns zero. (The three `"source"` hits under `emit/` — `sql.rs:245`,
  `docs.rs:575`, `docs.rs:1224` — are a different `source` key: a node's `reference` marker and
  `run_identity`'s field origin.) **Consequence: the retirement deletes zero generated query code.**

So: **no migration, no upcasting, no stored-shape story.**

**But it IS a behaviour change on the money path**: a leg that silently `skip`ped now retries and
alerts. That places the *build* in `HOLD: human` (stored-event/fold semantics + payments) and under
gate-then-stabilize — **for that reason, and never by borrowing migration language it does not
need.**

**Fence (young)**: if an ask reply is ever **persisted** — cached, or journaled into
`inbound_messages` — it stops being a conversation and the whole doctrine above changes. PROP §6's
line is kept verbatim:

> **"`inbound_messages` is a delivery guarantee, not evidence."**

## 10. Derived consumers that move — recorded so nobody is surprised

None of these is edited by this record. They move when the grammar change lands, and each is a lie
the moment it does:

- **`specs/database/databases.yaml`** carries a prose caveat in **two** database descriptions —
  `read_order` (the sentence beginning at `:92`, *"so #513's grant emitter must derive them
  mechanically rather than from this paragraph — but NOT VERBATIM"*, through its `#564 PR2`
  sequencing clause) and `read_common` (the clause ending at `:139`, *"#513 must NOT emit CONNECT
  here from them"*). Both are **entirely about `read:` steps declaring SHAPE and not SOURCE**. With
  `read:` retired the caveat has no subject: it must be **deleted**, not amended. It becomes false
  the moment the grammar change lands, and `make validate`'s own messages quote this prose.
- **Hand-written seams**: `crates/application/src/process_managers/payment_settlement.rs:54` (the
  `SettlementHooks.orders: &dyn OrderReadRepository` field) and
  `crates/application/src/process_managers/delivery_dispatch.rs:83` (same field on
  `DispatchOpenHooks`), used at `delivery_dispatch.rs:106`
  (`self.orders.by_id(order_id, &ReadScope::System)`).
- **`tools/codegen-rs/src/emit/behaviour_tests.rs:362`** — the `("DeliveryDispatchProcess",
  "OrderMarkedReady")` dispatch entry, whose call signature threads `&bed.orders`.

## 11. What is decided, and what is not

**Decided.** `read:` is retired from the process-manager step DSL. A generic escape hatch (a
surviving `read:` with carve-outs, or a generic exemption `$ref`) is not acceptable. "Exemption" is
rejected as the noun for either survivor. No migration is owed; the build is `HOLD: human`.

**Not decided — PMW-4.** How the two survivors are spelled: evans's two kinds (`index:` with `by:`
→ the unowned key scalar; `authority:` → the authoritative rule) **recommended**, vernon's single
differently-named kind recorded as the considered dissent.

**Untouched.** PMW-3 (the transport) stays parked and not adopted. PMW-2 (residency) is not ridden.
The #544 exhaustive-branch work is unmoved.

**Not in this change.** No `specs/**` edit — therefore no `docs/SPEC-LOG.md` sentence and no
`make warning-baseline`. The grammar change is separate, sequenced work.

## Consulted (ADR-20260812-143619 — one line per lens)

Full-mob briefing, 2026-08-31; four lenses carried content and are composed above.

- **vernon** — the diagnosis of §1, endorsed by all four: `source:` fixed the physics and left the
  ownership of the fold with the PM, so retirement is the level-4 form of the founder's own rule,
  not a change of rule. On §5 he argued for **ONE** differently-named kind carrying a mandatory
  exemption `$ref` — **recorded as the dissent**, with its cost (one fewer node kind) and the reason
  it was not adopted (§4c's noun, and 4a/4b collapsing into one grammar shape).
- **evans** — the §4 correction, which is load-bearing: the two survivors are **two classes**, not
  one. 4a is a genuine carve-out (`SessionId` belongs to no aggregate); 4b is **not a carve-out at
  all** but a Published Language with a Shared Kernel implementation, defended by a legal display
  guarantee. Hence §4c's rejection of "exemption" as both false and dangerous, and §5's two-kind
  shape (`index:`/`by:` → the unowned key scalar; `authority:` → the authoritative rule), plus the
  countable Done-when (`grep from_read specs/` → zero).
- **young** — the model-not-process argument that settles §2 structurally (`domain_events` is the
  write model's storage; a fold through the aggregate's own fold function IS the write side, so
  there is no third thing needing a transport); **rebuild neutrality** as the first-order property;
  §3's framing that the current wording understates the work (nine standing violations, eight on the
  money path); §6's refusal to credit the retirement with closing the #544 class **and** the
  ambiguous-absence-becomes-authoritative-absence win it does buy; §7 line 4 (ask for write-once or
  monotone values); §8's condition as a validator rule; and §9's `inbound_messages` fence.
- **architect** — ranked the ADR-20260815-030206 correction first (its stale sentences produced a
  false negative in a register check on 2026-08-31); the register wiring (PMW-1 migration out of the
  legacy allowlist, PMW-4's `reconsiders:` edge, PMW-3 stated as untouched); §10's derived-consumer
  inventory; and the antecedent discipline (ADR-20260817-105845) applied to every number above.

**Explicitly not consulted**: `design`/`ux` (no user-facing surface moves in this record), `legal`
(§4b *cites* an existing recorded legal posture and creates none — and no lens output is legal
clearance, ADR-20260812-143619).

**Divergence recorded, not blended**: §5 is the only place the four lenses disagreed, and it is the
half this record leaves OPEN rather than resolving in prose.
