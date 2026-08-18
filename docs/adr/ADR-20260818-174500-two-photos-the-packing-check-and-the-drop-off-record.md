# ADR-20260818-174500 — Two photos: what ships now, what waits, and the six things that must be true first

**Status**: Accepted as a direction; **the sequencing below is a team recommendation the founder may
overrule** · **Date**: 2026-08-18 ·
**Directive**: the **FOUNDER / Tech CEO** ·
**Relates**: [ADR-20260818-161500](ADR-20260818-161500-capture-on-delivered-dissolves-the-refund-gap.md)
(capture on delivered; the typed failure cause this owes) ·
[ADR-20260818-150000](ADR-20260818-150000-captain-is-the-tool-the-restaurant-carries-the-delivery.md)
(the control-indicium pile) · **#134** (file upload/storage — the dependency) ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

## The directive, verbatim

> *"A photo will be asked to the restaurant of the package with the content to avoid errors. The
> delivery person will have to take a picture of the delivery to prove the package has been done.
> These will help to secure the content and the delivery."*

Whole roster briefed; **twelve lenses replied.**

## 1. A live landmine, fix before anything dispatches

`PROP-20260725-120055` (the file-attachment framework) still reads `Status: Proposed` while
`DECISIONS.md` records it **decided 2026-08-08** — and its **body specifies Supabase Storage** while
the decided premise is **OVH Object Storage (EU), presigned S3**. An executor following it would put
**customer doorstep photos into Supabase**, against [ADR-20260807-002705](ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md)
*and* against CLAUDE.md's *"Supabase wrapped, identity-only — no business data"*. Proposals are LIVING:
**rewrite the body to the decided premise first.** A documentation fix, not new design.

## 2. The constraint that rewrites the directive

**The photo is necessary only for unattended drop-off.** Where the customer takes the bag from the
rider's hand, the delivery is proven by **the recipient's own act** — a one-time code, a tap, a name
confirmation. A photo in that case fails Art. 5(1)(c) minimisation. **A single always-photograph rule
is excessive on its face for the handed-to-recipient case. Two modes, two proofs.**

Consequently `HANDED_TO_RECIPIENT` **is not a loophole — it is the correct proof**, and making it one
tap is how the mandate stays honest while the privacy surface shrinks. The alternative is a rider
pointing a camera at a person on their own doorstep.

## 3. Six things that must be true before the first photo is taken

Not risks — **preconditions**. `legal-specialist`, no clearance given.

1. **A controller is named.** It is genuinely open whether the doorstep photo is the **restaurant's**
   processing with Captain as an Art. 28 **processor**, or **Art. 26 joint controllership**. Until
   decided, nobody can write the notice, own the balancing test, answer an access request, or author a
   retention decision. *"It is a paragraph of paperwork and it blocks everything else."*
2. **Art. 13 notice at ordering time**, on the surface where the address is entered.
3. **A documented Art. 6(1)(f) balancing test**, including why a recipient-entered code does not
   achieve the purpose in the unattended case. Art. 6(1)(b) contract is the *wrong* basis and will not
   hold; **consent is a trap** — a basis that must be refusable cannot gate a mandatory step.
4. **A working no-photo completion path**, and an Art. 21 objection that stays honored. A customer who
   objects still gets dinner, the rider still gets paid, the order still closes.
5. **The photo's own retention clock.** `ORDER_RETENTION_WINDOW_DAYS` defaults to **3650** — inheriting
   the order's clock means **Captain retains imagery of customers' homes for ten years**, an Art.
   5(1)(e) failure on its face. Two clocks: a short default TTL, plus a **legal hold that only a
   disputed order acquires**, itself a recorded fact with a release.
6. **Described in the DPIA before deployment.** The DPIA is already owed by three independent records
   before the first real order, so the question is not whether the photo creates the obligation but
   whether it is described in the one already owed. **Shipping first inverts Art. 35(1).**

**Erasure, and the index is wrong**: crypto-shredding makes the *reference* unreadable and leaves the
*image* in object storage. *"Owned by the subject"* is precisely the wrong index — the doorstep photo
is keyed by the **order**, uploaded by the **rider**, depicts the **customer's premises**, and may
depict **a third party in no index at all**. An uploader-keyed purge misses it and **the erasure
receipt asserts something false.**

**And a sentence that must not travel**: *"storage, moderation and GDPR retention handled generically
by the framework"* is fine for a rare optional complaint upload that stores nothing today. Copied onto
two mandatory photos per order it becomes a compliance claim about a system that does not exist. **A
DPIA cannot cite "the framework" as its safeguard.**

## 4. Unanimous: the photo never gates the transition or the capture

Seven lenses independently. `DeliveryCompleted` → `OrderDelivered` **is the capture trigger**, so a
photo that can block drop-off **blocks the restaurant being paid for food already cooked and
delivered** — a worse failure than the dispute it prevents. `dba` adds that OVH object storage and the
WAL archive share a provider and region: **one failure domain, two load-bearing paths.**

**The agreed shape is a closed sum, not a nullable**: `Captured(ref) | Waived(reason)` over a declared
enum. "No photo and no explanation" stays **unspellable**, the money never stalls, and **the waiver
reason is itself the typed cause the rider-pay ruling in ADR-20260818-161500 requires** — do not give
photo-missing a second free-text field.

Three requalification aggravators to design out now: **automated rejection** of a photo gating
completion or pay; **photo-compliance feeding dispatch ranking or rider scoring**; and **hard-blocking**
completion or pay. *"This photo is not the indicium that decides the case. The pile is what decides
the case, and this adds to the pile."*

## 5. Naming — "proof" does not enter an identifier

- **"Attachment"** is the carrier — right for the scalar, wrong for an event.
- **"Evidence"** is a role the photo acquires **later**, when a claim cites it. At 20:15 nothing is
  contested yet.
- **"Proof" is refused in identifiers.** *"A `DeliveryProof` type asserts the delivery is established,
  and the platform is then the party that said so — in the dispute where it also decides rider pay."*
  It stays in the founder's sentence as intent.

Proposed: **`OrderPackingPhotoRecorded`** on the **Order**, and **`DropOffPhotoRecorded`** on the
**DeliveryJob**.

**"Handover" is the trap, and this is load-bearing.** Handover-to-rider and handover-to-customer are
two different handovers, so the word forks and the concept splits under it. The restaurant photo's
purpose is identical whether a **rider** or the **customer** collects — the invariant moment is
**packing**, which happens for `DELIVERY` and `COLLECTION` alike. Hence `OrderPacking…` on the Order,
**never on DeliveryJob** — model it there and the anti-error photo **silently vanishes for counter
handover**.

`AttachmentRef` is **already false today** (declared as *"attachment on a conversation message"*, used
by three non-conversation sites). **Reword, do not split** — a second scalar for one concept breaks
one-name-one-scalar in the other direction.

## 6. The obvious implementation shows the restaurant the customer's front door

There is **no per-field role mechanism** today: `roles:` is per-operation. A `dropoffPhotoRef` on
`DeliveryJob` is readable by that query's whole role set — **including RESTAURANT**. *"Visible how:
nowhere — no error, no metric, no gate; it looks exactly like a working feature until a customer
complains, and by then the frame has been screenshotted."*

**Separate top-level operations, one per photo, each with its own literal `roles:`.** The restaurant
gets the **derived fact** (state, capturedAt), never the doorway image. **No resolved URL in the
graph** — it survives in logs, crash reporters, screenshots and caches, and **outlives erasure because
it was copied**. An erased ref is a **typed state**, never null and never an error:
`REQUIRED | CAPTURED | ERASED | RETENTION_EXPIRED | WAIVED`. *"The FACT the photo was taken is an
immutable event and stays; the bytes go — the schema must express that gap, not lie with null."*

**"Mandatory" is not a required input field** — that is non-additive and breaks every deployed rider
client. Nullable input + an aggregate precondition + a typed error. And the gate **must** be an
aggregate precondition: put it in a screen, the gateway or the client and **the behaviour suite stays
green while the handover is blocked at 20:15.**

## 7. The recommendation — what ships now, what waits

**The team's sequencing. The founder may overrule it; it is recorded as a recommendation.**

- **Now, as content on an already-scheduled delivery-touching chunk — never as its own programme**: a
  **nullable `AttachmentRef`** on the two command/event pairs that already exist. A handful of lines,
  no new aggregate, no workflow step. **Free today and a migration after the first real order**, which
  is the final-vision-first argument for landing the shape early.
- **Photo 1's purpose is reachable today with ZERO bytes stored.** *"Avoid errors"* — wrong or missing
  items — is a **per-line packing confirmation** at packing, against order lines already in the model.
  Same purpose, no storage, no GDPR surface, no upload path, one tap. `ux-designer` adds the reason it
  is better than a photo: an overlay of the ordered lines on the glass is **a forcing function at the
  instant the error is made**, not evidence after it.
- **Photo 2 has no byteless version** — proof requires an artifact — so it waits on **#134**, and #134
  waits on there being a delivery to prove.
- **The mandate is reversible policy**: ship capture **optional**, and make it required later as a
  separate recorded flip under gate-then-stabilize, **decided by watching real handovers rather than
  guessing at them.**

**The reason for waiting, stated plainly**: *nobody has ever taken a photo, because nobody has ever
handed over a bag.* A mandatory step inside a live workflow is being designed for a workflow that has
never run once. Declared WIP is three and none is finished; photos would make it four, and the fourth
blocks on #134, which stores no bytes and is on nobody's board.

**Order recommended**: finish and merge **#618** → draft the **terms artifact** (the only thing between
the team and a real restaurateur's signature) → run the **collection walk** → then photos, with the
first real handover already observed.

## 8. Two more that would have shipped broken

- **The obvious first shape 413s on every real phone.** The framework's default body limit is 2 MiB
  with **no override anywhere**, and a phone JPEG is several MB. It fails uniformly, and **only with
  real images** — a 20 KB fixture is green forever.
- **Bytes through the app pod eat the connection pool.** One pod, pool of 5: five concurrent slow
  rider uploads leave **zero connections for checkout**, visible as checkout collapsing while the
  delivery path looks healthy. Presigned direct-to-store, with the size cap **signed into the presign**
  so an oversized upload is unspellable rather than rejected.

Sizing, `UNVERIFIED input` on volumes: ~240 photos/day at target Tours volume (only delivery orders
have both) ⇒ **~8.6 GB at 90-day retention capped at 400 KB**, or **~86 GB uncapped** — which in
Postgres would fill the 20 Gi volume in about three weeks.

## 9. Two things this photo is NOT

- **Not an allergen control**, and it must never be described as one: the information is owed **before
  the contract concludes**, so a photo taken after cooking cannot discharge it — and it must not
  migrate allergen liability toward the rider carrying the bag.
- **Not a rider performance metric.** No secondary use without a fresh basis: no model training, no
  marketing, **no scoring**.

## Consulted

Twelve lenses. **`business-specialist` corrected its own earlier number**: under capture-on-delivered a
failed delivery now costs the restaurant **food COGS only**, not a basket plus a rider fee — roughly
3× smaller than this brief assumed, and the design must be priced against the smaller number. It also
**challenges half the directive**: the drop-off photo genuinely reduces disputes, but the pass photo
*relocates* the missing-item argument from restaurant↔customer to **restaurant↔rider** — a counterparty
Captain is not contracted with — and mandatory-at-the-pass is the most gamed step in food operations.
Its counter-proposal is restaurant-level **opt-in with an adjudication benefit**. Recorded, not
adopted; the founder decides.

**And the line that should govern the framing whichever way it goes**: *if the restaurant cannot see
and use the photo, it is surveillance, and it will be read as surveillance no matter how it is
described.*

---

## Amendment 2026-08-18 — the founder's reference image: labels are OPTIONAL, the photo is mandatory

The founder supplied a reference (a tray where every item carried a printed label — order number,
service mode, item name, a position count `1/3` `2/3` — plus a manifest slip) and then clarified,
verbatim:

> *"For the label it's not required for restaurant just a recommendation and yes the app on the
> restaurant side can provide this kind of label if the restaurant want it. The most important thing
> is the fact that the restaurant made a picture of the order."*

**Coordinator correction, on the record.** The coordinator initially told the founder the per-item
labels "answer the objections almost completely." **That was overstated**, and both consulted lenses
said so. The labels answer the objections only *for a restaurant that adopts them*; adoption is
optional, app-generated, and self-selecting. The mandatory element is the **photo of the order**
alone.

**What this settles:**

1. **No label printer is a precondition** — the hardware dependency the packing analysis flagged is
   removed. Labels are an app-offered convenience; a restaurant prints nothing unless it chooses to.
2. **The bare photo's primary value is a completion / handoff proof, not a dispute artifact**
   (business-specialist). It guards the two failures the domain lens ranks highest — **a paid order
   nobody acted on**, and **handing the rider the wrong bag** — and that value does **not** depend on
   labels. As a *dispute* artifact its value is largely contingent on the optional label layer, and a
   busy Friday-19:30 kitchen is the least likely to adopt it, so **the design plans for the
   low-adoption case, never the reference tray**.
3. **Two values, one photo, mandatory half stands alone**: mandatory photo = completion/handoff
   proof; optional labels = a dispute-resolution upgrade on top, for restaurants that choose it.

**The privacy floor does NOT improve, and slightly worsens** (legal-specialist). With the pseudonymous
label optional, a non-adopting restaurant photographs what it has — plausibly its own **POS ticket,
routinely carrying customer name, phone, delivery address and the itemised basket** (Art. 4(1); a
special-category risk where diet or religion is inferable from items). *"The subject is the food, the
label is incidental"* **does not change the grade** — GDPR has no incidental-capture exemption where
the data is legible and the controller **mandates** the capture; Art. 5(1)(c) minimisation is judged
against what the mandate foreseeably ingests, not the photographer's intent. Therefore:

- **The §3 six preconditions are UNCHANGED in number** — the optional label subtracts none of them; it
  only offers a non-adopter a mitigation it may decline.
- **§3.3 (Art. 6(1)(f) balancing) and §3.6 (DPIA) are REINFORCED**: both must now be written to the
  **worst realistic input — the bare POS ticket with customer-identifying data — never to the
  pseudonymous label.**
- **§3.5 (retention clock) is unchanged in shape but its stakes rise**: the 3650-day default would now
  retain name/phone/address imagery, not an order number.

`VERIFY-FIRST` (legal): the actual French POS-ticket fields, with an avocat, before build. No
clearance.

**A still-missing signal both lenses named**: no `specs/observability.yaml` fold on reclamation rate
and missing-item dispute cost **split by whether the order carried a label**. Until it exists, both
the label ROI and the bare-photo ROI are asserted, not measured.
