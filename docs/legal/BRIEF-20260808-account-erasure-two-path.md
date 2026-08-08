# Account-level erasure — "same behavior as Facebook" reconciled with GDPR Art. 17

**Date-stamped 2026-08-08** · **Status**: In discussion with the customer · **Prepared by**: the
legal-specialist lens, session https://claude.ai/code/session_01AKgDqRbCcCxtUePWPRfxtp ·
**Decision thread**: [#404 "Decision thread: GDPR erasure depth"](https://github.com/TheCaptainCompany/captain-food/issues/404)
· **Customer direction** ([ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md)):
*"Same behavior as Facebook, people can recover their account anytime."*

**Grades**: (a) established obligation · (b) interpretation for counsel to confirm · (c) unknown.
*This maps obligations; none of it is legal advice, and it never substitutes for licensed French
counsel.* Already decided, not reopened: order-level erasure = tombstone + stream deletion
([ADR-20260731-160000](../adr/ADR-20260731-160000-order-erasure-tombstone-then-stream-deletion.md)).

## The finding in one line

The customer's instinct is incomplete, not wrong: **"Facebook behavior" is two buttons, and
Facebook ships both** — *deactivate* (reversible anytime, data kept) and *delete* (~30-day
change-your-mind window, then irreversible, up to ~90 days to purge backups; per published Meta
policy as of knowledge cutoff — verify currency). We build both. "Recover anytime" is the
deactivate button; it cannot be the only button.

## 1. Recoverable-only is not Art. 17 compliance — grade (a) — BLOCKER

- Deactivation is a UX state, not a legal act: data stays, processing continues under Art. 6(1)(b).
  Lawful to offer. (a)
- An erasure REQUEST cannot be satisfied by a recoverable state — data held ready for reactivation
  is by definition not erased. Answering an Art. 17 request with "we deactivated you" is
  non-compliance on its face. (a)
- "Forever" independently violates Art. 5(1)(e) storage limitation: a deactivated account needs a
  documented inactivity retention window (CNIL gestion-commerciale benchmarks; commonly ~3 years,
  then notify-then-delete — (a) that indefinite is not a retention period, (b) the number).
- Exposure: ignored/deflected erasure requests are the classic CNIL complaint trigger,
  Art. 83(5)(b) tier. (a)

## 2. The lawful shape — the two-path model

**Path 1 — Deactivate** (the customer's "recover anytime"): plain domain state, presence hidden,
login restores. Conditions: the UI says plainly *this is not deletion, your data is kept*
(Arts. 12/13, (a)); a dormant-account sunset with prior notice ((b) on N); **equal prominence** —
delete as findable as deactivate, or it is the textbook dark pattern (EDPB Guidelines 03/2022;
DSA Art. 25 — (b), exposure-level).

**Path 2 — Delete** (the Art. 17 path): grace window (§3), then real erasure — identity deleted at
the provider (Supabase, a processor instruction under Art. 28), files purged, prospection data
gone, conversations tombstoned, orders via the existing tombstone-then-stream-deletion machinery.
(a) on the obligation; (b) on per-store sequencing. What survives is the §4 carve-out list, held
in restricted-access intermediate archive (CNIL base active / archivage intermédiaire doctrine,
(a)). The erasure receipt (pseudonymous ledger) is the Art. 5(2) accountability artifact; its
minimality is a counsel question (b). Art. 19 recipient notifications: map, likely minimal (b).

## 3. The 30-day grace window — defensible, grade (b), with conditions

- Art. 12(3) gives one month to act; a ≤30-day window ending in executed erasure lands inside the
  clock. Natural reading, but no named CNIL doctrine blesses recovery-grace-windows; Meta's
  practice under EU supervision is persuasive, not law — verify. (b)
- Documented protective rationale: deletion via a compromised/shared account is a real harm
  vector; grace + re-login-cancels is a proportionate safeguard. (b)
- Backups may lawfully lag on the rotation cycle if documented and never restored without
  re-applying erasure (Meta's "up to 90 days" is this). (b)
- Conditions: disclosed at request time; re-login-cancels is the user's act, never an admin
  resurrection; clock and execution **automatic** (the scheduled-actor-message machinery,
  ADR-20260731-153000, is the right executor).
- (c): can a subject demand immediate execution and skip the 30 days? Prudent design: skippable on
  explicit confirmed demand. Counsel confirms.

## 4. What survives deletion regardless — the carve-outs

Art. 17(3)(b)/(e) mechanism, (a). All retained items move to restricted-access archive, minimized:

| What | Instrument | Window | Grade |
|---|---|---|---|
| Accounting books + supporting documents (invoices, the financial skeleton of orders) | Code de commerce L123-22 | 10 years from close of financial year | (a) |
| Tax/VAT records | LPF L102 B | 6 years (the 10-year commercial rule usually governs) | (a) |
| E-contracts ≥ 120 € | Code conso L213-1 | 10 years from delivery | (a) — rare (catering baskets) |
| Fraud/dispute/chargeback evidence | Art. 17(3)(e) + Code civil 2224 | ~5 years, case-linked | (b) |
| Do-not-contact suppression entry | Art. 21 doctrine, liste repoussoir | indefinite, minimal identifier | (a) doctrine, (b) fields |
| The erasure receipt itself | Art. 5(2) | policy-defined | (a)/(b) |
| Food-safety incident evidence | Code civil 1245-15 | 10 years from circulation, case-linked | (b) |

This table IS the written retention schedule CNIL expects — windows declared **once, in the DSL**,
feeding both the sweep and the DPIA. The open item from ADR-20260731-160000 (financial skeleton
survives phase 2 OR is exported before it) **must be closed when account deletion ships** — the
10-year records must exist somewhere after the streams die. (a) that one must be chosen; (b) which.

## 5. Counsel-packet additions E1–E8

Appended to the packet in
[BRIEF-20260808-listing-opt-out-objections.md](BRIEF-20260808-listing-opt-out-objections.md).

## Triage

- **BLOCKER**: recoverable-deactivation as the only exit. The delete path must exist at launch.
- **EXPOSURE**: no dormant sunset; grace window undisclosed or manually driven; delete buried
  relative to deactivate; financial-skeleton question open past phase 2.
- **HYGIENE**: deactivate screen never says "deleted"; delete screen never promises recovery
  beyond the grace window.

**One-line version for the customer**: *"Facebook behavior" is exactly what we'll build — and
Facebook's behavior is two buttons, not one. Deactivate = come back anytime, we keep everything.
Delete = 30 days to change your mind, then genuinely gone except what French accounting law forces
us to keep for 10 years, de-identified and locked away. The first button is product; the second is
the law.*
