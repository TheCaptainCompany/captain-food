# Dispatch — #614 "Erasure's safety bound fails OPEN after the split"

- **Issue**: [#614](https://github.com/TheCaptainCompany/captain-food/issues/614)
- **Base**: `main` @ `27be663` — verify before relying; four cards in a row have carried a stale or wrong header.
- **Reversibility class**: **HIGH-CONSEQUENCE** — erasure is a legal surface. **`HOLD: human`**: it stops at ready-for-review for the team's independent reviewer pass; no auto-merge.

> **Roster sizing, stated so it is auditable rather than assumed.** The standing rule is a full-roster briefing for legal surfaces. I am briefing **five** — `dba` (found it), `legal-specialist` (what erasure owes and when), `young` (checkpoints, folds and what a projection guarantees), `beck` (how a fail-closed guard is proven closed), `observability-agent` (a halted erasure that nobody can see). My reasoning: the *surface* is legal, but the *change* is one query, one default and a test — no stored shape, no money, no client-visible behaviour. If any lens thinks that sizing is wrong, **say so and I will widen it**; a lens objecting to the roster is a legitimate output of this briefing.

> **Antecedent rule** ([ADR-20260817-105845](../adr/ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)): every claim below is sourced to #614 or marked `UNVERIFIED input`. The last four cards were each wrong about something a lens caught by measuring. Assume this one is too.

## The defect

`crates/infrastructure/src/deletion.rs:229-231`:

```sql
SELECT COALESCE(MIN(position), 9223372036854775807) FROM projection_checkpoint ...
```

The deletion engine runs in `captain_write` and deletes from `domain_events`/`domain_stream` — the correct placement. Its entire safety property is *"every fact it acts on has already been folded by every read model"*, and that property rests on this `MIN`.

`specs/database/databases.yaml` places `projection_checkpoint` **per read database**.

So after the split, the `MIN` sees only whatever checkpoints happen to live in `captain_write` — possibly none. `COALESCE` then yields `i64::MAX`, the bound collapses to head, and **erasure runs ahead of the projectors**: streams deleted before read models have tombstoned their rows, leaving orphaned personal data in a read model with no source left to re-derive the deletion from.

**Not a defect today.** It is correct while everything is one database. It becomes live the moment the split lands.

## Why this one and why now

`dba` found three split hazards. The other two fail **loudly** — a query errors, a guard skips, something goes red. This one fails **quietly, and in the direction of a breach**: no error, no log line, no metric. The system reports healthy while personal data survives an erasure request recorded as completed.

It is cheap now and an incident later, and it is a **precondition of the split**, not a follow-up — which is why it is being done while the split is decision-blocked rather than after it is unblocked.

## Scope

- **Fan the bound across the placed read databases**, and **fail closed** on any unreachable one. Concretely the `COALESCE` default becomes `0`, not `i64::MAX` — the safe direction is *assume nothing has been folded*, which halts deletion, rather than *assume everything has*, which authorises it.
- **A behaviour test that drives the fail-closed path**: with one read database unreachable, erasure must **not** advance. Mutation named as the semantic edit — *restore the `i64::MAX` default* — going red with a message naming the uncorked bound.
- **The halted state must be observable.** A fail-closed guard nobody can see becomes a silently unmet erasure SLA, which is its own compliance problem — erasure has a deadline, and a stop that nobody notices burns it just as surely as a leak. Whether that is an `observability.yaml` entry, an alertable signal, or something else is `observability-agent`'s call, not pre-decided here.

## Open questions the briefing should settle, not the executor

1. **How is "the placed read databases" known at runtime?** Derived from the placement inventory, configured, or discovered? Today there is one connection and one database; the fan-out needs a source of truth for the set, and picking the wrong one builds a second registry that can disagree with the declaration — the exact defect class #596 was about.
2. **Is a checkpoint fan-out even the right shape**, or does the bound belong somewhere else entirely once the projections are physically separate? `young` should rule: a `MIN` over other databases' checkpoints is the write side reading the read side's state to make a write decision.
3. **What does erasure owe while halted?** If the guard trips, requests queue rather than complete. `legal-specialist` on whether a halted-but-visible erasure is a compliant posture and for how long — and note the standing caveat that no lens output is legal advice or clearance.

## Fences

- **Do not implement the split.** This makes the bound correct *for* a split that does not exist yet; it must remain correct on today's single database, and the test must prove both.
- **Do not touch what erasure deletes, or when a request is accepted.** The bound is the subject; the policy is not.
- **`specs/**`** in scope only for an observability contract if one is added, with its `SPEC-LOG.md` sentence in the same commit.
- Every other defect found becomes an issue, not this diff.

## Findings

_(Lenses and the executor append here. "Nothing in my lens" is a complete answer, and so is "the roster is too small".)_
