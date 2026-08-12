# ADR-20260812-214500 — A read target is DECLARED, never inferred: the `reads:` ownership wall

- **Status**: Accepted
- **Date**: 2026-08-12
- **Deciders**: founder (directive, verbatim below), recorded and executed by the team (architect →
  executor, mob-reviewed)
- **Composes with**:
  [ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md)
  (compiler first; a check is the fallback) ·
  [ADR-20260811-014129](ADR-20260811-014129-a-business-metric-is-a-projection-and-every-reference-is-a-ref.md)
  Decision 2 (every reference is a `$ref`; only a declaration introduces a bare name) ·
  [ADR-20260728-011344](ADR-20260728-011344-slug-lifecycle-and-sirene-inbound-events.md) (reservation tables) ·
  [ADR-20260720-015500](20260720-015500-acceptance-first-graphql-envelope.md) (the narrowly-scoped
  initiator reads: `paymentStatus`, `operationStatus`)
- **Realized by**: [#500 "#242 Runtime D: retire command_journal"](https://github.com/TheCaptainCompany/captain-food/pull/500)'s
  aftermath — the retirement that earned this record

## Context — what the last retirement cost, and why

Retiring `command_journal` cost 110 files and ~3,400 deleted lines. The founder, in session:

> *"it should never be used directly because we have to pass through the actor clients that
> encapsulate the insert"*
>
> *"it's unacceptable"*

He is right, and the diagnosis is precise: the table had **leaked out of its encapsulation**. GraphQL
resolvers read `command_journal` in query and subscription bodies, so deleting an implementation
detail became a cross-cutting change. The same hazard was live for the tables that replaced it —
`inbound_messages` and `mailbox_partitions`.

Two mechanisms let it happen, and neither was a rule anyone had written down wrongly. Both were
**absences**.

**1. The declaration site was an unguarded opt-in.** `reads-unknown-view` accepted as a legal target
any table under `specs/database/tables/*.yaml` carrying `reference: true`. Infrastructure-owned tables
merely *declined* to carry the flag, and the reason lived in a **header comment** —
`specs/database/tables/integration_staging.yaml` literally read *"NOT a GraphQL `reads` target (no
`reference: true`)"*. One word on `hubrise_connections` and the wall opened, with a leaked OAuth token
as the production symptom. A comment is not a gate.

Partly mitigated, in the `$ref` form only: §1b's ref-kind contract already constrained
`api.yaml types.*.reads[*]` to three kinds, so `reads: [{ $ref: 'database/tables/journals.yaml#/inbound_messages' }]`
did error. But `api::name_list` accepts a **bare string**, and the §1b refs walker collects `$ref`
nodes only — so `reads: ['inbound_messages']` plus the planted flag **passed `make validate` with zero
errors**, verified on `786bcfa`. That is [#413](https://github.com/TheCaptainCompany/captain-food/issues/413)'s
defect class again: *"silently invisible everywhere"*.

**2. Transience was inferred from an omission — and this is the one that actually let the journal
through.** A type counted as transient because it had no `reads:` (`validate::core`'s
`transient_types`). So a query escaped every read-side rule by **leaving a line out** and writing
"NON-PROJECTED (transient)" in prose. Deleting a `reads:` line from the committed catalog passes
`make validate` with zero errors and zero warnings, also verified on `786bcfa`. The journal resolvers
declared no `reads:` at all: **a reads-side rule, however strict, would never have looked at them.**

## Decision

**A read target is declared, and its owner decides whether it may be one.**

1. **Ownership comes from the classifier, never from a flag and never from a name.**
   `refs::classify` — the same function §1b uses — maps every `database/tables/*.yaml` catalog to a
   `Kind`. `refs::read_target_kind` partitions those kinds into read-legal (`ProjectionView`,
   `ProjectionTable`, `ReferentialTable`) and infrastructure/adapter-owned (`JournalTable`,
   `PmStateTable`, `StagingTable`, `ConnectionTable`, `EventStoreTable`, `ReservationTable`). Its
   `match` is **exhaustive over `Kind`**, so a new **`Kind`** does not compile until it is classified.
   Stated precisely, because the honest version is what makes it reusable: that is a compile-time
   guarantee about a new *kind*, **not** about a new catalog **file** — `classify`'s `_ => None` arm
   accepts an unknown `database/tables/*.yaml` with no code change, and such a file then fails closed at
   **validate** (`ref-kind-unknown` on any `$ref` into it, plus `reads-unknown-view`) rather than at
   compile. `reservations.yaml` is the proof: it had no arm for months and built fine. Level 4 for the
   kind, level 3 for the file; both directions closed, only one of them the compiler. A name pattern was
   rejected — `external_%` matches one of seven categories and misses `auth_sessions`,
   `hubrise_connections`, `inbound_messages`, `mailbox_partitions`, `payment_process_manager`,
   `slug_reservations`, `domain_events` — and so was the author-supplied `staging: true`, forgeable by
   the same omission this ADR is about.

2. **`reference: true` is the read side's opt-in and nobody else's** —
   `reference-flag-not-a-read-target` fires at the DECLARATION, so the wall cannot be widened in one
   change and walked through in the next.

3. **An infrastructure table named by `reads:` is refused on ownership alone**
   (`reads-infrastructure-owned`), independent of the flag, with a message that names the category and
   the owning catalog rather than reporting "unknown view".

4. **Transience is a DECLARATION.** A type a query or subscription returns declares either `reads:` (a
   read model) or **`readsInfrastructure:`** — the write-path table it is served from, as a `$ref` the
   loader resolves, mutually exclusive with `reads:` and constrained by a REF_CONTRACT row to
   **`JournalTable` and `PmStateTable` only** (`refs::TRANSIENT_READ_KINDS`) — the two categories a
   resolver is actually served from today. **Not** "any infrastructure kind": the first cut of this
   change admitted the whole partition, which made the new key strictly WEAKER than the `reads:`
   contract beside it — `reads:` already refused a `$ref` into `hubrise_connections` or
   `domain_events`, so the permissive form *opened a door that had been shut*, on the PUBLIC-reachable
   `Operation` type, to precisely the leaked-credential exposure rule 1 exists to prevent. Caught by the
   independent review of this ADR's own PR, which is an argument for the shape and not against it:
   serving a query from a credential store, a staging mirror or the event log must cost a reviewed edit
   to that one line. Omitting both is `transient-type-undeclared-infrastructure`. Four types
   declare it today: `MailboxLane`, `PoisonedMailboxMessage`, `Operation` (the mailbox) and
   `PaymentIntent` (the saga row — ADR-20260720-015500's declared exception). Nested output types
   reached through a parent's `reads:` owe nothing, because no store is touched there.

5. **Every entry in either list is a `$ref`** (`reads-not-a-ref`), because a bare name is invisible to
   §1b and would walk past all of the above.

**The consequence that matters**: the next retirement is a **resolved reference**, not a repo-wide
grep. `git grep readsInfrastructure` plus the validator names every API surface a write-path table
feeds, before the table is deleted rather than after.

## What this does NOT do

- **It does not touch `architecture/c4-l3.yaml`'s `components.*.reads`.** That is the *correct* home
  for infrastructure readers — the mailbox worker, the ACLs, the process managers. Banning it there
  would delete the only place a mailbox reader can be declared.
- **It does not claim to have closed the leak by itself.** Honest scope: rule (2) would **not** have
  caught `command_journal` — all eight `reference: true` declarations already sat in
  `referential.yaml`, and the journal queries declared no `reads:` at all. Rule (4) is the one aimed at
  the actual failure; (2) is a ratchet that keeps the opt-in from being widened later.
- **It is a check, and a check is the fallback** (ADR-20260803-234035). The compiler-first answer
  exists and is only half-applied: `crates/actor_client`'s `MailboxAccess(pub(crate) ())` witness makes
  `Mailbox::insert` uncallable outside that crate — the founder's own model, already built. The query
  side (`MailboxLaneRepository`, `MailboxRequeue` in `crates/application/src/queries.rs`) has no
  witness, and cannot reuse that one: `actor_client` depends on `application`, so the dependency
  arrow points the wrong way. Extending it means moving those ports and changing the resolver emitter
  — a slice of its own, tracked separately, and the stronger fix when it lands. A `reads:` binding is
  YAML, so no type can reach *it*: for that half, the gate is correct rather than lazy.

## Consequences

- Five new validator ERRORS, all seen red first, all pinned by fixture tests that mutate the **real**
  committed catalog (`tools/codegen-rs/src/tests.rs`, `mod read_target_ownership`) — including one that
  proves legality follows the declaring catalog rather than the table's name, so the wall survives a
  table moving between files, and one that plants a credential store and the event log under
  `readsInfrastructure:` and requires both to be refused. That last one was **missing from the first
  cut, which is exactly why the widened-door defect shipped past a green gate** — the lesson being that
  a new permission needs a mutation test of its own, not just the rule it was added to serve.
- One new DSL key, `api.yaml types.*.readsInfrastructure`, on four existing types. It changes **no
  generated artifact** — not the SDL, not a resolver — so nothing promised to a client moves.
- `database/tables/reservations.yaml` gains a classifier arm (`Kind::ReservationTable`). It had none,
  so `slug_reservations` resolved to no `Kind` at all and any kind-keyed rule silently skipped it.
