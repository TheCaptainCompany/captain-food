# PROP-20260808-221424 — Rider/delivery slices 1–2: the exact spec diff (vocabulary retirement + PM-send credit)

- **Status**: Approved (customer, 2026-08-08, live in session — §2 approved as written and applied to `main` by the run per [ADR-20260808-230800](../adr/ADR-20260808-230800-rider-delivery-slices-1-2-approved-and-applied.md); §3.2 approved, lands with the D6 validator mechanism)
- **Date**: 2026-08-08
- **Parent proposal**: [PROP-20260808-141817 "The rider/delivery write surface: journeys, vocabulary
  verdict, and V0 slices"](PROP-20260808-141817-rider-delivery-write-surface.md) (Approved 2026-08-08;
  this document realizes its slices 1–2 only)
- **Tracking issue**: [#348 "Epic: the rider/delivery write surface does not exist (24 of main's 32 validator warnings)"](https://github.com/TheCaptainCompany/captain-food/issues/348)
- **Decisions applied** (settled — [DECISIONS.md §20](DECISIONS.md), ADR-20260808-155656; not reopened
  here): D1/D2 retire the second vocabulary; D6 declared `sends:` for the wrapper-seam dispatch. D3/D4/D5
  land in later slices and are untouched by this diff.
- **Author**: architect agent, session https://claude.ai/code/session_01CHREdBUBbUgT9HNyhkXSF7

---

> ## APPROVED AND APPLIED (2026-08-08)
>
> `specs/**` is frozen for autonomous loops (CLAUDE.md, non-negotiable); this document was the
> prepared exact diff, and the customer approved it **as written, live in session**, choosing
> immediate application by the run over the §6 plan-mode vehicle — authorization recorded in
> [ADR-20260808-230800](../adr/ADR-20260808-230800-rider-delivery-slices-1-2-approved-and-applied.md).
> §2 is applied to `main`; §3.2 lands with the D6 validator mechanism. This exception rides an
> exact-text approval and is no precedent for unapproved spec edits.

**Why now — the retirement window is closing.** Every line deleted below describes an event type of
which **zero instances exist in any production log**. Today, retiring
`DeliveryAssignedToPartner`/`DeliveryPartnerStatusUpdated` is a pure spec deletion: `git revert`
restores it byte-for-byte if the verdict is ever wrong. The moment the platform goes live and a
single event of a retired type is appended to a production `domain_events`, deletion stops being an
option — stored events are immutable contracts, and removing their type becomes an
upcasting/migration project with its own design, testing and replay burden (the GDPR
tombstone-then-stream-deletion path, ADR-20260731-160000, is the one recorded exception and not a
precedent). The same clock runs on the slice-6 D3 rename. **This is why slices 1–2 are sequenced
before any surface work: they are free now and expensive forever after.**

## 1. Scope of this document

Exactly the parent proposal's slices 1–2, nothing else:

- **Slice 1 — `delivery-vocabulary-cleanup`** (decisions D1/D2): retire the
  `AssignDeliveryToPartner`/`DeliveryAssignedToPartner` and
  `UpdateDeliveryPartnerStatus`/`DeliveryPartnerStatusUpdated` families; declare
  `CustomerIdentified` and `PaymentFailed` as `nonProjectedEvents` (category a). §2.
- **Slice 2 — `validator-credits-pm-sent-commands`** (D6's spec half): the declared `sends:` shape
  for the wrapper-seam dispatch; the credit for step-derived PM sends needs **no spec change at
  all**. §3.

Per proportionality (docs/proposals/README.md): this child document opens **no new option space** —
every decision it applies was arbitrated in the parent (per-option pros/cons in parent §4, sequence
diagrams §5a, mockups §5b). It exists because the parent recorded *verdicts* and the customer must
approve the *exact spec text* that realizes them.

Every `file:line` below was verified by grep + read on a clean `origin/main` worktree, 2026-08-08.
The retired names occur in exactly **four source files** (`specs/delivery/events.yaml`,
`specs/delivery/commands.yaml`, `specs/delivery/actors.yaml`, `specs/tests.yaml`) plus generated
artifacts. **Zero occurrences** in `api.yaml`, `stories.yaml`, `specs/screens/**`, `rules.yaml`
(names), `errors.yaml`, or any `processmanager.yaml` — the families are spec-complete but
surface-dead, which is the whole case for retiring them.

## 2. Slice 1 — the exact diff, per file

### 2.1 `specs/delivery/commands.yaml` — delete two command payloads

Delete `AssignDeliveryToPartner` (lines 125–131 + separator) and `UpdateDeliveryPartnerStatus`
(lines 146–155 + separator). `UnassignDeliveryFromPartner` (lines 134–143) **stays untouched** —
it is renamed by slice 6 (D3), not retired.

```diff
--- specs/delivery/commands.yaml
@@ (after UpdateDeliveryStatus, line 122)
-AssignDeliveryToPartner:
-  description: "Assign a pending delivery job to a delivery partner for fulfilment."
-  type: object
-  properties:
-    deliveryJobId: { $ref: 'scalars.yaml#/DeliveryJobId' }
-    partnerRef: { $ref: 'scalars.yaml#/ExternalReference' }
-  required: [deliveryJobId, partnerRef]
-
-
 UnassignDeliveryFromPartner:
   description: "Unassign a delivery job from its partner (to re-offer it)."
   ...unchanged...
   required: [deliveryJobId]
-
-
-UpdateDeliveryPartnerStatus:
-  description: "Apply a partner-reported status change to the delivery job (from the avelo37-acl inbound report)."
-  type: object
-  properties:
-    deliveryJobId: { $ref: 'scalars.yaml#/DeliveryJobId' }
-    partnerRef:
-      $ref: 'scalars.yaml#/ExternalReference'
-      nullable: true
-    status: { $ref: 'scalars.yaml#/DeliveryStatus' }
-  required: [deliveryJobId, status]
```

### 2.2 `specs/delivery/events.yaml` — delete two event payloads

Delete `DeliveryAssignedToPartner` (lines 225–233) and `DeliveryPartnerStatusUpdated` (lines
248–260). `DeliveryUnassignedFromPartner` (lines 236–245) **stays untouched** (slice 6).

```diff
--- specs/delivery/events.yaml
@@ ("Delivery — operational facts" section, line 221)
-DeliveryAssignedToPartner:
-  description: "A delivery job was assigned to a delivery partner (e.g. Avelo37) for fulfilment."
-  type: object
-  properties:
-    deliveryJobId: { $ref: 'scalars.yaml#/DeliveryJobId' }
-    partnerRef:
-      $ref: 'scalars.yaml#/ExternalReference'
-      description: "Partner-side delivery id; idempotent key for inbound updates."
-  required: [deliveryJobId, partnerRef]
-
-
 DeliveryUnassignedFromPartner:
   ...unchanged...
-
-
-DeliveryPartnerStatusUpdated:
-  description: "A partner-driven status change applied to the job by the DeliveryJob aggregate (from the inbound partner report)."
-  type: object
-  properties:
-    deliveryJobId: { $ref: 'scalars.yaml#/DeliveryJobId' }
-    partnerRef:
-      $ref: 'scalars.yaml#/ExternalReference'
-      nullable: true
-    status: { $ref: 'scalars.yaml#/DeliveryStatus' }
-    occurredAt:
-      type: string
-      format: date-time
-  required: [deliveryJobId, status]
```

(Note in passing, for slice 6 or the parent's follow-ups, NOT this diff: the surviving
`DeliveryPartnerStatusUpdated` payload carried an `occurredAt` field, which the envelope doctrine
says never belongs in a payload — deleting the event also deletes the repo's last payload-level
`occurredAt` in the delivery scope.)

### 2.3 `specs/delivery/actors.yaml` — 7 lifecycle edges, 2 inbox entries, 1 comment

Four hunks on the `DeliveryJob` actor:

**(a)** Delete the `DeliveryAssignedToPartner` edge (line 60). The adjacent
`DeliveryUnassignedFromPartner` edge (line 61) **stays**:

```diff
       - { from: [PENDING], event: { $ref: 'events.yaml#/DeliveryAcceptedByPartner' }, to: ASSIGNED }
-      - { from: [PENDING], event: { $ref: 'events.yaml#/DeliveryAssignedToPartner' }, to: ASSIGNED }
       - { from: [ASSIGNED], event: { $ref: 'events.yaml#/DeliveryUnassignedFromPartner' }, to: PENDING }
```

**(b)** Delete the six `DeliveryPartnerStatusUpdated` dynamic-target edges (lines 73–78):

```diff
       - { from: [PENDING, ASSIGNED, PICKED_UP, OUT_FOR_DELIVERY], event: { $ref: 'events.yaml#/DeliveryStatusUpdated' }, to: FAILED, via: status }
-      - { from: [PENDING], event: { $ref: 'events.yaml#/DeliveryPartnerStatusUpdated' }, to: ASSIGNED, via: status }
-      - { from: [ASSIGNED], event: { $ref: 'events.yaml#/DeliveryPartnerStatusUpdated' }, to: PICKED_UP, via: status }
-      - { from: [PICKED_UP], event: { $ref: 'events.yaml#/DeliveryPartnerStatusUpdated' }, to: OUT_FOR_DELIVERY, via: status }
-      - { from: [PICKED_UP, OUT_FOR_DELIVERY], event: { $ref: 'events.yaml#/DeliveryPartnerStatusUpdated' }, to: DELIVERED, via: status }
-      - { from: [PENDING, ASSIGNED, PICKED_UP, OUT_FOR_DELIVERY], event: { $ref: 'events.yaml#/DeliveryPartnerStatusUpdated' }, to: CANCELLED, via: status }
-      - { from: [PENDING, ASSIGNED, PICKED_UP, OUT_FOR_DELIVERY], event: { $ref: 'events.yaml#/DeliveryPartnerStatusUpdated' }, to: FAILED, via: status }
     terminal: [DELIVERED, CANCELLED]
```

**(c)** Delete the two inbox entries — `AssignDeliveryToPartner` (lines 141–146, with its three
`throws` mappings) and `UpdateDeliveryPartnerStatus` (lines 153–157, with its two `throws`
mappings). The `UnassignDeliveryFromPartner` inbox entry between them (lines 147–151) **stays**:

```diff
-    - message: { $ref: 'commands.yaml#/AssignDeliveryToPartner' }
-      emits: [{ $ref: 'events.yaml#/DeliveryAssignedToPartner' }]
-      throws:
-        - { $ref: 'errors.yaml#/DeliveryJobNotFound' }
-        - { $ref: 'errors.yaml#/InvalidDeliveryStatus' }   # must be a PENDING job
-        - { $ref: 'errors.yaml#/DeliveryAlreadyAssigned' }
     - message: { $ref: 'commands.yaml#/UnassignDeliveryFromPartner' }
       emits: [{ $ref: 'events.yaml#/DeliveryUnassignedFromPartner' }]
       throws:
         - { $ref: 'errors.yaml#/DeliveryJobNotFound' }
         - { $ref: 'errors.yaml#/InvalidDeliveryStatus' }   # must be an ASSIGNED job
-
-    - message: { $ref: 'commands.yaml#/UpdateDeliveryPartnerStatus' }
-      emits: [{ $ref: 'events.yaml#/DeliveryPartnerStatusUpdated' }]
-      throws:
-        - { $ref: 'errors.yaml#/DeliveryJobNotFound' }
-        - { $ref: 'errors.yaml#/InvalidDeliveryStatus' }   # must be a valid status transition
     - message: { $ref: 'commands.yaml#/DeclineDelivery' }
```

**(d)** Rewrite the lifecycle comment (lines 46–51) that names the retired event — a spec comment
claiming a capability the catalog no longer has is worse than no comment:

```diff
-  # PENDING → ASSIGNED → PICKED_UP → OUT_FOR_DELIVERY → DELIVERED, with the PICKED_UP → DELIVERED
-  # hand-over shortcut; CANCELLED/FAILED close the job early. DeliveryStatusUpdated (rider/admin
-  # correction) and DeliveryPartnerStatusUpdated (partner report) carry their target state in the
-  # payload — the DSL's dynamic-target form (`via: status`, ADR-20260721-093027): one event, one
-  # declared edge per target. FAILED is NOT terminal: a failed dispatch can still be manually
-  # cancelled (rules.yaml#/DeliveryCancellableBeforeCompletion), but no status report leaves it.
+  # PENDING → ASSIGNED → PICKED_UP → OUT_FOR_DELIVERY → DELIVERED, with the PICKED_UP → DELIVERED
+  # hand-over shortcut; CANCELLED/FAILED close the job early. DeliveryStatusUpdated — the ONE
+  # status vocabulary: emitted by the UpdateDeliveryStatus command (rider/admin correction) AND
+  # recorded directly as the inbound partner report via the avelo37 ACL — carries its target state
+  # in the payload — the DSL's dynamic-target form (`via: status`, ADR-20260721-093027): one event,
+  # one declared edge per target. FAILED is NOT terminal: a failed dispatch can still be manually
+  # cancelled (rules.yaml#/DeliveryCancellableBeforeCompletion), but no status report leaves it.
```

### 2.4 `specs/tests.yaml` — 2 fixtures, 2 tests deleted; 1 surviving test rewired

Delete the two fixtures whose `type:` refs would dangle (lines 475–477, 481–483):

```diff
-  deliveryAssignedToPartner:
-    type: { $ref: 'events.yaml#/DeliveryAssignedToPartner' }
-    data: { deliveryJobId: "deliv-1", partnerRef: "avelo-77" }
   deliveryUnassignedFromPartner:
     ...unchanged...
-  deliveryPartnerStatusUpdated:
-    type: { $ref: 'events.yaml#/DeliveryPartnerStatusUpdated' }
-    data: { deliveryJobId: "deliv-1", partnerRef: "avelo-77", status: "PICKED_UP" }
```

Delete the two behaviour tests of the retired commands — `TestDeliveryAssignedToPartner` (lines
2867–2877) and `TestDeliveryPartnerStatusUpdated` (lines 2892–2903).

**Rewire `TestDeliveryUnassignedFromPartner`** (line 2885) — this test SURVIVES (its command is
renamed in slice 6, not retired), but its `given:` uses the retired `deliveryAssignedToPartner`
fixture to reach ASSIGNED. Replace it with the canonical acceptance fact
`deliveryAcceptedByPartner` (fixture line 455; same `partnerRef: "avelo-77"`, same
PENDING→ASSIGNED edge, `actors.yaml:59`):

```diff
   TestDeliveryUnassignedFromPartner:
     name: "An assigned delivery job is unassigned from its partner to be re-offered"
     rules: [{ $ref: 'rules.yaml#/DeliveryPartnerAssignmentLifecycle' }]
     actor: { $ref: 'actors.yaml#/DeliveryJob' }
     given:
       - { $ref: '#/fixtures/deliveryRequested' }
-      - { $ref: '#/fixtures/deliveryAssignedToPartner' }
+      - { $ref: '#/fixtures/deliveryAcceptedByPartner' }
     when:
       type: { $ref: 'commands.yaml#/UnassignDeliveryFromPartner' }
       data: { deliveryJobId: "deliv-1", reason: "Re-offering to another channel" }
     then:
       - { $ref: '#/fixtures/deliveryUnassignedFromPartner' }
```

This rewrite is also semantically MORE correct: after D1, acceptance is the only way a partner job
ever becomes ASSIGNED, so the test now exercises the real precondition path.

### 2.5 `specs/delivery/rules.yaml` — reword one rule description

`DeliveryPartnerAssignmentLifecycle` (lines 39–40) keeps two verifying tests after the deletions
(`TestDeliveryJobRecordsPartnerStatusReport`, `TestDeliveryUnassignedFromPartner`), so the
bidirectional rule↔test gate (ADR-0032) holds with no structural change — but its description
still guarantees "can be assigned to a delivery partner (once)", a capability this diff retires.
Description-only reword; the rule NAME is kept (renaming it belongs to slice 6's D3 sweep, which
touches the same two test `rules:` refs anyway):

```diff
 DeliveryPartnerAssignmentLifecycle:
-  description: "A PENDING delivery job can be assigned to a delivery partner (once), unassigned to be re-offered, and partner-reported status changes apply only as valid transitions."
+  description: "A delivery job ASSIGNED to a partner (by the partner's acceptance — the only assignment path) can be unassigned to be re-offered, and partner-reported status changes apply only as valid transitions."
```

### 2.6 `specs/database/projection_views.yaml` — two `nonProjectedEvents` declarations

Both category (a) per the file's own taxonomy (lines 41–47). Insert after the `RefundRequested`
entry (line 50):

```diff
   - { $ref: 'events.yaml#/RefundRequested' }        # saga trigger consumed by RefundProcess; the settled PaymentRefunded fact is what gets projected
+  - { $ref: 'events.yaml#/PaymentFailed' }          # (a) transient checkout fact: served to the customer from the PlaceOrderProcess run row via paymentStatus/paymentStatusChanged (the declared PM-table exception, payments/api.yaml); ops watches the failure RATE via the observability contracts, not a View_* — the real customer-facing gap is the checkout FAILED screen state (PROP-20260808-141817 §1d, slice 8)
+  - { $ref: 'events.yaml#/CustomerIdentified' }     # (a) saga trigger for CartBindingProcess (ordering/processmanager.yaml); the durable change lands via CartBoundToCustomer, which IS projected
   - { $ref: 'events.yaml#/DeliveryEscalationRequested' }   # internal dispatch mechanics (#60); advances the ranked walk, feeds no View_*
```

(Event anchors verified: `specs/payments/events.yaml:28`, `specs/customer/events.yaml:49`; the
refs are kind-logical, so scope placement needs no path.)

### 2.7 Generated artifacts — regenerated, never hand-edited

`specs/generated/documentation.generated.md` / `.html` (state diagrams, command/event/test tables)
and every downstream generated crate/SQL artifact carry the retired names today; they are
regenerated by `make rust` in the applying session. No hand edits.

### 2.8 What does NOT change (and why)

- **`specs/delivery/errors.yaml` — untouched.** All three errors mapped by the deleted inbox
  entries survive on other commands: `DeliveryJobNotFound` and `InvalidDeliveryStatus` on nearly
  every DeliveryJob command, `DeliveryAlreadyAssigned` on `AcceptDelivery` and `DeclineDelivery`
  (`actors.yaml:104,163`). Only the *mappings* die, with the inbox entries.
- **The `UnassignDeliveryFromPartner` family — untouched.** Command (`commands.yaml:134`), event
  (`events.yaml:236`), edge (`actors.yaml:61`), inbox entry (`actors.yaml:147`), fixture and test
  all stay under their current names; the D3 rename to
  `ReleaseDeliveryAssignment`/`DeliveryAssignmentReleased` is slice 6, a separate approval.
- **`api.yaml`, `stories.yaml`, `specs/screens/**`, `processmanager.yaml` — untouched.** Grep
  finds zero occurrences of the retired names: nothing routed to them, no story reached them, no
  screen bound them. Nothing to unwire.
- **No new surface of any kind** — no mutation, no screen, no projection column. Slices 3–8 own
  those.

## 3. Slice 2 — the PM-send credit: spec-side shape

### 3.1 The two credited strays need NO spec change

The `command-no-mutation` credit for `BindCartToCustomer` and `GrantCustomerCredit` is earned by
spec that **already exists**: resolvable `send` steps in `specs/ordering/processmanager.yaml` —
CartBindingProcess sends `BindCartToCustomer` to the Cart (lines 137–144), ReclamationProcess
sends `GrantCustomerCredit` to the CustomerCredit ledger (lines 204–211). Slice 2's deliverable
for these two is **validator code only** (`tools/codegen-rs`): credit a command as covered when a
PM step demonstrably sends it — the `$ref` resolves AND the target actor's inbox accepts it. Per
D6's decision, an annotation alone never earns the credit. There is deliberately **no**
`internal: true` marker in this diff: the parent's slice-2 Concern rejected self-declared
exemptions, and the ensemble's D6 verdict confirmed the checkable-edge form.

### 3.2 D6's declared `sends:` — the exact YAML (applies WITH the validator mechanism, not before)

`PlaceReplacementOrder` is dispatched from the ReclamationProcess's hand-written wrapper seam
(`ordering/processmanager.yaml:172-179`), not from a step, so the step-derived credit cannot reach
it. D6 (decided, ensemble consent, ADR-20260808-155656 — customer veto window open) covers it with
a declared `sends:` on the wrapper-seam receive, parallel to the existing declared `emits:`
precedent on the same receive (lines 193–199). The exact spec change, on the
`ReclamationResolved` receive of `ReclamationProcess`:

```yaml
      emits:
        # (unchanged — the wrapper-seam REFUND-arm declaration, lines 193-199)
        - { $ref: 'events.yaml#/RefundOpened' }
        - { $ref: 'events.yaml#/RefundApproved' }
      sends:
        # Wrapper-seam REPLACEMENT arm (#159, ADR-20260726-171736): a REPLACEMENT resolution reads
        # the original order and dispatches the no-charge replacement. Not step-derived — carried
        # in the hand-written wrapper seam (crate::process_managers::reclamation), declared here so
        # the command-no-mutation credit and behaviour-test coverage see the dispatch. Checkable
        # BOTH ways: this $ref must resolve AND the target actor's inbox must accept the command.
        - command: { $ref: 'commands.yaml#/PlaceReplacementOrder' }
          to: { $ref: 'actors.yaml#/Order' }
```

Target-inbox check verified by inspection: the Order actor accepts `PlaceReplacementOrder`
(`specs/ordering/actors.yaml:112-116`).

**Sequencing.** The `sends:` key does not exist in the DSL today (zero hits repo-wide); the schema,
loader and both-ways check are validator code, sequenced AFTER
[#399 "Validator gap: a tombstone event absent from the view's fedBy silently never dispatches"](https://github.com/TheCaptainCompany/captain-food/issues/399)
(in flight as PR #412 "fix(#399): view-tombstone-not-fedby — a declared tombstone must be
routable"), which is reworking the same dispatch-coverage territory of the validator. The `sends:`
YAML above therefore lands **in the same change as** the D6 validator mechanism — never before,
since an unparsed key would be dead text the validator cannot see. Until then,
`PlaceReplacementOrder`'s warning **survives slice 2 by design** (parent §10): its disappearance
without the D6 mechanism would mean the credit was granted on an annotation, i.e. the safeguard
failed.

## 4. Expected validator delta (against the 43-warning baseline, 2026-08-08 — re-measure on a pristine `main` before comparing)

| Change | Warning kind | Delta |
|---|---|---|
| Slice 1: retire 2 commands | `command-no-mutation` | 13 → 11 |
| Slice 1: retire 2 events | `event-not-projected` | 11 → 9 |
| Slice 1: declare `CustomerIdentified` + `PaymentFailed` non-projected | `event-not-projected` | 9 → 7 |
| Slice 2: PM-send credit (`BindCartToCustomer`, `GrantCustomerCredit`) | `command-no-mutation` | 11 → 9 |
| **Total slices 1–2** | | **43 → 35 (−8)** |
| D6 mechanism + §3.2 `sends:` (later, with the validator change) | `command-no-mutation` | 9 → 8 (the parent's "~9 cleared" counts this one) |

Consistency check on the residue: the 9 surviving `command-no-mutation` warnings after slices 1–2
are exactly the commands later slices own — `RegisterRider`, `UpdateRiderInfo` (slice 3),
`DeclineDelivery` (4), `ReportDeliveryIssue`, `ResolveDeliveryIssue` (5),
`UnassignDeliveryFromPartner` (6), `UpdateDeliveryStatus` (7), plus `PlaceReplacementOrder` (D6)
and `ConsumeCustomerCredit` (D5, V1). The 7 surviving relevant `event-not-projected` warnings map
to slices 3–6 the same way. Nothing unexplained remains; **zero new warnings** of any kind is part
of the definition of done.

**Must stay 0 errors** — in particular: every deleted command/event had its tests deleted with it
and every surviving test's refs re-pointed (§2.4), so `$ref` resolution, ADR-0032 test/rule
bidirectionality (`DeliveryPartnerAssignmentLifecycle` keeps 2 tests) and story coverage
(untouched — the retired commands never had story steps) all hold by construction.

**Revert cost** (parent §8, restated): deletion-first is cheap to revert *now* — `git revert`
restores spec, tests and edges verbatim while no production events of the retired types exist.
After go-live the revert is no longer the risk; the failure to delete is (see the banner).

## 5. Deviations from the parent's slice text — found on contact with the specs

Three items the parent's slice-1 sentence ("delete commands, events, edges, inbox entries, tests")
did not name, all forced by ref-resolvability or spec truthfulness, all included in §2 rather than
silently improvised:

1. **`TestDeliveryUnassignedFromPartner` must be rewired, not just left alone** — its `given:`
   uses the retired `deliveryAssignedToPartner` fixture (`tests.yaml:2885`); without the §2.4
   rewrite the deletion breaks `$ref` resolution in a test the parent explicitly keeps.
2. **`DeliveryPartnerAssignmentLifecycle`'s description claims the retired capability**
   (`rules.yaml:40`) — description-only reword (§2.5); the structural rule↔test links survive
   without it, but the prose would lie.
3. **The `DeliveryJob` lifecycle comment names `DeliveryPartnerStatusUpdated`**
   (`actors.yaml:48`) — reworded (§2.3d).

None changes the parent's verdicts; all three are the sweep the parent's own "grep the OLD term"
discipline demands.

## 6. Approval mechanics

- **To approve**: the customer replies (issue comment on
  [#348 "Epic: the rider/delivery write surface does not exist (24 of main's 32 validator warnings)"](https://github.com/TheCaptainCompany/captain-food/issues/348),
  or any recorded channel) approving this proposal — wholly, or per section (§2 slice 1 and §3.2
  are severable; §3.1 needs no spec approval at all, being validator-code-only). The applying
  session then flips this file's Status line and notes the approval date and scope.
- **Application**: a plan-mode session (never an autonomous loop) applies §2 exactly as written,
  runs `make rust` to regenerate and gate (0 errors; warning kinds/counts diffed against a
  re-measured pristine-`main` baseline per §4), and pushes as a spec change to `main` per the
  spec-only directive. §3.2 is applied only together with the D6 validator change, after PR #412
  "fix(#399): view-tombstone-not-fedby — a declared tombstone must be routable" lands.
- **To reject or amend**: name the section; this file is rewritten (living document,
  ADR-20260801-020000) before any application.
