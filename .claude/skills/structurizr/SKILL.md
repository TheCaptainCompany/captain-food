---
name: structurizr
description: >
  Visualize Captain.Food's architecture as C4 diagrams. Use when the user wants to see/render the
  system architecture, produce or export C4 (Structurizr DSL or Mermaid), open Structurizr Lite, update
  the architecture views, or edit bounded contexts / containers / components. The source of truth is
  specs/architecture/c4-l2.yaml + c4-l3.yaml; the DSL/Mermaid are GENERATED — never hand-edit them.
---

# Structurizr / C4 for Captain.Food

Architecture is **source-managed DSL** under `specs/architecture/` and validated by the codegen
(ADR-0008). Three views of the same model exist:

| Output | File | Renderer | Use |
|---|---|---|---|
| Structurizr DSL | `specs/generated/c4.generated.dsl` | Structurizr Lite / `structurizr-cli` | proper C4 auto-layout, export to PNG/SVG/PlantUML/Mermaid |
| Mermaid | `specs/generated/c4.generated.md` | GitHub / VS Code / mermaid.live | quick inline diagrams |
| Interactive SVG | `specs/generated/documentation.generated.html` §13 | any browser (zero-dependency) | drill-down System → container → context → aggregate flow |

## Generate

```bash
cd tools/codegen && npm run validate && npm run generate
# → specs/generated/c4.generated.dsl, specs/generated/c4.generated.md (+ the HTML doc's interactive map)
```

`validate` enforces C4 consistency: every aggregate/process-manager is mapped to a bounded context
(`c4-actor-unmapped`), and every `$ref` (container `realizes`, component `handles`/`updates`) resolves —
so the diagrams cannot drift from `actors.yaml`/`views.yaml`.

## Render the Structurizr DSL

Two make targets do this for you (both gracefully skip if the tooling is absent):

- **`make c4-render`** — opens **Structurizr Lite** at http://localhost:8080 (SystemContext, Containers,
  ApiComponents views) with the **ADRs and docs embedded** (it stages a docs-enriched workspace under
  `.structurizr/`, leaving the portable `specs/generated/c4.generated.dsl` clean). Needs Docker.
- **`make c4-export`** — parse-**validates** the DSL with the real Structurizr toolchain (structurizr-cli
  if installed, else the maintained `structurizr/structurizr` Docker image — NOT the deprecated
  `structurizr/cli`, which is now a no-op stub) and exports Mermaid into `out/`. Use this as the C4 gate —
  it catches any emitter syntax drift our brace check can't.

Manual equivalents:
```bash
# Lite
mkdir -p .structurizr && cp specs/generated/c4.generated.dsl .structurizr/workspace.dsl
docker run --rm -p 8080:8080 -v "$PWD/.structurizr:/usr/local/structurizr" structurizr/lite
# CLI export
structurizr-cli export -workspace specs/generated/c4.generated.dsl -format plantuml
```

### Validate WITHOUT Docker (no admin required)

`structurizr-cli` is a Java app (needs JDK 17+). Download the release zip once and run it with any
JDK 17 on the machine (e.g. a JetBrains JBR). `export` parse-validates the DSL — a non-zero exit means
the DSL is invalid. Example (PowerShell):
```powershell
# one-time: download + unzip structurizr-cli (≈94 MB)
Invoke-WebRequest https://github.com/structurizr/cli/releases/latest/download/structurizr-cli.zip -OutFile "$env:TEMP\structurizr-cli.zip"
Expand-Archive "$env:TEMP\structurizr-cli.zip" "$env:TEMP\structurizr-cli" -Force
# run with a JDK 17 (any will do)
& "<path-to-jdk17>\bin\java.exe" -cp "$env:TEMP\structurizr-cli\lib\*" `
  com.structurizr.cli.StructurizrCliApplication export `
  -workspace specs\generated\c4.generated.dsl -format mermaid -output "$env:TEMP\c4out"
```

## Render the Mermaid

`specs/generated/c4.generated.md` has two ```mermaid blocks (L2 containers; domain bounded-contexts → aggregates →
read models). It renders on GitHub, in VS Code (Mermaid extension), or at https://mermaid.live.

## What the model contains

- **L2** (`c4-l2.yaml`): `system`, `boundedContexts` (aggregates/process-managers by `$ref`),
  `containers` (runtime units; `realizes` aggregates), `externalSystems` (Stripe, HubRise, delivery,
  Supabase Auth), `relationships`.
- **L3** (`c4-l3.yaml`): `components` of the `api` container, each with `instrumented: true|false`
  (the observability boundary) and `handles` (aggregates) / `updates` (`View_*`).
- Structurizr tags: `Aggregate`, `ProcessManager`, `External`, `Instrumented`, `Domain` (styled in the
  generated `views { styles { … } }`). The api component view shows aggregates grouped by bounded
  context + the technical components wired by the canonical CQRS/ES pipeline.

## Editing workflow (do this, in order)

1. Edit `specs/architecture/c4-l2.yaml` / `c4-l3.yaml` (NOT the generated files).
2. Add a new aggregate → put it in a bounded context (and, if it runs in `api`, in a component's
   `handles`); add a new `View_*` → add it to `projection-updaters.updates`.
3. `cd tools/codegen && npm run validate` (must be 0 errors; only the 4 known view warnings are ok).
4. `npm run generate` to refresh the DSL/Mermaid/HTML map.

See also: `docs/claude/c4.md` (rules) and ADR-0008 / P-11.
