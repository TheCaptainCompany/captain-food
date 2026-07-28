# Mermaid in Markdown — house rules

Diagrams are rendered by **GitHub's mermaid** (in `.md` files) and by the docs HTML emitter. Two repo
facts make a couple of characters unsafe inside ```mermaid blocks — get these wrong and the diagram
renders blank or half-drawn.

## Rules (inside ```mermaid fences only)

1. **No angle brackets `<` `>` in labels/messages.** GitHub renders mermaid labels as HTML, so `Order-<id>`
   is parsed as an unknown `<id>` tag and **disappears** (you get `Order-`). **Use curly-brace
   placeholders instead:** `Order-{id}`, `Cart-{id}`, `StripeEvent-{evt id}`, `DeliveryJob-{uuidv5(x)}`.
2. **Do NOT rely on `&lt;` / `&gt;` escaping.** The repo's Markdown formatter **decodes HTML entities**
   back to raw `<` / `>` on save (observed on `sagas.md`, `stripe-process.md`), which silently re-breaks any
   diagram you "fixed" by escaping. Curly braces are the only durable form.
3. **No bare comparison `<` / `>` either** (e.g. `position > checkpoint`, `|now−t| > 300s`). Reword:
   `position after checkpoint`, `|now−t| over 300s`.
4. **No `;` in label/message text** — mermaid can treat it as a statement separator. Reword or drop it.
5. **`<br/>` for line breaks is fine** — it's real HTML the renderer wants, and the formatter leaves it.

## Quick reference

| Instead of (breaks) | Write (safe) |
|---|---|
| `Order-<id>` / `Order-&lt;id&gt;` | `Order-{id}` |
| `position > pm:{Name} checkpoint` | `position after pm:{Name} checkpoint` |
| `reject \|now−t\| > 300s` | `reject \|now−t\| over 300s` |
| `do A; then B` | `do A, then B` |

Prose **outside** mermaid fences is unaffected: `` `Order-<id>` `` inside backticks renders literally and is
fine — this rule is strictly about ```mermaid blocks.

## Every sequence diagram carries a mermaid.live pan/zoom link (product-owner directive, 2026-07-28)

GitHub renders an inline mermaid block too small to read comfortably, and moving the diagram to its
own page renders it just as small (that approach was tried and rolled back the same day). The fix:
**the diagram STAYS inline in the doc** (readable in context, single source), and **directly under
the closing fence** it carries a link that opens the same source in mermaid.live's pan/zoom viewer:

```markdown
[Open this diagram with pan and zoom (mermaid.live)](https://mermaid.live/view#pako:…)
```

- The `#pako:` fragment **encodes the diagram source itself** (deflate + base64url of the editor
  state), so the link works with zero build tooling — but it also means the link **goes stale the
  moment the fenced block is edited**. A stale link silently shows an OUTDATED diagram, which is
  worse than no link: **regenerating the link is part of any edit to the block**, same discipline
  as regenerated artifacts.
- Regenerate with this snippet (stdlib only — prints one `view` URL per ```mermaid block in the file):

  ```bash
  python3 - <<'EOF' path/to/doc.md
  import base64, json, re, sys, zlib
  src = open(sys.argv[1], encoding='utf-8').read()
  for i, code in enumerate(re.findall(r'```mermaid\n(.*?)```', src, re.S), 1):
      state = json.dumps({"code": code.strip(), "mermaid": {"theme": "default"}})
      pako = base64.urlsafe_b64encode(zlib.compress(state.encode(), 9)).decode().rstrip('=')
      print(f"diagram {i}: https://mermaid.live/view#pako:{pako}")
  EOF
  ```

- Use the **`/view`** page (read-only pan/zoom); swap `view` for `edit` in the URL when you want
  the editor pane too.
- Applies to every **hand-written** doc (proposals, ADRs, docs/**) — retrofit a doc's diagrams
  whenever you edit it. Generated artifacts (`specs/generated/**`, `c4.generated.md`) are out of
  scope until their emitters adopt the same rule.

## Represent the architecture faithfully (hexagonal / ports & adapters)

A sequence diagram must show the **clean hexagonal architecture**, not the raw plumbing. Domain events are
**facts decided by an aggregate or process-manager — the actor** — that receives a message; they are
persisted **through the `Repository`** (the actor's write-side journal, ADR-20260719-031136). `PgEventStore`
/ the `domain_events` table is the **one adapter behind the Repository**, never the author of the facts.

- **Don't** draw `PgEventStore ->> domain_events: INSERT OrderPlaced` — that reads as the storage adapter
  inventing business events. And **don't** show `PgEventStore` *and* `domain_events` as two participants —
  **pick one** (the adapter); the table is inside it.
- **Do** draw, in order: **(1)** the actor *receives the message* (command/event) and *decides* the facts
  (pure, no I/O); **(2)** those facts are *saved through the `Repository`*; **(3)** the Repository appends
  via its **one** adapter (`PgEventStore`). The runner/handler is the imperative shell — it calls the
  Repository, never the `EventStore` port directly.
- **Group by hexagon layer** with `box`: **application core** (the aggregate/PM decision + the `Repository`
  it depends on) vs **infrastructure adapters** (the runner, `PgEventStore`, external clients). The core
  depends on the port; the adapter implements it — the dependency arrow points inward.

Canonical write leg:

```
box application core
    participant AGG as PlaceOrderProcess (decides — pure)
    participant REPO as Repository (actor journal)
end
box infrastructure adapters
    participant PG as PgEventStore (→ domain_events)
end
AGG-->>REPO: save(OrderPlaced, CartCheckedOut)   %% the actor's decided facts
REPO->>PG: append (behind the port) — UNIQUE(stream, version)
Note over REPO,PG: Repository is the port the core depends on — PgEventStore is the one adapter behind it
```

Apply this to **every** sequence diagram (write legs, inbound-ACL legs, saga reactions). The one exception:
a **non-aggregate idempotency envelope** (the Stripe `StripeEvent-{id}` fact) records through the low-level
journal, not the Repository — label it as such.
