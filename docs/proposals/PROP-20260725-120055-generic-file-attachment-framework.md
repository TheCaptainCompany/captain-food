# PROP-20260725-120055 — Generic file-attachment framework (`/files/<uuid7>.<ext>` + a `files` registry)

- **Status**: Proposed — plan-mode proposal. **No `specs/**` or code changed yet.** On approval it becomes an ADR that lands with the first implementation slice.
- **Date**: 2026-07-25
- **Tracking issue**: [#134 "Order conversations: image attachments (Supabase Storage + signed upload + retention) (epic #129)"](https://github.com/TheCaptainCompany/captain-food/issues/134) (every proposal MUST have one, ADR-20260724-143000; always the full clickable link, never a bare number)
- **Parent epic**: [#129 "Epic: in-app order conversations (customer/restaurant/rider/admin messaging) with private notes, translation, in-thread refunds, images"](https://github.com/TheCaptainCompany/captain-food/issues/129)
- **Realized by**: _(filled at completion — ADR + PR)_
- **Related**: [PROP-20260725-013008](PROP-20260725-013008-order-conversations-messaging.md) §4/§9.6 (the epic design this fills in) · [ADR-20260725-015921](../adr/ADR-20260725-015921-order-conversations-reserve-data-model.md) (product-owner decision: *"image attachments are handled generically by the framework — opaque `attachmentId`/`storageRef` hooks in `Conversation`, not a bespoke pipeline"*) · ADR-0047 (JWT via JWKS at the role-path boundary) · ADR-0042 (Render + Supabase, EU/Frankfurt) · ADR-20260721-025159 (`sweep_retention()` is the one place retention windows live) · ADR-0041 (acting user is envelope, not payload) · [#18 "Retention policy"](https://github.com/TheCaptainCompany/captain-food/issues/18)

---

## TL;DR

Slice 1 of messaging ([#130](https://github.com/TheCaptainCompany/captain-food/issues/130), merged in
`2b95dd3`) already landed the **hook**: `MessagePosted.attachmentRefs: AttachmentRef[]`, where
`AttachmentRef` is deliberately an **opaque string** — *"storage, moderation and GDPR retention are
handled generically by the framework, not by this aggregate"*. This proposal fills in that framework,
and does so **generically** (not messaging-specific), per the product owner's directive.

Five decisions, in one sentence each:

1. **The event carries a relative path, never bytes and never a signed URL.** `AttachmentRef` =
   `/files/<uuid7>.<ext>`.
2. **A new `files` registry table** — application-owned, *not* event-sourced, *not* projected — holds
   the metadata, the ACL and the lifecycle. `(owner_type, owner_id)` is indexed and **not unique**, so
   one message has N files.
3. **Who may read is recorded at save time** as `audience` (a set of roles) **×** `scope_type`/`scope_id`
   (which order) — *not* a list of user ids, which would go stale the moment a rider is reassigned.
4. **`GET /files/<uuid7>.<ext>` is guarded by the existing auth seam** (the httpOnly site-wide cookie
   *or* `Authorization: Bearer`), then 302s to a 60-second signed Supabase Storage URL.
5. **`expires_at` on the row drives auto-deletion** — swept by a `FileRetentionWorker` that deletes the
   **object first, then the row**. An SQL-only sweep would orphan the bytes in the bucket forever.

**Nothing in the already-merged `Conversation` slice changes.** `AttachmentRef` keeps its exact
declared shape (opaque string, `maxLength: 500`); no event, aggregate, command or projection is
touched. That is the whole point of the "framework, not bespoke pipeline" decision, and it is why this
can be built without reopening #130.

---

## 1. Where this starts from

`specs/scalars.yaml` (landed):

```yaml
AttachmentRef:
  type: string
  maxLength: 500
  description: >
    Opaque reference to a framework-managed attachment on a conversation message. Storage, moderation
    and GDPR retention are handled generically by the framework, not by this aggregate (#129).
```

Two things follow from "opaque":

- The **domain** never interprets it. `Conversation` stores and replays strings; it has no idea what a
  bucket is. The dependency rule (ADR-0035, `domain` imports nothing) holds by construction.
- The **framework** owns the format, and is free to evolve it — but only forward-compatibly, because
  refs already written into `domain_events` are **immortal** (§6.3).

⚠️ **Supersedes** the epic's §2.1 sketch of a `MessageAttachmentAdded { attachmentId, kind, storageRef,
contentType }` event and the corresponding paragraph of [#134](https://github.com/TheCaptainCompany/captain-food/issues/134)'s
scope. Slice #130 chose `attachmentRefs[]` **on `MessagePosted`** instead, which is better: the
attachment is part of the message fact, not a second event that can be missing when the message is
folded. No separate attachment event is needed for the normal path.

## 2. The ref format (decision 1)

```
AttachmentRef = "/files/" <uuid7> "." <ext>
example:        /files/019826b1-4c7e-7a31-9f0d-2a5b8c1e4d90.jpg
```

Three properties, each load-bearing:

- **Relative, not absolute.** The storefront is multi-tenant (`{slug}.captain.food`, `live.captain.food`).
  A host baked into an immutable event would be *wrong* the day a restaurant changes its slug, and would
  break the marketplace/storefront split. The client resolves the ref against its own origin.
- **Non-expiring.** It is not a signed URL. It is a stable name, resolved through our guard on every
  request — so authorization is evaluated *at read time*, against current membership, not frozen at
  upload time.
- **Self-identifying.** The UUIDv7 *is* the `file_id` — the lookup key. `<ext>` is cosmetic (it makes the
  URL behave in browsers, download managers and CDNs) and is **validated, not trusted**: a request whose
  extension does not equal the stored one is a 404, so `/files/<id>.svg` cannot be used to re-label a
  stored JPEG (§4.3).

UUIDv7 matches what the write path already mints everywhere (`message_id`, `inbound_event_id`), and its
time-ordered prefix keeps the b-tree and the bucket's key distribution healthy.

> **Not a secret.** UUIDv7 carries ~74 random bits — unenumerable, but it is an *identifier*, not a
> capability token. The guard in §4 is what protects the file; the id is never the protection. This is
> exactly why alternative (c) in §8 is rejected.

## 3. The `files` registry (decisions 2 + 3)

New DSL file `specs/database/tables/files.yaml`, in the **application/transport-owned** table category
alongside `auth_sessions` (`integration_connections.yaml`): never fed from `domain_events`, never a
GraphQL `reads` target, no `View_*`.

**Why not event-sourced?** Because almost everything on the row *mutates* and *must be forgettable*:
a moderation verdict, an extended expiry, and above all **deletion**. An event fold cannot forget —
that is the property we normally want and precisely the property GDPR forbids here. The event log keeps
the immutable **fact** ("this message had attachment X"); the registry keeps the erasable **bytes and
their access rules**. Splitting them this way is what lets us honour an erasure request without ever
rewriting history.

### 3.1 Columns

```yaml
files:
  columns:
    file_id:       { type: uuid, pk: true }              # UUIDv7 — the <uuid7> in the ref
    extension:     { type: text }                        # canonical lowercase, from the allowlist
    content_type:  { type: text }                        # SERVER-determined (sniffed), never client-asserted
    byte_size:     { type: bigint }
    checksum:      { type: text, index: true }           # sha256 hex — integrity + dedupe
    storage_key:   { type: text, unique: true }          # object key in the bucket (ADR-0042)

    owner_type:    { type: FileOwnerType, index: true }  # CONVERSATION_MESSAGE (first; extensible)
    owner_id:      { type: uuid, index: true }           # = ConversationMessageId for that owner_type
    position:      { type: int }                         # stable ordering of the N files of one owner

    scope_type:    { type: FileScopeType }               # ORDER (first)
    scope_id:      { type: uuid, index: true }           # = OrderId — the membership test
    audience:      { type: UserType[] }                  # roles allowed to read, recorded at save time
    kind:          { type: FileKind }                    # ORDER_PHOTO | DELIVERY_PROOF | RECLAMATION

    uploaded_by:   { type: uuid, nullable: true }        # auth subject (envelope-style, ADR-0041)
    mode:          { type: Mode, nullable: true }        # test mode (ADR-0038)
    created_at:    { type: timestamptz }
    expires_at:    { type: timestamptz, index: true }    # the auto-deletion deadline
    deleted_at:    { type: timestamptz, nullable: true } # object purged; row survives briefly as a tombstone -> 410
```

New scalar enums: `FileOwnerType`, `FileScopeType`, `FileKind` (the codegen emits a `ref_*` lookup table
per scalar enum, ADR-0037).

### 3.2 N files per message (requirement: *"more than one file per messageId"*)

`(owner_type, owner_id)` is a **plain composite index, deliberately not unique**. `position` gives the
thread a stable render order independent of insertion timing. Ordering by `file_id` would *also* work
(UUIDv7 is time-ordered) but would silently reorder if a client uploads in parallel — `position` is the
explicit contract.

The event side already agrees: `attachmentRefs` is an **array** on `MessagePosted`.

> **Naming note, already resolved upstream:** `owner_id` for `CONVERSATION_MESSAGE` is a
> `ConversationMessageId` — the *business* message — **not** the envelope `MessageId` (the
> `command_journal` submission id). Slice #130 introduced that distinction as two separate scalars,
> which is why this table can say "messageId" without ambiguity.

### 3.3 Who can access, recorded at save time (requirement: *"indicate who can access this file when we are saving it"*)

Two columns, and the split between them is the design's main idea:

| what | column | when evaluated |
|---|---|---|
| **which roles** may read it | `audience` (set of `UserType`) | **frozen at save time** |
| **which people** hold those roles | `scope_type` + `scope_id` | **resolved live, at read time** |

So a `DELIVERY_PROOF` photo is saved with `audience = {CUSTOMER, RESTAURANT, RIDER, ADMIN}`,
`scope_type = ORDER`, `scope_id = <orderId>` — and it is served to *the customer of that order*, never
to any other customer.

**Why not store the allowed user ids?** It was the obvious reading of "indicate who can access", and it
is wrong in this domain:

- a **rider is reassigned** → the new rider cannot see the proof-of-delivery photo they need;
- an **admin is escalated in later** (epic §2.5) → the escalation grants conversation access but not
  file access, a silent asymmetry;
- **restaurant staff churn** → stale ids accumulate as permanent grants on personal data.

Freezing *roles* and resolving *membership* gives the intent ("staff of this order may see this") a
representation that stays true as the people change. It also keeps the guard cheap: one indexed row
read plus one membership check, no join into the conversation projection.

### 3.4 The visibility-coupling hazard (worth a rule + a test)

Files are uploaded **before** the message is posted (the client needs the refs to build `PostMessage`).
So at upload time the message's `visibility` is not yet a fact — and a file uploaded with a PUBLIC
audience that ends up on an `INTERNAL` staff note would be **over-exposed**: the customer could not see
the note, but *could* fetch its attachment. That is a real hole and it is easy to miss.

Mitigation: the `PostMessage` handler **narrows** the `audience` of every row named in `attachmentRefs`
to match the message's visibility (`INTERNAL` ⇒ drop `CUSTOMER`). Narrowing only, never widening, so a
replayed or hostile `PostMessage` cannot escalate a file's audience. This becomes:

- a **rule** in `rules.yaml`: *an attachment is never readable by an audience broader than its message's visibility*;
- a **behaviour test** asserting a customer gets 403 on the attachment of an INTERNAL note.

## 4. Serving `GET /files/<uuid7>.<ext>` (decision 4)

An Axum route in `crates/server`, outside `/{role}/graphql`.

### 4.1 Authentication — reuse the seam that exists

`crates/server/src/auth.rs` already verifies a Supabase JWT via JWKS into a
`Principal { user_id, role }` (ADR-0047), and `auth_routes.rs` already sets a **site-wide**
(`Path=/`), `HttpOnly`, `Secure`, `SameSite=Lax` access cookie with an `AuthContext` cookie fallback.
Both carriers are already wired; this route just consumes them.

> **The cookie is not an implementation detail here — it is the reason the design works.**
> A browser `<img src="/files/…">` **cannot** attach an `Authorization` header. Guarding this route with
> Bearer-only would force every image through a `fetch()` + `blob:` URL dance in the Leptos renderer.
> The existing site-wide httpOnly cookie is sent automatically on the image request, so
> `<img src="/files/…">` just works, and native shells (SwiftUI/Compose over UniFFI) keep using Bearer.

`SameSite=Lax` on a cross-site `<img>` means the cookie is *not* sent from a third-party page — which is
correct: an attacker embedding our file URL on their site gets a 401, not the bytes. This is a GET with
no side effects, so cookie auth carries no CSRF exposure.

**There is no anonymous access.** No `@public` files in V1 — not even for `DELIVERY_PROOF`, which is
exactly the kind that photographs someone's front door.

### 4.2 The guard, in order

1. Resolve `Principal` (cookie or Bearer) — absent/invalid ⇒ **401**.
2. Load the row by `file_id` — absent ⇒ **404**.
3. Extension in the path ≠ stored `extension` ⇒ **404**.
4. `deleted_at IS NOT NULL` **or** `expires_at < now()` ⇒ **410 Gone** (§6.3).
5. `principal.role ∉ audience` ⇒ **403**.
6. `principal.user_id` is not a member of `scope_id` ⇒ **403**.
   *(member of an ORDER = its customer · staff of its restaurant · its assigned rider · any ADMIN.)*
7. Serve (§4.4).

### 4.3 Response hardening — where user-supplied bytes bite

We are serving **attacker-influenceable bytes from our own origin**, next to an httpOnly session cookie.
The non-negotiables:

- **SVG is not in the allowlist.** An SVG is a script container; served same-origin it is stored XSS
  against the session. Allowlist: `jpg`/`jpeg`, `png`, `webp`. `heic` is accepted on upload and
  **transcoded** (it is the iPhone camera default, so rejecting it would break the rider's
  proof-of-delivery flow on half the fleet).
- `content_type` is **sniffed server-side at upload** from the magic bytes, never taken from the client's
  `Content-Type`, and only a sniffed type on the allowlist is stored.
- Always `X-Content-Type-Options: nosniff`, `Content-Disposition: inline; filename="<file_id>.<ext>"`,
  and `Content-Security-Policy: sandbox` on the response.
- `Cache-Control: private, no-store` on the redirect (§4.4) — a shared cache must never hold a
  per-principal authorization decision.

### 4.4 Bytes: 302 to a signed URL (recommended) vs stream through the BFF

**Recommended: 302 → a 60-second signed Supabase Storage URL.** The authorization decision stays
entirely ours; the egress stays entirely Supabase's. Render bandwidth and BFF request-time are not
spent proxying photos, which matters when a rider uploads on mobile data and a back-office operator
scrolls a day of threads.

Trade-off, stated honestly: for those 60 seconds the signed URL is bearer-shareable and will appear in
browser history and any intermediate log. Streaming through the BFF avoids that and keeps a single
choke point for byte-level auditing, at the cost of egress + CPU on every image view.

Given the content (order photos, doorstep photos), the 60-second window is an acceptable trade — but it
**is** a privacy trade, so it is decision **D1** in §9 rather than an assumption.

## 5. Upload

`POST /files` (multipart) through the BFF — one place for the size limit, the type allowlist, the
sniffing, per-principal rate limiting (which composes with the epic's mute) and a future moderation
hook. Returns `{ ref, fileId, expiresAt }`; the client puts `ref` into `PostMessage.attachmentRefs`.

Direct-to-storage signed upload (client → Supabase, then `registerFile`) is a **later optimization**,
deliberately not V1: it moves validation to after the bytes already exist, so the allowlist and size
limit stop being preconditions and become cleanup jobs.

**Orphan uploads get handled for free.** A row is created with a short initial `expires_at` (24 h);
`PostMessage` **extends** it to the kind's retention window when the file is actually attached. Upload
that is abandoned (user closes the tab, message never sent) expires on its own — no separate
reconciliation job, no orphan-hunting query.

## 6. Retention and deletion (decision 5)

### 6.1 Windows per kind

| kind | proposed window | rationale |
|---|---|---|
| unposted (orphan) | **24 h** | abandoned uploads; self-cleaning (§5) |
| `ORDER_PHOTO` | **30 days** | restaurant's own pre-packaging photo; low dispute value after the order settles |
| `DELIVERY_PROOF` | **90 days** | matches the delivery-dispute horizon and the existing 90-day journal/mirror windows |
| `RECLAMATION` | **180 days** | a claim or card chargeback can run months; the evidence must outlive it |

Aligned with #18 and expressed the same way as the other tables (`retention:` block, documentary; the
executable policy lives in one place).

### 6.2 A SQL-only sweep is not enough — this is the operational catch

`sweep_retention()` is pure SQL. `DELETE FROM files WHERE expires_at < now()` deletes the **row** and
**leaves the bytes in the bucket forever** — an unbounded storage bill and, far worse, a GDPR failure
that *looks* like compliance because the database is clean.

So the purge is a **two-phase, app-side `FileRetentionWorker`** (the in-process worker pattern already
used by `RetentionSweepWorker`): select expired → **delete the object** → then delete the row.
Object-first ordering means a crash mid-way leaves a row pointing at a missing object (served as 410,
retried next pass) rather than a bucket full of unreferenced bytes nobody knows about.

`sweep_retention()` keeps only a **documentary** clause plus a tombstone sweep for rows whose
`deleted_at` is set — never the primary mechanism.

### 6.3 The event outlives the file — and that is correct

`MessagePosted.attachmentRefs` is in `domain_events`, which is **never swept** (the forever log). So a
90-day-old thread will hold a ref to bytes that no longer exist. This is the design working, not a bug:
the *fact* that a proof-of-delivery photo was posted is permanent audit history; the *personal data* in
it is not.

The contract that makes it usable: an expired or purged file returns **410 Gone**, distinguishable from
**404 Not Found**. The renderer shows "attachment expired" rather than a broken image or a misleading
"not found". A short-lived tombstone row (`deleted_at` set, purged after a grace period) is what lets us
answer 410 truthfully instead of degrading to 404 the moment the row vanishes.

### 6.4 Erasure on request

A data-subject erasure request must purge **before** expiry. Same worker, driven by an erasure request
over `scope_id` / `uploaded_by` — which is exactly why both are indexed. This slots into the erasure
story of #18 rather than inventing a parallel path.

## 7. Completeness obligations (ADR-0032) and observability

**Rules** (each with ≥1 behaviour test, both directions enforced by `make validate`):

1. A file is served only to a principal whose role is in its recorded `audience` **and** who is a member of its `scope`.
2. An attachment is never readable by an audience broader than its message's visibility (§3.4).
3. An expired or purged file returns 410 and never bytes.
4. Domain events never carry file bytes — only refs.
5. An attachment ref whose extension does not match the stored file is not served.

**Observability contracts** (`specs/observability.yaml`): upload outcome + size distribution;
**403 rate on `/files` as an operator signal** (a spike is id-probing, not user error); redirect/stream
latency; sweep deletions per kind; bucket bytes per tenant (cost).

## 8. Alternatives considered

- **(a) Bytes in the event** (base64 in the payload). Rejected — and this is the requirement that
  started the proposal. It bloats the forever log by orders of magnitude, destroys `domain_events`
  scan performance, and makes personal data **unerasable** by construction: you cannot honour an
  erasure request against an append-only log.
- **(b) A signed storage URL in the event.** Rejected: the URL expires, so an immutable log would fill
  with dead links; it leaks bucket topology; and it freezes the authorization decision at write time,
  so a rider reassignment or an admin escalation can never be reflected.
- **(c) Public bucket + unguessable UUID (capability URL).** Rejected. UUIDv7 is not enumerable but is
  **not a secret** — it is time-ordered and appears in DOM, logs, referrers and screenshots. A leaked
  URL would be permanent, unrevocable, unexpirable and unaudited. Doorstep photos of identifiable
  people cannot be protected by obscurity.
- **(d) An explicit allowed-subject list on the row.** Rejected — §3.3: goes stale on rider
  reassignment, staff churn and admin escalation, in both the over- and under-granting directions.
- **(e) A bespoke `message_files` table.** Rejected as the primary shape: the product owner asked for a
  generic framework, and KYC documents, restaurant cover photos and dispute evidence are all visible on
  the roadmap. `owner_type` + `owner_id` costs one column and one enum today and avoids three more
  near-identical tables later. (A dedicated table would win only if the ACL differed per owner type —
  it does not; `audience` × `scope` covers all of them.)
- **(f) Serve straight from Supabase Storage with Postgres RLS.** Rejected: our authorization model
  lives in the JWT/role path (ADR-0047), not in RLS. Using RLS here would fork the ACL into two engines
  that must be kept in agreement — the exact failure mode ADR-0006 exists to avoid.
- **(g) A third-party asset/CDN service** (Cloudinary, Uploadcare). Rejected for the same reason the
  epic rejected a third-party chat SDK: it sends customer personal data to another processor, costs the
  "ethical alternative" positioning, and buys transformations we do not need yet.

## 9. Decisions this proposal asks the product owner to make

| # | decision | recommendation |
|---|---|---|
| **D1** | Bytes: **302 → 60 s signed URL** vs **stream through the BFF** (§4.4) | 302 — the privacy trade is small and bounded; revisit if it proves otherwise |
| **D2** | Retention windows per kind (§6.1) | 24 h orphan / 30 d `ORDER_PHOTO` / 90 d `DELIVERY_PROOF` / 180 d `RECLAMATION` |
| **D3** | Type allowlist excludes **SVG**; **HEIC transcoded** on upload (§4.3) | confirm — SVG same-origin is stored XSS; HEIC is the iPhone default and must be accepted |
| **D4** | Size limits | 10 MB per file, 5 files per message |
| **D5** | Moderation in V1 | manual report path + rate limits; automated scanning is later hardening |

## 10. Scope of the implementation slice

**In:** `specs/database/tables/files.yaml` + the three scalar enums · the `FileRetentionWorker` +
`sweep_retention()` tombstone clause · `POST /files` + `GET /files/<uuid7>.<ext>` in `crates/server` ·
the `PostMessage` audience-narrowing step (§3.4) · rules + behaviour tests + observability contracts.

**Out (unchanged, deliberately):** `AttachmentRef`, `MessagePosted`, the `Conversation` aggregate and
the `OrderConversation` projection — slice [#130](https://github.com/TheCaptainCompany/captain-food/issues/130)
needs **zero** rework. Also out: direct-to-storage upload, automated moderation, image
transformation/thumbnails, and any owner type other than `CONVERSATION_MESSAGE`.

## 11. Verification plan

- `make rust` green (build + test + validate + generate), `make validate` at **0 errors** with no new warnings.
- Behaviour tests for the five rules of §7, including the negative cases: customer → INTERNAL note's
  attachment ⇒ 403; wrong-order customer ⇒ 403; expired ⇒ 410; extension mismatch ⇒ 404; anonymous ⇒ 401.
- A retention test proving **object-then-row** ordering, and that a crash between the two leaves a
  410-serving row rather than an orphaned object.
