# PROP-20260725-120055 — Generic file-attachment framework (`/files/<uuid7>.<ext>` + a `files` registry)

- **Status**: Proposed — plan-mode proposal. **No `specs/**` or code changed yet.** On approval it becomes an ADR that lands with the first implementation slice.
- **Date**: 2026-07-25
- **Tracking issue**: [#134 "Order conversations: generic file-attachment framework (`/files/<uuid7>.<ext>` + files registry + retention) (epic #129)"](https://github.com/TheCaptainCompany/captain-food/issues/134) (every proposal MUST have one, ADR-20260724-143000; always the full clickable link, never a bare number)
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

## 2b. Sequence diagrams

Added 2026-07-26 to meet the standing proposal directive (docs/proposals/README.md). Drawn faithfully
to the hexagonal architecture: the aggregate/PM **decides** (pure, no I/O), facts are saved through the
`Repository` and appended by its one adapter `PgEventStore` (ADR-20260719-031136, docs/claude/mermaid.md).

**(a) Upload, then post — why the file exists before the message does.** The client needs refs to build
`PostMessage`, so the row is created first with a SHORT expiry that the post then extends. An abandoned
upload expires on its own; no orphan-hunting job.

```mermaid
sequenceDiagram
    autonumber
    actor U as Rider or staff client
    box edge adapter
        participant BFF as BFF (POST /files)
    end
    box infrastructure adapters
        participant OBJ as Supabase Storage
        participant REG as files registry
    end
    box application core
        participant CONV as Conversation aggregate (decides, pure)
        participant REPO as Repository
        participant PG as PgEventStore
    end
    U->>BFF: POST /files (multipart)
    BFF->>BFF: sniff magic bytes, allowlist, size cap
    BFF->>OBJ: put object
    BFF->>REG: insert row (audience, scope, expires_at = 24h)
    BFF-->>U: { ref = /files/<uuid7>.<ext> }
    U->>BFF: postMessage(order, body, visibility, attachmentRefs)
    BFF->>CONV: handle PostMessage
    CONV-->>REPO: save(MessagePosted with refs)
    REPO->>PG: append
    BFF->>REG: NARROW audience to the message visibility, extend expires_at
    Note over BFF,REG: narrowing only, never widening -- see 3.4
```

**(b) Serving one file — the guard, in order.** Authorization is evaluated at READ time against current
membership, which is why the ref is a stable name and never a signed URL.

```mermaid
sequenceDiagram
    autonumber
    actor B as Browser (img src) or native shell
    box edge adapter
        participant GUARD as /files middleware
    end
    box application core
        participant SM as ScopeMembershipRepository (port)
    end
    box infrastructure adapters
        participant REG as files registry
        participant OBJ as Supabase Storage
    end
    B->>GUARD: GET /files/<uuid7>.<ext> (cookie or Bearer)
    GUARD->>GUARD: verify JWT -> Principal -> ReadScope
    GUARD->>REG: load row by file_id
    alt expired or purged
        GUARD-->>B: 410 Gone (renderer shows "attachment expired")
    else extension mismatch or unknown id
        GUARD-->>B: 404
    else role not in audience
        GUARD-->>B: 403
    else
        GUARD->>SM: is_member(scope_type, scope_id, scope)
        alt not a member
            GUARD-->>B: 403 (counted as an operator signal)
        else
            GUARD->>OBJ: sign URL (60s)
            GUARD-->>B: 302 -> signed URL
        end
    end
```

**(c) Retention — the object dies, the row does not.** Pure SQL cannot reach the bucket, so a daily
worker owns the purge; the row survives forever as the tombstone that keeps 410 answerable.

```mermaid
sequenceDiagram
    autonumber
    participant W as FileRetentionWorker (daily)
    participant REG as files registry
    participant OBJ as Supabase Storage
    W->>REG: select rows past expires_at, deleted_at is null
    loop each expired file
        W->>OBJ: delete object
        W->>REG: set deleted_at (row KEPT)
    end
    Note over W,REG: crash between the two -> next pass re-deletes (idempotent) and sets the marker
    Note over REG: the guard checks expires_at, NOT deleted_at alone -- so a file stops being served the<br/>moment it expires, never up to a day later when the worker next runs
```

## 2c. Screen mockups (wireframes)

Low-fidelity, per use case — to fix the shape, not the visual design.

**Customer — an attachment in the order thread.** The rider's proof-of-delivery photo, served through
the guard on the customer's own session cookie.

```
+-------------------------------------------+
|  <  Back        Order A1B2 - Chez Marco   |
+-------------------------------------------+
|   * Out for delivery         12:20        |
|                                           |
|   Rider (Sam)                12:34        |
|   +--------------+                        |
|   |   [ photo ]  |  Left at your door     |   <- GET /files/0198..jpg (cookie auth)
|   +--------------+                        |
|   * Delivered                12:35        |
+-------------------------------------------+
```

**Customer — the same thread 90 days later.** The event still carries the ref; the bytes are gone. This
is the design working, and it is why 410 must be distinguishable from 404.

```
|   Rider (Sam)                12:34        |
|   +--------------+                        |
|   | (!) Attachment|  Left at your door    |   <- 410 Gone -> "expired", never a broken image
|   |    expired    |                       |
|   +--------------+                        |
```

**Rider — capture proof of delivery.** The upload that creates the row before any message exists.

```
+----------------------------------------+
|  Job A1B2 - deliver to Marie D.        |
+----------------------------------------+
|  [   Take proof-of-delivery photo   ]  |   -> POST /files  (DELIVERY_PROOF)
|  [   Attach + mark delivered        ]  |   -> postMessage(attachmentRefs)
+----------------------------------------+
   HEIC from the camera is transcoded server-side; SVG is refused (4.3)
```

**Restaurant back office — an INTERNAL note with an attachment.** The audience-narrowing case: this
photo must NOT be reachable by the customer even though they are a member of the order.

```
+-------------------------------------------------+
|  Order A1B2 - Marie D.                          |
|   [lock] INTERNAL - staff only                  |
|   You -> staff             12:05                |
|   "Packed wrong, see photo"                     |
|   +--------------+                              |
|   |  [ photo ]   |                              |   <- audience = {RESTAURANT, RIDER, ADMIN}
|   +--------------+                              |      customer -> 403 (3.4)
+-------------------------------------------------+
| [ attach ] [ Internal v ]      [ Reply... ][Send]|
+-------------------------------------------------+
```

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

    owner_type:    { type: FileOwnerType, index: true }  # CONVERSATION_MESSAGE | RECLAMATION (see below)
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

> **The generic `owner_type` is already justified in practice, not just in principle** (2026-07-26).
> `ReclamationEvidenceAttached` ([#156](https://github.com/TheCaptainCompany/captain-food/issues/156),
> merged `ec7357a`) is a **second** consumer of `AttachmentRef`, and its commit says so explicitly:
> *"the actual upload/storage is the #134 attachment framework's concern (still open); this models the
> domain fact only."* So there are now two owner types in reality — `CONVERSATION_MESSAGE` and
> `RECLAMATION` — which settles alternative (e) in §8: a bespoke `message_files` table would already
> need a sibling, one release later.

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
read plus one **`ScopeMembership`** lookup (PROP-20260725-185140 §3.4) — no join into the conversation
projection, and no knowledge of the order model in the file guard at all.

`scope_type` + `scope_id` on the row are exactly the **binding** into that index: the file says *which
instance it belongs to*, and `ScopeMembership` says *who belongs to that instance right now*. The two
columns exist for no other purpose.

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
6. `principal` is not a member of `(scope_type, scope_id)` ⇒ **403** — one index-only lookup against
   the **`ScopeMembership`** projection (PROP-20260725-185140 §3.4):

   ```sql
   SELECT EXISTS(SELECT 1 FROM "ScopeMembership"
                  WHERE scope_type=$1 AND scope_id=$2 AND member_type=$3 AND member_id=$4);
   ```

   The guard never learns what an order *is*. A new `FileScopeType` is a projector rule, not new code
   in the guard — and `/files` shares the mechanism with every GraphQL read rather than owning a copy.
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
**is** a privacy trade, so it is decision §9.1 rather than an assumption.

### 4.5 Where the scope check lives, and the prerequisite it exposes

Step 6 of §4.2 ("is this principal a member of `scope_id`?") is the question that makes the whole ACL
real, and answering it turned up a gap that is **wider than this proposal**.

#### 4.5.1 The layer: the read repository *port*, not the resolver

On the **write** side the aggregate is the choke point — every mutation funnels through it, so it is the
natural place for a business security check, and that is where these checks belong (product-owner
position, 2026-07-25). On the **read** side there is no aggregate: queries go straight from the resolver
to a read model (projection-on-read, ADR-0035). So the choke point must be somewhere else, and the only
place every read funnels through is the **read repository trait in `crates/application/src/queries.rs`**.

| layer | verdict |
|---|---|
| `domain` | **No.** No aggregate on the read side, and domain must not know auth subjects (ADR-0041 keeps the acting user in the envelope). Row ownership is not a business invariant. |
| `server` (resolver, `/files` route) | **Supplies** the verified `Principal`; must not **own** the rule — otherwise every transport (GraphQL, `/files`, SSR, later the mobile shells) reimplements it. |
| **`application` (the read ports)** | **Yes.** "A customer reads their own order" is a use-case rule. |
| `infrastructure` (`Pg*Repository`) | Where the predicate **executes** (the SQL `WHERE`), not where the decision **lives**. |
| Postgres RLS | No — forks the ACL into a second engine (§8f). |

**Make it structural, not procedural.** Today the ports are unscoped *by signature*:

```rust
// crates/application/src/queries.rs:252 — anyone who may call the query gets any order
async fn by_id(&self, id: OrderId) -> Result<Option<OrderTrackingRow>, DomainError>;
```

An unscoped read is *expressible*, so correctness depends on every caller remembering. The fix is to
make it inexpressible:

```rust
async fn by_id(&self, id: OrderId, scope: &ReadScope) -> Result<Option<OrderTrackingRow>, DomainError>;

enum ReadScope { Public, Customer(CustomerId), Restaurant(RestaurantId), Rider(RiderId), Admin }
//   constructible ONLY from a verified Principal
```

Each `Pg*Repository` turns the scope into a predicate against **`ScopeMembership`**. **Filter, don't
check**: pushing it into the `WHERE` makes list queries correct by construction, leaks no existence on
collections, and costs one round trip instead of two. A review rule catches this most of the time; a
type signature catches it every time.

**Two distinct gates, both required** — `@auth`/`@public` (api.yaml, at the role path) answers *may this
**role** call this operation?*; `ReadScope` answers *may this **principal** see this row?* The former
structurally cannot express the latter: the schema does not know which order is being asked for.

`/files` is the **direct-fetch** case: one object, nothing to filter, so it returns a status —
**403, not 404**, because the id is an unguessable UUIDv7 only obtainable from a thread the caller
already had access to (negligible existence leak), and collapsing both cases to 404 would destroy the
probing signal of §7. It is mounted as an **Axum middleware**; the GraphQL side uses an async-graphql
`Guard`/`Extension`, because an HTTP layer runs before the GraphQL body is parsed and cannot see which
query was selected (PROP-20260725-185140 §3.3.2). Same check, two mounting points.

#### 4.5.2 The prerequisite: `ScopeMembership` and its projector rules

The membership answer comes from the **`ScopeMembership`** projection
(PROP-20260725-185140 §3.4) — `(scope_type, scope_id, member_type, member_id)`, maintained by an
app-layer projector through declared `grants`/`revokes` per event. `/files` does not resolve membership
itself; it asks that index, exactly as every GraphQL read does.

That projection does not exist yet, and neither do two of the four identity bridges it needs
(`RiderRegistered.authRef` is projected nowhere; nothing maps an auth subject to a restaurant — though
the RESTAURANT/RESTAURANT_ACCOUNT **JWT claims** now cover the common case). This is **not** a
files-specific gap: ADR-0047 gates the *role path* but explicitly defers per-field `@auth`, and the
resolvers are thin read models, so nothing has yet had to enforce *which* restaurant a staff user
belongs to. `/files` is simply the first surface that does, because it is the first one serving
personal data outside the GraphQL ACL.

> ⚠️ **Do not resolve the restaurant from the `Host` header.** The tenant middleware maps
> `{slug}.captain.food` → restaurant, and reusing it here looks free — but it identifies the storefront
> being *viewed*, not the restaurant the user *works for*. Staff visiting a competitor's subdomain would
> authorize as that competitor's staff. Host is tenant routing, never authorization.

**Consequence for this proposal:** cross-cutting read authorization (`ScopeMembership` + its projector
rules + the bridges) is a **blocking prerequisite**, tracked separately because the whole back office
needs it, not just attachments. Two operational conditions before the guard goes live: the membership
**backfill replay** must have run (otherwise every historical order's files 403), and membership writes
must commit in the **same transaction** as the projections they derive from. Shipping `/files` with
CUSTOMER + ADMIN only and failing RESTAURANT/RIDER closed was considered and rejected: it would 403 the
restaurant on the order photo it had just uploaded.

**Caching:** memoize membership per `(scope_type, scope_id)` in request extensions — a thread rendering 5
images then costs 1 check, not 5. Beyond the request, cache **negatives** freely but **positives**
briefly or not at all: the point of §3.3 is that reassignment revokes instantly, and a 60-second
positive cache hands the previous rider a 60-second window after they are off the job.

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

### 6.2 A daily worker deletes the OBJECT and keeps the ROW (decided 2026-07-25)

`sweep_retention()` is pure SQL, and pure SQL cannot reach the bucket. `DELETE FROM files WHERE
expires_at < now()` would delete the **row** and **leave the bytes in the bucket forever** — an
unbounded storage bill and, far worse, a GDPR failure that *looks* like compliance because the
database is clean.

**Product-owner decision (2026-07-25):** a `FileRetentionWorker` runs **once a day**, and for each row
past `expires_at` it **deletes the storage object only — the row is never deleted**, just marked
`deleted_at`. The row becomes a permanent tombstone.

This is simpler and strictly better than the grace-period variant first proposed here, for three
reasons:

- **410 is answerable forever.** There is no window in which a purged attachment degrades to 404
  because its row aged out, and no grace period to tune (§6.3).
- **Crash safety becomes trivial.** The sequence is: delete object → set `deleted_at`. A crash between
  the two leaves the object already gone and the marker unset; the next day's pass re-issues the delete
  (a no-op on a missing key — object deletion is idempotent) and sets the marker. No two-phase
  reconciliation, no orphan-hunting query.
- **The row is the audit record.** "An attachment existed here, and was purged on this date" is exactly
  what a retention audit needs to show, and it survives.

**The daily cadence is safe because the row — not the object — is the authority on policy.** The guard
checks `expires_at < now()` *as well as* `deleted_at` (§4.2 step 4), so a file stops being served the
second it expires, not up to 24 h later when the worker next runs. Object deletion is cleanup, never
the enforcement point. Getting this backwards would give every expired file a free extra day of life.

`sweep_retention()` keeps only a **documentary** `retention:` block for this table — it never deletes
from `files`.

**Unbounded row growth is not a concern at this scale:** a tombstone is a few hundred bytes, and Tours
V0 volumes put this in the low thousands of rows per year. Revisit if a `files` count ever approaches
the millions.

### 6.3 The event outlives the file — and that is correct

`MessagePosted.attachmentRefs` is in `domain_events`, which is **never swept** (the forever log). So a
90-day-old thread will hold a ref to bytes that no longer exist. This is the design working, not a bug:
the *fact* that a proof-of-delivery photo was posted is permanent audit history; the *personal data* in
it is not.

The contract that makes it usable: an expired or purged file returns **410 Gone**, distinguishable from
**404 Not Found**. The renderer shows "attachment expired" rather than a broken image or a misleading
"not found". Because the row is kept forever (§6.2), 410 stays answerable for the whole life of the
event that references it — the ref and its explanation never fall out of step.

### 6.4 Erasure on request

A data-subject erasure request must purge **before** expiry. Same worker, driven by an erasure request
over `scope_id` / `uploaded_by` — which is exactly why both are indexed. This slots into the erasure
story of #18 rather than inventing a parallel path.

One consequence of keeping the row forever (§6.2): the tombstone still carries `uploaded_by` (an auth
subject) and `scope_id`. Ordinary **expiry** leaves those in place — they are operational metadata
about an order, and the personal data (the image) is gone. A genuine **erasure request** is stronger
and must also **null `uploaded_by`**, keeping the row as an anonymous "an attachment existed and was
erased" record. Expiry and erasure are therefore not the same operation on the row, only on the object.

## 7. Completeness obligations (ADR-0032) and observability

**Rules** (each with ≥1 behaviour test, both directions enforced by `make validate`):

1. A file is served only to a principal whose role is in its recorded `audience` **and** who is a member of its `scope`.
2. An attachment is never readable by an audience broader than its message's visibility (§3.4).
3. An expired or purged file returns 410 and never bytes.
4. Domain events never carry file bytes — only refs.
5. An attachment ref whose extension does not match the stored file is not served.
6. A read never returns a row outside the caller's `ReadScope` — a customer cannot read another
   customer's order, a restaurant cannot read another restaurant's, a rider cannot read a job they are
   not assigned to (§4.5.1; product-owner position, 2026-07-25). This one is **cross-cutting**, not
   files-specific, and lands with the prerequisite issue.

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
  near-identical tables later. **Confirmed 2026-07-26**: reclamation evidence
  ([#156](https://github.com/TheCaptainCompany/captain-food/issues/156)) shipped against the same
  opaque `AttachmentRef` within a day of this proposal — the second owner type arrived before the
  first was even built. (A dedicated table would win only if the ACL differed per owner type —
  it does not; `audience` × `scope` covers all of them.)
- **(f) Serve straight from Supabase Storage with Postgres RLS.** Rejected: our authorization model
  lives in the JWT/role path (ADR-0047), not in RLS. Using RLS here would fork the ACL into two engines
  that must be kept in agreement — the exact failure mode ADR-0006 exists to avoid.
- **(g) A third-party asset/CDN service** (Cloudinary, Uploadcare). Rejected for the same reason the
  epic rejected a third-party chat SDK: it sends customer personal data to another processor, costs the
  "ethical alternative" positioning, and buys transformations we do not need yet.

## 9. Decisions this proposal asks the product owner to make

Each decision lists its options with **pros / cons** (standing directive, docs/proposals/README.md);
a settled choice is marked **→ CHOSEN**.

**9.1 How the bytes reach the client** (§4.4)
| Option | Pros | Cons |
|---|---|---|
| **302 → 60 s signed Supabase URL** *(recommended)* | Authorization stays entirely ours; egress stays Supabase's — no Render bandwidth or BFF request-time spent proxying photos, which matters when an operator scrolls a day of threads | For 60 s the signed URL is bearer-shareable and lands in browser history and intermediate logs |
| Stream through the BFF | Nothing leaks past the guard; one choke point for byte-level auditing | Pays egress + CPU on every image view; the BFF becomes a media proxy |

**9.2 Retention windows per kind** (§6.1)
| Option | Pros | Cons |
|---|---|---|
| **24 h orphan / 30 d `ORDER_PHOTO` / 90 d `DELIVERY_PROOF` / 180 d `RECLAMATION`** *(recommended)* | Each window matches its dispute horizon; 90 d aligns with the existing journal/mirror windows; 180 d outlives a card chargeback | Four values to remember; `RECLAMATION` holds personal data longest |
| One uniform window for all kinds | Trivial to reason about and to audit | Either over-retains doorstep photos or under-retains dispute evidence — the two have genuinely different horizons |

**9.3 Purge mechanism** — **DECIDED 2026-07-25** (§6.2)
| Option | Pros | Cons |
|---|---|---|
| **Daily worker deletes the OBJECT, keeps the ROW → CHOSEN** | 410 answerable forever; crash-safe by idempotence (delete object → set marker); the row is the retention audit record | Rows grow unboundedly (negligible at Tours scale — low thousands/year) |
| Delete object then row after a grace period | No tombstone growth | A grace period to tune, and a window where an expired attachment degrades to 404 instead of 410 |
| SQL-only sweep in `sweep_retention()` | Fits the existing retention machinery | **Wrong** — pure SQL cannot reach the bucket, so the bytes survive forever: an unbounded bill and a GDPR failure that *looks* like compliance because the database is clean |

**9.4 Type allowlist** (§4.3)
| Option | Pros | Cons |
|---|---|---|
| **`jpg`/`png`/`webp`, HEIC transcoded, SVG refused** *(recommended)* | Closes same-origin stored XSS (an SVG is a script container served next to the session cookie); HEIC is the iPhone camera default, so accepting it keeps rider proof-of-delivery working | Transcoding costs CPU on upload and needs an image library |
| Also allow SVG | Vector evidence, smaller files | A stored-XSS vector against the session — not acceptable for user-supplied bytes on our own origin |
| Refuse HEIC outright | No transcoding dependency | Breaks proof-of-delivery for roughly half the rider fleet |

**9.5 Size limits**
| Option | Pros | Cons |
|---|---|---|
| **10 MB per file, 5 files per message** *(recommended)* | Comfortably fits a modern phone photo; bounds abuse and storage per thread | A burst-shot rider may hit the 5-file cap |
| Larger / unbounded | Never blocks a legitimate upload | Storage cost and upload-time abuse scale with no ceiling |

**9.6 Moderation in V1**
| Option | Pros | Cons |
|---|---|---|
| **Manual report path + per-participant rate limits** *(recommended)* | Ships now; composes with the epic's mute; no third-party processor sees customer images | Abusive content is visible until reported |
| Automated scanning at upload | Catches it before anyone sees it | A third-party processor receives customer personal data (a GDPR + positioning cost), plus latency and false positives on food photos |

## 10. Scope of the implementation slice

**Blocked on:** [#144 "Read-side per-instance authorization: ReadScope on the read ports + RESTAURANT/RIDER identity bridges"](https://github.com/TheCaptainCompany/captain-food/issues/144), designed in [PROP-20260725-185140](PROP-20260725-185140-read-side-per-instance-authorization.md) — cross-cutting read authorization — `ReadScope` on the read ports + the RESTAURANT/RIDER
identity bridges + the **`ScopeMembership`** projection and its projector rules (§4.5). Tracked
separately; `/files` cannot enforce §3.3 for two of its four roles until it lands, and needs the
membership backfill replay done before the guard is switched on.

**In:** `specs/database/tables/files.yaml` + the three scalar enums · the `FileRetentionWorker` (daily,
object-only) · `POST /files` + `GET /files/<uuid7>.<ext>` in `crates/server` ·
the `PostMessage` audience-narrowing step (§3.4) · rules + behaviour tests + observability contracts.

**Out (unchanged, deliberately):** `AttachmentRef`, `MessagePosted`, the `Conversation` aggregate and
the `OrderConversation` projection — slice [#130](https://github.com/TheCaptainCompany/captain-food/issues/130)
needs **zero** rework. Also out: direct-to-storage upload, automated moderation, image
transformation/thumbnails, and any owner type other than `CONVERSATION_MESSAGE`.

## 11. Verification plan

- `make rust` green (build + test + validate + generate), `make validate` at **0 errors** with no new warnings.
- Behaviour tests for the five rules of §7, including the negative cases: customer → INTERNAL note's
  attachment ⇒ 403; wrong-order customer ⇒ 403; expired ⇒ 410; extension mismatch ⇒ 404; anonymous ⇒ 401.
- A retention test proving the daily worker **deletes the object and keeps the row**, that a crash
  before `deleted_at` is set is recovered idempotently by the next pass, and — the one that matters —
  that a row **past `expires_at` but not yet swept** is already refused with 410, so the daily cadence
  never grants an expired file an extra day of life.
