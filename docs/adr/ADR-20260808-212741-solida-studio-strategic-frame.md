# ADR-20260808-212741 — The Solida strategic frame: studio of products, delivery-channel sequencing, market-parity demo, rebrand pending

**Status**: Accepted · **Date**: 2026-08-08 · **Deciders**: the customer (product owner), in
session — a batch of strategic directives recorded verbatim so no future session re-derives or
contradicts them. Also adds the **`holub` focus-coach agent** (`.claude/agents/holub.md`) at the
customer's request, under the ADR-20260808-144738 constraint: it advises on focus, it is never a
PM proxy.

## 1. Delivery-channel sequencing — Uber Direct first, own riders grown, avelo37 at volume

> "I want to anticipate the fact that restaurants will have HubRise. We need also to anticipate
> the fact that we will have to pass through Uber Direct to deliver orders because we do not have
> enough riders to start with. We need to handle the rider onboarding and the rider partner
> onboarding. For Tours it's avelo37 — we will consider to partner with them once we have enough
> orders per week."

Consequences:
- **HubRise import is a first-class onboarding path**, not an integration afterthought — most
  target restaurants will arrive with a POS behind HubRise. (Composes with
  [#380 "Onboarding stalls at the menu: a restaurant without HubRise must type its whole catalog by hand"](https://github.com/TheCaptainCompany/captain-food/issues/380)
  for the non-HubRise minority.)
- **Uber Direct is the launch delivery channel** — the dual-channel delivery model
  (`INDEPENDENT` | `PARTNER`) already in the DSL is confirmed, and the partner leg leads at V0.
  Rider onboarding (own riders) AND partner onboarding are both required product surfaces.
- **avelo37 is a volume-triggered partnership**, not a launch dependency: revisit when a stated
  orders-per-week threshold is reached (threshold to be set with real data — an observability
  contract, not a guess).

## 2. Market-parity credibility and the public try-before-committing demo

> "We need to cover what is currently proposed in the market to be accepted by restaurants as an
> alternative, otherwise we will lose credibility. That's the reason why we need to have the
> platform working — to show with test customer, test restaurant, test order, test rider how the
> platform behaves. We can provide this demo on the marketing web site to allow customers,
> restaurants and riders to touch the product before committing themselves."

Consequences:
- **Feature parity with incumbent platforms is a credibility floor** for restaurant adoption —
  gaps are weighed against "would a restaurant refuse to switch over this?".
- **A public demo sandbox is a product surface**: seeded test restaurant/customer/order/rider,
  linked from the marketing site, letting every audience touch the real product before
  committing. Tracked by its own epic issue. This is also the working-software demo the
  operating model measures progress by — one artifact, two jobs.

## 3. The crash-test verdict on Render+Supabase — learnings ratified

> "Our current hosting system Render+Supabase was an error / a crash test, and we learned that we
> need more space, outgoing bandwidth, stop polling — use notify instead, put in place AI error
> resistance for shortcuts. That's the reason why I have decided to split the system in clean
> crates and decided to use Kubernetes and multiple databases to reflect this structural design
> that improves the delivery efficiency and splits responsibility."

This retroactively names the rationale already carried by the OVH/MKS migration
(ADR-20260731-061609, ADR-20260807-002705), the bins split (ADR-20260808-062432/062933), the
crate isolation program (#290/#306/#307), LISTEN/NOTIFY over polling (PROP-170500 D5), and the
compiler-first doctrine ("AI error resistance for shortcuts" = make the shortcut unspellable,
ADR-20260803-234035). Recorded so the infra investment is never re-litigated as gold-plating:
it is a decided response to measured crash-test failures — **and it is bounded by the same
decision**: the structure exists to improve delivery efficiency, so delivery cadence remains the
test it must keep passing.

## 4. Rebrand — Captain → Solida (PENDING, do not rename yet)

> "The company will not be Captain Food or The Captain Company because of brand names already in
> place. I'm currently fighting to keep the name **Solida**, which means for me « solide et
> solidaire », which matches perfectly what I'm doing. So instead of captain.food we will have
> **solida.food** (already bought). I need confirmation from the opposer that I can use Solida
> for class 42."

Consequences:
- The future names: company **Solida**, product **solida.food** (domain acquired). The SASU of
  ADR-20260808-195315 ch. 4 is expected to carry the Solida name — brand and entity land
  together.
- **Nothing is renamed until the class-42 opposition is resolved** (customer-external). A
  tracking issue holds the rename scope (repo, domains, tenant host pattern
  `{slug}.captain.food`, docs, marketing) so the day the confirmation arrives, the sweep is a
  checklist, not an archaeology dig. New user-facing surfaces should avoid baking "Captain"
  deeper where cheap to avoid.

## 5. Solida is a studio — the product is also the platform for the next product

> "Solida Food is the first product for the company Solida. I will reuse the work done in this
> product to do another product, so we have to think as a studio of products. That's the reason
> why I invest a lot on the infrastructure and clean design, because we will reuse this work for
> other products. I also want to prove myself that I can create something great."

Consequences:
- Reusability of the platform machinery (spec DSL + codegen, actor runtime, mailbox, SDUI,
  multi-tenancy, observability contracts, the operating model itself) is a **stated goal**, not
  an accident — generic/product boundaries (e.g. `domain-common` vs scope crates, platform vs
  scope configuration) deserve the extra care they get.
- The studio frame **never outranks the first product's users**: reuse is harvested from
  working software, not designed speculatively ahead of it (the focus-coach lens exists to hold
  this line). Solida Food must succeed for the studio thesis to mean anything.
