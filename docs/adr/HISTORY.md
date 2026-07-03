# Decision history — Captain.Food (Nov–Dec 2025)

This file integrates the **complete ADR history** from the kDrive working folder
(`Work/Captain Food/`) into the repo, so the project's decision lineage lives next to the code.

It is a **catalog, not a set of individual ADRs**. The historical docs each renumber `ADR-001…` for a
**different vision** (six clashing numbering schemes — see below), and the founder *"changed his mind a
few times"*. Reproducing ~50 conflicting-numbered files would be dishonest about what is actually live.
Instead, every historical decision is recorded here with its **original source/ID/date** and its
**current status** relative to the repo's active ADRs ([`README.md`](README.md), `0001`–`0026`).

The repo's numbered ADRs are the **source of truth for what holds today**:
`0001`–`0015` (technical / operating-model, realized in the repo) and `0016`–`0026`
(product/business, June 2026). This catalog is the **archaeology** behind them.

## Status legend

| Mark | Meaning |
|---|---|
| ✅ **Active** | Still holds; reflected in the current specs/repo (often promoted to a numbered ADR). |
| 🔧 **Partly active** | Principle survives; specifics changed (link to the superseding ADR). |
| ⛔ **Superseded** | Replaced by a later/numbered decision (link). |
| 🗑 **Obsolete** | Belongs to an abandoned vision; not pursued. |
| ⏸ **Deferred** | Valid but post-V0 (not in the current walking skeleton). |
| 📄 **Context** | Business/funding/analysis, not an architecture decision; kept for record. |

---

## Vision evolution (the "I changed my mind a few times" thread)

Six documents, six restarts of the ADR numbering, each a distinct strategic frame:

1. **2025-11-05 — Crypto / Web3 co-op.** Rust + Vue + Stellar stablecoins (EURC/USDC), a future
   cooperative stablecoin (EURWSKY), crypto KYC on-ramp (Privado/Transak), SCIC governance.
2. **2025-11-06 — Marketplace economic model.** Stripe Connect 3-way split, restaurant **margin
   guarantee**, dynamic Avelo37 delivery cost, **10% commission**, Open Collective transparency, tips,
   events. ← *this is the economic backbone the June-2026 ADRs evolved from.*
3. **2025-11-12 — Focus Tours pivot.** Drop all expansion (Paris/Lyon/SCIC federation/CoopCycle);
   **100% Tours, M0–M18**. Commission reworked **CA-based** (10/8/6% by monthly revenue); HubRise offered
   free; KDS via HubRise OrderLine instead of native. ← *this is the frame the current repo V0 sits in.*
4. **2025-11-27 — White-label SaaS for restaurants.** Google Workspace, `.restaurant` domains owned by
   Captain.Food, **WordPress (Cloudways) + Nuxt (Vercel)**, accounting integrations, physical terminals,
   mandatory offline mode. ← *largely a different product; mostly superseded/deferred.*
5. **2025-12-05 — Growth-led product (Phase 2).** **Captain Groups** (viral group ordering, pitched as
   the core MVP) + **Captain Coins** (gamified local loyalty), financing strategy, 18-month roadmap.
   ← *deferred product directions, not in the current V0 specs.*
6. **2026-06 — Current local-first marketplace.** **0% commission on food + proportional service fee**,
   Sirene/Google pre-registration, Google Business Profile button, Uber Eats price comparison, AI
   tooling. ← *recorded in the repo as ADRs `0016`–`0026`; technical/operating-model as `0001`–`0015`.*

### Two timelines worth tracking across the pivots

**Commission model** — the single most-revised decision:

| Date | Model | Status now |
|---|---|---|
| 2025-11-05 | 10% flat (vs Uber 30%) | ⛔ → 0% + service fee |
| 2025-11-06 | 10% fixed + margin guarantee | ⛔ → 0% + service fee |
| 2025-11-12 | **CA-based progressive** 10% / 8% / 6% by monthly revenue | ⛔ → 0% + service fee |
| 2025-12-05 | "5–8%" (positioning vs competitors) | ⛔ → 0% + service fee |
| 2026-06 | **0% on food + proportional Captain service fee** | ✅ [`0016`](0016-proportional-captain-service-fee.md) |

**Tech stack** — settled away from both crypto and WordPress:

| Date | Stack | Status now |
|---|---|---|
| 2025-11-05 | Rust + Vue/Alpine + Stellar/Soroban + Lightning, CleverCloud/OVH | 🗑 Obsolete |
| 2025-11-20 | Reject WordPress → confirm **custom Nuxt + Rust** | 🔧 "custom not WordPress" survives; Nuxt/Rust did not |
| 2025-11-27 | **WordPress (Cloudways) + Nuxt (Vercel)** + GraphQL/Apollo | ⛔ WordPress→marketing-only; see below |
| current | **Next.js + Node/TS (Hono/NestJS) + GraphQL + PostgreSQL/Supabase**, CQRS-light + event log | ✅ CLAUDE.md / repo |

GraphQL (introduced 2025-11-06/27) and HubRise-as-hub are the two stack ideas that **carried through** to
the repo ([`0006`](0006-graphql-role-as-path-acl.md), `specs/integrations/hubrise.md`).

---

## 1. 2025-11-05 — Crypto / Web3 co-op vision
Source: `20251105-captain_food_adrs.txt` (ADR-001…010). Earliest decisions; SCIC + Stellar framing.

| Orig. | Decision | Status |
|---|---|---|
| 001 | Name **Captain.Food** + military-salute 🫡 + fork logo (navy `#0047AB` / orange `#FF6B35`) | ✅ Active (brand; refined 2025-11-19) |
| 002 | "10% fair commission" vs Uber 30% | ⛔ → [`0016`](0016-proportional-captain-service-fee.md) |
| 003 | Stack: **Rust + Vue/Alpine + Stellar (Soroban) + Lightning**, CleverCloud/OVH | 🗑 Obsolete (no crypto; Next.js/Node/GraphQL/Postgres now) |
| 004 | KYC/AML via **Privado ID + Transak** (fiat→stablecoin on-ramp), self-custody wallets | 🗑 Obsolete (crypto custody dropped; auth now Supabase — [`0015`](0015-wrap-supabase-auth-behind-graphql.md)) |
| 005 | GTM: food trucks Tours + events privatization | 🔧 Tours focus ✅ (2025-11-12); **events** ⏸ deferred |
| 006 | **SCIC ESUS** + holacracy, multi-collège governance (SAS→SCIC at ~€300k ARR / M18) | ✅ Active (long-term legal/governance vision; recurs through Phase 2) |
| 007 | **EURWSKY** cooperative stablecoin (Phase 2–3), ESS reserves | 🗑 Obsolete (crypto abandoned) |
| 008 | Privacy-by-design + GDPR, EU-only servers, no ad-tracking | ✅ Active (principles; minus wallet-address data) |
| 009 | Branding "À la tête de la nourriture" + "MiamMiamMiam" campaign | 🔧 Marketing; refined by 2025-11-19 brand identity |
| 010 | MVP 3 pillars: **Order / Events / Analytics** | ⛔ Superseded (V0 = ordering; events & analytics deferred) |

## 2. 2025-11-06 — Marketplace economic model
Source: `20251106-captain_adrs_final_nov6.md` (ADR-001…013). The economic backbone behind June-2026 ADRs.

| Orig. | Decision | Status |
|---|---|---|
| 001 | **Stripe Connect** for payment split | ✅ → [`0017`](0017-3way-stripe-connect-split.md) |
| 002 | **Restaurant margin guarantee** (CORE model) | 🗑 Obsolete (margin-guarantee model dropped for 0% + service fee) |
| 003 | **Dynamic delivery cost** (Avelo37 decides) | ✅ Active (transparent delivery cost; [`0018`](0018-transparent-checkout-fee-display.md), [`0025`](0025-amount-split-pedagogical-display.md)) |
| 004 | Restaurant **flexible margin targets** | 🗑 Obsolete (bound to the margin-guarantee model) |
| 005 | **10% Captain commission** (fixed) | ⛔ → CA-based (2025-11-12) → [`0016`](0016-proportional-captain-service-fee.md) |
| 006 | **Immediate 3-way split** (+ optional tips) | ✅ → [`0017`](0017-3way-stripe-connect-split.md); tips ⏸ deferred |
| 007 | **Transparent pricing** (no hidden fees) | ✅ → [`0018`](0018-transparent-checkout-fee-display.md) |
| 008 | **Avelo37** as delivery partner (MVP core) | ✅ Active |
| 009 | **Open Collective** for financial transparency | ✅ Active (referenced by [`0025`](0025-amount-split-pedagogical-display.md)) |
| 010 | Subscriptions for retention | ⏸ Deferred (resurfaces as "Captain One", 2025-12-05) |
| 011 | Real-time communication + disputes chat | ⏸ Deferred (post-V0) |
| 012 | Tips timing & distribution | ⏸ Deferred |
| 013 | Events multi-payment (post-MVP, on-demand) | ⏸ Deferred |

## 3. 2025-11-12 — "Focus Tours" pivot ⭐
Source: `20251112-captain-decisions-focus-tours-nov12.md` (one major decision + roadmap).
**Foundational to the current repo** — V0's stated goal is "validate product–market fit in Tours".

| Decision | Status |
|---|---|
| **Focus 100% Tours, M0–M18**; target 50 active restaurants; ignore Paris/Lyon/SCIC-federation/CoopCycle/national expansion until an M18 conditional review | ✅ **Active / foundational** (CLAUDE.md V0 framing) |
| Deferred-but-valid (revisit at M18 if Tours succeeds): SCIC federation, CoopCycle Paris partnership, expansion financing, Tours SAS→SCIC conversion, founder multi-role comp | ⏸ Deferred (explicitly "premature before M18") |

*Note:* the doc projected M6 ≈ May 2026 launch / M18 ≈ Nov 2026. As of June 2026 the repo is at the
**spec + codegen** stage (no `apps/` yet), so the build timeline slipped relative to that projection;
the **Tours-first strategy itself stands**.

## 4. 2025-11-12 (19h29) — Economic model refinement
Source: `20251112_19h29-updated-decisions-nov12.md` (ADR-001…008, explicitly "changes from Nov 6").

| Orig. | Decision | Status |
|---|---|---|
| 001 | Commission → **CA-based progressive** (10% ≤€1,400 / 8% / 6% >€2,100 per month; anti micro-order abuse) | ⛔ → [`0016`](0016-proportional-captain-service-fee.md) (0% + service fee) |
| 002 | **HubRise offered free** (Starter 0€ vs Professional 1000€ packaging) | 🔧 HubRise integration ✅; the free/premium packaging is TBD/superseded |
| 003 | **KDS** via HubRise **OrderLine** + "Priority Smart", not native | ⏸ Deferred (no KDS in V0 specs) |
| 004 | Revenue model: CA-based + **charge différentielle** (min €50/mo, suspend after 3 mo) | ⛔ → service-fee model ([`0016`](0016-proportional-captain-service-fee.md)) |
| 005 | Target profiles: food trucks / hybrid kitchens / petits restaurants (72 Y1, 6/mo) | 🔧 Tours focus ✅; acquisition now via Sirene+Google pre-registration ([`0019`](0019-restaurant-pre-registration-sirene-google.md), [`0020`](0020-restaurant-sync-cron-prospection.md)) |
| 006 | Pricing: Starter (0€) / Professional (1000€: branding+domain+HubRise trial+credit) | 🗑 Mostly obsolete (white-label packaging; superseded by service-fee model) |
| 007 | Growth mechanics: customer/restaurant referrals, SEPA incentive | ⏸ Deferred |
| 008 | Go-to-market: **Touraine-focused**, phased (food trucks→hybrid→petits) | ✅ Active (consistent with Tours focus) |

## 5. 2025-11-27 — White-label SaaS for restaurants
Source: `20251127-captain_food_adrs_final.md` (ADR-001…010). A markedly different product frame;
mostly superseded or deferred. (Note: ADR-003 here re-adopts WordPress, contradicting the 2025-11-20
rejection — see §7.)

| Orig. | Decision | Status |
|---|---|---|
| 001 | **Google Workspace** for email/collaboration | ⏸ Internal ops choice (not a product/architecture decision) |
| 002 | Domains registered **in Captain.Food's name** | 🔧 Partial — relates to V0 multi-tenant `{slug}.captain.food`; `.restaurant` ⏸ deferred |
| 003 | **WordPress (Cloudways) + Nuxt (Vercel)** architecture | ⛔ Superseded (Next.js; WordPress = marketing site only, per 2025-11-20) |
| 004 | **GraphQL** as the principal backend layer (Apollo) | ✅ → [`0006`](0006-graphql-role-as-path-acl.md) (GraphQL is the API surface) |
| 005 | **GetMyInvoices + MyUnisoft** accounting automation | ⏸ Deferred (post-V0 ops/back-office) |
| 006 | **Commission advance** (€1000–2000 + 10%) as initial revenue | 🗑 Obsolete (conflicts with 0% food + service fee) |
| 007 | **`.restaurant`** as the primary domain extension | 🔧 Deferred (V0 uses `*.captain.food` wildcard multi-tenant) |
| 008 | **Mandatory offline mode** (Plus/Pro tiers) | ⏸ Deferred (not in V0) |
| 009 | **HubRise as the integration hub** | ✅ Active (`specs/integrations/hubrise.md`) |
| 010 | **Stripe Terminal + Yavin** for physical/in-person payments | ⏸ Deferred (V0 is online checkout) |

## 6. 2025-12-05 — Growth-led product (Phase 2)
Source: `20251205-ADR Captain.Food Phase 2.pdf` (ADR-001…008). Product-growth + funding strategy.

| Orig. | Decision | Status |
|---|---|---|
| 001 | **Captain Coins** — gamified hyper-local loyalty (multipliers, 5 VIP tiers, public leaderboard) | ⏸ Deferred (V2 product) |
| 002 | **Captain Groups** — collective ordering as the *core* viral MVP (rooms, split-pay, B2B "for Business") | ⏸ Deferred — **note tension**: pitched as the priority MVP, but current V0 specs cover single-customer ordering only |
| 003 | Financing strategy €2–3M (love money → grants → BA → seed/crowdfunding) | 📄 Context (funding, not architecture) |
| 004 | Coins + Groups **virtuous loop** (retention × acquisition) | ⏸ Deferred (depends on 001/002) |
| 005 | Positioning vs competitors (incl. commission stated as **"5–8%"**) | 🔧 Positioning ✅; the 5–8% figure ⛔ → [`0016`](0016-proportional-captain-service-fee.md) |
| 006 | 18-month product roadmap (MVP→V1→V2) | ⛔ Superseded by the Tours-focus roadmap / current phases |
| 007 | Critical risks & mitigation | 📄 Context (still useful) |
| 008 | Success metrics & KPIs (MAU, viral K, retention, GMV, NPS) | 📄 Context |

## 7. 2025-11-20 — Stack QAs addendum
Source: `20251120-captain-food-adrs-qas-update.md` (ADR-011…013 — **a third reuse** of 011–013).

| Orig. | Decision | Status |
|---|---|---|
| 011 | **Reject WordPress/WooCommerce** for the transactional platform → confirm custom (Nuxt + Rust); WordPress only for the marketing site | 🔧 "Custom, not WordPress for transactions" ✅ (repo is custom Next.js/Node); Nuxt/Rust ⛔; **directly contradicts 2025-11-27 ADR-003** |
| 012 | **3-way split native from the MVP** (`transfer_group`, webhooks, Open Collective logging) | ✅ → [`0017`](0017-3way-stripe-connect-split.md) |
| 013 | **Proprietary code** / avoid GPL on the core (IP for fundraising) | ✅ Active (business decision) |

---

## Supporting documents (not ADRs — reference only)

In the same kDrive folder, kept as background (no decisions to integrate beyond the above):

- **Story maps / QAs** — `20251105-captain_story_map.txt`, `20251105-captain_food_qas.txt`,
  `20251106-captain_{qas,story_map}_final_nov6.md`, `20251127-captain_food_{qas,story_mapping}_final.md`.
  The repo's living version is the generated product documentation (from `specs/stories.yaml`).
- **Financial projections** — `20251112_19h26-*` (`final-ca-based-model`, `mvp-final-projections`,
  `pos-analysis`) + the `*.csv` and `Simulations.xlsx`. 📄 Context for the (now superseded) CA-based model.
- **Expansion / recap** — `20251112-expansion-strategy-deferred.md`, `20251112_13h02_DECISIONS_RECAP.md`
  (the detailed backing for §3's defer-everything-but-Tours decision).
- **Brand identity** — `20251119-01h10-captain-food-brand-identity.md` (refines §1 ADR-001/009).
- **Business model canvas** — `20251127-captain_food_business_model_canvas_final.md`.
- **Competition study** — `20251211-Etude Concurrence Captain.Food.pdf`.
- **WordPress site assets** — `LWS-WORDPRESS-*.pdf` (the **marketing** site per §7 ADR-011; not the platform).
- **June 2026 source docs** (`20260627-*`, `20260628-*`) — already integrated as repo ADRs
  [`0016`](0016-proportional-captain-service-fee.md)–[`0026`](0026-ai-automation-tooling.md) and the
  operating-model playbook.

## Open reconciliation questions

Surfaced by the pivots — worth confirming when the relevant phase is planned (not blocking V0):

1. **Captain Groups** (2025-12-05) was pitched as the *core* MVP yet is absent from the current V0 specs
   (single-customer ordering). Is Groups a V1+ product, or should it re-enter the V0 scope?
2. **SCIC governance** (§1 ADR-006) and **Open Collective** (§2 ADR-009) are "active vision" but have no
   structural footprint in the specs yet — they realize post-PMF (M18+ per §3).
3. **Events / B2B** (recurring since §1) stay deferred — confirm they remain out of V0.
</content>
</invoke>
