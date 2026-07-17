# ADR-0044 — Licensing: Captain.Food Coopyleft (AGPL v3 + cooperative-only commercial reservation)

## Status
Accepted

## Context
Captain.Food is built as a commons for the **social and solidarity economy** — its long-run vision
(SCIC/cooperative governance, Open Collective; see `docs/adr/HISTORY.md`) is explicitly *not* a
profit-extracting marketplace. The whole premise (0% commission on food, ADR-0016) is a reaction to
extractive intermediaries (Uber Eats, Deliveroo). We need a license that:

1. keeps the source **open** to study, run, modify and redistribute;
2. is **strong copyleft**, including over-the-network use, so hosted derivatives stay open;
3. **prevents capture** — i.e. does not simply hand the platform to the very extractive commercial actors
   we exist to displace.

Plain FOSS licenses don't satisfy (3): AGPL-3.0 gives network copyleft but permits *any* commercial use,
including by large platforms. CoopCycle solved the same problem with a **Coopyleft** license derived from
the **Peer Production License** — AGPL semantics plus a commercial-use reservation for the SSE.

## Decision
License the repository under the **Captain.Food Coopyleft License** (`LICENSE.md`), modeled on CoopCycle's
Coopyleft:

- It **adopts the GNU AGPL v3** (full text in `LICENSES/AGPL-3.0.txt`) for study/execution/modification/
  redistribution, **except** for the commercial-use reservation below.
- **Article 3** reserves **commercial use** to **cooperatives / non-profit / limited-profit organizations
  of the social and solidarity economy** (per EU and French `loi ESS` criteria; worker-employed cooperative
  model; wage-portage exception). Non-commercial use (study, research, teaching, contribution) is open to
  everyone.
- **Copyleft is preserved and un-relicensable:** derivatives must be distributed under this same license
  *including Article 3* — they may not be relicensed to plain AGPL or anything that drops the reservation.

**Cargo metadata:** there is no SPDX identifier for this custom license, so crates use
`license-file = "LICENSE.md"` (inherited via `license-file.workspace = true`) rather than the `license`
field. This replaces the placeholder `license = "UNLICENSED"`.

## Alternatives considered
- **AGPL-3.0 (plain)** — true OSI open source with network copyleft, but permits commercial use by anyone,
  including extractive platforms. Rejected: fails force (3), which is the political point of the project.
- **Permissive (MIT/Apache-2.0)** — rejected: no copyleft, trivially captured/closed.
- **Proprietary / source-available-only** — rejected: we want an open commons, contributions, and scrutiny.
- **Peer Production License directly** — effectively what CoopCycle's Coopyleft already packages on top of a
  modern strong-copyleft base; we follow CoopCycle's precedent for ecosystem familiarity.

## Consequences
### Positive
- License matches the mission: open commons, strong (network) copyleft, protected from capture.
- Familiar to the cooperative-tech ecosystem (CoopCycle lineage).

### Negative / caveats
- **Not OSI-approved "open source."** The commercial-use restriction fails the Open Source Definition
  (#6, no discrimination against fields of endeavour); it is **source-available / "copyfair."** Some
  contributors, distros, and tooling may treat it accordingly.
- **Not SPDX-identifiable**, hence `license-file` (some tools show "non-standard license"). A custom
  `LicenseRef-Captain-Food-Coopyleft` SPDX expression is an option later if tooling needs it.
- **Legal novelty / enforceability**: the SSE commercial reservation is not battle-tested law everywhere —
  **get legal review** before relying on it, and before first external contribution.
- **Contributor terms** (CLA/DCO) are not yet defined — needed so inbound contributions are licensed
  compatibly and Article 3 stays enforceable.

### Follow-up actions
- Obtain **legal review** of `LICENSE.md` (esp. Article 3 wording and enforceability in FR/EU).
- Decide **contributor licensing** (DCO sign-off vs CLA) before accepting external PRs.
- Add the per-file short header from `LICENSE.md` §"How to apply" to source files as the codebase grows.
- Optionally register a `LicenseRef-Captain-Food-Coopyleft` SPDX id if/when tooling requires it.

## References
`LICENSE.md`, `LICENSES/AGPL-3.0.txt`, README license section. Relates to ADR-0006 (BFF/ACL), ADR-0016
(0% food commission), and the SSE/SCIC vision in `docs/adr/HISTORY.md`. Based on the
[CoopCycle Coopyleft License](https://wiki.coopcycle.org/en:license).
