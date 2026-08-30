# ADR-20260830-183000 — The actor inbox is a GENERATED enum and the routing match is HUMAN-OWNED

- **Status**: Accepted
- **Date**: 2026-08-30
- **Decider**: the **FOUNDER / Tech CEO**. Directive, verbatim:

  > **"Go for the generated per-actor enum."**

  It followed his own challenge, also verbatim:

  > **"the strongly typed actors type consumer subscribe to the actor type without any message type
  > restriction… Are we on the same page?"**

  That question is the defect statement. `specs/*/actors.yaml` already declares each actor's
  `receives:` set, and the runtime threw the declaration away at the door.

- **Relates**:
  [ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md)
  (compiler first; deleting a gate the compiler subsumes is a correct outcome) ·
  [ADR-20260829-230418](ADR-20260829-230418-aggregates-own-the-facts-isolation-first.md)
  (the isolation doctrine this closes on the transport) ·
  [ADR-20260808-235113](ADR-20260808-235113-final-vision-first-no-intermediate-steps.md) ·
  [#595 "The reclamation replacement birth writes Order-{id} with no transaction and no lane"](https://github.com/TheCaptainCompany/captain-food/issues/595) ·
  [#766 "C3-BLOCKING: the saga-command dispatch table has no completeness gate"](https://github.com/TheCaptainCompany/captain-food/issues/766) (superseded — its remedy is never built) ·
  [#358 "Split the monolith into per-deployable runtimes"](https://github.com/TheCaptainCompany/captain-food/issues/358) (consumer-first deploy ordering, below)
- **Realized by**: [#771](https://github.com/TheCaptainCompany/captain-food/issues/771) /
  PR [#776](https://github.com/TheCaptainCompany/captain-food/pull/776)
- **Register**: [TYPED-ACTOR-INBOX](../decisions/TYPED-ACTOR-INBOX.yaml)

## The decision

**One `<Actor>Inbox` enum per mailbox actor, GENERATED from `receives:`, spanning every message kind
(COMMAND / inbound FACT / REMINDER) with each variant carrying its typed payload — and the routing
`match` over it written and owned by humans.** A message an actor declares it receives and nobody
consumes is a `rustc` **E0004** build failure, not a `FAILED` row a customer pays for.

## What was wrong

Two failures, both silent at every existing gate.

**1. Unconsumed message.** The router (`crates/infrastructure/src/generated/command_router.rs`) was
a flat `match` over a `&str` message type across ALL actors, ending in `_ => None` → `FAILED
"unroutable command type"`. Its saga loop did `else { continue }` where its mutation twin asserted —
same class of omission, two postures. #595 hit it by hand: `PlaceReplacementOrder` had no arm and a
replacement order was silently never born.

**The cutover measured the real size of it: TEN commands were declared in some actor's `receives:`
with no dispatch arm at all on `main` — and all TEN are now wired.** Every one of them had its
handler already written in `application::commands` and was missing only a table row — the exact
history the `UNWIRED_MUTATIONS` doc comment recounts for `recordDeliverySatisfaction` and
`escalateDelivery`, repeating, ×10. Two are already declared process-manager `sends:`:

- `CartBindingProcess` → `BindCartToCustomer` — a returning visitor's guest cart never binds.
- `ReclamationProcess` GOODWILL_CREDIT arm → `GrantCustomerCredit` — **a resolved reclamation's
  goodwill credit never granted.** Money the customer was told they had.

**2. Cross-actor dispatch.** `dispatch_command` received `message.message_type` and **never**
`message.actor_type`. The router was flat over all commands, so a row on lane A could drive a handler
that writes aggregate B — under A's fence. That is ADR-20260829-230418 violated **by the transport
itself**, not by a process manager.

## Why this shape

**Variants carry typed payloads and the enum is not `#[non_exhaustive]`.** `#[non_exhaustive]` would
*force* every downstream match to carry a wildcard arm — the precise opposite of the point.

**Closed enum on dispatch, open parse at the boundary.** The wire stays a string;
`ActorInbox::parse(actor_type, message_type, payload)` is the single fallible edge. It takes the LANE
as well as the message, so a `PlaceOrder` payload on a `Cart` lane is not a mis-routed value — it is
a value that cannot be constructed. The cross-actor hole closes in the type, not in a check.
`inbound_messages` is untouched: **no stored shape and no wire format change.**

**The enum is generated; the match is human-owned.** *(beck, decisive.)* If one emitter walk produced
both the variants and the arms, the match would be exhaustive **by construction** and the compiler
would catch exactly nothing. Generated `match`es that are total *projections* of the variant set
(`parse`, `message_type()`, `kind()`) are fine — they encode no decision a human could get wrong. The
one match that encodes a decision lives in `crates/infrastructure/src/inbox.rs`, which states that
boundary in its own header.

**Whole-tree cutover, no flag.** A per-lane flag would mean two routers live at once — the exact
ambiguity being removed. This does not overturn gate-then-stabilize: gating decides WHEN a finished
thing takes over, and here the change is compile-time with no stored shape and no wire change, so
**revert is the rollback**.

**Unknown message type is TRANSIENT, never terminal.** During a rolling deploy an old consumer
legitimately meets a message type a newer producer already emits; terminal-failing it buries a paid
order. The delivery aborts, the row stays RECEIVED and is retried with backoff, then flips at
`max_delivery_attempts` into the **existing** poison path — which is already loud by construction:
`mailbox_poison_failed_total{actor_type}`, the ADMIN `poisonedMailboxMessages` read, and
`RequeueMailboxMessage` as the operator's way back. **No new status and no migration.**
`InboundMessageStatus` has no `PARKED` member and is a stored, promised column; inventing one would
have turned a compile-time change into a stored-shape migration. Park is the poison path, reached by
aborting instead of failing.

**A deferral is DSL content, not a Rust const — and it ships with ZERO users, deliberately.**
`UNWIRED_MUTATIONS` was a bare list of names in an emitter — no reason, no owner, nobody read it.
Its successor is `receives[].deferred: { reason, issue }`, validator-enforced
(`receives-deferred-shape`: closed key set, both fields required, the issue a FULL URL). *The URL
rule stands on the CLAUDE.md ground that a bare `#NN` does not auto-link outside issues/PRs/commits
— **not** on the claim, made in the first version of this record and falsified by the round-1
reviewer pass on PR #776, that the text renders into `specs/generated/documentation.generated.md`.
It does not: no docs emitter reads `deferred:`. It renders into the GENERATED Rust doc comment on
the inbox variant and into `inboxes::DEFERRED_MESSAGES`, and it is copied by hand into PR and issue
bodies — which is exactly where a bare `#NN` dies.*

Its intended first user was `DeliveryJob`/`UpdateDeliveryStatus`, believed to be the one of the ten
with no handler written. **That was wrong**: `update_delivery_status` is the generated `via: status`
handler, is re-exported beside `change_rider_status`, and passes
`tests.yaml#/TestDeliveryStatusUpdatedByCommand`. It is wired like the other nine, and the deferral
is withdrawn rather than restated — a deferral whose `reason:` is false is worse than no grammar,
because it is a reviewable artifact nobody can review correctly.

**So the grammar has zero instances, and we keep it.** Stated as a decision, not a side effect: a
new DSL key, a validator rule and two tests with no user is real carrying cost, and the honest
alternative was to delete it and re-derive it when the first genuine deferral arrives. We keep it
because (a) C3's remaining `deliver:` routes are each an opportunity to declare a route whose handler
is not yet written, which is precisely the shape the grammar exists for, and (b) the moment a
deferral is *needed* is the moment nobody has time to design its reviewable form, so a grammar
invented under that pressure is the one that degenerates into `UNWIRED_MUTATIONS` again. The ratchet
that makes this safe is the codegen test `the_corpus_declares_no_deferral`: the corpus declares none,
and the FIRST deferral must fail that test and be argued for in the same change. The counter-argument
is recorded too — unused grammar rots, and if C3 lands without ever using it, deleting it is the
correct outcome, not a defeat.

## The gates that die, subsumed by the compiler

Per ADR-20260803-234035, *deleting a gate the compiler subsumes is a correct outcome*. The dispatch
delegated this judgement explicitly; the reasoning is recorded rather than assumed.

- **`UNWIRED_MUTATIONS` and its `assert_eq!` (`server_graphql.rs:876-889`) — DELETED.** The assert
  asked *"does this mutation have a row in the handler-call table?"*. After the cutover there is no
  handler-call table. The question it protected — *does an addressed message actually reach a
  handler?* — is answered by E0004 over a **strictly larger** set: all 100 commands some actor
  receives, not just the 90 reachable from a mutation. **The ten in that gap are precisely what the
  assert could not see, and two of them were live PM `sends:`.** Nothing survives it that the
  compiler cannot reach.
- **`wired_saga_command_dispatch` — DELETED**, and the second `assert_eq!` that #766 proposed is
  never born.
- **`wired_mutation_dispatch` — DELETED.** It held 90 handler CALLS as Rust source in **string
  literals**, which no compiler checked until the emitted file was built downstream. Those calls are
  ordinary source in `infrastructure::inbox` now.
- **`every_api_mutation_has_a_handler` — REPLACED** by
  `every_enqueueable_row_can_be_parsed_by_its_actor_inbox`, which asks what the compiler *cannot*:
  do the ENQUEUE side (`mailbox_address`, `ACTOR_INBOUND_FACTS`) and the CONSUME side agree on the
  same `(actor, message)` pairs? They are two independent scans of actors.yaml in two crates, and a
  row the door accepts but no inbox declares would abort delivery forever and poison the lane. That
  is a cross-artifact question, so it stays a gate.
- **The mutation resolver's wiring gate becomes ADDRESSING**, which is the right question: a resolver
  only ENQUEUES, so whether a handler exists says nothing about whether the resolver can be written.

**Two gates are ADDED, both where types cannot reach.** A catch-all arm still absorbs every future
variant, and no type can forbid one (`#[non_exhaustive]` does the opposite), so
`every_arm_of_the_human_owned_router_names_an_inbox_variant` reads the one human-owned file. **It
asserts the PROPERTY, not a spelling**: its first version line-scanned for `_ =>`, and the round-1
reviewer pass on PR #776 bypassed it with `_other =>` — a named binding is a wildcard wearing a
name, equally total, and it compiled clean with every gate green. The test now parses the file with
`syn` and asserts the positive form — every arm of a lane match names an `<Actor>Inbox::` variant,
and no arm anywhere is `Pat::Wild` or a bare `Pat::Ident` — plus its own reach, because a scanner
that silently matches nothing passes vacuously. The general lesson is worth more than the fix: *a
gate that names the spelling of a defect gates the spelling; the property has to be asserted against
the parsed artifact.* And
`a_widened_receives_set_is_a_compile_error` proves the whole guard RED against real `rustc` — it
emits Cart's inbox from a clone of the real model and from a model with one extra `receives:` entry,
compiles both against the same arm set, and asserts the control compiles CLEAN and the widened one
fails with `error[E0004]` naming the unconsumed message. A guard never seen red is an unverified
claim.

## What the guarantee does NOT cover

**It is over COMMANDS and over the routing DECISION — not over fact DELIVERY.** E0004 proves that
every message an actor declares it receives reaches a decision in `infrastructure::inbox`. A
decision of `InboxOutcome::RecordFact` then hands the message to the fact route in
`mailbox::handler`, which is still a `match` over the wire `message_type` **string** ending in
`_ => Failed("no delivery route for inbound fact type")` — and **twelve declared inbound facts have
an inbox variant and a `RecordFact` arm but no arm there** (`CartCheckedOut`, `OfferStockUpdated`,
`CustomerErasureDue`, `CustomerIdentityUnlinked`, `DeliveryDispatchFailed`, `DeliveryOfferTimedOut`,
`DeliveryRequested`, `PaymentCaptureFailed`, `PaymentIntentCreated`, `RefundApproved`,
`RefundDenied`, `RefundOpened`). That is pre-existing and unchanged by this decision, but the
guarantee must never be quoted unconditionally: the #595 shape survives one function downstream, on
the payments and refunds lanes among others. Extending the same proof to the fact route is
[#780](https://github.com/TheCaptainCompany/captain-food/issues/780). Raised by the independent
reviewer pass on PR #776.

## Consequence for [#358](https://github.com/TheCaptainCompany/captain-food/issues/358)

**Consumer-first deploy ordering is now a precondition of the runtime split**: consumers that
understand a message type must deploy before producers emit it. The transient-then-park posture makes
the wrong order *survivable* rather than *fatal* — it costs latency, a head-of-line-blocked lane and
an operator requeue, not a buried order — but it is not a licence to ignore the ordering, and *park*
means **terminal `FAILED` on the poison queue** (`DeliveryInfrastructureError`, ~5 min at cap 5 per
`STATUS.md` 10c), never a row that recovers itself. Recorded in the register row and in
`PROP-20260811-090000` §5 — where the #358 cutover work reads its preconditions — so a rollout
planner cannot read "parked" as "self-healing" (the round-1 reviewer pass on PR #776 found exactly
that misreading in the proposal's wording).

## Consulted

Per ADR-20260812-143619, a record created from a founder directive carries one line per lens; a lens
never asked must be distinguishable from a lens with nothing to say. The consult ran across 8 lenses
BEFORE the directive, and the checkpoint set was those who declared a concern.

- **vernon** — named the cross-actor hole precisely: `dispatch_command` takes `message_type` and
  never `actor_type`, so a lane-A row can drive a handler writing aggregate B under A's fence. The
  outer `ActorInbox` variant is the answer: the lane is the type, not a parameter.
- **young** — the wire stays a string and the enum is the in-process shape; nothing about
  `domain_events` or `inbound_messages` moves, so this is not an event-versioning change and no
  upcasting question is opened.
- **beck** — decisive on the division of labour, and on the acceptance criterion: the guard must be
  SEEN red from a mutated model, or the whole chunk is an unverified claim. Both halves of the E0004
  proof (control + mutation) are his; the control caught a scaffold defect on its first run.
- **farley** — whole-tree cutover, no per-lane flag: two live routers is the ambiguity being removed.
  Revert is the rollback because the change is compile-time with no stored shape.
- **dba** — nothing in the schema moves; the only stored-shape question raised (`PARKED`) was
  answered by *not* adding it, since the existing poison path already gives park semantics.
- **evans** — "inbox" is the spec's own word (`actors.yaml` `receives:`, the "two-layer inbox" note
  on `DeliveryJob`); no new coinage was needed and none was made.
- **holub** — nothing in this lens: the change removes a dependency (a string table) rather than
  adding one, and the router moves toward the layer that owns its ports.
- **evans/young jointly on `deferred:`** — a deferral is spec content because it is a statement about
  the product's declared surface, not about Rust.

**Not consulted**: no legal lens (no legal surface is touched); no design lens (no user-facing change).

## Consequences

- Adding a `receives:` entry now costs a decision in `crates/infrastructure/src/inbox.rs`. That is
  the intended cost, and it is the point: C3's twelve remaining `deliver:` routes (Payment ×7,
  DeliveryJob ×4, Cart ×1) are each an opportunity to add a route with no arm, and this makes that
  unspellable rather than merely gated.
- The generated `command_router.rs` shrinks from 761 lines to 42 — the addressing surface, nothing
  else.
- Ten previously-dead handlers become reachable. None is enqueued by anything live today except
  through the two declared PM `sends:` above, so no behaviour a customer sees changes on merge; what
  changes is that those two declared routes now work when C3 reaches them.
