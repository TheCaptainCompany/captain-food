# The walk — one order, browse to captured, on one local database

Issue [#556 "Local acceptance harness"](https://github.com/TheCaptainCompany/captain-food/issues/556),
feeding [#554 "Smoke L5 - acceptance lifecycle legs"](https://github.com/TheCaptainCompany/captain-food/issues/554).

**No order has ever gone through this system end to end.** This harness exists to produce one, or to
say honestly which leg stops it. It is a **reading, not a certificate**
([ADR-20260817-105844](../../docs/adr/ADR-20260817-105844-the-walk-goes-first-on-one-database-and-production-stays-suspended.md)).

## Running it

```bash
tools/walk/up.sh      # postgres + migrations (sqlx-cli) + JWKS stub + one `server` process
tools/walk/walk.sh    # the walk itself
```

`up.sh` needs `sqlx-cli` and a built server binary:

```bash
curl -sSL https://github.com/cargo-bins/cargo-quickinstall/releases/download/sqlx-cli-0.8.3/sqlx-cli-0.8.3-x86_64-unknown-linux-gnu.tar.gz | tar xz
install -m755 sqlx /usr/local/bin/sqlx
cargo build -p server --bin server
```

**There is deliberately no second migration runner here.** `sqlx-cli` is the single migration
authority (ADR-0043), the same tool CI's `db-migrate.yml` runs. An earlier revision of this harness
carried its own runner because `sqlx` was absent locally; it was deleted. Two implementations of
"apply the chain" is how a local green and a CI red diverge — and the semantics are not trivial:
migration `20260730043000` is a `VACUUM FULL` carrying sqlx's `-- no-transaction` directive, which a
naive runner wraps in a transaction and fails on.

## Target independence

The same script runs against the local bare process, a k3s rehearsal and later MKS by changing
`WALK_BASE_DOMAIN` / `WALK_SCHEME` only. Nothing hard-codes a port, a loopback address or a local
path. This is a hard constraint — do not break it.

## The auth stub, and what it is not

Only the **JWKS endpoint** is stubbed. `SUPABASE_URL` keeps its real baked value, so `iss` is the
real issuer string; only key distribution moves to loopback, because Supabase is unreachable from
the walk environment (the proxy answers 502 to CONNECT — a policy denial).

The verifier is **unmodified and fail-closed**. Measured:

| probe | result |
|---|---|
| no token on `/admin/graphql` | 401 |
| minted ADMIN token on `/admin/graphql` | 200 |
| same token on `/restaurant/graphql` | 403 (strict role equality) |
| tampered signature | 401 |
| wrong issuer | 401 |
| valid signature, no `captain_food` role | 403 |

**This is not a GoTrue fake.** It cannot register a user, send or verify an OTP, or stamp
`app_metadata` on a real Supabase user. That is deliberate: a fake that minted convincing customers
would make the walk's first leg *look* real while proving nothing, which is the exact failure this
harness exists to avoid. The registration leg is therefore left **RED and named**.

## The two non-negotiable assertions

1. **Assert on `domain_events` rows, and keep the read-model assertion alongside — not instead.**
   The projector's `OrderPlaced` arm writes `payment_status = 'AUTHORIZED'` at *birth*, with no
   payment fact anywhere. So `paymentStatus == AUTHORIZED` proves the order was **born**, not that
   an authorization was recorded — one safe indirection today, and fully vacuous the moment
   anything else births an order.
2. **The accept leg is performed by the OWNING restaurant's identity** — never ADMIN, never the
   path role alone, or the walk passes identically before and after the authorization fix.

## Declaring a leg IS running it

The run loop is driven by the `LEGS` array. A final assertion fails the run as **INCOMPLETE** if any
declared leg reported no verdict. A declared leg that never runs while the script prints a summary
is the single most likely defect in a harness like this — fix the leg, never this check.

Verdicts are `PASS` / `RED` / `BLOCKED`, each with a `PROVENANCE` of `WALKED`, `INJECTED` or
`SIMULATED`, printed per leg in the transcript rather than footnoted. `BLOCKED` means *not reached*
— a cascade, not evidence — and never counts toward a green.

## What a green walk does NOT prove

Printed verbatim at the end of every run:

> No deploy path, image, TLS, DNS or ingress. Stripe TEST with one card: no 3DS/SCA — in France the
> common path, not an edge case — no decline, no partial capture, no dispute. **Nobody was told**:
> the accept leg is script-issued. One instance, no WAL archiving, no base backup, restore drill
> never executed against this stack. One order, one user: nothing about Friday peak. One database,
> so the boundary rows pass because the wall is not physical. Flags OFF, so this certifies the
> pre-flip configuration and the evidence does not transfer the day a default flips.
