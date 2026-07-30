# ADR-20260730-231500 — The write path becomes an actor mailbox; the read side gets a batched, partitioned projection runtime

- **Status**: Accepted (product owner, 2026-07-30, in-session: "we are at the same page, we can build it now")
- **Amends**: ADR-20260720-015300 (command journal), ADR-20260720-015400 (inbound events), ADR-20260720-015500 (acceptance-first — the *contract* stands, the *executor* moves)
- **Proposals carrying the full design and trade-offs** (approved same day, not restated here):
  [PROP-20260728-135632 "Aggregate state as spec"](../proposals/PROP-20260728-135632-aggregate-state-as-spec.md) ·
  [PROP-20260728-152752 "The write path becomes an actor mailbox"](../proposals/PROP-20260728-152752-actor-mailbox-write-path.md) ·
  [PROP-20260730-230803 "Projection runtime"](../proposals/PROP-20260730-230803-projection-runtime-batched-partitioned.md)
- **Tracking issues**: [#235 "Write-side per-instance authorization…"](https://github.com/TheCaptainCompany/captain-food/issues/235) ·
  [#242 "Write path becomes an actor mailbox…"](https://github.com/TheCaptainCompany/captain-food/issues/242) ·
  [#267 "Projection runtime…"](https://github.com/TheCaptainCompany/captain-food/issues/267)

## Decision (summary — the proposals are the reference)

1. **One durable mailbox**: `inbound_messages` replaces `command_journal` + `inbound_events`.
   Every write intent — command, inbound fact, reminder MESSAGE — is a row addressed to
   `(actor_type, actor_id)`, positioned by one sequence, partitioned by a frozen
   `hash(actor_id) mod N` (N = `mailbox.partitions` in the DSL).
2. **Workers, not resolver spawns, execute**: per-actor-type workers lease partition ranges
   (registry `mailbox_partitions`: checkpoint + lease + `ownership_version` fencing counter);
   GraphQL journals and returns ACCEPTED, nothing more; clients follow via
   `operationStatus`/`operationStatusChanged` (the 015500 contract, unchanged).
3. **One transaction per decision** (the invariant 015300 lacked): event append + message status
   flip + reminder inserts/cancellations (+ pm_state for process managers) commit together under
   the `ownership_version` guard.
4. **The actor owns its whole behaviour**: declared `state:` with event lineage, generated
   `apply`/`fold` ON the actor, generated `requires` (acting/claims) precondition, `identity:`
   addressing — process managers included, first-class (directly addressable when they receive
   commands).
5. **Typed clients are the only door** (both directions), with `send` / `schedule` / `watch` /
   `ask` (bus or journal-backed call-and-wait, internal); reminders (`scheduled_at`,
   position-at-promotion) replace bespoke timers.
6. **Activations** (virtual actors): apply-after-commit promotion, micro-mailbox single-flight,
   batched turns with the held-state memento, spec-configured expiry — all zero-correctness-weight
   optimizations behind `Mailbox`/`PlacementLookup` ports (no framework adopted; Proto.Actor field
   study D2.1; Redis only ever as cache, never truth — D7).
7. **Read side rhymes**: projectors process batches through a GENERATED identity-map/unit-of-work
   (no ORM), one transaction per batch, lanes partitioned by a spec-declared per-event
   `businessKey` (column on `domain_events`), per-key order guaranteed; projections declare a
   `target:` (`ScopeMembership` → redis, served from Postgres until Redis lands).

## Consequences

- `specs/**` changes are authorized as scoped in the proposals' §8 lists (the approval covers
  them); realized via the claim → draft-PR flow starting at #242's foundation slice.
- ADR-20260720-015300/-015400's tables are end-of-life once migrated; 015500's public contract is
  reaffirmed verbatim.
- The stale-`RECEIVED` sweep becomes crash hygiene only; `#193`'s single-flight concern is
  answered by leases for this path.
- One open flag for the product owner: `messages.yaml` as the third payload catalog (recommended,
  approved-by-default, veto window open).
