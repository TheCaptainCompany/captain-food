# ADR-0032 — Business-rules layer + blocking spec-completeness gates

## Status

Accepted

## Context

The validator enforced referential integrity and behaviour-test *shape*, but two completeness gaps let the
spec silently drift:

1. **Test / story completeness was advisory.** `test-uncovered-message|event|error` were **warnings**, so a
   new command/event/error could ship with no test and still pass `validate`. And there was **no** check
   that every API mutation/query is anchored to a persona story — the story check was one-directional
   (steps must resolve), so ops could exist with no use case.
2. **Tests didn't say *what* they guarantee.** Reading `tests.yaml` shows Given/When/Then mechanics but not
   the business intent being verified — "reading the tests does not explain what rules we want to check."

## Decision

1. **New source file `specs/rules.yaml`** — a catalog of business rules/invariants, each a readable
   guarantee (keyed PascalCase name + `description`). Registered in `SOURCE_FILES` so it is `$ref`-able.
2. **Bidirectional rule↔test linkage, enforced (errors):**
   - Every test carries `rules: [{ $ref: 'rules.yaml#/<Rule>' }]` (≥1). Missing → `test-no-rule`; a ref not
     targeting rules.yaml → `test-rule-wrong-file`.
   - Every rule is asserted by ≥1 test. Orphan → `rule-uncovered`.
   A rule may span several tests (typically a happy path + its rejection), so rules stay coarse/readable
   while tests stay fine-grained.
3. **Completeness is now BLOCKING (promoted warning→error):** `test-uncovered-message`,
   `test-uncovered-event`, `test-uncovered-error` are **errors** — every actor message, emitted event and
   throwable error must be exercised by a test.
   **Amended 2026-09-04 (PR #875, #639 part C step 4-i)**: an error that a `tests.yaml` fixture
   CANNOT spell by construction — today only the rejection of a `readOnlyCatchAll` decode, which no
   closed-enum fixture can carry — is exercised by a named Rust test instead, under a declared
   `noTestFixturePossible: true` on the `errors.yaml` item whose legality the validator DERIVES
   (`error-exemption-unjustified`: every command throwing it carries a `readOnlyCatchAll` scalar);
   the gate is neither weakened nor bypassed — `test-uncovered-error` still fires the moment the flag
   is removed. Grammar: `docs/claude/dsl.md` § `noTestFixturePossible:`.
4. **Story completeness (new, error):** `op-uncovered-by-story` — every `api.yaml` mutation and query must
   be referenced by ≥1 story step, so the whole API surface anchors to a persona use case. Subscriptions
   are exempt (the story step model carries only query/mutation; a subscription is a transport variant of a
   query).
5. **`npm run validate` is the single gate for the WHOLE spec** — schema/refs, actor wiring, api↔model,
   views, C4, observability, **and now** tests, stories and rules. Its printed summary shows the rule +
   story-coverage lines and a `business rules` count so completeness is visible, not silent.

## Alternatives considered
- **Keep coverage as warnings** — the exact drift risk that prompted this; rejected.
- **One rule per test (1:1)** — redundant with test names; loses the "a rule spans several tests" value.
  Rejected in favour of coarse rules linked N:1 to tests.
- **Rules as free text inside tests.yaml** — not `$ref`-checkable, not reusable across tests, invisible as a
  catalog. Rejected for a first-class `rules.yaml`.
- **Making subscriptions story-mandatory** — the step model has no subscription opKind; forcing it would
  distort the story map. Exempted instead (documented).

## Consequences
### Positive
- A new command/event/error/mutation/query now **cannot** pass `validate` until it has a test AND a story
  step AND (via its test) a business rule — completeness is mechanically guaranteed, not remembered.
- `rules.yaml` is a readable, reviewable statement of what the system guarantees, cross-linked to its tests.
### Negative
- More upfront work per feature (author a rule, link the test, add a story step). This is the intended cost
  of the guarantee.
### Follow-up
- Render the rules ↔ tests cross-reference in the generated documentation (readable traceability). Deferred.

## References
`specs/rules.yaml`; `tools/codegen/src/validate.ts` (§6 story completeness, §7 rule linkage + promoted
coverage), `src/model.ts` (`SOURCE_FILES`), `src/cli.ts` (summary). Complements ADR-0007 (behaviour tests in
the DSL) and ADR-0010 (executable, blocking gates). CLAUDE.md "Non-negotiable rules" updated.
