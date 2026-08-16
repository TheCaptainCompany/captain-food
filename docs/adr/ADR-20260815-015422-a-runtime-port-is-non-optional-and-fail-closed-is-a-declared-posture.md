# ADR-20260815-015422 — A runtime port is non-optional, and a fail-closed default is a DECLARED posture, never a constructor fallback

## Status

Proposed — the prohibition is recorded here; the live defect it names is not yet fixed (see
Follow-up actions). Promote to Accepted when the composition roots stop being able to express it.

## Enforced by

n/a — no behavioral guarantee. This ADR constrains how a composition root is SHAPED, not what the
domain guarantees; the rule it states is a type-level one and its enforcement is the type system,
not a `rules.yaml` entry.

## Context

Found during the nine-lens mob checkpoint of
[#564 "Derive reader sets mechanically: a declared, walkable `reads:` grammar that distinguishes
source from shape"](https://github.com/TheCaptainCompany/captain-food/issues/564) — adjacent to the
work, not caused by it, and recorded here rather than fixed in that PR.

`crates/bin_runtime/src/lib.rs` declares two runtime ports as optional:

```rust
pub struct PmRuntime {
    /// The delivery-partner port (only DeliveryDispatchProcess declares it).
    pub partner: Option<Arc<dyn DeliveryService>>,
    /// The payment port (PlaceOrder/Refund/Reclamation declare it).
    pub payments: Option<Arc<dyn PaymentService>>,
}
```

and `crates/infrastructure/src/process_manager/runner.rs` supplies a default for each when the
option is `None`:

```rust
partner: Arc::new(NoopDeliveryService),
payments: Arc::new(crate::integrations::payments::FailClosedPaymentGateway),
```

Six composition roots construct `PmRuntime` (`crates/bins/pm-*/src/main.rs`). Four pass
`payments: Some(..)`; two pass `None` **correctly**, because their process manager has no payment
port at all. **Nothing distinguishes a correct `None` from a forgotten one.** On
`pm-payment-settlement`, changing `Some(payments.clone())` to `None` compiles, deploys, passes both
probes, and turns every capture into `PaymentGatewayRefused` — food delivered, money never
collected, at peak, with a green pod.

The failure is silent in a specific and worse-than-described way. The one WARN that names
`FailClosedPaymentGateway` (`bin_runtime/src/lib.rs:98`) belongs to the *declared* degraded path —
`STRIPE_SECRET_KEY` unset — and is emitted by `fail_closed_payments()`. A `None` in the struct never
reaches it: `spawn_pm_runtime` simply skips `with_payments`, the runner keeps its constructor
default, and **no log line is produced at all**. The operator sees a healthy bin.

Two existing rules already decide this and were not applied to a struct field:

- **Compiler first; a check is the fallback** (ADR-20260803-234035). `Option<T>` is the type system
  being told the field is optional. Nothing later can recover the distinction, because the
  distinction was deleted at the declaration.
- **A silent fallback is worse than the thing it replaces** (ADR-20260810-231300). That ADR is
  written about polling, but its reasoning is about *observability of degradation*, and a
  constructor default is the same defect with no transport in it.

## Decision

**1. A runtime port a component needs is NON-OPTIONAL in that component's construction type.** Where
different app classes need different ports — which is the real reason `Option` was reached for — the
answer is a type per app class (or a sealed enum of them), not one struct with holes. An app that
needs no payment port must not have a payment field to leave empty.

**2. A fail-closed stand-in is a DECLARED POSTURE, never a constructor fallback.** Fail-closed is
correct behaviour and stays; what is refused is *arriving at it by omission*. A composition root that
wants it says so, in a value that names itself, and the choice is visible in telemetry — the
`STRIPE_SECRET_KEY`-unset path (an explicit `fail_closed_payments()` with a WARN naming the binding
and the impl) is already the right shape and is the model to generalize.

**3. On a money path this is not a style preference.** The test for "is this field allowed to be
absent" is not "can the code proceed" — fail-closed code always proceeds. It is: *if this is absent
by mistake, what does an operator see?* Where the answer is "a healthy pod", the field may not be
optional.

## Alternatives considered

- **A validator/gate rule scanning composition roots for `None` on money ports.** Rejected under
  compiler-first: it is a source-text scanner over a boundary the type system can close outright, the
  exact shape [#329](https://github.com/TheCaptainCompany/captain-food/issues/329) spent seven review
  rounds and ~191 lines proving is the wrong level — every gap in that scanner was found by a
  reviewer rather than by the scanner.
- **Keep `Option` and require a WARN on the `None` branch.** Rejected: it makes the mistake
  *reportable* rather than *unspellable*, and a WARN on a correctly-`None` port (cart-binding,
  dispatch) trains operators to ignore the line that matters.
- **Keep `Option` and rely on the smoke suite.** Rejected: the smoke suite exercises the deployed
  configuration, so it proves the current wiring and says nothing about the next edit — and this
  defect's whole character is that it survives every check that is not a compile.

## Consequences

### Positive

- The forgotten-port class stops being expressible rather than becoming detectable.
- A fail-closed money path becomes readable from the composition root: *this bin declines captures,
  on purpose* is a sentence someone can find, rather than a default two crates away.
- Composes with #564's direction: PR2 derives per-app port sets from the spec, and a per-app-class
  construction type is what that derivation would naturally emit into.

### Negative

- More types than one struct with two `Option`s. That is the cost of the distinction and the reason
  it was avoided; ADR-20260808-235113 (final vision first) says to pay it rather than ship the shim.
- The change touches all six `pm-*` composition roots at once — a mechanical but wide diff.

### Follow-up actions

- **The live defect is unfixed.** `crates/bin_runtime/src/lib.rs:159,161` (the two `Option` fields)
  and `crates/infrastructure/src/process_manager/runner.rs:175-176` (the two constructor defaults)
  are the sites. Needs its own issue and its own PR; it is deliberately out of scope for
  [PR #566](https://github.com/TheCaptainCompany/captain-food/pull/566), whose diff is a spec grammar
  and its tests.
- When it lands, `partner` gets the same treatment. `NoopDeliveryService` is the *designed* stand-in
  for "no partner ACL configured" (`crates/application/src/ports.rs:114`) and it is better behaved
  than the payments default — it WARNs on every `offer_job` rather than only at wiring time — but it
  returns `Ok(())`, so it fails OPEN: the leg proceeds and the job is offered nowhere. Reaching that
  posture by forgetting a struct field is what this ADR refuses; declaring it stays correct.
