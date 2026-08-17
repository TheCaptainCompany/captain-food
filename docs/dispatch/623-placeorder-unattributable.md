# Dispatch — a failed PlaceOrder is unattributable, and the safe-looking fix leaks

- **Issues**: [#623](https://github.com/TheCaptainCompany/captain-food/issues/623) (the discard) · [#625](https://github.com/TheCaptainCompany/captain-food/issues/625) (provider strings reach the journal row) · [#624](https://github.com/TheCaptainCompany/captain-food/issues/624) part 1 (`technical_error` unreachable)
- **Base**: `main` @ `6678236` — **corrected; the first card's header said `4077188`, which was stale.** All three lenses caught it. Its own antecedent rule, applied to its own header.
- **Reversibility class**: **HIGH-CONSEQUENCE** — money path, secrets handling, alert semantics. **Not `HOLD: human` as scoped below**, because nothing here changes a verdict, an outcome or what an alert *means*. The parts that would are explicitly deferred; see §Deferred.
- **Briefed**: `observability-agent`, `beck`, `young` — **all three declared a concern**, all three are at the checkpoint.

> **The first version of this card was wrong in five places and materially mis-scoped the work.** Read §0 first. Third card in a row with a wrong derived claim; all three were caught at briefing, which is the design working rather than failing.

## §0 — What the briefing changed

**The chunk is smaller than the card said, and the surrounding problem is much larger.**

| The card said | What the tree says |
|---|---|
| One `Internal` spans four seams — gateway rejection, timeout, append, cart read | **One live producer** of an *empty*-context `Internal`: the gateway's deterministic refusal. The other seams either carry `detail` already or land attributed as `DeliveryInfrastructureError{detail, attempts}` at the retry cap. (`young`; corroborated by `observability-agent` at 0.85 confidence, with `attempts = 0` as the discriminator.) |
| Missing call site **or** missing classification | **Neither, and also a third thing.** The classification is correct and the string is *in scope and explicitly discarded* — the sibling branch four lines above keeps it. Plus: the code it classifies, `PaymentGatewayRefused`, is **minted at five sites and declared nowhere**. |
| "The typed `context` shape is the fence" | **That fence does not exist.** No per-error context struct is generated or validated anywhere; `context:` in `errors.yaml` only names interpolation placeholders. An executor relying on it to keep a key out of a journal row would be relying on a document. |
| Consider distinct typed errors per seam | **Ruled out, and it would have broken this card's own fence silently.** `verdict_of_error` uses *catalogue membership* as the REJECTED-vs-FAILED discriminator, so declaring a new code **flips the verdict from FAILED to REJECTED**. (`young`.) |
| — | **The empty `{}` is the only branch on this path that is not leaking.** Provider message text — which for a bad key contains the key — already reaches `context.detail` on catalogued codes. The precedent-following fix leaks by construction. (`beck`, `observability-agent`, independently.) |
| — | **`technical_error` is structurally unreachable on this workflow**, and gateway failures are actively scored `business_rejected`. Filed as [#624](https://github.com/TheCaptainCompany/captain-food/issues/624). |

## Why this chunk, in one paragraph

A failed `PlaceOrder` records `{"code":"Internal","context":{}}` with nothing at ERROR or WARN. The diagnostic string exists — the Stripe adapter produces `PaymentGatewayRefused: … refused deterministically (HTTP 401, code '…')` — and is thrown away three lines from a branch that keeps it. But the obvious fix (widen `detail`, as ten sites already do) would write a Stripe message containing the API key into a jsonb column that persists 90 days and is served as `Operation.errorCode`. **So the fix and the safety have to land together, or each one makes the other worse.**

## Scope — four items, in this order

**1. The leak canary, landed RED first.** Configure a gateway stub with a key-shaped literal `sk_test_LEAK_CANARY`; feed a **recorded** 401 body through the **real** adapter decoder (not a live call, and not a hand-written error string); assert the journal row's `error` JSON contains no `sk_`. **This goes red today on the `PaymentDeclined` path.** Land it red, then green it. It has standing value independent of everything else here.

**2. A bounded attribution type, so a provider body is unspellable.** A small constructor in the mailbox crate whose fields are the only permitted attribution — `seam`, `reason`, and an optional gateway status — serialised into `context`. Both `seam` and `reason` come from **closed sets declared as scalars and `$ref`'d**, per the bounded-population rule. Free text goes to the log, never to `error`/`context`. This is the compiler-first floor; the full typed gateway-failure enum is deferred.

**3. Populate the discard.** With (2) in place, the `Invariant` non-catalogued arm records which seam failed and why, instead of `{}`. **Do not declare a new error code to do it** — see §0.

**4. `otel.status_code` declared at construction on the payment span and set on `Err`, plus the validator rule that would have caught its absence.** The rule: *every emitted contract declaring `technical_error.any_span_errors: true` must have at least one declared span constructing `otel.status_code` and at least one recorder setting it to `ERROR`.* Pure text check, same shape as the existing contract test. **Land it red — it fails today on `place-order`** — with the instrumentation in the same PR. Precedent for the span guard already exists in the same file for two other spans.

## Deferred — do not do these here

- **Re-classifying gateway failures out of `business_rejected`** ([#624](https://github.com/TheCaptainCompany/captain-food/issues/624) part 2). That changes what the error budget and the refusal rate *mean* to a human reading them — `HOLD: human`, its own chunk, right after this one.
- The create-leg `reason` classifier for `checkout_payment_failures_total` (#624), the typed gateway-failure enum (#625), and declaring `PaymentGatewayRefused` / `DeliveryInfrastructureError` in the catalogue — the last one is a verdict flip and must not ride along.
- **Diagnosing leg 6 of the walk.** This chunk is what makes that diagnosable. If the fix incidentally reveals the cause, file it and say so.

## Evidence

`beck` specified the set; take it from the briefing, with these three as required:

- **M1 (the one that matters) — collapse two seams onto one context.** One-token edit: label the read seam the same as the gateway seam. Expected red: *"two seams, one context: the failure is recorded but not attributable"*.
  **This is why the two seams must be ONE test.** Split into two tests each asserting its own constant and **both stay green under M1** — the mutation that restates the actual defect becomes unobservable. Splitting them *is* the vacuity.
- **M2 — restore `"context": {}`** at the discard site. Expected red naming the discarded detail. The floor, not the proof.
- **M3 — the drift.** Mutate the adapter's error prefix and report **what goes red**. `beck` predicts nothing does. If nothing does, that is the finding, and the compiler-first answer is a shared const or newtype so an undeclared prefix is unspellable.

Assert on the context's **discriminating power**, never its prose: distinctness; a declared key with a closed-set value; the gateway status present on one arm and **absent** on the other; and the canary. Four independently falsifiable claims.

## Fences

- **No verdict, outcome, status or customer-facing message changes.** A command that fails today still fails, identically, with the same retry behaviour and the same generic apology. This chunk changes the record and the instrumentation.
- **No new `errors.yaml` code.** It would flip a verdict. If you believe one is needed, stop and say so.
- **The log.** The card previously said "a log line at the boundary" while also fencing off the mailbox runtime — `beck` caught the contradiction, because the verdict-blind DEBUG line lives in `actor_runtime`. **Resolution: you may add a `tracing::error!` at the discard site in `handler.rs`. You may not change the worker's logging.** Making the verdict channel itself loud is the correct larger fix and it is a separate chunk.
- **`specs/**` in scope for**: `observability.yaml`, and the scalars that declare the `seam`/`reason` closed sets. Each with its `SPEC-LOG.md` sentence in the same commit. `errors.yaml` only if you are *not* adding a code.
- Every other defect found becomes an issue, not this diff.

## One divergence to settle at the keyboard, not by vote

The three lenses read the `Repository` arm three subtly different ways: `young` called it dead (intercepted before `verdict_of_error` on the command arm), `beck` proposed driving a test through it, `observability-agent` called it unreachable *for PlaceOrder* specifically because the PM path returns it as `Err(sqlx)` earlier. These reconcile — **it is intercepted on the command arm and terminal on the process-manager arm** — but the reconciliation is a claim, not a measurement. **Establish it, and write down which path you drove.** If the read seam turns out to be undrivable, M1 needs a different second seam and you should say so rather than weaken the mutation.

Also worth knowing at the keyboard: the append seam is **not** injectable — the commit flush takes a `Transaction`, not the port — so "drive the append to fail" is not in the same cost class as the gateway. Cut it rather than building fault-injection machinery.

## Findings

_(Lenses and the executor append here.)_

- **`young`** — reading the diff for: no new `errors.yaml` code; the mutation targeting the real discard site; and the attribution type making a raw body unspellable at the type level rather than by convention.
- **`beck`** — the two seams as one test, and the canary assertion present in the diff rather than considered.
- **`observability-agent`** — the contract-violation scope, and any `context` shape landing adjacent to the live leak.

### Executor, claim time — the `Repository` divergence, read from source (measurement to follow)

The card's reconciliation ("intercepted on the command arm and terminal on the process-manager
arm") does **not** survive reading the tree. `verdict_of_error` has exactly three call sites, and
`DomainError::Repository` is intercepted before **every one** of them:

| call site | the interception above it |
|---|---|
| `handler.rs:228` (in-tx command arm) | `handler.rs:218` `Some(Err(DomainError::Repository(detail))) => return Err(sqlx::Error::Protocol(detail))` |
| `handler.rs:282` (PM arm — the `PlaceOrder` arm) | `pm_delivery.rs:194`, inside `prepare`: `Err(DomainError::Repository(detail)) => return Err(sqlx::Error::Protocol(detail))`, so `PreparedPmCommand.outcome` can never hold a `Repository` |
| `handler.rs:841` (PM fact arm) | `handler.rs:838`, same shape |

So the `DomainError::Repository(_)` arm of `verdict_of_error` (`handler.rs:865`) is **dead on every
live path**, not merely on the command arm — `observability-agent` was closest, but the interception
that kills it for `PlaceOrder` is in `prepare`, not "the PM path returns it as `Err(sqlx)` earlier"
in general. **M1 therefore needs a different second seam**; see the next finding.

### Executor, at the checkpoint — what was measured

**1. The `Repository` divergence: none of the three readings holds, and the card's reconciliation is
wrong.** Measured above at claim time and unchanged by the work: `DomainError::Repository` is
intercepted above **all three** `verdict_of_error` call sites, and the interception that kills it for
`PlaceOrder` is `pm_delivery.rs:194` inside `prepare`, not anything in `handler.rs`. The arm is dead
on every live path. **M1 therefore drives `COMMAND_PAYLOAD` as its second seam** — a journaled
payload that does not decode into the deployed command shape (`pm_delivery.rs`'s three arms), which
is live, terminal, and needs a genuinely different operational response from a gateway refusal. The
mutation is undiminished: it is still a one-token relabel and it is still red.

**2. M3: the prefix had ZERO consumers, which is stronger than "nothing goes red".** `beck` predicted
nothing would. Renaming `PaymentGatewayRefused` in the adapter left `crates/application`'s 363 unit
tests green; the only three failures were the adapter's own tests, which restate the literal they
mint. Nothing anywhere matched it — `classify_capture_error` falls through to `GATEWAY_REFUSED`
without ever spelling it, and `verdict_of_error` only asks the catalogue. So it was not a coupling
that a shared const would protect; it was prose sitting in a durable row. **This change is what gives
it machine meaning**, so the shared builder/reader pair lands in `application::ports` next to
`VERSION_CONFLICT_PREFIX`, with the HTTP status encoded AFTER the first colon so `rejection_code`
keeps working the day #625 catalogues the prefix. That ordering is itself a pinned test.

**3. The canary was red on exactly one of three arms, and it was not the one #623 reports.**

| recorded body | before | after |
|---|---|---|
| HTTP 401, invalid API key | `{"code":"Internal","context":{}}` | `{seam: PAYMENT_GATEWAY, reason: GATEWAY_REFUSED, gatewayStatus: 401}` |
| HTTP 400, `parameter_missing` | `{"code":"Internal","context":{}}` | same shape, `gatewayStatus: 400` |
| HTTP 402, `card_declined` | **`{"code":"PaymentDeclined","context":{"detail":"…sk_test_LEAK_CANARY…"}}`** | `{"code":"PaymentDeclined","context":{}}` |

So the empty `{}` was the only branch on this path that was NOT leaking, exactly as §0 said — and
the leak lives on the arm the card did not scope. Dropping `detail` there changes no customer-facing
message: **no `errors.yaml` message template in any scope fragment interpolates `{detail}`** (checked
across all eight fragments), so `message_en(code, context)` renders identically.

**4. A consequence worth stating plainly: #625's "declare `PaymentGatewayRefused` in the catalogue"
would, on today's code, have turned the REAL 401 body into a REAL key in a 90-day row.** It routes
the 401 down the arm that copies `message` verbatim. The canary is now what makes that land red
instead of landing quietly, and `journal_attribution.rs` asserts the code is still uncatalogued.

**5. `§21` found its own defect on its first run.** Splitting `spans.rs` on `pub fn` runs each block
into the NEXT function's doc comment, and this file's doc comments discuss `otel.status_code` at
length — so the rule certified `cart.read` because `cart_price`'s doc comment explains why IT
declares the field. A text rule that reads prose as instrumentation certifies exactly the contracts
whose documentation is most conscientious. Fixed, and kept as a test.

### Three defects found and NOT fixed here (issues, not this diff)

- **`classify_capture_error` puts 500 characters of the provider's message into a STORED EVENT.**
  `payment_settlement.rs:160` → `PaymentCaptureFailed.detail`, in `domain_events`, immutable and
  forever. Same leak class as this chunk, one tier worse, and squarely `HOLD: human` (stored event
  shape). **This is the most serious thing found today.**
- **Three sibling `context: { detail: e.to_string() }` arms** in `handler.rs` (the staged-flush
  failures, lines ~208/309/820) still write free text into the journal row. Not on the canary's path
  and not scoped by the card.
- **The GENERATED command router classifies an undecodable payload as `DomainError::Repository`**
  (`command_router.rs`), i.e. RETRY FOREVER, while the hand-written PM prepare correctly makes the
  same seam terminal. A crafted payload could wedge a lane. Changing it moves retry behaviour, which
  this card's fence forbids.

### The checkpoint question, banked

**Did the three-lens set miss anything a wider roster would have caught?** One item, and the
attribution is **card defect**, not roster width.

`vernon`/`evans` were not at the briefing and neither is the lens that would have caught it. What was
missed is that **the card scoped the leak to the branch that was not leaking**: §0 states the
catalogued arm already leaks, and §Scope then asks only for the discard site to be populated, with
the canary as evidence rather than as work. The greening of the canary — dropping `detail` on the
`PaymentDeclined` arm — is a change to a REJECTED row's shape that no scope item names, and it was
discovered at the keyboard by running the canary rather than by reading the card. A wider roster
reading the same card would have read the same omission. The fix is the card's, not the roster's:
**a scope list must name the change each piece of evidence will force**, not only the evidence.

The three declared concerns were all met by the diff, and each lens's named artifact exists:
`young`'s unspellability is a generated type rather than a convention, `beck`'s two seams are one
test with the distinctness assertion that makes splitting it vacuous, and
`observability-agent`'s contract-violation scope is a validator rule that went red on `place-order`
before the instrumentation landed.
