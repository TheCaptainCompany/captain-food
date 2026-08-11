# ADR-20260810-221840 — `specs/**` is the team's work: the freeze is lifted, the reporting obligation replaces it

- **Status**: Accepted
- **Date**: 2026-08-10
- **Deciders**: product owner (directive, verbatim below), recorded by the architect
- **Supersedes**: the CLAUDE.md non-negotiable *"DSL source files (`specs/**`) are **never** modified
  by autonomous/execution loops — only plan mode proposes DSL changes, with approval"* and its three
  restatements (`docs/PLAYBOOK.md`, `docs/claude/dsl.md`, `.claude/skills/architecture-review/SKILL.md`)
- **Composes with**: [ADR-20260810-215503](ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md)
  (prioritisation delegated) · [ADR-20260810-011500](ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md)
  (team ownership) · [ADR-20260809-013142](ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md)
  (mob programming) · [ADR-20260808-144738](ADR-20260808-144738-product-ownership-lives-in-the-team-no-pm-agent.md)
  (product ownership lives in the team)

## Context

The product owner, in session, 2026-08-10:

> *"I'm surprise that I read that the spec was untouchable now that we have the team working together
> we don't need to have this constraint anymore"*
>
> *"We can perhaps have a discussion if the team is willing to change the structure of the specs. But
> I'm pretty sure the team will ensure the right naming and scope. Just keep me informed"*

This is the last of four delegations to land in three days, and it is the one that actually reaches
the work. Prioritisation moved to the team on 2026-08-10 (ADR-20260810-215503); sessions became
self-starting on 2026-08-10 (ADR-20260810-011500); product ownership moved into the team on
2026-08-08 (ADR-20260808-144738). Each of those delegated *judgement*. None of them delegated
*capability*: with `specs/**` frozen, a team that owned the priorities, owned the product questions
and started its own sessions still could not add a command, an event, a rule, a screen binding or an
observability reason without stopping and asking.

The measured cost of the freeze on the day it was lifted: **8 of 98 open issues carry an explicit
AMBER flag** — blocked on nothing but the spec window
([#476](https://github.com/TheCaptainCompany/captain-food/issues/476),
[#468](https://github.com/TheCaptainCompany/captain-food/issues/468),
[#465](https://github.com/TheCaptainCompany/captain-food/issues/465),
[#462](https://github.com/TheCaptainCompany/captain-food/issues/462),
[#461](https://github.com/TheCaptainCompany/captain-food/issues/461),
[#409](https://github.com/TheCaptainCompany/captain-food/issues/409),
[#347](https://github.com/TheCaptainCompany/captain-food/issues/347),
[#210](https://github.com/TheCaptainCompany/captain-food/issues/210)) — plus four more
([#398](https://github.com/TheCaptainCompany/captain-food/issues/398),
[#400](https://github.com/TheCaptainCompany/captain-food/issues/400),
[#401](https://github.com/TheCaptainCompany/captain-food/issues/401),
[#402](https://github.com/TheCaptainCompany/captain-food/issues/402)) whose checklists route a
sub-task to plan mode. ADR-0032 completeness made this worse rather than better: a single new command
also needs its event, error, rule, test and story, **all in `specs/**`**, so almost no feature was
ever autonomously executable. [#210](https://github.com/TheCaptainCompany/captain-food/issues/210)
recorded the structural consequence in its own body: *"🟠 AMBER — needs a `specs/**` change | ~22 |
blocked on plan-mode approval"*.

## Decision

**`specs/**` is ordinary work.** Autonomous and execution loops may add and amend DSL content and
structure under the same gates as any other change. The freeze is deleted, not narrowed.

### The boundary is not content vs structure

The obvious line — *content is delegated, structure is a discussion* — was considered and **rejected**,
because it is drawn on the DSL's grammar while the risk in this repo is distributed along entirely
different seams. It is not merely imprecise; it is **anti-correlated with risk in both directions**:

- **A large structural move can be free by construction.** CLAUDE.md's own design guarantees it:
  *"`$ref`s are KIND-logical: `commands.yaml#/X` names a kind, never a file location, so moving an
  item between scope folders rewrites no refs."* Relocating a whole aggregate between scopes — the
  most structural-looking edit available — rewrites nothing and is validator-proved by the placement
  and cross-scope-DAG rules.
- **A one-word content edit can be irreversible.** Changing a field's type, nullability or name on an
  event that already has rows in `domain_events` is a contract mutation, and Greg Young's position
  (*Versioning in an Event Sourced System*) is that stored events are immutable: the answer is
  upcasting, never mutation. The GDPR tombstone-then-stream-deletion path
  ([ADR-20260731-160000](ADR-20260731-160000-order-erasure-tombstone-then-stream-deletion.md)) is the
  one recorded exception and is explicitly not a precedent.

`specs/common/` is the same mistake in miniature. It looks structural; it is Evans's **shared kernel**
— that is, *high fan-out*, and fan-out is mechanically countable (the validator already enforces
kernel purity and the cross-scope `$ref` DAG). Adding a new scalar to `common/` is near-free because
nothing refs it yet; changing an existing one costs fan-out × the tests below. Freezing `common/`
wholesale would freeze the one place where *"one name = one dedicated scalar"* is actually enforced —
defeating precisely the discipline the product owner said he trusts the team with (*"the right naming
and scope"*).

### The boundary is three questions, asked in order

Before any `specs/**` edit lands:

1. **Does it contradict, or create, a recorded decision?** — check
   [`docs/proposals/DECISIONS.md`](../proposals/DECISIONS.md) and `docs/adr/`. If yes, **it is not a
   spec edit; it is a decision reversal wearing a spec edit's clothes.** Stop and file a register row.
   Size is irrelevant here: a one-line change that reverses a decided row outranks a thousand-line
   refactor that reverses nothing.
2. **Is the shape already emitted, stored or promised?** — does it exist in `domain_events`, in a
   shipped client, in an alert route, in a partner contract (HubRise/Stripe/Uber), or in a legal
   artifact (allergens, VAT, receipts, retention windows)? If yes, **it is a migration, not an edit**:
   the versioning story is recorded before it lands.
3. **Otherwise it is the team's** — including structure, including `specs/common/`, including renames.

### Structure needs no separate gate — proportionality already carries it

The product owner offered *"perhaps a discussion"* about structural change. That is an offer of a
forum, not a reservation of a veto, and **the forum already exists**: CLAUDE.md's proportionality rule
sends anything with a real option space to a proposal + tracking issue + a DECISIONS.md row. A
structural change has a genuine option space essentially by definition — that is what makes it
structural — so proportionality routes it to the product owner automatically, with per-option
pros/cons and a recommendation, which is exactly the discussion that was offered.

Proportionality is a strictly better discriminator than content/structure because it keys on **whether
there is a decision**, not on **which part of the grammar was touched**. A content edit with a real
option space gets a proposal; a structural edit with no option space (a file move that rewrites no
refs) gets a commit message.

## The reporting obligation: `docs/SPEC-LOG.md`

*"Just keep me informed"* is a standing obligation with no mechanism, and an obligation with no
mechanism decays. The mechanism is **one gated page**, [`docs/SPEC-LOG.md`](../SPEC-LOG.md):

- **One row per landed spec change**, newest first: date · **what the product now promises differently,
  in one sentence a non-engineer reads** · the tier (0 free / 1 migration / 2 decision) · PR + ADR
  links · the `make validate` delta.
- **The row is written in the same commit as the spec change**, by the executor who made it. Cost: one
  sentence — smaller than the PR body they already write.
- **A gate, not a habit** (CLAUDE.md: *prefer executable over prose*): if a commit range touches
  `specs/**` and `docs/SPEC-LOG.md` is unchanged, the check fails. Prose obligations decay; a gate
  cannot. `makefile_recipe_lines_are_ascii` is the model.
- **No cadence, no push.** No weekly digest, no standing report, no ritual. The product owner reads
  the page when they want it, exactly as they read `DECISIONS.md`. A pull surface that is
  gated-current beats a push cadence nobody runs — that is the whole design judgement, and it is why
  this is not the fifth process to be quietly abandoned.
- **The tier column is where the boundary is enforced, at the cheapest possible moment.** Tier 2 must
  never appear: an executor about to write "this reverses a recorded decision" has, by writing it,
  discovered that the change is not theirs. The log doubles as the boundary's tripwire.

The shape of the gate is the one open decision this ADR creates; it is filed as a DECISIONS.md row
with four options and a recommendation. The **page itself is created now and is usable immediately** —
the obligation exists from today, and it does not wait on the gate.

## What still protects correctness — and what does not

Stated honestly, because the freeze was doing real work by accident and it is worth knowing exactly
how much.

**Still protects (mechanical, unchanged by this ADR):**

- **`make validate`** — 173 distinct rule codes across `tools/codegen-rs/src/validate/`: schema and
  `$ref` resolution, `$ref`-kind appropriateness, scope placement, the cross-scope `$ref` DAG, kernel
  purity, api↔model agreement, view wiring, C4, observability contracts, and ADR-0032 bidirectional
  completeness. **0 errors and no NEW warning against a freshly measured `main`.**
- **The drift gate** — CI's `codegen` job fails on any spec↔generation divergence, so a spec edit that
  does not regenerate cannot land.
- **`rustc`** — most of the DSL's meaning becomes generated types. A removed event variant, a changed
  scalar or a renamed command breaks compilation across the generated domain crates. This is the
  compiler-first floor (ADR-20260803-234035) doing exactly what it was built for.
- **The mob** (ADR-20260809-013142) — the whole roster is briefed before any code, and any lens may
  stop the work. This is the strongest protection in the list and it is the one that actually reads
  for *intent*.
- **The independent full-diff review** by eyes that did not write it (product-owner directive,
  2026-08-01).

**Does NOT protect — the validator cannot see product intent, and this is not a theoretical risk:**

`make validate` is a **closure checker**. Every one of its 173 rules asks *"is this graph closed and
consistent?"* Not one asks *"is this the product we want?"* All of the following pass at **0 errors**:

- A `Rule` and a `Test` that assert each other and nothing real — the ADR-0032 bidirectional link is
  satisfied by *any* consistent pair.
- Widening `roles:` on a mutation. The story-map rule is *reachability* (≥1 step, persona authorized),
  not least-privilege; authorization **breadth** is not a validated property.
- Deleting a `gaps:` declaration from a screen. Gaps are declarative honesty and nothing checks that a
  removed gap corresponds to shipped capability.
- Changing what a field *means* in its `description:` while keeping its type. Load-bearing semantics
  genuinely live in descriptions and comments here — the LIVE-vs-LOCKED cart-pricing decision is
  carried in comments in `specs/ordering/api.yaml`, and the `cart-price` contract's *"canonical
  bounded set"* of reasons is three `#` comment lines at `specs/observability.yaml:271-273`, with the
  emission site hardcoding `"offer_gone"` at `crates/server/src/graphql/cart_read.rs:143`. Nothing
  validates that an emitted reason is in the set, or that a declared reason is ever emitted.

**The empirical proof is in this repo's own record.** The screen-roles hole
([#466](https://github.com/TheCaptainCompany/captain-food/issues/466)) shipped a PUBLIC screen bound
to a CUSTOMER-only resolver and **`make validate` passed with 0 errors**; it was caught *"only by a
product-owner domain reminder, one phase later"*. That is the exact failure mode this ADR must be
honest about: a perfectly self-consistent spec that is not the product, with every gate green.

The sharpest available illustration of the difference between a gate that *runs* and a gate that is
*reported* as running is the [#474](https://github.com/TheCaptainCompany/captain-food/issues/474)
measurement taken the same day: over a deliberately re-planted migration defect, `make rust` exits
**0** and `cargo test --workspace` reports **990 passed** — bit-identical whether or not the database
suites ran — while the same command against a real Postgres with `DB_TESTS_REQUIRED=1` exits **101**
on `cart_events_fold_into_the_read_model`. A test that already existed catches the defect; it simply
never runs locally. Green is not evidence. **Which gate ran is.**

**So, plainly**: the mob catches intent breaks *inside* a dispatch. Nothing catches intent drift
*across* dispatches except `docs/SPEC-LOG.md` — and a log is detection, not prevention: it catches
drift after it lands. That is the honest residual risk of this delegation, it is accepted knowingly,
and the mitigation is that the log is short, current and readable by the one person who can say "that
is not the product I want."

## Consequences

- The 🟠 **AMBER lane loses its primary cause.** It is redefined, not deleted: AMBER now means *a
  recorded decision is missing or contradicted*, or *the shape is already emitted/stored/promised and
  the versioning story is not recorded* — not *"it touches `specs/**`"*.
- **Eight AMBER-flagged issues and four plan-mode sub-tasks are re-triaged**; the register's §25 note
  that rows exist *"because they need a `specs/**` edit (frozen for execution loops, CLAUDE.md)"* is
  now historical.
- **`event_version` becomes load-bearing and does not exist.** PROP-170000 D2 decided
  *"additive-only + validator gate; add `event_version` now (cheaper before the log grows)"* by
  ensemble consent on 2026-08-08. Verified 2026-08-10: **zero occurrences of `event_version` across
  `specs/`, `crates/`, `migrations/` and `tools/`.** The freeze was silently substituting for it — a
  payload shape nobody could change needed no versioning story. Removing the freeze makes test 2 above
  the only thing standing between a spec edit and a stored-event contract, and test 2 is prose.
  **This is the structural work the delegation actually calls for**, and the window is open only while
  the log is empty (ADR-20260807-002705 D6 chose start-clean, no dump restore).
- Records to update in the same change: `CLAUDE.md`, `docs/PLAYBOOK.md`, `docs/claude/dsl.md`,
  `.claude/agents/architect.md`, `.claude/skills/architecture-review/SKILL.md`, `docs/STATUS.md`,
  `docs/proposals/DECISIONS.md`.
- **Not delegated by this ADR, unchanged**: the decision register itself; external, legal and
  admin-gated matters; and the value-first prioritisation method, which remains binding
  (ADR-20260810-215503).
