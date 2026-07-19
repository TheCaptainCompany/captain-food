<div align="center">

<img src=".github/assets/logo.png" alt="Captain.Food — a mustachioed chef-hatted skull over a crossed golden fork and knife, on a white card" width="190">

# Captain.Food

**Your dishes, your prices, your customers.**

Local-first food ordering & delivery for independent restaurants and food trucks.
**100 % of your orders. 0 % commission — for real.**

[![ci](https://github.com/Captain-Food/captain-food/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/Captain-Food/captain-food/actions/workflows/ci.yml)
[![db-migrate](https://github.com/Captain-Food/captain-food/actions/workflows/db-migrate.yml/badge.svg)](https://github.com/Captain-Food/captain-food/actions/workflows/db-migrate.yml)
[![render](https://img.shields.io/website?url=https%3A%2F%2Flive.captain.food%2Fhealth&up_message=live&down_message=down&label=render%20deploy&labelColor=0e3a5f&up_color=1c7a4d)](https://live.captain.food/health)
[![render commit](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2FCaptain-Food%2Fcaptain-food%2Fbadges%2Frender-deploy.json&labelColor=0e3a5f)](https://github.com/Captain-Food/captain-food/actions/workflows/render-status.yml)

[![join the crew](https://img.shields.io/badge/join.captain.food-%E2%9A%93%20come%20aboard-e8613a?labelColor=0e3a5f)](https://join.captain.food)
[![built with Rust](https://img.shields.io/badge/built%20with-Rust-a2402a?logo=rust&logoColor=fffdf8&labelColor=0e3a5f)](Cargo.toml)
[![license: Coopyleft](https://img.shields.io/badge/license-Coopyleft%20%28AGPL--3.0%20based%29-e0a12b?labelColor=0e3a5f)](LICENSE.md)

</div>

## The mission

<img src=".github/assets/captain.png" align="right" width="170" alt="The Captain — a mustachioed chef-admiral wielding a golden fork">

Delivery platforms take 25–30 % of every order from the restaurants that cook it. Captain.Food
gives independent restaurateurs their orders back: their own ordering site, their own prices,
their own customer relationship — with **zero commission**, governed as a cooperative of the
social and solidarity economy (ESUS/SCIC) so it can never turn against its members.

**V0** validates product–market fit in **Tours** (France), with a mobile-first web UX and a
backend that can evolve towards CQRS + an event log.

| Ports of call | |
| --- | --- |
| ⚓ [join.captain.food](https://join.captain.food) | the landing page for restaurateurs (the pitch, the model, the FAQ) |
| 🧭 [live.captain.food](https://live.captain.food/health) | the deployed V0 service |
| 🗺️ [`specs/`](specs/) | the domain & architecture model — the source of truth |
| 📜 [`docs/PLAYBOOK.md`](docs/PLAYBOOK.md) | the operating model |
| 🏴‍☠️ [`docs/adr/`](docs/adr/) | every decision, on the record |

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

The **ci** workflow (first badge) is the whole gate, on **every branch push and every PR**: it builds
the full Cargo workspace, runs the complete behaviour-test suite, runs the spec validator (must be
0 errors), then regenerates every artifact and fails on any spec↔generation drift — so
`specs/generated/` is always in sync. The **db-migrate** workflow (second badge) applies
`migrations/*.sql` only after `ci` succeeds on `main` (ADR-0043), and Render auto-deploys once the
checks pass — the **render deploy** badge probes the live service's `/health` (which also gates on
the migrated schema version), so green means deployed, migrated, and answering. The **render commit**
badge is the precise one: the `render-status` workflow asks the Render API for the latest deploy and
republishes it as `<status> @ <sha>` plus a `render/deploy` commit status on the exact deployed
commit (needs only the `RENDER_API_KEY` repo secret — the service id is discovered by name; skips gracefully until set).

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

---

<div align="center">
  <sub>⚓ Brand assets come from the <a href="https://join.captain.food">join.captain.food</a> landing page
  (<a href="https://github.com/Captain-Food/captain-food.github.io">captain-food.github.io</a>).</sub>
</div>
