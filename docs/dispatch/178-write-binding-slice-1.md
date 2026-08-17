# Dispatch — enforcement, slice 1: generate the write binding for the bindable class

- **Issues**: [#178 "Write-side per-instance authorization"](https://github.com/TheCaptainCompany/captain-food/issues/178) · register §39 IDOR-1 · reads are [#618](https://github.com/TheCaptainCompany/captain-food/issues/618), **not this slice**
- **Base**: `main` @ `7debaf7` — **verify it.** Five cards in a row have carried a stale or wrong header, and the last one predicted itself.
- **Reversibility class**: **HIGH-CONSEQUENCE**, and **`HOLD: human`** — money path, authorization semantics, a legal surface. Stops at ready-for-review for the independent reviewer pass; no auto-merge.
- **Roster**: **full**, per the standing rule for money and legal surfaces. Five lenses already produced findings on this exact surface today and are being asked to confirm-or-update rather than start cold.

> **Founder directive, 2026-08-17**: *"the erasure is not required for going on production, we will deal with that later — what's important to me is the enforcement, the split and the order process."* Enforcement is first because it is unblocked, it is the largest single defect on the board, and it carries his own deadline: **the earliest of a second restaurant credential including a demo or pilot, a non-team rider credential, or the first real order.**

## The defect, at its measured size

**83 of 118 operations** carry no binding between the caller's verified identity and the thing acted on. `WriteScope` has zero hits repo-wide; the tenant-scope type is read by **zero** mutations. There is no test in either polarity — nothing would go red if this were fixed and later regressed.

The path-role guard authorizes on the URL path role, which is the declared design and works correctly. It answers *"is this caller a restaurant?"* It was never meant to answer *"is this caller **that** restaurant?"*, and its own doc comment says identity was a separate workstream. **So this is not a check written wrong; it is a check that was scheduled and never landed while the surface it guards shipped.**

The write side has no verified identity to compare against today: it re-derives the domain id with a **database lookup**, in the mailbox worker rather than the request, and **only for `CUSTOMER`**. Every other role gets `None`. Any claim that the handler already has the identity is false.

## Scope — slice 1 only

The write side divides three ways, and this slice takes exactly one of them.

| Class | Count | Shape | This slice? |
|---|---|---|---|
| **A — bindable** | 37 | a payload field **is** the caller's own scope scalar | **YES** |
| B — unbindable | 39 | no payload field corresponds to the caller at all | no — needs the folded-state check |
| N — role is the scope | 10 | admin/partner only | no |

**Deliver:**

1. **The validator rule, landed RED.** A mutation whose `roles:` include a bound role, whose command carries that role's principal id scalar, and which has no emitted binding → **error**. It enumerates class A mechanically, so the remaining set is a shrinking list rather than a sweep someone has to remember. **This is the durable half of the slice** — land it red first, then green it.
2. **The generated binding, at the request seam.** The spec already has everything: `specs/common/actors.yaml` declares each role's principal with its id scalar, and every command property is already a `$ref` to a dedicated scalar. **The scalar identity is the join** — derivable with **zero new DSL**. Emit `payload.P == claim(R)`; mismatch refuses.
3. **Behind a gate** — see below.
4. **The red test**, four shapes plus a positive case — see Evidence.

**Explicitly not in this slice**: class B's `requires.acting` work, the seven read doors (#618), removing ids from payloads, the `Lane`-style newtype for `Actor.domain_id`, and the partner-token non-attribution. Each is filed or named; none rides along.

## Why the seam, and why it is free

The seam is **the only place the verified claim exists** — by the time the command reaches the handler, the token is gone. The envelope carries the subject and the role text and no domain binding, and `inbound_messages` has columns for exactly those. So checking at the seam needs **no new column, no migration, no envelope change**.

And it makes roughly thirty already-written consistency checks **load-bearing for free**: `require_order` compares the order's stored owner against the supplied one, which becomes a sound authorization check the moment the supplied one is proven equal to the claim.

**At peak it costs nothing and saves something.** A bindable check is a comparison against a value already in the request context — zero I/O. Doing it properly also **deletes** the per-command database lookup the write path performs today.

**One arm is not free and must be priced, not assumed**: `RESTAURANT_ACCOUNT`'s claim is an account id while the payload carries a restaurant id. That is not claim equality — it needs account→location membership, one primary-key `EXISTS`, and it applies to roughly forty mutations including the whole catalog surface.

## Gate-then-stabilize — required, and its first setting is not "refuse"

Behaviour on a critical path ships behind a gate (founder-approved 2026-07-31). **First setting: count and log a mismatch, do not refuse.** Flipping to refuse is a **separate recorded decision** after the gated form has been smoked.

That is not caution theatre — it is the only way to discover whether any legitimate client sends a mismatched id before a refusal starts rejecting real traffic. The counter *is* the evidence for the flip.

**The flag must be flipped before the founder's trigger fires.** A gate left at count-and-log past that point is the same as not having done the work, and the deadline is published.

## Additive, and the deprecation half does not exist

Enforcing at the seam changes **no schema and breaks no client**: the payload field stays and simply stops being trusted. Removing it later is cosmetic hygiene on an already-safe surface — and removing an input field is a **hard break** that fails validation outright, which at Friday peak means a restaurant that cannot accept, i.e. a paid order nobody acts on.

Worth knowing before anyone plans that step: **`@deprecated` is not emitted anywhere** and there is no emitter support for it. The repo has no mechanism for the deprecation half of the evolution rule. Filed, not fixed here.

## Evidence

**The defect currently cannot be tested from a `tests/` directory**, and that is itself a finding: the principal type's constructors are all crate-private and the server crate declares no test feature, so a test can inject a *role* but never a bound *identity*. The cheapest honest red is **in-crate**.

Four shapes minimum plus a positive, because one is not a matrix:

- accept an order as a restaurant bound to a different restaurant → refused;
- mark delivered — **the capture trigger, so this is the money one**;
- a rider claiming a job as another rider;
- **a positive case**: the owning restaurant accepts its own order and **succeeds**. Without this arm, a deny-everything implementation ships green.

**Assert on the journal, not the response code**: the request is refused **before any row exists**. And the mutation that matters is *"guard present but runs after the append"* — if that stays green, the test is asserting the wrong thing.

Under the count-and-log setting the assertions invert: the mismatch is *recorded* rather than refused, and the test asserts the counter moved and the row still appeared. **Say which setting each test runs under**, or the suite will quietly prove the wrong configuration.

## Fences

- **No read-side changes.** #618 is sequenced with this but is not this diff.
- **No payload field removals.** Non-additive, and the deprecation mechanism does not exist.
- **Do not weaken the path-role guard** — it is correct and orthogonal.
- **No new `errors.yaml` code** without stopping first: catalogue membership is the rejected-vs-failed discriminator, so a new code flips a verdict. That cost this team a full re-scope yesterday.
- Every other defect found becomes an issue, not this diff.

## Findings

_(Lenses and the executor append here. "Nothing in my lens" is a complete answer, and so is an objection to the slice.)_
