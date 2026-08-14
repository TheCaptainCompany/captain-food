# PROP-20260814-000240 — Strix autonomous pentest: a first gated, sandboxed, defensive run

- **Status**: Proposed
- **Date**: 2026-08-14
- **Tracking issue**: [#548 "Evaluate Strix for a gated pre-launch DAST pass against our own endpoints (authorized defensive)"](https://github.com/TheCaptainCompany/captain-food/issues/548)
- **Realized by**: _(filled at completion)_
- **Concerns**:
  - [ ] sandbox-containment: the run executes third-party exploit code — it lands only under the D-A containment (throwaway container, no repo write, no real secrets, dev-only egress)
  - [ ] quota-safety: an autonomous looping agent must not be pointed at the shared Claude account quota without a bounded scope + token cap (D-B)
  - [ ] compliance-framing: the output is a pentest evidence pack for counsel, NEVER a PCI/RGPD certificate or clearance (D-C)

> **Framing (hard line, up front).** This is **authorized defensive security testing of our own
> pre-launch product against our own endpoints.** Every option below is scoped to that. No lens
> output in this document is legal advice or security clearance (ADR-20260812-143619); the artifact a
> run produces is **evidence for counsel and for the team**, not a certificate.

---

## 1. Context

Strix ([github.com/usestrix/strix](https://github.com/usestrix/strix)) is an autonomous AI
penetration-testing agent of the DAST/agentic class: it drives a running application, forms and
executes exploit attempts, and reports what it could reach. Its heuristics are built for **interpreted
web stacks** (Python/JS/PHP source plus a running app). Our backend is **full-stack Rust** with a
**generated** GraphQL surface, schema-boundary `@auth`, tenant-by-`Host` middleware, typed `sqlx`, and
an event store. The value question is therefore not "is Strix good" but "**where does a black-box
agent add signal our own gates cannot already produce**".

**Verified baseline — what we already enforce (do not let a scan re-file these as new):**

| Control | Where | Evidence |
|---|---|---|
| **Fail-closed JWT verifier**, type-level (no "skip issuer" state is spellable; `503` when unconfigured) | auth verifier | `crates/server/src/auth.rs:362-428` (`Verifier`, `validation`), `:602-613` (`verify`), `#519`/`#520` |
| **Product-proof claim** (a valid Supabase token from a sibling product is refused `403`, never defaulted to CUSTOMER) | auth grant | `crates/server/src/auth.rs:344-355` (`AppMetadata::grant`), `:810-820` (`parse_role` fails closed) |
| **`alg`-confusion closed** (asymmetric families only; alg taken from the JWK, not the header) | auth | `crates/server/src/auth.rs:777-800` |
| **Tenant is an authorization input from `Host`, fail-closed** (`current` is zero-argument; unknown/failed lookup ⇒ `TenantScope::None` = no rows, never "newest anywhere") | tenant middleware | `crates/server/src/graphql/tenant.rs:30-95` |
| **Read-side per-instance authz** (`ReadScope` on read ports) | | `#144` (open), read scope derived from verified claims only |
| **OTP send guards**: fail-closed `+33`-style allowlist, per-number caps, global daily ceiling + no-deploy kill switch, shared atomic counter | sms quota | `specs/database/tables/integration_connections.yaml:45-79`, `crates/application/src/sms_guard.rs`, `#516`/`#523`/`#535`, ADR-20260813-021500 |
| **SMS spend is compiler-gated** (`OvhSmsClient::send` takes `AuthorizedSmsRecipient` **by value** — one claim buys one send) | | ADR-20260813-021500 |
| **Typed `sqlx`, no string-concatenated SQL** | infra | query ports are typed; no dynamic SQL assembly in the write/read paths |

A scan "discovering" any of the above, or **`#508` (HubRise `access_token` stored plaintext at
`specs/database/tables/integration_connections.yaml:91`, and thereby copied into WAL archives —
already triaged)**, is **not a new finding**. These four issues (`#508`, `#519`/`#520`, `#516`,
`#523`) are declared inputs to any run so it cannot re-file them.

**Verified GAPS a running-endpoint DAST could actually reach (the reason to consider Strix at all):**

| Gap | Evidence | Why our gates miss it |
|---|---|---|
| **No GraphQL depth / complexity / cost limit anywhere** | zero hits for `limit_depth`/`limit_complexity`/`depth`/`complexity` across `crates/server/src/graphql` | The validator checks spec↔generation correctness, not runtime request cost; a deeply-nested or aliased query at Friday 19:00–21:30 peak is a pure-runtime property nothing static asserts |
| **Introspection is filtered per role but NOT disabled**, and Voyager endpoints are served | `crates/server/src/graphql/routes.rs:308-333`, `graphql_acl.rs` (introspection filtered) | Per-role filtering is by design; whether the *composition* of filters leaks a reachable-but-unintended field is a runtime probe, not a spec check |
| **Tenant spoofing depends on an ingress precondition the code cannot enforce**: `X-Forwarded-Host` is client-forgeable and the ingress MUST overwrite it | `crates/server/src/graphql/tenant.rs:43-60` (documented infra precondition) | This is a deployment setting; only a request to the **running ingress** proves whether it holds. Blast radius is bounded (caller's own/held-session cart), but it is exactly a DAST target |
| **Cross-tenant / cross-role IDOR is a runtime composition** of verifier + `ReadScope` + tenant scope | the three controls above are individually verified; their *product* under real tokens is not | "Tenant A's token cannot read Tenant B's orders/payments" and "a CUSTOMER token cannot reach `/restaurant`/`/admin`" are properties of the running system under real tokens, not of any single unit |
| **SSRF on outbound adapters** (HubRise, Uber Direct, CoopCycle, avelo37) | adapter outbound calls exist per integration specs | Whether an attacker-controlled input can steer an outbound call is a black-box runtime property |
| **Secret exposure in responses / logs / errors** (Stripe, Supabase, OVH SMS) | secrets are broadly distributed to pods (e.g. `adapter-stripe` carries 13 secrets, `bam` 18 — §30 finding) | Whether any surfaces in an error body is observable only by exercising error paths on a running target |

**Consequence, stated plainly:** our strongest controls are compiler- and validator-enforced, which
means the residual risk has migrated to exactly the places static gates do not look — **runtime
authorization composition, request-cost, ingress configuration, outbound-call steering, and error-path
leakage.** That is a real DAST-shaped surface. It is also narrow, and most of it is reachable by a
cheaper, deterministic harness (see D1).

---

## 2. Recommended approach

**GO-NARROWLY**, in this sequence:

1. **Approve the containment first (D-A)** — nothing runs until the throwaway-container, no-repo-write,
   no-real-secrets, dev-egress-only posture is in place. This is the gate-then-stabilize / compiler-first
   discipline applied to *tooling* risk: the run cannot touch anything it is not explicitly handed.
2. **Bound the run and its LLM budget (D-B)** before pointing an autonomous looping agent at a shared
   Claude quota we already exhausted once tonight.
3. **Run against a DEV target only** (D-Scope): dev host under the agent proxy, Stripe **test** keys,
   OVH SMS kill-switch/allowlist **on**, synthetic identities, **never prod**.
4. **Feed the known-findings list in** so `#508`/`#519`/`#520`/`#516`/`#523` cannot be re-filed as new.
5. **Treat every output as PoC-required-before-treat** (D-Scope / beck lens): a finding is real only
   with a reproducing request; false positives are **listed, not hidden**.
6. **Ship the result as a pentest evidence pack for counsel (D-C)** — never a compliance certificate.

**The honest refinement:** the highest-value targets (cross-tenant IDOR, cross-role escalation,
GraphQL cost, tenant spoofing) are all **enumerable and deterministic** — they are better served by a
small, permanent, in-repo **authz-matrix + cost-limit test suite** that runs in CI every push than by
a one-off autonomous scan. Strix's genuine additive value over that suite is **breadth on the
unenumerated** (SSRF steering, error-path secret leakage, chained exploits we did not think to assert).
So the recommendation is **GO-NARROWLY as a one-off breadth pass whose confirmed findings become
permanent deterministic tests** — the scan is a discovery instrument, not a standing gate.

---

## 3. Decisions surfaced

### D1 — Is Strix worth it on OUR stack? (the value question)

| Option | Pros | Cons |
|---|---|---|
| **GO-NARROWLY — one-off black-box DAST pass against the running per-role endpoints; confirmed findings become permanent in-repo tests** ✅ **recommended** | Adds real signal on exactly the surface our gates miss (runtime authz composition, GraphQL cost/depth, ingress-dependent tenant spoofing, SSRF on adapter outbound, error-path secret leakage); breadth on the *unenumerated* is its true edge; a defensive evidence pack is genuinely useful to counsel pre-launch | Costs containment + quota engineering; most high-value targets are also reachable deterministically (see below), so part of the value is duplicable more cheaply; white-box Rust scanning it will also attempt is noise |
| GO-BROADLY — adopt Strix as a standing/recurring scanner incl. white-box source scanning | Continuous coverage | Its Rust source heuristics do not map (expect high noise); a looping agent as a standing cost on a shared LLM quota is a real bill; duplicates CI's own gates for the deterministic part |
| NOT-WORTH-IT — rely on the compiler + validator + a hand-written authz/cost test suite only | Zero tooling risk, zero quota cost, everything deterministic and in CI | Leaves the *unenumerated* surface (SSRF steering, error-path leakage, chained exploits) unprobed by anything adversarial before launch; "we asserted what we thought of" is not the same as "an adversary tried" |

**Verdict: WORTH-IT-NARROWLY.** GO for a **single gated sandboxed breadth pass**; do **not** adopt it
as a standing scanner, and do **not** rely on its white-box Rust output. Whatever it confirms is
migrated into deterministic CI tests (the durable artifact); the scan itself is a discovery run.

### D-A — Install / sandbox posture (team-owned engineering call; recorded here, not in the register)

Strix installs via a `curl | bash`-style flow and executes exploit code. The safe option strictly
dominates, so this is a team-ownable engineering discipline decision, not a founder option space.

| Option | Pros | Cons |
|---|---|---|
| **Pinned version + checksum-verified artifact, throwaway container, NO repo write access, NO real secrets, egress allow-listed to the dev target only** ✅ **recommended** | Containment by construction — the run cannot reach anything it was not handed; matches compiler-first/gate-then-stabilize applied to tooling; reproducible (pinned + checksum) | Setup cost (a container image, an egress allow-list); slightly slower first run |
| `curl \| bash` on a dev workstation with ambient credentials | Fastest to start | Executes third-party exploit code with the operator's real tokens, repo write access and open egress — an unacceptable blast radius for an agent whose whole job is to break out of boundaries |
| Managed/hosted Strix cloud (if offered) | No local containment work | Sends our endpoint topology and any captured responses to a third party; wrong data-residency posture for a pre-launch French product; defer until self-hosted containment is understood |

### D-B — STRIX_LLM provider + quota (FOUNDER-OWED — register row STRIX-2)

The available provider is Claude via the agent proxy. We **hit the account API usage limit tonight**
with our own agents (reset 22:40 UTC). An autonomous **looping** pentest agent on the same quota is a
real cost/rate risk that can starve the team's own loop.

| Option | Pros | Cons |
|---|---|---|
| **Bounded run: explicit scan scope + hard time cap + hard token cap, on the shared proxy** ✅ **recommended** | No new provisioning; the cap makes the spend predictable and the run interruptible; fits "one-off breadth pass" | Still draws on the shared quota during a launch crunch — the cap must be low enough that a runaway loop cannot exhaust it |
| Separate budget / dedicated key for Strix | Isolates the pentest spend from the team loop entirely | Admin-gated provisioning; a second key to manage and revoke; overkill for a single run |
| Defer the run until after the quota pressure clears | Zero contention now | Delays the evidence pack; the pre-launch window is finite |

**Recommendation: bounded run with explicit scope + time cap + token cap.** Escalate to a separate key
only if a first bounded run shows the cap is too tight to finish.

### D-C — Compliance framing (LEGAL LENS, hard line — no option space, stated as a constraint)

The artifact a run produces **IS**: a **pentest evidence pack** — a list of attempted attacks, which
reached, PoCs for the confirmed ones, and remediations — for **counsel and the team**, as evidence of
an **authorized defensive test** of our own pre-launch product.

It **IS NOT**, and may never be presented as: a **PCI-DSS or RGPD compliance certificate**, an
attestation, or security **clearance**. Our **PCI scope is Stripe-mediated** (payment-agent posture:
card data never transits our servers), so a scan does not and cannot certify PCI. **RGPD erasure
endpoints get TESTED** (does erasure actually erase; does a tombstone leak) — they are **not
certified**. No lens agreement in this document upgrades a hedged finding to a settled legal one
(ADR-20260812-143619).

### D-Scope — Target + assertions of a first run (farley + beck lenses; team-owned, recorded here)

- **Target**: a **DEV host under the agent proxy** — never prod (no live Stripe/OVH/partner creds).
- **Payments**: Stripe **TEST** keys only.
- **SMS**: OVH kill-switch on + allowlist on (`#523`) — a scan must not be able to spend a real euro.
- **Identities**: **synthetic** customer/restaurant/rider/admin tokens.
- **Auth-bypass probing**: any bypass attempt goes through the **REAL fail-closed verifier**
  (`crates/server/src/auth.rs`) using **admin-minted or local-issuer test tokens** — the verifier is
  **never weakened** to let the scan in; a scan that needs a weakened verifier to find something has
  found nothing about production.
- **What a finding must ASSERT (beck)**: PoC-**required**-before-treat — a reproducing request, or it
  is not a finding; **false positives are listed, not hidden**; a "pass" for a probe means "the
  attempted exploit was refused with the expected status and no data crossed a boundary", not "no
  error was thrown".

---

## 4. Screen / operator-view mockups

This is tooling, not a user-facing feature, so per the brief the load-bearing artifact is the
**run-flow sequence diagram (§5)**, not screens. The only operator surfaces are the run harness
invocation and the evidence-pack shape; both are shown so the review can see what an operator handles.

**Operator run harness (containment made visible):**

```
$ ./tools/security/strix-run.sh            # NOT part of this proposal to build; shape only
  target:        https://dev.captain.food   (agent-proxy dev host)  [REQUIRED, must resolve to dev]
  container:     strix@<pinned-digest>      (checksum verified)     [throwaway]
  repo-write:    DENIED                                              [mounted read-only or not at all]
  secrets:       NONE                        stripe=TEST  ovh=KILL-SWITCH-ON
  egress:        allow -> dev target only    deny  -> everything else
  llm:           claude via agent proxy      token-cap=<N>  time-cap=<T>   [D-B bound]
  known-findings feed: #508 #519 #520 #516 #523   (suppress as "already triaged")
  --------------------------------------------------------------------
  REFUSES TO START if: target is prod-shaped | any real secret present | egress unrestricted
```

**Evidence-pack shape (the deliverable, for counsel):**

```
+---------------------------------------------------------------+
| Strix pentest evidence pack -- dev.captain.food -- 2026-08-14 |
| AUTHORIZED DEFENSIVE TEST. NOT a PCI/RGPD certificate.         |
+---------------------------------------------------------------+
| Attempted        | Reached? | PoC        | Maps to            |
|------------------|----------|------------|--------------------|
| cross-tenant IDOR| NO       | req+resp   | tenant.rs fail-clsd |
| cross-role esc.  | NO       | req+resp   | auth.rs role gate   |
| GraphQL depth abu| REACHED  | req (500ms)| NO cost limit (gap) |
| SSRF adapter out | ?        | req        | needs triage        |
| secret in error  | NO       | req+resp   | -                   |
|------------------|----------|------------|--------------------|
| FALSE POSITIVES (listed, not hidden): 3  -> see appendix       |
| KNOWN/SUPPRESSED (#508 #519 #516 #523): 4                      |
+---------------------------------------------------------------+
| Each REACHED row -> a permanent deterministic CI test (D1).   |
+---------------------------------------------------------------+
```

---

## 5. Sequence diagram — the gated run flow

```mermaid
sequenceDiagram
    autonumber
    actor Op as Operator (coordinator)
    participant Sbx as Throwaway container (D-A)
    participant Strix as Strix agent
    participant Proxy as Agent proxy (Claude, D-B cap)
    participant Dev as DEV target (/{role}/graphql, /auth/*)
    participant Adp as Outbound adapters (MOCKED: HubRise/Uber/CoopCycle/avelo37)

    Op->>Sbx: launch pinned+checksummed image (no repo write, no real secrets)
    Sbx->>Sbx: assert target is DEV, egress allow-list = dev only, else REFUSE
    Op->>Strix: scope + known-findings feed (#508/#519/#520/#516/#523) + time/token cap
    loop bounded by time + token cap (D-B)
        Strix->>Proxy: reasoning / next-attack request
        Proxy-->>Strix: plan (spend counts against cap)
        Strix->>Dev: exploit attempt (synthetic tokens via REAL fail-closed verifier)
        Dev-->>Strix: response (status + body)
        alt outbound-triggering input (SSRF probe)
            Dev->>Adp: adapter call (mocked; egress denied beyond dev)
            Adp-->>Dev: canned response
        end
        Strix->>Strix: capture PoC (req+resp) IFF a boundary was crossed
    end
    Strix-->>Op: evidence pack (reached / not-reached / false-positives listed)
    Op->>Op: PoC-before-treat triage; suppress known findings
    Op->>Dev: each CONFIRMED finding -> permanent deterministic CI test (D1 durable artifact)
    note over Op,Dev: Output = evidence for counsel. NOT a PCI/RGPD certificate (D-C).
```

---

## 6. Alternatives considered for the cluster as a whole

- **Do nothing / rely on gates only.** Rejected as the *sole* posture: leaves the unenumerated
  adversarial surface (SSRF steering, error-path leakage, chained exploits) unprobed before launch.
  But it is *most* of the right answer for the enumerable part — which is why the recommendation folds
  it in as the durable output (confirmed findings become deterministic CI tests).
- **Adopt Strix broadly / recurring, including white-box Rust scanning.** Rejected: Rust source
  heuristics do not map (noise), and a standing looping agent on a shared LLM quota is an ongoing bill
  duplicating CI's deterministic gates.
- **Hand-written authz-matrix + GraphQL cost-limit suite only, no agent.** Strong and recommended **in
  addition** — it is the durable half. It does not replace the one-off breadth pass, because you can
  only assert what you already imagined; the agent's edge is the attacks you did not enumerate.
- **The recommended cluster**: one gated, sandboxed, bounded breadth pass on a dev target → triage
  PoC-before-treat → migrate confirmed findings into permanent CI tests. Agent as discovery
  instrument, deterministic tests as the standing gate.

---

## 7. Verification plan — what a "pass" means

- **A finding is real** only with a reproducing request/response PoC (beck: PoC-before-treat). No PoC ⇒
  not a finding.
- **False positives are enumerated in the pack**, never silently dropped.
- **A probe "passes"** when the attempted exploit is refused with the expected status **and** no data
  crossed a boundary — not merely "no 500".
- **Regression capture is the deliverable**: each confirmed finding becomes a deterministic test:
  - cross-tenant IDOR / cross-role escalation → an authz-matrix test (tenant A token vs tenant B rows;
    CUSTOMER token vs `/restaurant`,`/admin`) that **must fail on a build lacking the control**.
  - GraphQL depth/complexity → a cost-limit test asserting a bounded-depth ceiling (this **fails
    today** — there is no limiter, `crates/server/src/graphql` has zero `depth`/`complexity` hits — so
    it doubles as the proof the finding was real).
  - SSRF / error-path secret leakage → a negative test on the specific adapter input / error path.
- **Observability tie-in (observability lens)**: the run should generate load that exercises the
  documented telemetry; a REACHED GraphQL-cost finding is also an argument for a
  `specs/observability.yaml` contract on request cost/depth at peak, which does not exist today.

---

## 8. Open questions for the founder

1. **STRIX-1 (D1 + adoption GO/NO-GO)** — Approve a **single gated, sandboxed, bounded** Strix breadth
   pass against a **dev** target, framed as a **defensive pentest evidence pack for counsel**, with
   confirmed findings migrated into permanent CI tests? _Recommendation: **GO-NARROWLY**._
2. **STRIX-2 (D-B, quota)** — Run it on the **shared Claude quota with a hard time + token cap**, or
   provision a **separate key/budget**, or **defer** until quota pressure clears? _Recommendation:
   **bounded run on the shared quota**; escalate to a separate key only if the cap proves too tight._

D-A (sandbox posture), D-C (compliance framing) and D-Scope (dev target + assertions) are **team-owned
engineering/legal-posture constraints recorded here**, not founder option spaces — the safe option
dominates in each.

---

## Drawbacks (why we might regret the whole thing)

- **Tooling risk is real**: an agent whose purpose is to break boundaries is being run at all. The D-A
  containment is load-bearing; if it is done loosely the cure is worse than the disease.
- **Quota contention**: even bounded, it draws on a quota the team's own loop needs — timing matters.
- **Noise tax**: expect Rust white-box false positives; triage effort is non-trivial and must not be
  skipped (PoC-before-treat is the guard).
- **False assurance**: the biggest risk is someone reading the pack as "we passed security". D-C exists
  precisely to prevent that; the framing must travel with the artifact.

## Unresolved questions (copied to the tracking issue on approval)

- The exact **time cap and token cap** for a bounded run (D-B) — a number only a first dry-run can
  calibrate; instrument-then-decide.
- Whether a **`specs/observability.yaml` request-cost/depth contract** should be filed regardless of
  the run (the GraphQL cost gap is confirmed independently of Strix).
- Whether the durable **authz-matrix + cost-limit CI suite** is filed as its own issue now (it is the
  higher-leverage half and does not depend on Strix approval).

## Consulted

- **architect**: GO-NARROWLY — verified our strongest controls are compiler/validator-enforced, so the
  residual risk sits in runtime authz composition, request cost, ingress config, outbound steering and
  error-path leakage; the agent's real edge is breadth on the unenumerated, and confirmed findings must
  become deterministic CI tests (the durable artifact).
- **farley**: the run is safe only behind the D-A containment (throwaway container, no repo write, no
  real secrets, dev-only egress) and a dev target with real fail-closed verifier + minted test tokens;
  the harness must REFUSE to start against a prod-shaped target or with any real secret present.
- **legal-specialist**: the artifact is a defensive pentest evidence pack for counsel; PCI scope is
  Stripe-mediated so nothing here certifies PCI; RGPD erasure endpoints are TESTED not certified; no
  lens agreement is clearance (ADR-20260812-143619). Authorized-test posture only.
- **beck**: PoC-required-before-treat; false positives listed not hidden; a "pass" is "exploit refused
  with expected status and no boundary crossed", not "no 500"; every REACHED row becomes a test that
  fails on a build lacking the control.
- **observability**: in-lens — a REACHED GraphQL-cost finding is also the argument for a missing
  `specs/observability.yaml` request-cost/depth contract; the run should be observable as itself.
- **dba**: in-lens on one point — `#508` (plaintext `access_token` in `hubrise_connections`, in WAL) is
  a declared known input, not a new finding; a scan touches only the dev database.
- **business-specialist**: nothing in my lens beyond timing — run it in the pre-launch window, not
  during Friday peak.
- **ux**: nothing in my lens (tooling, no user surface).

## 9. Refs

- `crates/server/src/auth.rs:344-355,362-428,602-613,777-800,810-820` — fail-closed verifier, product
  claim, `alg`-confusion defence (`#519`/`#520`).
- `crates/server/src/graphql/tenant.rs:30-95` (fail-closed tenant), `:43-60` (the `X-Forwarded-Host`
  ingress precondition).
- `crates/server/src/graphql/routes.rs:308-333` — Voyager/introspection endpoints (filtered, not
  disabled).
- Zero hits for `depth`/`complexity` in `crates/server/src/graphql` — no GraphQL cost limiter.
- `specs/database/tables/integration_connections.yaml:45-79` (OTP quota, `#516`/`#523`), `:81-95`
  (`hubrise_connections.access_token` plaintext, `#508`).
- `crates/application/src/sms_guard.rs`, ADR-20260813-021500 — SMS kill-switch / allowlist, compiler-
  gated spend.
- [#508 "HubRise access_token stored plaintext, copied into WAL"](https://github.com/TheCaptainCompany/captain-food/issues/508) ·
  [#519 "A token must prove the product, not only the provider"](https://github.com/TheCaptainCompany/captain-food/issues/519) ·
  [#520 "fix(auth): a token must prove WHO issued it and WHAT it is for"](https://github.com/TheCaptainCompany/captain-food/issues/520) ·
  [#516 "The OTP endpoint is anonymous with no rate limit and no country allowlist"](https://github.com/TheCaptainCompany/captain-food/issues/516) ·
  [#523 "SMS kill-switch / allowlist"](https://github.com/TheCaptainCompany/captain-food/issues/523) ·
  [#144 "Read-side per-instance authorization"](https://github.com/TheCaptainCompany/captain-food/issues/144) ·
  [#178 "Write-side per-instance authorization"](https://github.com/TheCaptainCompany/captain-food/issues/178)
- ADR-20260812-143619 (no lens output is legal clearance) · gate-then-stabilize / compiler-first
  directives (CLAUDE.md).
