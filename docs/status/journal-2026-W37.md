# Status journal — 2026-W37

Journal entries for ISO week 2026-W37, newest first, in the order they were written.

> **2026-09-07 — [#834 "Four hard-coded English strings on the checkout pay step, and two declared
> keys with no runtime consumer"](https://github.com/TheCaptainCompany/captain-food/issues/834),
> PARTIAL slice, draft [PR #939](https://github.com/TheCaptainCompany/captain-food/pull/939), Lane B
> (session_01H3AFBVzhSiGXJcFuwKjiMQ, `834-checkout-title-from-the-catalog`).** The checkout screen's
> `<h1>` (`back_button_header`) and the SSR `<title>` now resolve `checkout.title`
> (`restaurant_frontoffice.translations.yaml:92`, en "Checkout", fr "Paiement") from the locale
> already normalized at `render_checkout_html:586` — never the caller's raw argument — instead of
> the hard-coded English literal `"Checkout"`. The " - Captain.Food" brand suffix stays a literal,
> exact spelling: four other `page_html` callers (tracking.rs, sign_in_return.rs,
> admin_sign_in_return.rs, invitation_accept.rs) spell the suffix identically; renderer.rs carries
> the bare brand with no suffix (corrected at the confirmation round — the round-1/2 hand-backs said
> "five", which was wrong). The brand is a proper noun, locale-invariant. **NO entry was written in
> `specs/translations.code_refs.yaml`** —
> ADR-20260725-013315's over-declaration precedent decided a lens split (reviewer, evans, farley,
> holub, graphql, vernon, beck said no gate needs it — the screen's own `$ref` already marks
> `checkout.title` used and an entry would re-create the over-declaration that ADR records; ux,
> business, dba, observability, young leaned yes but named no gate it changes) — the code_refs
> registry is for keys NOT referenced from any screen, and `checkout.title` is.
>
> **Reds, base**: four tests, all RED on the fr arm at 3806bb8 (en arms are regression guards only —
> the literal being replaced already equalled the en catalog value, so an en arm can never be red at
> this base): the heading test, the tab-title test, a wrong-key test (folded away in round 2, see
> below), and a region-tagged-locale test. **Mutants, planted/quoted/reverted**: (1) restore the
> `"Checkout"` literal in the `<h1>` — reds the heading test (and, before the fold, the wrong-key and
> region tests too — **the hand-back's original "reds only its own test" claims were wrong; the
> reviewer measured the h1 mutant actually reds THREE tests and the title mutant reds TWO**, since
> the wrong-key and region tests both duplicated the heading/title assertions); (2) restore the
> literal page title — reds the tab-title test (and the region test); (3) resolve `checkout.contact`
> instead of `checkout.title` — reds both locale arms, confirmed by planting it on the h1 site and
> observing the fr AND en arms go red INDEPENDENTLY (not "by construction"); (4) use the raw,
> un-normalized `lang` argument for the `<html lang>` attribute instead of the normalized one — reds
> the region test's `fr-FR` case (`<html lang="fr-FR">` vs expected `"fr"`). **The card's own region
> mutant ("resolve the title from the raw lang argument") was UNKILLABLE as literally stated**:
> `i18n::resolve` normalizes its own `locale` argument internally, so `resolve("checkout.title",
> "fr-FR")` already reduces to `"fr"` and matches regardless of which argument (raw or normalized)
> is passed — the actually-plantable disagreement is either the `<html lang>` attribute (mutant 4
> above) or an UNSUPPORTED locale, added as a second case (`"de"`) in round 2: `DEFAULT_LOCALE =
> "fr"` (`render_checkout_html`'s own fallback) and `FALLBACK_LOCALE = "en"` (`i18n::resolve`'s
> fallback for an unrecognized locale) are DIFFERENT constants, so resolving the title from the raw
> `"de"` argument renders `<title>Checkout - Captain.Food</title>` over a `<h1>Paiement</h1>` and
> `<html lang="fr">` — planted, confirmed red (`left: "<title>Checkout - Captain.Food</title>"
> right: "<title>Paiement - Captain.Food</title>"`), reverted.
>
> **Two decisions changed from the dispatch card, both reviewer-caught at the checkpoint**: (1) the
> card specified comparing against `i18n::resolve("checkout.title", lang)` directly; the tests
> instead assert LITERAL expected copy ("Paiement", "Checkout") — stricter, and avoids a
> resolve-versus-resolve tautology where a mutant that changes the call site AND happens to still
> resolve consistently would not be caught. (2) The card kept the `[checkout.title` fail-visible
> marker assertion "as a diagnostic"; it was never added — superseded by the exact-literal
> `assert_eq!`s, which already fail loudly (and print the marker verbatim in the slice) if the key
> ever went unresolved.
>
> **ux's reading-order sentence, verbatim**: "the customer's eye lands on French in the tab and the
> header, then hits English two elements later on the money — the tab and the heading are now the
> two most French things on a screen whose total still reads English: a visible seam, not a fixed
> screen." **legal's sentence**: "this slice remediates a loi 94-665 (Toubon) art. 2 exposure on a
> heading and a tab title and does not close it — nobody should read PR #939 as 'the checkout is
> French now'." Three counsel questions and a WCAG line appended to the standing packet on
> `docs/adr/ADR-20260904-152807-*.md` (addendum, 2026-09-07, this change).
>
> **Residue**: `:388` (`format!("{} items - {}", cart_line_count, formatted_total)`, the cart
> summary) and `:473` (`" from "` between the summary and the restaurant name) stay English pending
> PR #933's merge and
> the specs lock lifting; `checkout.processing` is a DECLARED key with no runtime consumer yet (a
> submit-in-flight signal this SSR tree lacks). `tracking.rs:508` carries the SAME hard-coded-title
> defect (`page_html("Your order - Captain.Food", lang, &body)`) on the order-tracking screen — a
> HIGHER-stakes surface (post-purchase, the customer is waiting on their order) — flagged as the
> next-chunk candidate on [#941 "#939 follow-ups (checkout
> copy)"](https://github.com/TheCaptainCompany/captain-food/issues/941) item 1, not fixed here.
>
> **Card defects**: the briefing's framing that "both [locale arms] are red" at the base was wrong —
> the en arms can never be red at this base (the literal being replaced already equals the en
> catalog value); the `[checkout.title` marker instruction was superseded before it was ever
> exercised; the code_refs framing undersold that the decision was ADR-20260725-013315's precedent
> deciding a lens split, not an open question; the region-test mutant as literally described in the
> card was unkillable (see above); and the round-1 hand-back's mutant-scope claims ("reds only its
> own test") were wrong, corrected by the reviewer's full-suite measurement.
>
> **Roster notes**: vernon's region-agreement test was reshaped (the "de" case added, the fr-FR-only
> design changed) at the round-2 checkpoint without vernon on that checkpoint's roster — a
> depth-of-the-invited-lens question, not a roster-width miss, since vernon's own briefing concern
> (locale agreement) is exactly what got strengthened. Thirteen lenses each read a pinned line number
> at the briefing and none rendered the whole French checkout page end to end (ux) — no lens caught
> that unsupported-locale fallback had no owner until beck's round-2 pass (beck). **A lens planted a
> mutant through a HARDLINKED scratch copy of `checkout.rs`** during the checkpoint — a hardlink
> shares the inode with the real file, so editing the "scratch" copy edited the file every build and
> test in the shared tree was reading, leaving a planted `raw_lang` mutant visible in the shared tree
> for a period before it was caught and reverted. **Scratch copies must be real copies (`cp`, not
> `ln`)** — recorded as `docs/claude/sessions/gates.md` §19l, the one line judged not derivable from
> the code (cost: a false stop-hook report plus a mutant visible in the shared tree during a
> checkpoint another lens was reading).
>
> Lane B. Links: [#834](https://github.com/TheCaptainCompany/captain-food/issues/834),
> [#939](https://github.com/TheCaptainCompany/captain-food/pull/939),
> [#941](https://github.com/TheCaptainCompany/captain-food/issues/941).
