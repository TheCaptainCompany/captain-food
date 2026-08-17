# Security Policy

Captain.Food handles orders, customer contact data and payments (via Stripe), so we take
vulnerability reports seriously — thank you for disclosing responsibly.

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

You will notice that we file *our own* security findings publicly, in this repository's issues and
decision register. That is deliberate and it is not a double standard: **we disclose our own defects,
and we protect yours.** A finding we already own is ours to publish and we hold ourselves to a dated,
tracked record of it; a finding you bring us is yours until you agree it is published, so it goes
through the private channel and into an advisory with credit if you want it.

Use GitHub's private vulnerability reporting instead: go to the repository's
**Security** tab → **Report a vulnerability** (or
<https://github.com/TheCaptainCompany/captain-food/security/advisories/new>). Reports go privately to
the maintainers.

Please include what you can of: the affected area (API, checkout/payment flow, auth, integrations),
reproduction steps, and impact. We'll acknowledge the report, keep you informed of progress, and
credit you in the advisory if you wish.

## Scope notes

- Payment card data never transits our systems — payment is handled by Stripe. Issues in the
  payment *flow* (order/refund state, webhooks) are absolutely in scope.
- The deployed V0 service is at `live.captain.food`; please keep testing non-destructive and
  proportionate (no DoS, no bulk data extraction).
- **`live.captain.food` is OUT OF SCOPE for testing until further notice (dated 2026-08-17).** We
  have a known, published, tracked authorization defect — per-instance (cross-tenant) authorization
  is absent on a large part of the API, tracked at
  [#178 "Write-side per-instance authorization"](https://github.com/TheCaptainCompany/captain-food/issues/178)
  and [#618 "Read surfaces missing `ReadScope` — the read half of the write-path authorization gap (#178)"](https://github.com/TheCaptainCompany/captain-food/issues/618)
  (see [DECISIONS §39](docs/proposals/DECISIONS.md)). Because we publish where that gap is, inviting
  testing against a live instance would be inviting you to reach other people's data — which is not
  something we are able to authorize, and not something we would want you to rely on us having
  authorized. **This exclusion stands until #178 and #618 are both closed**, at which point we will
  remove this bullet rather than let it quietly expire. Source-code review, local deployments and the
  specifications are all in scope and very welcome in the meantime.

## No production data in this repository — ever

**This repository is public.** The standing rule, binding on humans and agents alike:

> **No production data ever enters this repository.** Not in code, not in tests, not in fixtures, not
> in `docs/`, and **not as a pasted log line or stack trace in an issue or pull request** — which is
> the way it actually happens. Personal data (a real customer's phone number, address, order or
> message) and live secrets are both covered.

Two reasons it is absolute rather than a preference. Publishing personal data without a lawful basis
is unlawful on its own terms, independently of any other consideration. And **a deletion in public git
history is not a deletion**: once pushed, the only real remedies are rotating the secret or notifying
the person. If you believe something real has landed here, **do not quietly delete it** — report it
privately through the channel above so it gets a decision rather than a commit.

Reproducing a bug needs *synthetic* data. The repository's existing fixtures are the pattern to copy
(`+33612345678`, `@example.com`); a real value in a test is never necessary and never worth it.

*Swept 2026-08-17 across the tracked tree and 618 issues/PRs: no real personal data and no live
secrets found. Recorded so the next sweep knows where the last one got to.*

## Supported versions

Only the latest state of `main` (which is what is deployed) receives security fixes.
