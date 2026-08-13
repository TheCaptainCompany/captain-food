# ADR-20260813-021500 — The allowlist is the economic control, and only a global ceiling bounds the bill

**Status**: Accepted (2026-08-13)
**Tracking**: [#516 "The OTP endpoint is anonymous with no rate limit and no country allowlist, on our own SMS account"](https://github.com/TheCaptainCompany/captain-food/issues/516)
**Relates to**: [ADR-20260722-174500](20260722-174500-identity-federation-cross-tenant-personalization.md) (OVH is our own SMS account) ·
[ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md) (compiler first) ·
[ADR-20260810-231300](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md) (the inverted dead-man's switch) ·
[#517](https://github.com/TheCaptainCompany/captain-food/issues/517) · [#518](https://github.com/TheCaptainCompany/captain-food/issues/518)

## Context

`requestPhoneVerification` is anonymous **by design** — a visitor has no identity yet to rate-limit —
and it had **no limit of any kind**. Every accepted send spends real money on our own OVHcloud
account. That is the exact shape SMS-pumping automates: drive OTP requests at premium-payout mobile
ranges the attacker shares revenue on, and the invoice arrives overnight. The founder's recorded
position on cost — *"too expensive for me, I don't have the money for that"* at €67.80/month — makes
this a money defect, not a hardening nicety.

## Decision

### 1. The country allowlist is the ECONOMIC control, and it is not the rate cap

A rate cap limits how fast an attacker spends; the allowlist decides **whether spending pays them at
all**. Pumping profit lives in high-payout ranges, so an allowlist that excludes those removes the
attack's economics outright, at near-zero cost to real customers. It is the cheapest guard and the
most effective one, and it must be **fail-closed**: an unparseable number is refused, never sent.

**The boundary is PAYOUT TIER, not "EU"**, and the default set says so:
`+33,+32,+41,+44,+49,+34,+39,+1`. A `+33`-only list was considered and rejected — Tours has
international students and Loire-valley tourists, a real single-digit share of signups, and none of
BE/CH/UK/DE/ES/IT/US pays an attacker anything, so `+33`-only would refuse real customers while buying
no additional protection. It is configuration (`SMS_ALLOWED_DIALING_CODES`) with that reasoning in its
declaration, because the next person to widen it must have to read *why* the boundary is where it is:
**adding a code opens a spend path, and the question to answer first is what a send to that range
PAYS.**

### 2. Per-number caps do not bound spend — an attacker rotates numbers

3/hour and 5/day per number, with a 30s → 2min → 10min cooldown, stop **one** number being pumped.
They do not put a ceiling on the **total**, which is the actual failure mode: the founder wakes to an
invoice. Under number rotation, per-number caps *multiply* rather than limit.

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

### 3. The per-message price is UNKNOWN, and the default is a guess that says so

No EUR/SMS figure exists anywhere in this repository — `specs/common/configuration.yaml` names the OVH
account and its credentials but never a price. **A sane ceiling cannot be derived without one, so none
was invented.** The default is **200 sends/day**, chosen as "comfortably above any plausible V0 day in
Tours, low enough that the worst overnight case is tens of euros rather than hundreds".

**Owed by the founder**: the real per-message price of the OVH credit pack. The ceiling must be
re-derived from it, and until then the number above is a placeholder wearing a justification, not a
budget.

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
  indistinguishable from a live `1` in exactly the case the gauge exists for. `authorize` additionally
  re-declares the state where enforcement is actually DECIDED, dropping to `0` when the shared counter
  is unreachable (ADR-20260810-231300; this is the metrics export path re-reading our own state, not a
  poll of another component, so it is outside that ADR's push rule).
- **Never alert on send rate**: Friday 19:00–21:30 looks exactly like an attack by volume. Alert on
  the RATIO of sends to verifications (an attack moves only the numerator — nobody verifies the codes)
  plus ceiling burn. The join cannot use `correlation_id`, which **breaks across the provider's
  SMS-hook hop**; it is hashed phone plus a time window.
- **No `domain_events` row per send.** The command emits nothing, so a BAM fold cannot see this path —
  it is OTLP-only, and it must stay that way: a per-refusal event is a log an anonymous attacker gets
  to drive.
- **A per-IP cap was deliberately NOT built.** Nothing in this repository proves our ingress overwrites
  `X-Forwarded-For`, no IP extraction exists anywhere today, and a cap on a client-controllable header
  is theatre — the forgeability class is already documented at `crates/server/src/graphql/tenant.rs:49`.
  It is owed once the ingress guarantees the header, and is worth less than the guards above regardless.

## Alternatives considered

- **Rate cap only, no allowlist.** Rejected: it slows the spend without removing the payout, so the
  attack still pays, just more slowly.
- **`+33` only.** Rejected: refuses real Tours customers for no additional protection (§1).
- **In-memory per-pod limiter.** Rejected: allowance × replicas, reset per deploy — for a money
  ceiling, not a ceiling (§6).
- **Guard in the command handler only.** Rejected: the euro is not spent there (§4).
- **A CAPTCHA on the OTP form.** Not rejected on merit — genuinely effective against automation — but
  it is a conversion cost at the top of the funnel and a third-party dependency, and it does not bound
  the bill either. Reconsider if pumping continues past the allowlist.
