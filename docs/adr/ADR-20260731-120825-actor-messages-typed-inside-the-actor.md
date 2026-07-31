# ADR-20260731-120825 — Actor messages (reminders) are typed INSIDE the actor, not a third catalog

**Status**: Accepted (product-owner decision, in-session 2026-07-31 — closes the one open veto on
the approved actor-runtime set, ADR-20260730-231500)
**Context**: [PROP-20260728-152752 "The write path becomes an actor mailbox"](../proposals/PROP-20260728-152752-actor-mailbox-write-path.md) §3.4 (reminders),
[#242 "Write path becomes an actor mailbox…"](https://github.com/TheCaptainCompany/captain-food/issues/242) Runtime D

## Decision

The mailbox proposal left one flag approved-by-default with a veto window: whether reminder /
self-message payloads become a **third top-level catalog** (`specs/messages.yaml`, parallel to
commands.yaml and events.yaml). The product owner vetoed the catalog:

> "The reminder methods should be typed in the spec inside the actor. For now we don't need it —
> strongly typed methods are required to ensure that there is a receive method in the actor that
> will process the message."

Concretely:

1. **No `specs/messages.yaml`.** A reminder message is not a system-wide vocabulary item the way a
   command or an event is — it is one actor talking to itself. It is declared **inside the actor**
   in actors.yaml (a per-actor `messages:` section when the first use case lands).
2. **Strong typing means handler-proof, per actor.** Every declared message MUST have a matching
   `receives` entry on the SAME actor — the validator rejects a message no receive method
   processes, exactly as it rejects an unexercised command. The type and its handler cannot drift
   apart because they live on the same node of the spec.
3. **Deferred until a use case exists.** No reminder is needed today (the `CheckPreparationDelay`
   pilot from the proposal was an illustration, not a requirement). The mailbox's `MESSAGE` kind,
   the `scheduled_at` column and the promotion-pass design all stand as approved — only the
   payload-declaration SHAPE is decided now, so the first real reminder lands in a settled spec
   shape instead of forcing this decision under pressure.

## Consequences

- `specs/database/tables/journals.yaml`'s `message_type` note no longer names a `messages.yaml`
  key — the vocabulary for kind `MESSAGE` rows is the addressed actor's own `messages:` section.
- Runtime D's reminder work (`scheduled_at` promotion pass, the `Reminders` companion) waits for
  the first declared use case; when it arrives, the codegen grows per-actor message payload
  structs + the `receives`-coverage validation rule, both scoped to actors.yaml.
- The DECISIONS.md open-veto entry for the actor-runtime set is closed; the set is now fully
  decided.
