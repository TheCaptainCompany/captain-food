# ADR-20260802-170059 — No client method exposed without a usage declaration in the spec

- **Status**: Accepted
- **Date**: 2026-08-02
- **Refines**: [PROP-20260728-152752 §2.1](../proposals/PROP-20260728-152752-actor-mailbox-write-path.md)
  (the typed actor client), [PROP-20260802-130500](../proposals/PROP-20260802-130500-isolation-by-construction.md)
  (isolation by construction)
- **Realized by**: [#290 "Actor-client crate isolation (PROP-20260728-152752 D9): compiler-enforced door, then per-actor crates"](https://github.com/TheCaptainCompany/captain-food/issues/290) phase 1

## Decision (product-owner directive, 2026-08-02)

A generated actor client exposes a method **only if the spec declares a use for it** — the
declaration is the permission:

| method | emitted iff |
|---|---|
| `send` | the actor's `receives` declares ≥1 COMMAND |
| `record` | the actor's `receives` declares ≥1 inbound EVENT/fact |
| `schedule` / `cancel_scheduling` | the actor declares `reminders:` |

The withdrawal method is named **`cancel_scheduling`** (product-owner decision, 2026-08-02,
[#308 "Decide cancel lane-scoping (from the #288 review) — now OrderClient-only"](https://github.com/TheCaptainCompany/captain-food/issues/308)):
the name says exactly what it withdraws — a SCHEDULED reminder, never an in-flight command — which
resolves the #288 review's confusion concern; it stays keyed by `message_id` (minted by `schedule`,
so holding the id is the capability), lane-scoping declined.

Before, an unjustified method was *uncallable* (sealed trait with no implementors) but present;
now it is **absent** — calling it is a compile error, and the surface guard
`client_surface_exists_only_with_a_spec_declaration` re-derives the rule from `specs/actors.yaml`
so emitter and spec cannot drift apart. With today's catalog: `Payment` is record-only (nobody
commands a Payment — it reacts to Stripe facts), eight actors are send-only, and only `Order`
schedules.

The directive generalizes — *"we should do the same for every method of the system, because it's
a hole"* — and the system-wide audit (which surfaces already have this property, which are still
holes) is recorded in
[PROP-20260802-130500](../proposals/PROP-20260802-130500-isolation-by-construction.md), with the
remaining holes tracked on the #290 checklist.

## Why

An exposed-but-unjustified method is exactly the easy path the isolation program exists to remove:
it compiles, it invites use, and only review stands between it and production. Dead surface is not
neutral — it is an open door with no declared owner.
