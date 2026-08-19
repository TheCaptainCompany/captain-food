# BRIEF-20260811 — Obligation brief: the erasure-free zone, and the retention schedule that does not exist

- **Date**: 2026-08-11
- **Author**: legal-specialist lens (internal obligation map — **NOT legal advice**; grades
  (a) established obligation · (b) interpretation, counsel confirms · (c) unknown; all (b)/(c)
  and every cite's currency to be confirmed by licensed French counsel)
- **Session**: https://claude.ai/code/session_015qt428xktQNVNFpaxMNfmh
- **Occasion**: the legal-lens pass over
  [PR #488 "The open GraphQL path verifies credentials, and `current` is tenant-scoped by Host"](https://github.com/TheCaptainCompany/captain-food/pull/488)
  —
  [#469 "`current` leg 1 is dead on the web AND is not tenant-scoped — fix both together or neither"](https://github.com/TheCaptainCompany/captain-food/issues/469)
- **Subjects**:
  [#194 "GDPR erasure"](https://github.com/TheCaptainCompany/captain-food/issues/194) ·
  [#401 "Legal exposures from the opt-out obligation brief"](https://github.com/TheCaptainCompany/captain-food/issues/401) ·
  [#404 "Decision thread: GDPR erasure depth"](https://github.com/TheCaptainCompany/captain-food/issues/404)
- **Companion briefs**:
  [BRIEF-20260808-listing-opt-out-objections.md](BRIEF-20260808-listing-opt-out-objections.md)
  (Q/E/F counsel packet — **G1–G8 below are appended to it**) ·
  [BRIEF-20260808-account-erasure-two-path.md](BRIEF-20260808-account-erasure-two-path.md)

*This brief prepares the work of licensed counsel and clears nothing. Nothing in it is legal
advice, and no launch decision may rest on an item graded (b) or (c) until a French avocat has
confirmed it.*

---

## The finding in one line

`Cart-*`, `Customer-*` and `Restaurant-*` have **never** been an erasure-free zone — they were
personal-data streams from the day they were designed — and the honest statement of what
[#469](https://github.com/TheCaptainCompany/captain-food/issues/469) changed is narrower and
different in kind: **an external identity-provider identifier now lands in the immutable write
envelope of three stream categories that have no erasure path at all**. Because the production
event log is empty by decision, that is an unmet launch precondition, not a breach — but two
forward-looking traps now sit in front of the [#194](https://github.com/TheCaptainCompany/captain-food/issues/194)
work, and one of them would destroy a legal register if it is built the obvious way.

---

## 1. The framing correction — the streams were ALREADY personal data — grade (a)

**The record said the wrong thing.** The wording carried on the #469 work is that these streams
*"were an erasure-free zone and are now subject-attributable"*. The first half is right; the second
half is wrong, and the error is not cosmetic — "became personal data" invites the reading that
erasure obligations *attach from now on*, which would waive Art. 5(1)(e) storage limitation,
Art. 13/14 transparency and Art. 30 records for everything already designed.

The evidence, all of it in the specs and predating #469:

| Stream | Why it was already personal data | Cite |
|---|---|---|
| `Cart-*` | `CartStarted` **requires** `sessionId` — it is not optional and there is no cart without one | `specs/ordering/events.yaml:33-51` |
| `Cart-*` | The `SessionId` scalar is documented as *"used to bind carts and **track the user across devices**"* — the controller's own description of a linking purpose | `specs/common/scalars.yaml:13-16` |
| `Cart-*` | `CartBoundToCustomer` writes the domain `customerId` onto the same stream; `CartBindingProcess` is a **designed, operated** mechanism for making the link | `specs/ordering/events.yaml:8-21` |
| `Customer-*` | `CustomerRegistered` **requires** `phone` (E.164). A phone number is a direct identifier under any reading | `specs/customer/events.yaml:15-46` |
| `Restaurant-*` | The SIRENE population contains *entrepreneurs individuels* — natural persons — already established at [BRIEF-20260808 Q1](BRIEF-20260808-listing-opt-out-objections.md) on CJEU C-398/15 *Manni* | — |

**The law**: Art. 4(1) (identifiable = identifiable *by the controller or another person*, directly
or indirectly, in particular by an **online identifier**); Recital 30, which names cookies and
device identifiers as exactly this class; Art. 4(5), under which pseudonymised data **remains**
personal data; and CJEU C-582/14 *Breyer*, where an identifier is personal to a party holding
**legal means reasonably likely to be used** to make the link. Captain does not merely hold means —
it operates the linking mechanism as a named process manager. `Customer-*` never needed the
pseudonymity argument at all: it holds a phone number. **Grade (a).**

**What #469 genuinely creates** — narrower, and a different kind of thing: seven open-path commands
now stamp `domain_events.user_id` with the **Supabase `sub`** (`AuthContext::public_customer`
writes `user_id: Some(sub)`, `crates/server/src/auth.rs:112-116` on the `469-*` branch), putting an
**external identity-provider identifier** into the append-only write envelope of three stream
categories where none existed. Two properties make it worth naming separately from "these streams
hold personal data":

1. It is an identifier **we do not mint and do not control the lifecycle of** — it is the
   processor's primary key for the data subject.
2. It **survives deletion of the Supabase identity**. Deleting the account at the provider (the
   Art. 28 processor instruction the two-path model relies on) leaves the `sub` behind in an
   immutable log, orphaned. Whether that orphan is anonymous or still personal data is **G4**, and
   it is the question that decides whether crypto-shredding is optional or mandatory.

## 2. Why this is not an incident — grade (a) on the facts, (b) on the conclusion

The production event log is **empty by decision**: ADR-20260807-002705 D6 chose start-clean —
*"the window is open only while the log is empty"* (`docs/status/journal-2026-W33.md`, entry 2026-08-10). With no data subject in
`domain_events`, the elements of an Art. 17 infringement have no subject to attach to: there is no
request that could have been refused and no personal data being retained beyond its purpose. What
exists is an **unmet launch precondition**, and it is already filed — as
[#194](https://github.com/TheCaptainCompany/captain-food/issues/194), which has never claimed to be
finished.

So the correct handling is **not** an incident filing, and not a new obligation class. What changed
is **#194's size**, and the change is boundable:

- **three stream categories** — `Cart-*`, `Customer-*`, `Restaurant-*`;
- **one identifier kind** — the Supabase `sub` in `domain_events.user_id`;
- **no new obligation class** — Art. 17 already applied to all three.

Two conditions bound this conclusion, and neither is rhetorical:

- **The trigger moment is real and dated.** The precondition becomes an obligation at the **first
  real customer order** — the same moment Art. 35 fixes for a DPIA (*"prior to the processing"*)
  and the same moment the product owner already chose for *médiation de la consommation*
  registration. One date, three duties. **(b)** on whether counsel places the DPIA earlier
  (arguably at first real *prospect* processing, which SIRENE already reached — see §5).
- **The empty-log argument collapses if any environment holds real subject data.** This is a
  question of fact for the team, not for counsel, and §5 reports what could and could not be
  established.

## 3. The `Restaurant-*` trap — an `Order`-shaped deletion policy would destroy a legal register

**This is the dangerous one, and it is dangerous precisely because the obvious fix is wrong.**

`RestaurantListingOptedOut` (`specs/network/events.yaml:344-356`) **is** the Art. 21 objection
register. [BRIEF-20260808 Q4](BRIEF-20260808-listing-opt-out-objections.md) states the requirement
in terms that leave no room: the historical event must be retained because *"it is the register,
not stale data"*, and Q1 explains why — the *liste repoussoir* doctrine makes retaining the minimal
suppression identifier **the lawful act**, and deleting it the violation, because re-import
re-contacts the person who objected.

The trap: `Restaurant-*` will arrive at the #194 sweep as one of the three categories with no
erasure path, next to `Cart-*` and `Customer-*`, and the one built erasure mechanism is
`Order`-shaped — a `deletion:` block whose journey is *tombstone → delete the whole stream → ledger
receipt* (`specs/ordering/actors.yaml:97-103`). Giving `Restaurant-*` the same block would delete
the objection event **with** the stream. The consequence is not data loss, it is
**re-listability**: the ProspectionPipeline would find nothing recording the refusal, and the exact
failure the 0808 brief exists to prevent happens on schedule. Under Art. 5(2) the controller would
then be unable to evidence that an objection was ever honoured — and the log that would normally
prove continuity has been erased by our own engine.

**Can this be made unspellable?** Assessed against the spec model, and the honest answer is **not
today**. The deletion DSL is well-formed and validated (`tools/codegen-rs/src/validate/reminders.rs`,
rules `deletion-ref-unresolved` / `deletion-match-untyped` / `deletion-tree-cycle`), so the *shape*
of the rule is easy — *an actor that authors a legally-retained register event may not declare a
`deletion:` block that deletes its stream*. What is missing is the **left-hand side**: the spec has
no way to say "this event is a legal register". No marker exists on `events.yaml` entries, and the
only alternative is hard-coding `RestaurantListingOptedOut` in the validator — a comment written in
Rust, not a spec-derived gate, and the kind of thing CLAUDE.md's compiler-first hierarchy ranks at
the bottom. **The gate is one small spec addition away** and that addition belongs to
[#194](https://github.com/TheCaptainCompany/captain-food/issues/194):

> Declare the retention obligation **on the event**, not in prose — e.g. a `legalRetention:`
> clause naming the instrument (`Art. 21 objection register`) and the horizon (`indefinite`),
> `$ref`-able from the approved retention-window catalog that
> [DECISIONS MET-W](../proposals/DECISIONS.md) already approved. The validator rule then writes
> itself and is spec-keyed: **an actor whose `emits` reaches an event carrying `legalRetention`
> may not declare a stream-deleting `deletion:` block**, and separately, **every `legalRetention`
> event must name a window from the catalog**. Both are errors, both are unspellable-by-construction
> rather than reviewed-by-eye.

Until that lands, the hazard is prose — here, and one line in `docs/STATUS.md`. That is exactly the
weaker form CLAUDE.md warns about, and it is recorded as such.

**Triage: BLOCKER-on-arrival.** Nothing is broken today (`Restaurant` declares no `deletion:` block
— `specs/network/actors.yaml:33`). It becomes a breach the moment the #194 sweep gives it one.

## 4. The retention control is asserted and inert — and the document is worse than the silence

Three facts, each independently checkable:

1. **The spec asserts the control exists.** `specs/database/tables/eventstore.yaml:38-39`:
   *"Only ephemeral streams (e.g. Cart) get a row; Order/Restaurant/Customer keep full history."*
   Read as written, `Cart-*` is bounded by a per-stream retention policy.
2. **No stream has one.** `domain_stream` has **zero production writers**. The only `INSERT` in the
   tree is a test fixture (`crates/infrastructure/tests/main/deletion_engine.rs:99`), placed there
   to prove the deletion journey removes the config row with the stream. Every other reference is a
   `DELETE` (`crates/infrastructure/src/deletion.rs:397`), a comment, or a validator note. Nothing
   ever creates a retention row, so `$maxAge`/`$maxCount` bind nothing, and abandoned guest carts
   accumulate forever.
3. **A brief of ours claims the schedule already exists.**
   [BRIEF-20260808-account-erasure-two-path.md:82](BRIEF-20260808-account-erasure-two-path.md)
   said *"This table IS the written retention schedule CNIL expects — windows declared once, in the
   DSL, feeding both the sweep and the DPIA."* [DECISIONS MET-W](../proposals/DECISIONS.md) already
   recorded that claim as false — no duration scalar exists, and the windows are in a markdown
   table, not in the DSL. **That sentence is corrected in place in the same change as this brief.**

**Why this ordering matters legally, grade (a) on the principle.** Art. 5(2) is an *accountability*
duty: the controller must be able to **demonstrate** compliance. A controller-authored document
asserting a retention schedule that its own system does not implement is worse evidence than having
written nothing — silence is an unmet obligation, a contradicted document is a statement the
regulator can hold you to and then falsify from your own repository in the same audit. The same
pattern applies to `eventstore.yaml:38-39`, which is not merely stale: it is the DPIA's future
source for "how long do carts live", and it currently answers with a control that does not run.

**The fix has already been decided and only needs sequencing**: MET-W approved a **named catalog of
approved retention windows**, `$ref`'d rather than a `Duration` scalar with a pattern (*"a pattern
catches `P90DD` but not a well-formed window nobody approved"*), sequenced **with**
[#194](https://github.com/TheCaptainCompany/captain-food/issues/194). Until it lands, both the
brief and the spec must say what is true: **the windows are proposed, not implemented.**

**Triage: EXPOSURE now, BLOCKER at first real cart.** An unbounded `Cart-*` population of
session-identified guest carts is a straight Art. 5(1)(e) storage-limitation failure with no
declared window to defend it.

## 5. The open question of fact — does any non-production environment hold real subject data?

**This is for the team, not for counsel**, and the §2 argument collapses without it. What the
repository can and cannot establish:

**Established — no declared non-production environment holds subject data:**

- **No staging or preview environment is declared anywhere it could hold data.** `render.yaml`
  declares no staging service; `deploy/generated/manifests/` targets one topology; and the
  `staging` value of `APP_PROFILE` is a profile the config model *supports*
  (`specs/common/configuration.yaml:53`) with no service bound to it. The only other hits for
  "staging" in the tree are the SIRENE **staging table**, an unrelated internal term.
- **CI's database is ephemeral and synthetic.** `.github/workflows/ci.yml:260,280` runs a
  `postgres` service container at `postgres://postgres:postgres@localhost:5432/postgres`, created
  and destroyed per job, populated only by `migrations/` plus test fixtures.
- **The 2026-08-11 cutover rehearsal held nothing.** CNPG `captain-db` was created by `initdb` and
  the 45-migration chain applied to an **empty** database; smoke levels L3/L4 (the auth and money
  legs) never ran for want of `SUPABASE_SECRET_KEY`. No customer path executed.
- **No local data fixture ships personal data.** There is no `docker-compose`, no `.env`, and the
  single `*seed*` artifact in the tree is `migrations/20260717180000_seed_referential_policies.sql`
  — referential policy rows, no subjects.
- **No customer has ever transacted in production.** `docs/status/journal-2026-W32.md` (entry 2026-08-09) records that the smoke
  customer has no domain `Customer` at all (`verifyPhone` needs real SMS), so its own order read is
  refused by design.

**NOT established, and it is the part that matters:**

- **The `DATABASE_URL` repo secret is opaque, and something behind it has held real personal data.**
  `.github/workflows/sirene-sync.yml:102,134` writes **real INSEE rows** into
  `external_sirene_restaurants` using that secret, and `.github/workflows/db-migrate.yml:29`
  documents the same secret as *"the Supabase Session-pooler string"*. The SIRENE population
  contains *entrepreneurs individuels* — personal data per *Manni*. Which database that secret
  points at today, and whether it still exists, **cannot be answered from the repository**.
- **This was not hypothetical.** `docs/status/journal-2026-W33.md` (entry 2026-08-11) records ~200k SIRENE-derived restaurant
  listings, ~200k `domain_events` tuples per sweep in the then-live database, and 6,649 staging rows
  actually present. *(The original cited `docs/STATUS.md:2262,2276` and `:2230`/`:2014`; those line
  numbers were already stale before the 2026-08-19 journal move and are not reconstructible — the
  three figures are quoted from the entry itself, which is where they can be re-read.)* A `domain_events` containing real
  `Restaurant-*` streams **demonstrably existed** before the cutover decision. Start-clean
  (ADR-20260807-002705 D6) governs the **new** cluster; the disposition of the **old** store is an
  operational fact nobody has recorded.
- **Whether any Supabase Auth project holds real end-user identities** (phone numbers from a real
  `verifyPhone`) is not repository-answerable. No Supabase console access was used for this brief.

**Consequence, stated precisely.** The empty-log argument in §2 holds **for the streams #469
actually touches** — `Cart-*`, `Customer-*` and `Order-*` — because no customer path has ever run
against a production store. It does **not** hold unqualifiedly for `Restaurant-*`. Two answers are
owed by the team before §2 can be relied on in a DPIA:

1. Does the database behind the `DATABASE_URL` secret still exist, and does it contain
   `external_sirene_restaurants` rows and/or `Restaurant-*` streams today?
2. Does any Supabase Auth project — production or otherwise — hold real end-user identities?

Until both are answered "no" **in writing**, §2 is a conclusion about three stream categories, not
about the estate.

---

## Triage

- **BLOCKER-on-arrival** — `Restaurant-*` must never receive an `Order`-shaped whole-stream
  deletion policy (§3). The spec marker + validator rule is the durable form; the prose here and
  in `docs/STATUS.md` is the interim.
- **BLOCKER at first real cart** — no retention window binds `Cart-*` (§4); the control is asserted
  and inert.
- **BLOCKER at first real order** — the Art. 17 path for `Cart-*`/`Customer-*` does not exist
  ([#194](https://github.com/TheCaptainCompany/captain-food/issues/194)); same deadline as the DPIA
  and the mediator registration (§2).
- **EXPOSURE** — the orphaned Supabase `sub` after processor-side identity deletion (G4); the
  unconstrained `dietaryTags` field (G7); no Art. 18 restriction mechanism (G8).
- **OPEN QUESTION OF FACT, team-owned** — §5. Not a counsel question and not deferrable, because
  every conclusion above is conditioned on it.
- **HYGIENE** — the "became subject-attributable" framing, corrected here (§1); the false retention
  claim, corrected in place (§4).

## Counsel packet G1–G8

The eight questions are appended to the consolidated packet in
[BRIEF-20260808-listing-opt-out-objections.md](BRIEF-20260808-listing-opt-out-objections.md),
after the Q/E/F series. The full context an avocat needs for each is in the correspondingly
numbered discussion below.

### G1 — Empty-log reliance, and the trigger moment

*Context.* Production starts clean by a recorded decision (ADR-20260807-002705 D6); there is no
data subject in `domain_events` and no Art. 17 execution path for `Cart-*`, `Customer-*` or
`Restaurant-*`. *Question.* With no data subject present, is the absent erasure path an **unmet
precondition** rather than an infringement — i.e. correctly handled as an open engineering item
(#194) and not as a personal-data breach requiring an Art. 33/34 assessment? And is the trigger
moment the **first real customer order**, the same deadline Art. 35 fixes for the DPIA (*"prior to
the processing"*) and the same one already chosen for *médiation de la consommation* registration —
or does counsel place the DPIA earlier, at first real **prospect** processing, which the SIRENE
pipeline has already reached? *Our reading:* precondition, trigger at first real order.
**Grade (b).**

### G2 — Whole-stream deletion as the Art. 17 mechanism, and the pseudonymous receipt

*Context.* The built erasure journey is *tombstone → delete the entire event stream → record a
receipt on a separate deletion ledger*. The receipt (`OrderDeleted`,
`specs/ordering/events.yaml:536-558`) carries **pseudonymous domain references only** — never the
erased payloads — because "whose order was erased" must stay answerable under Art. 5(2).
*Question.* (i) Does deleting the whole stream satisfy Art. 17(1) for an event-sourced store, or
must we additionally evidence that every derived read model and backup has folded the erasure?
(ii) Is the pseudonymous receipt a **proportionate accountability artifact** under Art. 5(2), or is
it itself excessive retention under Art. 5(1)(c) — and if proportionate, does the receipt need its
own declared retention window, or is indefinite retention correct for an accountability record?
**Grade (b).** (Overlaps E7; G2 asks it of the built mechanism rather than the design.)

### G3 — Accounting retention vs Art. 17: which closure is more defensible — **BLOCKING**

*Context, and why this blocks.* The one built erasure path deletes the **whole stream** on a
**single configurable window with no per-category split**. The spec says so about itself
(`specs/ordering/configuration.yaml:10-21`): the window is set to 3650 days — the conservative
accounting horizon — explicitly *"because the per-data-category split (personal vs financial
retention, a legal/product input) is still open — shortening it below the accounting horizon before
that split lands would delete financial facts French commercial law retains."* So today the
mechanism can hold the personal data for ten years or delete the financial facts, and it cannot do
both. *The instruments.* Code de commerce **L123-22** — livres et pièces justificatives, 10 years
from close of financial year; LPF **L102 B** — 6 years; against Art. 17 and Art. 5(1)(e), under
which ten years of a customer's delivery address and phone number is not a defensible personal-data
window. *Question.* Which of these two closures does counsel consider more defensible under audit,
and is either **required** rather than merely permitted?

- **(A) Keep the 10-year window on the `Order` stream and tombstone the personal fields through
  projections** — the financial facts stay where they are; personal payloads become unreadable at
  the read models. Concern: the raw payloads remain in an append-only log, so "erased" is a
  projection-level statement, not a storage-level one.
- **(B) Export a financial skeleton before deleting the stream** — a bookkeeping record carrying
  only what L123-22 requires (amounts, VAT, dates, invoice references, no personal identifiers),
  then delete the stream on a **personal-data** window measured in months, not years. Concern: a
  second store, a second retention artifact, and the export becomes a compliance-critical path.

ADR-20260731-160000 left exactly this open and the account-deletion work forces it. *Our reading:*
(B) is the stronger Art. 17 posture and the weaker operational one; we need counsel's view before
building either. **Grade (a)** that one must be chosen, **(b)** which. (Refines E4.)

### G4 — The orphaned `sub`: anonymous under Recital 26, or still personal data?

*Context.* Identity lives at Supabase, a processor. The Art. 17 path deletes the identity there as
an **Art. 28 processor instruction**. But `domain_events.user_id` — an **append-only** column —
retains the Supabase `sub` for every event that principal caused
(`crates/server/src/auth.rs:112-116`), and after the provider-side deletion that `sub` refers to an
account that no longer exists anywhere. *Question.* Is the orphaned `sub` **anonymous** under
Recital 26 — no means reasonably likely to be used remain, because the only re-identification key
was destroyed at the processor — or is it **still personal data**, on the basis that the events it
keys are behavioural records of one identifiable individual and re-identification could proceed
through them (a phone number in `CustomerRegistered`, an address in an order)? *Why it decides an
architecture.* If it is still personal data, then in an append-only store the row cannot be updated
and **crypto-shredding of the identifier columns — destroying the key that decrypts `user_id` — is
the only mechanism compatible with immutability**. We ask counsel to say this directly rather than
by implication, because the alternative designs (rewriting history, or deleting streams we are
required to keep for §G3) are each unacceptable for an independent reason. **Grade (b)/(c).**

### G5 — Art. 21 register vs Art. 17 on `Restaurant-*`, and the minimum field set

*Context.* `RestaurantListingOptedOut` (`specs/network/events.yaml:344-356`) is our objection
register; [BRIEF-20260808 Q1/Q4](BRIEF-20260808-listing-opt-out-objections.md) established that the
historical event must be **retained** — it is the register, not stale data — and that deleting it
would permit re-listing and re-contact. But an *entrepreneur individuel* who objected may also
issue an **Art. 17 erasure request** over the same `Restaurant-*` stream, which holds their name,
address and SIRET. *Question.* (i) Confirm that Art. 17(3)(b)/(e) — or the Art. 21 doctrine itself —
permits us to **refuse erasure of the suppression entry** while erasing everything else on the
stream, and that this is not merely permitted but **required**, since deleting it would foreseeably
cause renewed unlawful contact. (ii) State the **minimum field set** for a compliant suppression
entry keyed on **SIREN/SIRET** — is `{SIREN/SIRET, the fact of refusal, timestamp, channel}`
sufficient and is anything in it excessive? (iii) Must the entry be keyed on the **state
identifier** rather than our internal id (so a re-import matches), or is that prudent rather than
required? *Why the field set is urgent:* it determines what a `Restaurant-*` erasure must leave
behind, and therefore whether the deletion mechanism can be stream-shaped at all. **Grade (a)** on
the doctrine, **(b)** on the fields. (Sharpens Q3 against the built engine.)

### G6 — The per-category retention schedule, validated against the CNIL référentiel

*Context.* We have no implemented retention windows (§4). We must declare a schedule and we would
rather declare one counsel has validated than discover it in an audit. The benchmark we intend to
work from is the **CNIL référentiel « gestion commerciale »** (délibération **2021-044**).
*Question.* Validate, category by category, and name the ones where our reading is wrong:

| Category | Our proposed window | Note |
|---|---|---|
| **Abandoned guest carts** (session-identified, never ordered) | short — weeks, not years | Today unbounded and the control is inert; the only personal datum is the session id + basket contents |
| **Identified customer carts** (bound to a `Customer`) | duration of the account relationship | Is a shorter dedicated window required, given a cart is not a contract? |
| **Dormant customer accounts** | ~3 years from last activity, **notify-then-delete** | The referential benchmark; confirm the number and that prior notice is required rather than courteous |
| **Prospect restaurant records not obtained from the subject** (SIRENE) | ~3 years from last contact | Plus the **Art. 14 notice timing** question: data not obtained from the subject — notice within one month, or at latest at first communication? Our pipeline's first communication *is* the first contact |
| **Order streams** | per G3 | The personal/financial split |
| **Suppression entries** | indefinite | Per G5 |

We also ask whether the *référentiel* binds us at all or is a benchmark we depart from with a
recorded justification. **Grade (a)** that a written schedule is required, **(b)** on every number.
(Extends E1/E4 and Q5's retention point to the categories we now know are unbounded.)

### G7 — `dietaryTags` and Art. 9 — the DPIA cannot close while this field is open

*Context.* `SetCustomerPreferences` / `CustomerPreferencesSet` carries
`dietaryTags: array<Tag>` where `Tag` is a **free-form string, `maxLength: 80`, no enum**
(`specs/customer/events.yaml:148-151`, `specs/common/scalars.yaml:145-148`), persisted to
`View_Customer.preferences` as jsonb. The values `halal`, `kosher` and `allergy:peanut` are
**spellable today** — no validation would refuse them. The first two reveal **religious belief**;
the third reveals **health data**. Both are Art. 9(1) special categories. No screen currently binds
the field, which is why no review caught it — but the write path exists and the event is in the
catalog. *Question.* (i) Confirm the Art. 9 exposure attaches to the **field's capability**, not
only to values actually stored — i.e. is a free-text preference field a special-category processing
risk in itself under Art. 25 (data protection by design)? (ii) Of the two remedies, which does
counsel require: an **enum that excludes** religious and health values (dietary preference reduced
to a closed, non-revealing vocabulary), or a full **Art. 9(2)(a) explicit-consent** flow with the
separate consent artifact, notice and withdrawal path that implies? (iii) Confirm — because it
governs our sequencing — that the **DPIA cannot be finalised while the field is unconstrained**,
since a DPIA must describe the categories of data processed and this field's category is
indeterminate by construction. *Our reading:* the enum, before any screen binds the field; the
consent flow is disproportionate for a discovery preference. **Grade (b)**, exposure-level.

### G8 — Art. 18 restriction of processing, distinct from erasure

*Context.* Art. 18 is a **separate right** from Art. 17: on a contested accuracy claim, an unlawful
-processing claim, or a pending Art. 21 balancing test, the subject may require that data be
**stored but not otherwise processed**. We have no mechanism for it at all — the design conversation
has been entirely about erasure, and an event-sourced store has no natural "hold" state: the log is
append-only and every projection folds unconditionally. *Question.* For an event-sourced,
projection-based system, is a **marker event that all projections and folds honour** — a
`ProcessingRestricted` fact on the stream, after which every read model excludes the subject's rows
and no process manager acts on them, reversed by a `ProcessingRestrictionLifted` fact — an
**adequate** Art. 18 mechanism? Specifically: (i) does "storage but no other processing" permit the
data to remain in the log and in a projection that is filtered at read time, or must the projected
rows be physically removed? (ii) Does the marker have to block **derived** processing (analytics,
aggregate business metrics) as well as subject-facing reads? (iii) Art. 18(3) requires informing
the subject before the restriction is lifted — is an event carrying the notification reference an
adequate record? *Why we ask now:* if the answer is "filtered at read time is enough", the
mechanism is small and should be built **with** #194; if rows must be removed, it is the same
machinery as tombstoning and must be designed once for both. **Grade (b)/(c).**

---

## Reported, not acted on in this brief

Two items were identified in the same pass and are **out of this brief's lane**; both are recorded
here so they are not lost:

1. **The `SessionId` scalar description overstates what the code does.** The scalar says the session
   is *"used to bind carts and **track the user across devices**"* (`specs/common/scalars.yaml:13-16`).
   The implementation is an **origin-scoped `localStorage` UUIDv7**
   (`crates/web/src/session.rs:14-31`) — it is per-origin by construction, so it does **not** track
   across devices, and `{slug}.captain.food` and `live.captain.food` each keep their own. This is
   not pedantry: the wording is what decides whether the **Art. 82 LIL / ePrivacy 5(3)
   shopping-cart exemption** covers the storage without consent. An exemption is available for
   storage *strictly necessary* to a service the user requested — a cart identifier qualifies; a
   cross-device tracking identifier, as the spec currently describes it, does not. **The spec text
   is the riskier artifact than the code.** A `specs/**` change, routed separately.
2. **`/public/graphql` responses now vary by the `captain_auth` cookie** (ADR-20260811-113000) and
   **no `Vary` or `Cache-Control` header exists anywhere in the tree** — the only mention is a
   comment at `crates/server/src/web_ssr.rs:16`. A shared cache in front of that path could serve
   one customer's cart to another. `Cache-Control: private, no-store` on `/public/graphql` belongs
   on the [#469](https://github.com/TheCaptainCompany/captain-food/issues/469) branch, not here.

*This brief maps obligations; none of it is legal advice. Every item graded (b) or (c), and the
currency of every citation, must be confirmed by licensed French counsel before any launch decision
rests on it. It prepares counsel's work and clears nothing.*
