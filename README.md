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
- **[`tools/codegen-rs/`](tools/codegen-rs/)** — a deterministic Rust generator/validator (ADR-0034). It
  validates referential integrity + behaviour-test coverage + observability + C4 in one gate, then emits
  artifacts. (It began as a TypeScript tool, ported to Rust at parity — byte-identical artifacts + the same
  validation issue set — after which the TypeScript codegen was retired.)
- **[`specs/generated/`](specs/generated/)** — the committed generated artifacts: the GraphQL SDL, the
  `View_*` SQL DDL, the Structurizr/Mermaid C4, and the navigable product documentation
  (`documentation.generated.md` / `.html`). `tools/codegen-rs/out/` is ephemeral build scratch.

```bash
make validate     # the single blocking gate — must be 0 errors (needs a Rust toolchain: cargo)
make generate     # regenerate every artifact from the specs
```

The **codegen-consistency** workflow (the badge above) runs `validate` + `generate` on every push/PR and
fails if the committed artifacts drift from the specs — so `specs/generated/` is always in sync. Its single
`codegen` job builds + tests the Rust `tools/codegen-rs`, validates, regenerates, and diffs.

## Operating model

Planning is separate from execution, the DSL is never edited by execution loops, and the gates are
executable & blocking. See **[`docs/PLAYBOOK.md`](docs/PLAYBOOK.md)**, the topic rules in
[`docs/claude/`](docs/claude/), and the decisions in [`docs/adr/`](docs/adr/) (with the full
Nov 2025 – Jun 2026 history in [`docs/adr/HISTORY.md`](docs/adr/HISTORY.md)).

> Repository convention: all content is written in **English**.

## License

Captain.Food is released under the **Captain.Food Coopyleft License** — a copyleft license
inspired by [CoopCycle's Coopyleft](https://wiki.coopcycle.org/en:license). It adopts the
GNU Affero General Public License v3 for study, execution, modification and redistribution,
but **reserves commercial use to cooperatives, non-profit and limited-profit organizations**
of the social and solidarity economy. See [`LICENSE.md`](LICENSE.md) for the full terms and
[`LICENSES/AGPL-3.0.txt`](LICENSES/AGPL-3.0.txt) for the AGPL v3 text.
