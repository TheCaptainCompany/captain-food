# ADR-20260813-021500 — The ceiling is the economic control; the allowlist is the served-country decision

> Title corrected 2026-08-13 with [#535](https://github.com/TheCaptainCompany/captain-food/issues/535);
> the file keeps its original slug so links stay valid. The first form of this record claimed the
> allowlist was the economic control — §1 below records why that was backwards at V0 scale.

**Status**: Accepted (2026-08-13) · **Amended** (2026-08-13, [#535 "The SMS allowlist admits every NANP territory under a bare `+1`"](https://github.com/TheCaptainCompany/captain-food/issues/535)):
`+1` dropped from the default allowlist; four false assertions in the first form corrected in place —
§1's economic claim, §3's "the price is recorded nowhere / owed by the founder", the Context's
misattributed founder quote, and the failure mode (an invoice — it is a prepaid-pack outage).
**Tracking**: [#516 "The OTP endpoint is anonymous with no rate limit and no country allowlist, on our own SMS account"](https://github.com/TheCaptainCompany/captain-food/issues/516)
**Relates to**: [ADR-20260722-174500](20260722-174500-identity-federation-cross-tenant-personalization.md) (OVH is our own SMS account) ·
[ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md) (compiler first) ·
[ADR-20260810-231300](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md) (the inverted dead-man's switch) ·
[#517](https://github.com/TheCaptainCompany/captain-food/issues/517) · [#518](https://github.com/TheCaptainCompany/captain-food/issues/518)

## Context

`requestPhoneVerification` is anonymous **by design** — a visitor has no identity yet to rate-limit —
and it had **no limit of any kind**. Every accepted send spends real money on our own OVHcloud
account. That is the exact shape SMS-pumping automates: drive OTP requests at premium-payout mobile
ranges the attacker shares revenue on.

**The failure mode is an outage, not an invoice** (corrected with #535 — the first form of this
record said the founder wakes to a bill). The OVH SMS account is a **prepaid credit pack**:
`specs/common/configuration.yaml` (`OVH_SMS_SERVICE_NAME`) records that the account must hold SMS
credits, and with none the credentials still authenticate while the send fails at OVH. So a burn
night drains the pack, every OTP send then fails, and **nobody can sign up or sign in** — phone OTP
is the primary V0 login. Recovery is founder-gated: DECISIONS.md §35 INV-1 records *"I'm waiting for
a working version before paying OVH"*, so the outage has **unknown duration**. That — not a bill —
is what these guards bound, and it is the strongest argument for keeping the spend paths narrow.

(The first form of this record also cited *"too expensive for me, I don't have the money for that"*
at €67.80/month as the founder's SMS cost position. That quote is from
[ADR-20260807-114122](ADR-20260807-114122-mks-starts-at-one-node.md) and is about the
**MKS/CNPG hosting trio**, not SMS — corrected so the two budgets are never conflated in a decision
about either.)

## Decision

### 1. The CEILING is the economic control; the allowlist contains cost and refuses what we cannot serve

**Corrected with #535 — the first form of this section said the opposite, and it was backwards at V0
scale.** At 200 sends/day, an attacker's gross is a few euros and their revenue share a fraction of
that: the global ceiling has already destroyed the pumping economics for **every** range, including
the ones the allowlist admits. So the ceiling is the economic control. The allowlist's job — still a
good reason to have one — is **cost containment plus refusing what we cannot serve**: it keeps the
drain rate on a prepaid pack down and makes the served-country decision executable and fail-closed
(an unparseable number is refused, never sent).

**The default set answers ONE question — which countries does V0 Tours serve** — and the answer is
`+33,+32,+41,+44,+49,+34,+39`. A `+33`-only list was considered and rejected — Tours has
international students and Loire-valley tourists, a real single-digit share of signups, and
`+33`-only would refuse those real customers for no additional protection. It is configuration
(`SMS_ALLOWED_DIALING_CODES`) with the reasoning in its declaration, because the next person to widen
it must read *why* the boundary is where it is.

**A calling code is not a destination** (#535 — the first form of this record reasoned about `+1` as
"US", which is the false instruction that admitted it). `+1` reaches every NANP territory — twenty-odd
Caribbean and Pacific jurisdictions with their own operators and rates, **including the premium-payout
ranges this allowlist exists to exclude — all billed as if they were a Boston number**. So bare `+1`
is out of the default, and that IS the final shape of this decision: dropping
`+1` fully answers *which countries does V0 Tours serve*
([ADR-20260808-235113](ADR-20260808-235113-final-vision-first-no-intermediate-steps.md) is satisfied:
this is the answer, not an interim), whereas an NANP **area-code** allowlist
would implement *serve North America* — a market-expansion decision nobody has taken and not the
team's to take. Nor would a hand-maintained area-code table be the final shape if that decision were
ever taken: NANPA reassigns codes several times a year, so a stale table refuses an innocent US
customer with a new area code — the final shape there is region resolution through a
libphonenumber-class dependency, triggered by a market decision rather than queued. The same lens,
applied to what stays: `+44` also reaches Guernsey, Jersey and the Isle of Man (separate telecom
jurisdictions, own operators, commonly rated apart from the GB mainland) — an accepted, named cost;
`+41` is a cost item rather than a fraud one, Swiss termination being among Europe's more expensive
under a prepaid pack; `+33` is clean **because** the French overseas territories carry their own
codes (`+262`, `+590`, `+596`, `+594`, `+687`, `+689`, `+508`), so DOM-TOM is already unreachable
and would be a separate deliberate addition. **The question to answer before widening is what a send
to EVERY territory the code reaches pays.**

### 2. Per-number caps do not bound spend — an attacker rotates numbers

3/hour and 5/day per number, with a 30s → 2min → 10min cooldown, stop **one** number being pumped.
They do not put a ceiling on the **total**, which is the actual failure mode: the prepaid pack is
drained overnight and phone login stops for everyone, for a founder-gated, unknown duration (see
Context). Under number rotation, per-number caps *multiply* rather than limit.

So the money control is a **global daily ceiling with a hard kill switch**
(`SMS_MAX_SENDS_PER_DAY_GLOBAL`). Once the day's budget is spent, OTP sends stop — for everyone,
including legitimate sign-ups. **That is the correct trade against an unbounded bill**, and it is only
acceptable because it is loud: `otp_send_refused_total{reason=global_ceiling}` plus an ERROR log per
refusal naming the two things it can mean (the ceiling is too low for real traffic, or an attack is
under way) and what to do about each.

**The kill switch is operable without a deploy, two ways, and the second is the one to use at 02:00**:
the env key changes the ceiling persistently on the next rollout, while
`UPDATE sms_send_quota SET sent_count = 999999 WHERE quota_key = 'global:day'` stops sends
*immediately* on a live system with no rollout at all (`DELETE` on that row restores them). Both are
tested.

### 3. The per-message price IS recorded, and the ceiling is derivable from it

**Corrected with #535 — the first form of this section claimed no EUR/SMS figure exists anywhere in
this repository and that the price was owed by the founder. Both claims were false.**
[PROP-20260724-233605](../proposals/PROP-20260724-233605-ovh-sms-hook.md) (line 14,
founder-approved 2026-07-24, screenshot-confirmed) records **OVH SMS France at €0.06 HT/SMS at 100
credits, €0.058 at 1 000 (3% remise), plus 20 free credits on a new account**.

With that anchor, the **200 sends/day** default is **derivable rather than a guess**: €12/day worst
case France-rated. The remaining unknowns are OVH's **per-destination price multipliers** (a
non-France send costs more than €0.06) and **which pack was actually purchased** — those refine the
ceiling; they do not block deriving it.

### 4. The wall is at the send seam, not at the command

`request_phone_verification` only asks the identity provider to send. **The euro is spent later and
INBOUND**: Supabase calls our `/auth/sms-hook`, and that route hands the message to OVH on our own
account. A guard at the BFF edge or in the command handler is therefore *present, not unbypassable* —
anything able to make the provider send reaches the hook without passing our command path.

Two enforcement points, with different jobs, stated so neither is mistaken for the other:

| Point | Role | What it does |
|---|---|---|
| identity ACL (`SupabaseIdentityService::send_phone_otp`) | **cheap shedding** | `peek` — refuses a doomed request with a typed, renderable reason and saves a provider round-trip. Records nothing, so it never double-charges. Advisory by construction. |
| `/auth/sms-hook` | **the wall** | atomic claim against the shared budget, then the send. Authoritative. |

**Honest limitation**: the edge check runs in the WORKER, after the mailbox row is written, so it does
**not** protect `inbound_messages` from being used as a write amplifier by an anonymous caller. Doing
that needs the check in the GraphQL resolver's synchronous validation phase, which is generated code
and out of this change's scope. Reported, not silently absorbed.

### 5. Compiler first: an unguarded send is unspellable

`OvhSmsClient::send` takes an `AuthorizedSmsRecipient` **by value**, whose field is private to
`crate::sms_authorization` and which has no public constructor. The only way to obtain one is
`SmsSendAuthorizer::authorize`, which claims the budget first. A caller holding a phone number and a
message has **no path to the sender**. "Someone added a second call site and forgot the guard" is a
type error, not a review finding — which matters because this money path already has more than one
door.

Two properties, and **each needed its own mutant**, because the first one alone is not the guarantee:

| Property | Mutant planted | `rustc` says |
|---|---|---|
| **Unforgeable** — no witness without a claim | call `send_authorized` directly, skipping the guard | `error[E0624]: method 'send_authorized' is private` |
| **One claim, one send** — a witness is spent by sending | `let _ = sms.send(recipient, &message).await;` immediately before the real `sms.send(recipient, …)` | `error[E0382]: use of moved value: 'recipient'` … `move occurs because 'recipient' has type 'AuthorizedSmsRecipient', which does not implement the 'Copy' trait` / `value used here after move` |

The second row is a **correction recorded on purpose**, because the first form of this decision got it
wrong. `send` originally took `&AuthorizedSmsRecipient`, and the argument for one-claim-one-send was
that the type is not `Clone`. That argument is false for a shared reference: `for _ in 0..1000 {
sms.send(&w, m) }` compiles against a `&`-borrow and spends a thousand messages on one claim of quota
— the budget bypassed by a loop, which is precisely the property the witness exists to provide. A
missing `Clone` impl cannot stop that; only consuming the value can. So the rule is **move, not
`Clone`**, and adding a `&` to any signature taking this type re-opens the hole. The lesson generalises
past this file: *"not `Clone`"* bounds how many witnesses exist, never how many times one is used.

### 6. The counter is SHARED, and it is one atomic statement

`sms_send_quota` in Postgres, **not** per-pod memory. A per-pod limiter multiplies the allowance by
the replica count and resets on every deploy; for the global ceiling that is the difference between a
ceiling and a suggestion.

The claim is a single `INSERT … ON CONFLICT DO UPDATE … WHERE`, where **the `WHERE` clause is the
limit**. This is not stylistic: `RequestPhoneVerification` has **no per-phone actor lane** (the
GraphQL door mints a fresh `actor_id` per request), so nothing serialises two concurrent requests for
one number except this statement, and a load-modify-save would race for money. Proved against a real
Postgres with eight concurrent claimants on a ceiling of one: exactly one wins.

Two deliberate properties: a **refused claim records nothing** (a refusal costs no money and must not
burn the budget that bounds money), and a **backwards clock denies rather than resetting** (reading
`now < window_start` as expiry would make replica skew a bypass of every per-number cap at once).

### 7. A refusal is a typed, renderable fact — four states, not one

`RequestPhoneVerification` had `throws: []`, so there was no way for it to say *why* it refused. Four
distinguishable outcomes now exist, because the screen must say four different things: a rate limit
rendered as "code incorrect" is a lie at the highest-friction tap in the funnel, and "invalid number"
to a Belgian visitor is an accusation aimed at someone who did nothing wrong.

| State | Error | What the screen says |
|---|---|---|
| too soon | `RateLimited` + `retryAfterSeconds` | the SERVER's countdown, never a client guess |
| this number's day is spent | `VerificationSendLimitReached` | offer help — a countdown to tomorrow is not information |
| country not served | `PhoneCountryNotServed` + `dialingCode` | name the country, offer an exit |
| our ceiling is spent | `VerificationSendCapacityExhausted` | say nothing about the customer; they did nothing wrong |

## Consequences

- **A legitimate customer can be refused.** By the global ceiling (intended, loud, and the point) and
  by the per-number caps (3/hour, 5/day — a customer who burns five has a delivery problem no sixth
  SMS fixes).
- **Zero refusals reads identically to "the limiter is off"** — the inverted dead-man's switch. Hence
  `otp_send_guard_enforcing`, an **observable** gauge: its callback is invoked by the SDK on every
  collection cycle, so the value is re-asserted by construction and stops being emitted the moment the
  process dies. A synchronous `record` at composition would have been the defect wearing the fix's
  name — it says "the guard was wired when this process booted" and then says nothing again, so the
  series goes flat-`1` and stays flat-`1` however broken the guard becomes, which is a stale `1`
  indistinguishable from a live `1` in exactly the case the gauge exists for. **Both** enforcement
  points additionally re-declare the state where enforcement is actually DECIDED, dropping to `0` when
  the shared counter is unreachable (ADR-20260810-231300; this is the metrics export path re-reading our
  own state, not a poll of another component, so it is outside that ADR's push rule). Both, not just
  `authorize`, because during a counter outage the ACL sheds at `peek` and the hook path is never
  reached — a gauge maintained on `authorize` alone would hold `1` for the entire outage, the same
  blindness one door over. Each leg has its own assertion in
  `crates/server/tests/otp_guard_liveness_metric.rs` (phases 4 and 5), and phase 5 was confirmed
  non-vacuous by reverting only `peek`'s arm: `left: [..,1,1]` vs `right: [..,1,0]`.
- **Never alert on send rate**: Friday 19:00–21:30 looks exactly like an attack by volume. Alert on
  the RATIO of sends to verifications (an attack moves only the numerator — nobody verifies the codes)
  plus ceiling burn. The join cannot use `correlation_id`, which **breaks across the provider's
  SMS-hook hop**; it is hashed phone plus a time window.
- **No `domain_events` row per send.** The command emits nothing, so a BAM fold cannot see this path —
  it is OTLP-only, and it must stay that way: a per-refusal event is a log an anonymous attacker gets
  to drive.
- **OWED (#535, named here so it is not lost; deliberately not built in that change): an
  observed-but-not-served telemetry label set.** `specs/observability.yaml` bounds the
  `dialing_code` label to the allowlist plus `other`, and the Rust label list matches the served
  default — so with `+1` dropped, **every refused `+1` collapses into `other` and the North American
  refusal cohort is unmeasurable**, meaning the `+1` decision itself can no longer be re-derived from
  production signal. The fix is a small bounded set of codes we are willing to OBSERVE without
  serving (`+1` first), distinct from the served set.
- **OWED (#535, ranked by the business lens above the `+1` hole itself): a credit-balance gauge.**
  The dead-man's switch for "phone login is about to stop working": the pack drains silently, and
  the first symptom today would be every OTP send failing. OVH will not push the balance, and
  silence must not read as healthy — this falls squarely under monitoring's **permanent-poll
  carve-out** in
  [ADR-20260810-231300](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)
  (the observer is outside what it observes and has no durable record to reconcile against).
- **A per-IP cap was deliberately NOT built.** Nothing in this repository proves our ingress overwrites
  `X-Forwarded-For`, no IP extraction exists anywhere today, and a cap on a client-controllable header
  is theatre — the forgeability class is already documented at `crates/server/src/graphql/tenant.rs:49`.
  It is owed once the ingress guarantees the header, and is worth less than the guards above regardless.

## Alternatives considered

- **Rate cap only, no allowlist.** Rejected: it slows the spend without removing the payout, so the
  attack still pays, just more slowly.
- **`+33` only.** Rejected: refuses real Tours customers for no additional protection (§1).
- **Keep North America behind an NANP area-code allowlist** (instead of dropping `+1`, #535).
  Rejected: it answers a different question — *serve North America* is a market-expansion decision
  nobody has taken — and a hand-maintained area-code table would not be the final shape even then,
  because NANPA reassigns codes several times a year and a stale table refuses innocent customers.
  If that market decision is ever taken, the shape is region resolution through a
  libphonenumber-class dependency (§1).
- **In-memory per-pod limiter.** Rejected: allowance × replicas, reset per deploy — for a money
  ceiling, not a ceiling (§6).
- **Guard in the command handler only.** Rejected: the euro is not spent there (§4).
- **A CAPTCHA on the OTP form.** Not rejected on merit — genuinely effective against automation — but
  it is a conversion cost at the top of the funnel and a third-party dependency, and it does not bound
  the bill either. Reconsider if pumping continues past the allowlist.
