# ADR-20260804-154700 — Screen actions are checked against their command's inputs

## Status

Accepted

## Context

Asked whether anything declared the gap between screen form inputs and mutation input properties, the
answer was **no**. The full rule inventory covered only reference hygiene:

- `action-not-a-mutation` proves an action's `$ref` names a real `api.yaml` mutation. It never reads the
  action's `variables`.
- `op-uncovered-by-story` proves every mutation is reached by ≥1 story **step**. A step is not a screen.
- For queries, `validate_resolver_args` carries an explicit note that required-arg coverage is
  deliberately NOT checked — correct there, because a pinned `arg:` is a static DEFAULT the runtime
  merges caller variables over.

Nothing looked inside a mutation action's `variables`. That combination let a form be declared, story-
covered and validator-green while being **unsubmittable**, invisible until a human pressed the button.

`updateRestaurant` was the case that exposed it: reached by two story steps, present in **zero** screen
files, and therefore the reason `Restaurant.description` sat as a column no event fed — the mutation that
could set it had no form.

## Decision

Two rules, both **warnings**, both walking the screen component tree (which the validator did not walk at
all before):

- **`action-missing-required-input`** — a component's `action.variables` supplies no value for a required
  property of the bound command. A screen action is the CALLER, so its variables are the whole input;
  this is the opposite judgement from `validate_resolver_args`, and deliberately so.
- **`action-unknown-input`** — a variable naming no property of the command. The write-side mirror of
  `resolver-unknown-arg`: the value is dropped on the floor while the spec reads as if it were wired.

Warnings, not errors, because they found **17 pre-existing violations** on their first run. A gate that
fails the build on inherited debt gets weakened rather than paid down.

Also landed here: the **restaurant profile screen** (`/settings/profile`), which wires `updateRestaurant`
and is what makes `Restaurant.description` reachable by a human at all.

## Alternatives considered

- **Make them errors and fix all 17 first.** Rejected as one change: several fixes are decisions, not
  typos — where a rider's `riderId` comes from (the session principal) is an auth-model question, and
  `place_order`'s missing `orderId`/`customerContact`/`serviceType` touch checkout. Bundling them would
  have hidden the rule behind a large speculative diff.
- **A rule requiring every mutation to appear on some screen.** Rejected for now: many V1 mutations have
  no surface yet by design, so it would be noise against the same backlog `command-no-mutation` already
  tracks from the other side.
- **Prose in the screens README.** Rejected — the thing being fixed IS an undeclared convention, and the
  repo's own rule is that a validator rule beats a bullet point.

## Consequences

### Positive
- The gap is executable rather than folklore. 17 genuinely broken wirings are now visible, including the
  rider's Accept button, which passes an `orderId` that `AcceptDelivery` does not declare **and** omits
  both of its required inputs — that screen's primary action cannot work.
- `Restaurant.description` is reachable: the profile screen exists and supplies every required input, and
  the test asserts precisely that, so the screen cannot silently regress.
- The validator now walks screen component trees, which future screen-level rules can reuse.

### Negative
- **The warning baseline moves 26 → 43** (+10 `action-missing-required-input`, +7 `action-unknown-input`).
  This is a deliberate new-rule baseline change, not drift: compare against 43 from here.
- The rules read `commands.yaml` `required`/`properties` directly and so cannot see a value the client
  supplies implicitly (a route param, the session principal). Some of the 17 may resolve as "the runtime
  fills it" — in which case the fix is to make that explicit in the spec, which is the point.
- The profile screen carries four `gaps`: no `restaurantById` query (it reads
  `restaurantLocationsByAccount`, same as the storefront screen), and `openingHours` / `contact`+`address`
  / `marginRate` deliberately off the form. `openingHours` is off because a weekly slot editor is a
  repeatable control the SDUI set does not have, and a half-built one would silently drop slots on save.

### Follow-up actions
- [#342](https://github.com/TheCaptainCompany/captain-food/issues/342) tracks the 17 findings.
- **Closed in the same change**: `recordDeliverySatisfaction` and `escalateDelivery` were generating
  `Err("not implemented")` resolver bodies with no router arm while every spec gate stayed green. Both
  handlers already existed in `application::commands` — only the emitter's dispatch-table row was
  missing. Both are wired, and the omission is now impossible: the emitter asserts that the set of
  mutations reaching the stub arm equals an explicit `UNWIRED_MUTATIONS` allowlist (currently empty), so
  an unwired mutation FAILS generation rather than shipping a stub. Deliberately a generation-time
  assertion rather than a validator rule or a source scan: the table lives in the emitter, so no
  `specs/**` check can see it, and scanning generated Rust for `not implemented` would be #329 again.
