# Captain.Food

[![codegen-consistency](https://github.com/Captain-Food/captain-food/actions/workflows/codegen-consistency.yml/badge.svg?branch=main)](https://github.com/Captain-Food/captain-food/actions/workflows/codegen-consistency.yml)

Local-first food ordering & delivery for independent restaurants and food trucks.
**V0** validates product–market fit in **Tours**, with a mobile-first web UX and a backend that can
evolve towards CQRS + an event log.

## How this repo works

The **`specs/*.yaml` DSL is the source of truth**; everything else is **generated** and **derived** —
no LLM in the generation loop.

- **[`specs/`](specs/)** — the domain & architecture model: scalars, entities, events, commands, errors,
  actors (aggregates + process managers), views (read models), the GraphQL API surface, story map,
  behaviour tests, observability contracts, and C4 (`specs/architecture/`).
- **[`tools/codegen/`](tools/codegen/)** — a deterministic TypeScript generator/validator. It validates
  referential integrity + behaviour-test coverage + observability + C4 in one gate, then emits artifacts.
- **[`tools/codegen-rs/`](tools/codegen-rs/)** — the Rust port of the codegen (ADR-0034), at **parity**:
  same full validator + emitters, all artifacts byte-identical and the same issue set (CI-verified via
  `make rust`). The TypeScript codegen stays the blocking gate until it is retired.
- **[`specs/generated/`](specs/generated/)** — the committed generated artifacts: the GraphQL SDL, the
  `View_*` SQL DDL, the Structurizr/Mermaid C4, and the navigable product documentation
  (`documentation.generated.md` / `.html`). `tools/codegen/out/` is ephemeral build scratch.

```bash
cd tools/codegen
npm ci
npm run validate     # the single blocking gate — must be 0 errors
npm run generate     # regenerate every artifact from the specs
```

The **codegen-consistency** workflow (the badge above) runs `validate` + `generate` on every push/PR and
fails if the committed artifacts drift from the specs — so `specs/generated/` is always in sync. It runs
two jobs in parallel: `consistency` (TypeScript, the blocking gate) and `rust-codegen` (the Rust port —
`cargo build`/`test` + validate + generate + diff), keeping the two implementations in lockstep.

## Operating model

Planning is separate from execution, the DSL is never edited by execution loops, and the gates are
executable & blocking. See **[`docs/PLAYBOOK.md`](docs/PLAYBOOK.md)**, the topic rules in
[`docs/claude/`](docs/claude/), and the decisions in [`docs/adr/`](docs/adr/) (with the full
Nov 2025 – Jun 2026 history in [`docs/adr/HISTORY.md`](docs/adr/HISTORY.md)).

> Repository convention: all content is written in **English**.
