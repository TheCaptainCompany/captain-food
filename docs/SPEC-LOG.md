# Spec log — what the DSL now says differently

**One row per landed `specs/**` change, newest first.** This page exists to satisfy one standing
product-owner obligation, verbatim: *"Just keep me informed"*
([ADR-20260810-221840](adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md)).

It is a **pull surface, not a report**. There is no cadence, no weekly digest and nothing to send.
Read it when you want to know what the product now promises that it did not before — the same way you
read [`docs/proposals/DECISIONS.md`](proposals/DECISIONS.md). It is kept current by a gate, not by a
habit.

> **Not a changelog of files.** `git log -- specs/` already says which files changed and that is
> exactly the thing nobody can read. The **"What the product now promises differently"** column is
> the whole point of this page: one sentence, in product language, that someone who has never opened
> the repo can evaluate. A row that says *"added field `x` to `Y`"* has failed and should be rewritten.

---

## How to write a row

Write it **in the same commit as the spec change**. Cost: one sentence. If the sentence is hard to
write, that is signal — it usually means the change is a Tier 2 that has not noticed yet.

### The tier is the boundary, and writing it is where the boundary is enforced

| Tier | Test | What it means |
|---|---|---|
| **0 — free** | Nothing emitted, nothing stored, no client shipped, no recorded decision touched | Land it. One row here, and that is the whole obligation. |
| **1 — migration** | The shape already exists in `domain_events`, in a shipped client, in an alert route, in a partner contract (HubRise/Stripe/Uber), or in a legal artifact (allergens, VAT, receipts, retention) | **Not an edit — a migration.** The versioning story is recorded *before* it lands. Stored events are immutable contracts: upcasting, never mutation (Greg Young, *Versioning in an Event Sourced System*). The GDPR tombstone path ([ADR-20260731-160000](adr/ADR-20260731-160000-order-erasure-tombstone-then-stream-deletion.md)) is the one recorded exception and is not a precedent. |
| **2 — decision** | It contradicts, or creates, a recorded decision in [`DECISIONS.md`](proposals/DECISIONS.md) or `docs/adr/` | **Stop.** This is a decision reversal wearing a spec edit's clothes, and it is not the team's to make regardless of how small the diff is. File a register row instead. **A Tier 2 row should never appear on this page** — if you are writing one, the change should not be landing. |

Structure is not a tier. A structural change with a real option space is routed by CLAUDE.md's
**proportionality** rule to a proposal + tracking issue + a register row — which *is* the discussion
the product owner offered. A structural change with no option space (a file move that rewrites no
`$ref`s, because refs are kind-logical) is a Tier 0 like any other.

---

## Rows

| Date | What the product now promises differently | Tier | Change | `make validate` |
|---|---|---|---|---|
| — | *(nothing has landed under the lifted freeze yet; the first row belongs to whoever lands the next `specs/**` change)* | — | [ADR-20260810-221840](adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md) | — |

---

## Status of the gate

The gate that keeps this page current — *if a commit range touches `specs/**` and this file is
unchanged, fail* — is **not yet built**. Its shape is one open decision with four options and a
recommendation, filed as [`DECISIONS.md`](proposals/DECISIONS.md) §26 row **SPEC-1**.

Until it lands, this page is a prose obligation and is therefore exactly as reliable as CLAUDE.md
says prose obligations are. That is a knowingly accepted, short-lived gap, not an oversight: the page
had to exist the day the freeze lifted, and it must not wait on the gate that protects it.
