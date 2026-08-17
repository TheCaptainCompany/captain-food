#!/usr/bin/env bash
# Production E2E smoke test — Stripe TEST mode only.
#
# Layers (each logs PASS/FAIL; the script exits non-zero at the first failing layer):
#   L1  edge         — GET /ping == "pong", GET /health == 200
#   L2  public API   — public GraphQL introspection on a tenant host ({slug}.captain.food)
#   L3  fixture      — a dedicated TEST-mode smoke restaurant (slug `smoke-test`) with one
#                      product/offer exists; created via the real GraphQL mutations (ADMIN role)
#                      when missing. Idempotent: fixed fixture UUIDs, existence-checked first.
#   L3b checkout     — the served /checkout shell carries a pk_test_ publishable key on the Stripe
#                      mount div (#440); the DEGRADED shell fails with the named deploy fact.
#   L4  money path   — cart -> placeOrder (CUSTOMER role, ACCEPTANCE-FIRST: MutationAcceptance ->
#                      poll operationStatus(messageId) to SUCCEEDED, ADR-20260720-015500) -> find
#                      the Stripe PaymentIntent by its orderId metadata -> server-side confirm with
#                      pm_card_visa (manual capture: confirm AUTHORIZES, requires_capture) ->
#                      webhook (PaymentAuthorized) -> mailbox -> order-tracking read model shows
#                      the order PLACED with paymentStatus AUTHORIZED (bounded polling).
#                      Capture happens on delivered/picked up (ADR-20260808-195315 §1.2); the
#                      delivered-leg capture assertions belong to the FUTURE L5 fulfilment leg of
#                      the acceptance program (ADR-20260813-191111), not to L4.
#                      L4 RUNS ENTIRELY ON THE STOREFRONT HOST (#622) — see "WHICH HOST" below.
#
# WHICH HOST — L3 browses the MARKETPLACE, L4 walks a STOREFRONT (#622).
# The two are different products on different hosts and the smoke used to mix them: it wrote the
# guest cart on the marketplace host and read it back there too, then placed and tracked the order
# there — a journey no client walks. `current` resolves its tenant from the `Host` (#469) and
# correctly refuses to answer unbounded, so that read returned `{"current":null}` with NO error:
# byte-identical to "the cart never projected", and undiagnosable from the transcript.
#   * MARKETPLACE (live.<domain>) — L1 edge, and L3's restaurants-by-slug and catalog reads. That IS
#     the marketplace browse: a visitor comparing restaurants before choosing one.
#   * STOREFRONT ({slug}.<domain>) — L2 introspection, L3b the checkout shell, and ALL of L4. A guest
#     is on the restaurant's storefront when they build a cart, and stays there through checkout.
# The call sites cannot name a base: `marketplace*`/`storefront*`/`admin*` helpers each hardcode
# theirs, so the mistake above is not spellable here any more. Do not add a base parameter back.
#
# Safe to re-run against production: TEST-mode money only (sk_test key), one dedicated tenant,
# idempotent fixtures, fresh cart/order ids per run.
#
# Required env:
#   STRIPE_SECRET_KEY     sk_test_... (refused otherwise — this script must never move live money).
#                         CI supplies it from the repo secret STRIPE_SECRET_KEY_TEST; the unsuffixed
#                         repo secret was retired 2026-07-29 because its mode was not visible in its name.
#   SUPABASE_SECRET_KEY   the Supabase service key, used to mint role JWTs through the deployment's own
#                         auth provider (Supabase admin API). It is ITS OWN repo secret since #358: it
#                         used to be read off the Render service via RENDER_API_KEY, and that service
#                         ceases to exist at the OVH cutover — a smoke that can only authenticate
#                         against the platform we are leaving cannot verify the platform we are moving
#                         to. The Supabase URL is NOT a secret: it RIDES THE ARTIFACT (baked per-profile,
#                         ADR-20260729-020000) and is read from the DSL, overridable via SUPABASE_URL.
# Optional env:
#   SMOKE_BASE_DOMAIN     default captain.food. May carry a PORT for a local rehearsal
#                         (`captain.food:8080`) — host classification ignores the port. It may NOT
#                         carry a different DOMAIN: `surface_runtime::hosts::APEX` is a compile-time
#                         constant, so under any other apex every host classifies as `Default`, no
#                         request names a tenant, and every tenant-scoped read serves null. L4
#                         asserts this up front rather than failing as a mystery 60s later.
#   SMOKE_SCHEME          default https; set `http` when smoking a port-forwarded local stack.
#   SMOKE_TENANT_SLUG     default smoke-test
#   SMOKE_APP_PROFILE     which baked config profile the deployment runs (default production)
#   SMOKE_ORDER_TIMEOUT   seconds to wait for the authorized order (default 90)
set -euo pipefail

# --- Config ---------------------------------------------------------------------------------------
SMOKE_BASE_DOMAIN="${SMOKE_BASE_DOMAIN:-captain.food}"
SMOKE_SCHEME="${SMOKE_SCHEME:-https}"
SMOKE_TENANT_SLUG="${SMOKE_TENANT_SLUG:-smoke-test}"
SMOKE_APP_PROFILE="${SMOKE_APP_PROFILE:-production}"
SMOKE_ORDER_TIMEOUT="${SMOKE_ORDER_TIMEOUT:-90}"
# Repo root, so we can read baked (non-secret) config from the DSL (ADR-20260729-020000).
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

# PER-AUDIENCE HOSTS, not one `api.` host (#358). This used to target `https://api.<domain>` for every
# role path — a host the GENERATED Ingress routes NOWHERE: there, role = path on each audience host
# (ADR-0006), so /admin/graphql lives on system.<domain> and the public/customer paths on the
# marketplace host. The monolith answers every role path on every host (its routes are explicit and
# host-independent), so these hosts are correct BOTH today and after the cutover, whereas `api.` was
# correct only before it. `api.` itself stays alive on the monolith Ingress until the registered
# partner webhooks move to hooks.<domain> — it is a webhook address, not an API address.
PUBLIC_BASE="${SMOKE_PUBLIC_BASE:-${SMOKE_SCHEME}://live.${SMOKE_BASE_DOMAIN}}"
ADMIN_BASE="${SMOKE_ADMIN_BASE:-${SMOKE_SCHEME}://system.${SMOKE_BASE_DOMAIN}}"
TENANT_BASE="${SMOKE_SCHEME}://${SMOKE_TENANT_SLUG}.${SMOKE_BASE_DOMAIN}"
STRIPE_API="https://api.stripe.com"
# `surface_runtime::hosts::APEX`, mirrored. Not configurable HERE because it is not configurable
# THERE: the server compares the request host against this exact literal.
SMOKE_APEX="captain.food"

# Fixed fixture ids => idempotent creation (register/create-catalog replays are no-ops server-side,
# addProduct is guarded by an existence check below).
FIX_RESTAURANT_ID="e2e50000-0000-4000-8000-000000000001"
FIX_CATALOG_ID="e2e50000-0000-4000-8000-000000000002"
FIX_PRODUCT_ID="e2e50000-0000-4000-8000-000000000003"
FIX_OFFER_ID="e2e50000-0000-4000-8000-000000000004"
# The seeded offer price, ONE constant read by BOTH the L3 seed and the L4 total assertion — so the
# expected total cannot drift from the thing that produces it (see the L4 assertion for why the
# comparison is `==` and must stay `==`).
FIX_OFFER_PRICE_CENTS=1200
SMOKE_ADMIN_EMAIL="smoke-admin@${SMOKE_BASE_DOMAIN}"
SMOKE_CUSTOMER_EMAIL="smoke-customer@${SMOKE_BASE_DOMAIN}"
# The BRIDGED stranger for the #433 read-guard negative: its own user, its own per-run
# captain_food.customer_id claim — reusing the customer user with a different claim would clobber the
# first token's claim between steps.
SMOKE_STRANGER_EMAIL="smoke-stranger@${SMOKE_BASE_DOMAIN}"

# --- Helpers --------------------------------------------------------------------------------------
# pass/fail/say write to stderr so they survive command substitution (helpers like gql_ok and
# mint_token are called via $(...) — their diagnostics must not be swallowed into the captured value).
say()  { printf '%s\n' "$*" >&2; }
pass() { say "PASS  $*"; }
fail() { say "FAIL  $*"; exit 1; }

need() { command -v "$1" >/dev/null || fail "L0: missing required tool '$1'"; }
need curl; need jq

uuid() {
  if command -v uuidgen >/dev/null; then uuidgen | tr 'A-Z' 'a-z'; else cat /proc/sys/kernel/random/uuid; fi
}

# gql_raw <endpoint> <bearer-token-or-empty> <query> [variables-json] [session-id]
# Prints "<body>\n<http_code>" (the code travels in-band: helpers run inside $(...) subshells, so
# a global variable would be lost). The optional session id is sent as the X-SESSION-ID envelope
# header — the anonymous checkout identity (#12, ADR-20260720-213000).
gql_raw() {
  local endpoint="$1" token="$2" query="$3" variables="${4:-null}" session="${5:-}"
  local body auth=() sess=() out
  if [ -n "$token" ]; then auth=(-H "Authorization: Bearer $token"); fi
  if [ -n "$session" ]; then sess=(-H "X-SESSION-ID: $session"); fi
  body=$(jq -cn --arg q "$query" --argjson v "$variables" '{query:$q, variables:$v}')
  out=$(curl -sS -m 30 -w $'\n%{http_code}' -X POST "$endpoint" \
    -H "Content-Type: application/json" "${auth[@]}" "${sess[@]}" -d "$body") || { printf '{}\ntransport-error'; return 0; }
  printf '%s' "$out"
}

# gql <...> — just the response body (for polling loops that tolerate transient failures).
gql() {
  local out; out=$(gql_raw "$@"); printf '%s' "${out%$'\n'*}"
}

# gql_ok <layer> <endpoint> <token> <query> [variables] [session-id] — fails the run on HTTP!=200
# or GraphQL errors with full diagnostics; prints the response body on success.
gql_ok() {
  local layer="$1" endpoint="$2" token="$3" query="$4" variables="${5:-null}" session="${6:-}"
  local out code resp
  out=$(gql_raw "$endpoint" "$token" "$query" "$variables" "$session")
  code="${out##*$'\n'}"
  resp="${out%$'\n'*}"
  if [ "$code" != "200" ]; then
    fail "$layer: $endpoint returned HTTP $code — body: $(printf '%s' "$resp" | head -c 800)"
  fi
  if [ "$(printf '%s' "$resp" | jq -r 'has("errors")')" = "true" ]; then
    fail "$layer: GraphQL errors from $endpoint: $(printf '%s' "$resp" | jq -c '.errors' | head -c 800)"
  fi
  printf '%s' "$resp"
}

# --- Base-bound call sites: the host is a PROPERTY OF THE HELPER, never an argument (#622) --------
# The defect this closes was one token wide — `$PUBLIC_BASE` where `$TENANT_BASE` belonged — and it
# survived review, a rewrite and twenty nights because both spellings are grammatical. A comment
# saying "do not simplify these to $PUBLIC_BASE" survives until the next refactor; a call site that
# CANNOT name a base does not decay. So: pick the AUDIENCE, and the audience picks the host.
#
#   marketplace* -> live.<domain>       the browse surface (many restaurants, no tenant)
#   storefront*  -> {slug}.<domain>     one restaurant's shop (tenant resolved from the Host, #469)
#   admin*       -> system.<domain>     the back office
#
# Each takes <role-path> (public|customer|admin) and appends /graphql. The `_ok` variants fail the
# run on HTTP!=200 or GraphQL errors; the bare ones return the body for polling loops that tolerate
# transient failures. Do NOT add a base parameter to any of them.
marketplace()    { local p="$1"; shift; gql    "$PUBLIC_BASE/$p/graphql" "$@"; }
marketplace_ok() { local l="$1" p="$2"; shift 2; gql_ok "$l" "$PUBLIC_BASE/$p/graphql" "$@"; }
storefront()     { local p="$1"; shift; gql    "$TENANT_BASE/$p/graphql" "$@"; }
storefront_ok()  { local l="$1" p="$2"; shift 2; gql_ok "$l" "$TENANT_BASE/$p/graphql" "$@"; }
admin_gql()      { local p="$1"; shift; gql    "$ADMIN_BASE/$p/graphql" "$@"; }
admin_ok()       { local l="$1" p="$2"; shift 2; gql_ok "$l" "$ADMIN_BASE/$p/graphql" "$@"; }

# The storefront host must be one the SERVER can classify as a tenant, or every tenant-scoped read
# below returns null — with no error, at every host, indistinguishable from a broken cart. That is
# the #622 defect reachable a second way, through configuration instead of a typo, and it would be
# blamed on the #622 fix. `classify_host` requires `<label>.captain.food` exactly (port ignored,
# `surface_runtime::hosts::APEX` is a compile-time constant), so a base domain that is not the apex
# makes EVERY host `HostRoute::Default`. Fail here, naming the apex, not 60s later in a poll.
assert_tenant_host_classifiable() {
  local domain="${SMOKE_BASE_DOMAIN%%:*}"
  [ "$domain" = "$SMOKE_APEX" ] || fail "L4: SMOKE_BASE_DOMAIN is '${SMOKE_BASE_DOMAIN}', whose domain part '${domain}' is not the apex '${SMOKE_APEX}'. The server's host classifier (surface_runtime::hosts::APEX) is a COMPILE-TIME constant: under any other domain every host — including ${SMOKE_TENANT_SLUG}.${SMOKE_BASE_DOMAIN} — classifies as HostRoute::Default, no request names a tenant, and every tenant-scoped read (cart 'current', #469) serves null with no error. Use a PORT for a local rehearsal (${SMOKE_APEX}:8080 with --resolve or a hosts entry), never a different domain."
}

# --- Supabase role-token minting (the deployment's own auth provider) -----------------------------
# The two values have DIFFERENT homes since ADR-20260729-020000 ("non-secret config rides the artifact"):
#   * SUPABASE_URL        — NON-secret, baked per-profile into the image by the codegen and DELIBERATELY
#                           REMOVED from the deployment env (env > baked precedence means a leftover
#                           dashboard value would silently win over the digest). Read it from the baked
#                           source of truth, the per-scope configuration catalogs.
#   * SUPABASE_SECRET_KEY — a real secret, supplied directly. It USED to be read off the Render service
#                           via RENDER_API_KEY; that branch is GONE (#358) because the service it reads
#                           does not survive the cutover, and a smoke whose auth depends on the platform
#                           being retired cannot verify the platform replacing it.
# An explicit SUPABASE_URL env still overrides the baked lookup.
SB_URL="${SUPABASE_URL:-}"
SB_KEY="${SUPABASE_SECRET_KEY:-}"

# Read a baked per-profile config value straight from the DSL source of truth, coreutils only: the CI
# runner ships mikefarah yq while dev boxes often carry python-yq, and their query syntaxes differ, so a
# yq invocation is not portable here. Blocks are 2-space-indented keys; `deploy:` holds per-profile values.
#
# SCANS EVERY SCOPE CATALOG (`specs/*/configuration.yaml`) rather than one path. The catalogs moved to
# the per-scope layout (ADR-20260807-183024) and this function still pointed at the vanished
# `specs/configuration.yaml`; because a missing file returned EMPTY rather than failing, the daily smoke
# died at L3 saying "SUPABASE_URL not set" — blaming the environment for a repo-layout change. Absence
# of the catalogs ENTIRELY is now its own loud diagnosis, and a key that changes scope needs no edit here.
baked_config() {
  local key="$1" prof="$2" f out found=0
  for f in "${REPO_ROOT}"/specs/*/configuration.yaml; do
    [ -f "$f" ] || continue
    found=1
    out=$(awk -v key="$key" -v prof="$prof" '
      $0 ~ "^  " key ":" {inkey=1; next}
      inkey && /^  [A-Za-z_]/ {inkey=0}
      inkey && $1=="deploy:" {indeploy=1; next}
      indeploy && $1==prof":" {gsub(/^[^:]*:[[:space:]]*/,""); gsub(/^"|"$/,""); print; exit}
    ' "$f")
    if [ -n "$out" ]; then printf '%s' "$out"; return 0; fi
  done
  [ "$found" = "1" ] || fail "L3: no specs/*/configuration.yaml found under ${REPO_ROOT} — the configuration catalogs moved (ADR-20260807-183024 per-scope layout) and this smoke is reading the wrong path, which is a REPO bug, not a missing environment variable"
  return 0
}

load_supabase_creds() {
  [ -n "$SB_URL" ] && [ -n "$SB_KEY" ] && return 0
  if [ -z "$SB_URL" ]; then
    SB_URL=$(baked_config SUPABASE_URL "$SMOKE_APP_PROFILE")
    [ -n "$SB_URL" ] || fail "L3: SUPABASE_URL not set and no baked default for profile '${SMOKE_APP_PROFILE}' in any specs/*/configuration.yaml (ADR-20260729-020000); set SUPABASE_URL to override"
  fi
  [ -n "$SB_KEY" ] || fail "L3: SUPABASE_SECRET_KEY is not set — role JWTs cannot be minted. It is its own repo secret since #358 (it was previously read off the Render service, which the OVH cutover retires)."
}

# mint_token <email> <role> [customer_id] — ensure the smoke user exists with the role (and, when
# given, a per-run customer_id claim — #433: the JWT carries the domain ids), then magic-link verify
# to a session. Prints the access token. Nothing is emailed.
#
# #519: every claim lives INSIDE app_metadata.captain_food. The verifier refuses a token without
# that object, so a mint that still wrote the flat captain_* keys would produce a token this smoke
# cannot use — and the flat keys a previously-stamped smoke user still carries are inert siblings
# the shallow merge leaves behind, read by nothing.
#
# ORDER IS LOAD-BEARING (mob findings, beck+farley): the app_metadata stamp is UNCONDITIONAL and
# happens BEFORE the link used for verification is generated — a conditional repair keyed on the
# role would skip re-stamping a reused smoke user, whose token would then carry LAST run's
# customer id: the positive poll times out and the negative probe silently proves the wrong
# posture. Claims materialize at token ISSUANCE (/verify), and each generate_link rotates the OTP
# hash, so the sequence is: create -> stamp -> link -> verify. The PUT sends the WHOLE captain_food
# object every time — GoTrue's app_metadata merge is shallow and version-dependent, so our object is
# replaced wholesale and never merged key-by-key; never rely on it preserving a sibling claim.
mint_token() {
  local email="$1" role="$2" customer_id="${3:-}" link th sess tok uid meta payload claim
  load_supabase_creds
  # Idempotent create (an already-registered email errors; ignored).
  curl -sS -m 20 -o /dev/null -X POST "$SB_URL/auth/v1/admin/users" \
    -H "apikey: $SB_KEY" -H "Authorization: Bearer $SB_KEY" -H "Content-Type: application/json" \
    -d "$(jq -cn --arg e "$email" --arg r "$role" '{email:$e, email_confirm:true, app_metadata:{captain_food:{role:$r}}}')" || true
  # Resolve the user id (a first link also proves the user exists), then stamp UNCONDITIONALLY.
  link=$(curl -sS -m 20 -X POST "$SB_URL/auth/v1/admin/generate_link" \
    -H "apikey: $SB_KEY" -H "Authorization: Bearer $SB_KEY" -H "Content-Type: application/json" \
    -d "$(jq -cn --arg e "$email" '{type:"magiclink", email:$e}')")
  uid=$(printf '%s' "$link" | jq -r '.id // .user.id // empty')
  [ -n "$uid" ] || fail "L3: could not resolve the Supabase user id for $email: $(printf '%s' "$link" | jq -c 'del(.action_link, .email_otp, .hashed_token)' | head -c 400)"
  if [ -n "$customer_id" ]; then
    meta=$(jq -cn --arg r "$role" --arg c "$customer_id" '{app_metadata:{captain_food:{role:$r, customer_id:$c}}}')
  else
    meta=$(jq -cn --arg r "$role" '{app_metadata:{captain_food:{role:$r}}}')
  fi
  curl -sS -m 20 -o /dev/null -X PUT "$SB_URL/auth/v1/admin/users/$uid" \
    -H "apikey: $SB_KEY" -H "Authorization: Bearer $SB_KEY" -H "Content-Type: application/json" \
    -d "$meta"
  # Fresh link AFTER the stamp (each generate_link rotates the OTP hash — never hold two links).
  link=$(curl -sS -m 20 -X POST "$SB_URL/auth/v1/admin/generate_link" \
    -H "apikey: $SB_KEY" -H "Authorization: Bearer $SB_KEY" -H "Content-Type: application/json" \
    -d "$(jq -cn --arg e "$email" '{type:"magiclink", email:$e}')")
  th=$(printf '%s' "$link" | jq -r '.hashed_token // empty')
  [ -n "$th" ] || fail "L3: could not generate a sign-in link for $email: $(printf '%s' "$link" | jq -c 'del(.action_link, .email_otp, .hashed_token)' | head -c 400)"
  sess=$(curl -sS -m 20 -X POST "$SB_URL/auth/v1/verify" \
    -H "apikey: $SB_KEY" -H "Content-Type: application/json" \
    -d "$(jq -cn --arg t "$th" '{type:"magiclink", token_hash:$t}')")
  tok=$(printf '%s' "$sess" | jq -r '.access_token // empty')
  [ -n "$tok" ] || fail "L3: magic-link verification for $email yielded no session: $(printf '%s' "$sess" | jq -c 'del(.user, .access_token, .refresh_token)' | head -c 400)"
  # The smoke's own "seen red" (#433): decode the JWT payload (base64, no crypto) and assert the
  # claim on the token ACTUALLY ISSUED equals this run's value — every stale-claim failure mode
  # above would otherwise surface only as a 90s timeout or, worse, a false negative-probe pass.
  if [ -n "$customer_id" ]; then
    payload=$(printf '%s' "$tok" | cut -d. -f2 | tr '_-' '/+')
    case $(( ${#payload} % 4 )) in 2) payload="${payload}==";; 3) payload="${payload}=";; esac
    claim=$(printf '%s' "$payload" | base64 -d 2>/dev/null | jq -r '.app_metadata.captain_food.customer_id // empty')
    [ "$claim" = "$customer_id" ] || fail "L3: token for $email carries captain_food.customer_id '$claim', expected '$customer_id' — the stamp did not reach the issued token"
  fi
  printf '%s' "$tok"
}

# --- L1: edge -------------------------------------------------------------------------------------

# The release this smoke's L4 cart assertion describes: #451, live cart pricing at read
# (migration 20260810113000). The server self-reports the schema version its BINARY requires, so
# comparing against it tells us WHICH CODE is deployed, not merely which schema is applied.
SMOKE_MIN_SCHEMA_VERSION=20260810113000

l1() {
  local ping health body required
  ping=$(curl -sS -m 15 "$PUBLIC_BASE/ping" || true)
  [ "$ping" = "pong" ] || fail "L1: $PUBLIC_BASE/ping returned '$ping' (expected 'pong')"
  body=$(curl -sS -m 15 -w $'\n%{http_code}' "$PUBLIC_BASE/health" || true)
  health="${body##*$'\n'}"; body="${body%$'\n'*}"
  [ "$health" = "200" ] || fail "L1: $PUBLIC_BASE/health returned HTTP $health — body: $(printf '%s' "$body" | head -c 400)"
  # Deploy ORDERING (#451): L4 now asserts a cart prices to > 0 through `current`, which a pre-#451
  # binary cannot do — it has no `current` resolver and no read-side pricer. Without this check that
  # shows up as a baffling assertion failure deep in L4; with it, the run says plainly that the
  # smoke is ahead of the deployment. Smoke AFTER deploying, not before.
  required=$(printf '%s' "$body" | jq -r '.requiredSchemaVersion // 0' 2>/dev/null || echo 0)
  [ "$required" -ge "$SMOKE_MIN_SCHEMA_VERSION" ] 2>/dev/null \
    || fail "L1: deployed binary requires schema $required, smoke expects >= $SMOKE_MIN_SCHEMA_VERSION — DEPLOY BEFORE SMOKING (this smoke asserts live cart pricing, #451)"
  pass "L1 edge: /ping=pong, /health=200, binary at schema $required"
}

# --- L2: public GraphQL on the tenant host --------------------------------------------------------
l2() {
  local resp
  resp=$(storefront_ok "L2" public "" '{ __schema { queryType { name } } }')
  [ "$(printf '%s' "$resp" | jq -r '.data.__schema.queryType.name')" = "Query" ] \
    || fail "L2: unexpected introspection payload: $resp"
  pass "L2 public API: introspection OK on $TENANT_BASE/public/graphql"
}

# --- L3: idempotent smoke fixture -----------------------------------------------------------------
RESTAURANT_QUERY='query($slug: Slug!){ restaurant(input:{slug:$slug}) { id status orderAcceptance defaultCurrency } }'
CATALOG_QUERY='query($rid: RestaurantId!){ catalog(input:{restaurantId:$rid}) { id products { id offers { id availability } } } }'

fixture_state() { # prints: restaurant-status|offer-present (e.g. "ACTIVE|yes", "absent|no")
  local r c status offer
  # MARKETPLACE, correctly: resolving a restaurant BY SLUG and reading its catalog is the browse
  # surface a visitor uses before choosing a restaurant. These are the reads that must NOT move to
  # the storefront host with L4 (#622) — they are not storefront reads and never were.
  r=$(marketplace public "" "$RESTAURANT_QUERY" "$(jq -cn --arg s "$SMOKE_TENANT_SLUG" '{slug:$s}')")
  status=$(printf '%s' "$r" | jq -r '.data.restaurant.status // "absent"')
  c=$(marketplace public "" "$CATALOG_QUERY" "$(jq -cn --arg r "$FIX_RESTAURANT_ID" '{rid:$r}')")
  offer=$(printf '%s' "$c" | jq -r --arg o "$FIX_OFFER_ID" '[.data.catalog.products[]?.offers[]? | select(.id==$o and .availability=="AVAILABLE")] | if length>0 then "yes" else "no" end')
  printf '%s|%s' "$status" "$offer"
}

# wait_for <layer> <description> <deadline-secs> <cmd producing "ok" on success> [diagnose-fn]
#
# The optional DIAGNOSE function runs once, on timeout only, and its output is appended to the
# failure. A timeout says "the state never arrived" and nothing about WHY, and the two whys need
# opposite people: `wait_for` therefore refuses to end at "timed out" wherever a single extra read
# can separate them (the pattern the L4 order poll already used — generalised here so the next leg
# gets it by declaring it, not by hand-rolling a loop).
wait_for() {
  local layer="$1" what="$2" deadline="$3" checker="$4" diagnose="${5:-}" t=0 last="" why=""
  while [ "$t" -le "$deadline" ]; do
    last=$("$checker" 2>/dev/null || true)
    [ "$last" = "ok" ] && return 0
    sleep 3; t=$((t+3))
  done
  [ -n "$diagnose" ] && why=" — $("$diagnose" 2>&1 || true)"
  fail "$layer: timed out (${deadline}s) waiting for $what — last state: ${last}${why}"
}

l3() {
  local state admin
  state=$(fixture_state)
  if [ "$state" = "ACTIVE|yes" ]; then
    pass "L3 fixture: restaurant '$SMOKE_TENANT_SLUG' ACTIVE with offer $FIX_OFFER_ID (already present)"
    return 0
  fi
  say "      L3: fixture state '$state' — creating via GraphQL (ADMIN role)"
  admin=$(mint_token "$SMOKE_ADMIN_EMAIL" "ADMIN")

  # 1. Register (idempotent server-side: replaying an existing registration is a no-op). The slug is
  #    no longer part of registration (ADR-20260728-011344): it is chosen by a separate
  #    ConfigureRestaurantSlug command, issued right below. TEST mode => rules.yaml
  #    OrderTestModeIsolation applies.
  admin_ok "L3" admin "$admin" \
    'mutation($i: RegisterRestaurantInput!){ registerRestaurant(input:$i){ correlationId } }' \
    "$(jq -cn --arg id "$FIX_RESTAURANT_ID" '{i:{
        mode:"TEST", restaurantId:$id, displayName:"Smoke Test Restaurant",
        address:{line1:"1 rue du Test", postalCode:"37000", city:"Tours", country:"FR"},
        timezone:"Europe/Paris"}}')" >/dev/null

  # 1b. Configure the storefront slug (idempotent: re-submitting the current slug is a no-op). Same
  #     aggregate as the registration above, so the write-side ordering holds without a wait; the
  #     projection wait below then observes the slug becoming resolvable.
  admin_ok "L3" admin "$admin" \
    'mutation($i: ConfigureRestaurantSlugInput!){ configureRestaurantSlug(input:$i){ correlationId } }' \
    "$(jq -cn --arg id "$FIX_RESTAURANT_ID" --arg slug "$SMOKE_TENANT_SLUG" '{i:{restaurantId:$id, slug:$slug}}')" >/dev/null

  # The registration + slug must be projected before createCatalog (RestaurantNotFound guard reads
  # the view) and before the slug resolves the tenant host.
  check_restaurant_projected() {
    local r; r=$(marketplace public "" "$RESTAURANT_QUERY" "$(jq -cn --arg s "$SMOKE_TENANT_SLUG" '{slug:$s}')")
    [ "$(printf '%s' "$r" | jq -r '.data.restaurant.id // empty')" = "$FIX_RESTAURANT_ID" ] && echo ok || printf '%s' "$r" | jq -c '.data' 2>/dev/null
  }
  wait_for "L3" "restaurant projection" 60 check_restaurant_projected

  # 2. Activate (idempotent: activating an ACTIVE restaurant is a no-op).
  admin_ok "L3" admin "$admin" \
    'mutation($i: ActivateRestaurantInput!){ activateRestaurant(input:$i){ correlationId } }' \
    "$(jq -cn --arg id "$FIX_RESTAURANT_ID" '{i:{restaurantId:$id, reason:"prod smoke fixture"}}')" >/dev/null

  # 3. Catalog (idempotent server-side) + one product/offer (guarded by the offer existence check).
  admin_ok "L3" admin "$admin" \
    'mutation($i: CreateCatalogInput!){ createCatalog(input:$i){ correlationId } }' \
    "$(jq -cn --arg c "$FIX_CATALOG_ID" --arg r "$FIX_RESTAURANT_ID" '{i:{catalogId:$c, restaurantId:$r, name:"Smoke Catalog"}}')" >/dev/null
  if [ "${state##*|}" != "yes" ]; then
    local cat offer_known
    cat=$(marketplace public "" "$CATALOG_QUERY" "$(jq -cn --arg r "$FIX_RESTAURANT_ID" '{rid:$r}')")
    offer_known=$(printf '%s' "$cat" | jq -r --arg o "$FIX_OFFER_ID" '[.data.catalog.products[]?.offers[]? | select(.id==$o)] | length')
    if [ "${offer_known:-0}" = "0" ]; then
      admin_ok "L3" admin "$admin" \
        'mutation($i: AddProductInput!){ addProduct(input:$i){ correlationId } }' \
        "$(jq -cn --arg p "$FIX_PRODUCT_ID" --arg c "$FIX_CATALOG_ID" --arg r "$FIX_RESTAURANT_ID" --arg o "$FIX_OFFER_ID" --argjson price "$FIX_OFFER_PRICE_CENTS" '{i:{
            productId:$p, catalogId:$c, restaurantId:$r, name:"Smoke Pizza",
            taxRate:{delivery:10.0, collection:10.0},
            offers:[{id:$o, productId:$p, name:"Default", price:{amountCents:$price, currency:"EUR"}, availability:"AVAILABLE"}]}}')" >/dev/null
    fi
  fi

  # 4. Wait until the read side shows the complete, orderable fixture.
  check_fixture_complete() { [ "$(fixture_state)" = "ACTIVE|yes" ] && echo ok || fixture_state; }
  wait_for "L3" "ACTIVE restaurant + AVAILABLE offer in the catalog read model" 90 check_fixture_complete
  pass "L3 fixture: restaurant '$SMOKE_TENANT_SLUG' ACTIVE with offer $FIX_OFFER_ID (created)"
}

# --- L3b: the checkout shell delivers the Stripe publishable TEST key (#440) ----------------------
# The browser-side twin of L4's sk_test_ refusal: the payment element can only mount if the served
# shell carries a pk_test_ key on the Stripe mount div (data-pk). Outage-honest: an unreachable
# host or a non-checkout body fails as ITS OWN diagnosis before any key assertion — a 404 or the
# claim landing must never read as "key missing"; and the shell's own DEGRADED state is named as
# the #440 deploy fact (STRIPE_PUBLISHABLE_KEY absent/unusable on the service), not a generic miss.
l3b() {
  local out code body
  out=$(curl -sS -m 20 -w $'\n%{http_code}' "$TENANT_BASE/checkout" || true)
  code="${out##*$'\n'}"; body="${out%$'\n'*}"
  [ "$code" = "200" ] || fail "L3b: $TENANT_BASE/checkout returned HTTP $code — body: $(printf '%s' "$body" | head -c 300)"
  printf '%s' "$body" | grep -q 'data-hydrate="checkout"' \
    || fail "L3b: /checkout did not serve the checkout shell (misroute/outage, not a key problem): $(printf '%s' "$body" | head -c 300)"
  if printf '%s' "$body" | grep -q 'payment_unavailable_state'; then
    fail "L3b: checkout serves the DEGRADED shell — STRIPE_PUBLISHABLE_KEY is missing or unusable on the service (the #440 named deploy fact; also counted as checkout_degraded_render_total{reason=stripe_key_absent})"
  fi
  printf '%s' "$body" | grep -q 'data-pk="pk_test_' \
    || fail "L3b: checkout shell carries no pk_test_ key on the Stripe mount div (neither degraded nor configured — renderer drift?): $(printf '%s' "$body" | head -c 300)"
  pass "L3b checkout shell: pk_test_ publishable key delivered on the Stripe mount div"
}

# --- L4b: the read guard, executed in production (#144/#433) --------------------------------------
# The only executable proof the closed vulnerability stays closed where it matters: a caller who is
# NOT the order's member reads NOTHING — no by-id row, no list dump. Since #433 this runs as a
# BRIDGED stranger: a second smoke user carrying its OWN per-run captain_food.customer_id claim, so the
# probe exercises the membership EXISTS path itself (the sharper posture — #430's version proved
# only the unbridged fail-closed-Public arm, which the DB suite already pinned).
# (rules.yaml cannot carry read-guard coverage — #212 — so this assertion is the production gate.)
#
# EVERY EMPTINESS ASSERTION HERE IS PAIRED WITH A POSITIVE (#622). "The stranger's `orders` is empty"
# is a real proof only while something guarantees the list can be non-empty at all. `orders` is
# claim-scoped today and NOT host-bound, so it is real — but the same class of defect this issue
# fixed for `current` (a read that returns nothing because of the HOST, silently and with no error)
# would turn this security proof vacuous the day `orders` becomes tenant-scoped: green, because the
# host bound everything away, on a run that proved nothing. So the OWNER's list is asserted to
# CONTAIN the order first. If that positive ever fails, this whole probe is void, and it says so
# rather than reporting a guard that held.
l4_negative() {
  local resp others mine stranger stranger_id
  stranger_id=$(uuid)
  [ "$stranger_id" != "$customer_id" ] || fail "L4: stranger uuid collided with the customer id — probe would false-alarm"

  # POSITIVE CONTROL, before any emptiness is required of anyone: the paying customer's own list
  # contains this run's order. Same query, same host, same shape as the negative below — the ONLY
  # difference is whose token asks.
  mine=$(storefront customer "$customer" 'query{ orders { id } }' '{}')
  [ "$(printf '%s' "$mine" | jq -r 'has("errors")')" = "false" ] \
    || fail "L4: read-guard POSITIVE CONTROL errored (the negative below would be vacuous, so the probe is void): $(printf '%s' "$mine" | head -c 300)"
  [ "$(printf '%s' "$mine" | jq -r 'has("data")')" = "true" ] \
    || fail "L4: read-guard POSITIVE CONTROL returned no data envelope (outage — the probe is void): $(printf '%s' "$mine" | head -c 300)"
  [ "$(printf '%s' "$mine" | jq -r --arg id "$order_id" '[.data.orders[]? | select(.id==$id)] | length')" = "1" ] \
    || fail "L4: read-guard POSITIVE CONTROL failed — the PAYING customer's own \`orders\` does not contain $order_id, so 'the stranger sees nothing' would prove nothing (this query answering nobody, e.g. host-bound like cart 'current' #469/#622, is exactly the vacuous-green this control exists to catch): $(printf '%s' "$mine" | head -c 300)"

  stranger=$(mint_token "$SMOKE_STRANGER_EMAIL" "CUSTOMER" "$stranger_id")
  # Outage-honest (post-#430 review): an errored OR malformed response must FAIL the proof — a
  # transport failure collapses to `{}` in gql(), which has no `errors` key and null-chains
  # `.data.order` to null, so has("data") is load-bearing on every probe.
  resp=$(storefront customer "$stranger" \
    'query($id: OrderId!){ order(input:{id:$id}) { id } }' \
    "$(jq -cn --arg id "$order_id" '{id:$id}')")
  [ "$(printf '%s' "$resp" | jq -r 'has("errors")')" = "false" ] \
    || fail "L4: read-guard probe ERRORED (cannot prove the guard): $(printf '%s' "$resp" | head -c 300)"
  [ "$(printf '%s' "$resp" | jq -r 'has("data")')" = "true" ] \
    || fail "L4: read-guard probe returned no data envelope (outage, not a deny — cannot prove the guard): $(printf '%s' "$resp" | head -c 300)"
  [ "$(printf '%s' "$resp" | jq -r '.data.order')" = "null" ] \
    || fail "L4: READ GUARD BREACH — a bridged non-member customer read order $order_id: $(printf '%s' "$resp" | head -c 300)"
  others=$(storefront customer "$stranger" \
    'query{ orders { id } }' '{}')
  [ "$(printf '%s' "$others" | jq -r 'has("errors")')" = "false" ] \
    || fail "L4: read-guard list probe ERRORED (cannot prove the guard): $(printf '%s' "$others" | head -c 300)"
  [ "$(printf '%s' "$others" | jq -r 'has("data")')" = "true" ] \
    || fail "L4: read-guard list probe returned no data envelope: $(printf '%s' "$others" | head -c 300)"
  [ "$(printf '%s' "$others" | jq -r '.data.orders == []')" = "true" ] \
    || fail "L4: READ GUARD BREACH — a bridged non-member customer listed orders (the pre-#144 full-table dump): $(printf '%s' "$others" | head -c 300)"
  say "      L4: read guard held — the OWNER's list carries $order_id, and a bridged non-member resolved null by id and an empty list"
}

# --- L4: full money path (TEST mode) --------------------------------------------------------------
l4() {
  local cart_id line_id session_id order_id customer_id customer admin resp mkt message_id op_status pi secret confirm status pay_status deadline t last
  cart_id=$(uuid); line_id=$(uuid); session_id=$(uuid); order_id=$(uuid); customer_id=$(uuid)

  # The storefront host must be classifiable as a tenant, or every read below serves null with no
  # error — the #622 defect reachable through configuration instead of a typo (#622).
  assert_tenant_host_classifiable

  # 1. Build the cart ON THE STOREFRONT (PUBLIC role — guest carts by design). Acceptance-first: the
  #    mutation only acknowledges (MutationAcceptance); the cart-projection wait below observes the
  #    completion. Written and read on the SAME host, because a cart belongs to one restaurant's shop.
  storefront_ok "L4" public "" \
    'mutation($i: AddCartLineInput!){ addCartLine(input:$i){ messageId operationStatus } }' \
    "$(jq -cn --arg c "$cart_id" --arg r "$FIX_RESTAURANT_ID" --arg l "$line_id" --arg o "$FIX_OFFER_ID" --arg s "$session_id" \
      '{i:{cartId:$c, restaurantId:$r, sessionId:$s, line:{cartLineId:$l, offerId:$o, quantity:1}}}')" >/dev/null

  # placeOrder reads the cart PROJECTION — wait for it, through the GUEST path.
  #
  # `current` + X-SESSION-ID, NOT `cart(input:{id})` (#451). The by-id read is now guarded
  # [CUSTOMER, ADMIN], so the field is not in the PUBLIC schema at all; and even minting a customer
  # token would not help, because this cart is anonymous (customer_id NULL) and the by-id resolver's
  # claim-ownership narrowing resolves someone else's/unbound carts to null. The session leg is the
  # only path this guest fixture can legally reach — which is the right thing to smoke anyway: it
  # exercises the two-leg lookup, the live `price_cart` seam and the cart-price telemetry contract
  # in one probe, on exactly the flow a real guest walks.
  #
  # ONE PROBE IS NOT ENOUGH, BECAUSE `null` IS A LEGAL ANSWER FOR TWO DIFFERENT REASONS (#622): the
  # cart never projected, OR the host names no tenant so the resolver correctly declined. The two
  # are byte-identical on the wire and want opposite people. So the probe is a PAIR that differs in
  # EXACTLY the input under test — the host — and the three outcomes are all attributable:
  #   tenant non-null + marketplace null  -> correct, the only green
  #   BOTH null                           -> the cart is genuinely broken, said in seconds
  #   marketplace non-null                -> a cross-tenant leak: an incident, not a smoke bug
  #
  # The positive asserts the WHOLE priced shape — identity included — not "a row exists". Before
  # #451 the projection carried a stubbed 0/NULL price, so a cart that projected but priced to
  # nothing would have passed a status-only check while the customer saw no payable amount.
  #
  # THE TOTAL IS `==`, NOT `> 0`, AND MUST STAY `==`. That is a deliberate trade of
  # structure-insensitivity for alarm: if a fee, tip or delivery component ever enters cart pricing,
  # this equality goes red, and going red is the POINT — the fix is then to update the expected
  # total here, never to weaken the predicate.
  check_cart_projected() {
    local r; r=$(storefront public "" \
      'query{ current { id restaurantId status totalAmount { amountCents currency } } }' '{}' "$session_id")
    [ "$(printf '%s' "$r" | jq -r \
        --arg cart "$cart_id" --arg rest "$FIX_RESTAURANT_ID" --argjson cents "$FIX_OFFER_PRICE_CENTS" '
        (.data.current.id == $cart)
        and (.data.current.restaurantId == $rest)
        and (.data.current.status == "OPEN")
        and (.data.current.totalAmount.amountCents == $cents)
        and (.data.current.totalAmount.currency == "EUR")
      ' 2>/dev/null)" = "true" ] && echo ok || printf '%s' "$r" | jq -c '.data // .errors' 2>/dev/null
  }
  # The diagnosis arm (#622): the wait above can only say "never arrived". ONE ADMIN read of the
  # cart by id — a query that is claim-scoped, NOT host-bound, so it answers regardless of the
  # tenant question — separates the two worlds the null used to merge. Its absence is why a null
  # went undiagnosed for twenty nights.
  #
  # THE ARM MUST NEVER GO QUIET (seen red on the local rehearsal of this very change): when the
  # ADMIN mint failed, the arm emitted an EMPTY reading into a sentence offering two
  # interpretations, and an empty reading looks exactly like "row absent" — a diagnosis that
  # mis-attributes is worse than none, and it is the same shape as the defect being fixed. So an
  # unusable token, an unparseable body and a real answer are three DIFFERENT outputs.
  diagnose_cart() {
    local a r seen
    if ! a=$(mint_token "$SMOKE_ADMIN_EMAIL" "ADMIN") || [ -z "$a" ]; then
      printf 'DIAGNOSIS UNAVAILABLE for cart %s: no ADMIN token could be minted (see the mint failure above), so read-path-vs-projection is UNANSWERED — do NOT read this as "the row is absent"' "$cart_id"
      return 0
    fi
    r=$(admin_gql admin "$a" 'query($id: CartId!){ cart(input:{id:$id}) { id status totalAmount { amountCents currency } } }' \
      "$(jq -cn --arg id "$cart_id" '{id:$id}')")
    seen=$(printf '%s' "$r" | jq -c '.data.cart // .errors' 2>/dev/null | head -c 240)
    [ -n "$seen" ] || { printf 'DIAGNOSIS UNPARSEABLE for cart %s: the ADMIN read returned %s — UNANSWERED, not "absent"' "$cart_id" "$(printf '%s' "$r" | head -c 160)"; return 0; }
    printf 'ADMIN sees cart %s: %s (row PRESENT = the row projected and the STOREFRONT READ or its host binding is the defect; row ABSENT = the projection never happened, expected total %s cents)' \
      "$cart_id" "$seen" "$FIX_OFFER_PRICE_CENTS"
  }
  wait_for "L4" "the guest cart projected on the storefront and priced live to exactly ${FIX_OFFER_PRICE_CENTS} cents (== is deliberate: if cart pricing gains a component, UPDATE THIS EXPECTED TOTAL, never weaken the predicate to '> 0')" \
    60 check_cart_projected diagnose_cart

  # THE NEGATIVE CONTROL, once, AFTER the positive is green (#622). Same query, same session, same
  # cart — only the host differs, which is what makes the pair attributable. `current` must serve
  # null on the marketplace host: it names no tenant, and a tenant-scoped read serves nothing rather
  # than "the newest cart anywhere" (#469, tenant.rs "Absent => no tenant rows, fail closed").
  #
  # has("data") IS LOAD-BEARING, NOT DECORATION. gql() collapses a transport failure to `{}`, which
  # has no `errors` key and null-chains `.data.current` to null — so without the envelope check an
  # OUTAGE passes this control VACUOUSLY, and the control is precisely what a future reader will
  # trust when the positive goes red. This script has already been bitten by that exact shape once
  # (the #430 read-guard probe); the precedent is followed, not rediscovered.
  mkt=$(marketplace public "" 'query{ current { id status } }' '{}' "$session_id")
  [ "$(printf '%s' "$mkt" | jq -r 'has("errors")')" = "false" ] \
    || fail "L4: marketplace-host cart control ERRORED (cannot prove the binding): $(printf '%s' "$mkt" | head -c 300)"
  [ "$(printf '%s' "$mkt" | jq -r 'has("data")')" = "true" ] \
    || fail "L4: marketplace-host cart control returned no data envelope (outage, not a refusal — the control would pass vacuously): $(printf '%s' "$mkt" | head -c 300)"
  [ "$(printf '%s' "$mkt" | jq -r '.data.current')" = "null" ] \
    || fail "L4: TENANT BINDING BREACH — the marketplace host served cart $cart_id for session $session_id, which must resolve to NO tenant (#469). A storefront read answering off-tenant is a cross-tenant leak, not a smoke defect: $(printf '%s' "$mkt" | head -c 300)"
  say "      L4: cart pair OK — $cart_id priced ${FIX_OFFER_PRICE_CENTS} cents on the storefront, null on the marketplace host (#622)"

  # 2. Checkout as the smoke CUSTOMER (TEST mode order against the TEST restaurant).
  #    Acceptance-first (ADR-20260720-015500): placeOrder returns only the acceptance envelope; the
  #    outcome is read by polling operationStatus(messageId) — owned by this customer's JWT subject
  #    (the journal row's user_id) — until it leaves PENDING. The cart's session id rides along as
  #    the X-SESSION-ID envelope header: the run row stamps it (#12), so the session-scoped
  #    paymentStatus read below works exactly like a real guest checkout.
  #    customerId is REQUIRED as of #144 (structural, not domain-checked at placement): the smoke
  #    generates one per run and — since #433 — stamps the SAME value into the token's
  #    captain_food.customer_id claim, exactly what the product's verifyPhone mint will do (the
  #    stamp-before-issue precondition recorded on #429). The claim IS the identity: the order
  #    poll below runs as this customer, and the negative probe as a bridged stranger.
  customer=$(mint_token "$SMOKE_CUSTOMER_EMAIL" "CUSTOMER" "$customer_id")
  resp=$(storefront_ok "L4" customer "$customer" \
    'mutation($i: PlaceOrderInput!){ placeOrder(input:$i){ messageId operationStatus duplicate } }' \
    "$(jq -cn --arg o "$order_id" --arg r "$FIX_RESTAURANT_ID" --arg c "$cart_id" --arg u "$customer_id" '{i:{
        mode:"TEST", orderId:$o, restaurantId:$r, cartId:$c, customerId:$u,
        customerContact:{displayName:"Smoke Customer", phone:"+33600000000"},
        serviceType:"COLLECTION", paymentMethodId:"pm_card_visa"}}')" "$session_id")
  message_id=$(printf '%s' "$resp" | jq -r '.data.placeOrder.messageId // empty')
  [ -n "$message_id" ] || fail "L4: placeOrder returned no messageId (acceptance): $resp"
  say "      L4: placeOrder accepted (messageId $message_id) — polling operationStatus"

  op_status=""; t=0; last="(never observed)"
  while [ "$t" -le 60 ]; do
    resp=$(storefront customer "$customer" \
      'query($m: MessageId!){ operationStatus(input:{messageId:$m}) { status errorCode message } }' \
      "$(jq -cn --arg m "$message_id" '{m:$m}')")
    op_status=$(printf '%s' "$resp" | jq -r '.data.operationStatus.status // empty')
    case "$op_status" in
      SUCCEEDED) break ;;
      REJECTED|FAILED)
        fail "L4: placeOrder $op_status: $(printf '%s' "$resp" | jq -c '.data.operationStatus' | head -c 400)" ;;
    esac
    last="status=${op_status:-<no operation row>}"
    sleep 3; t=$((t+3))
  done
  [ "$op_status" = "SUCCEEDED" ] || fail "L4: placeOrder operation not terminal after 60s — last: $last"
  say "      L4: placeOrder SUCCEEDED — locating the Stripe PaymentIntent"

  # The Stripe intent id is server-assigned; the run row carries the checkout's X-SESSION-ID
  # (#12, ADR-20260720-213000), so the GUEST-visible paymentStatus(orderId) on /public/graphql
  # serves it — the exact read a real anonymous checkout performs (roles [PUBLIC, CUSTOMER, ADMIN]
  # per #31, session ownership in the resolver; strangers get null).
  pi=""; t=0
  while [ "$t" -le 30 ]; do
    resp=$(storefront public "" \
      'query($id: OrderId!){ paymentStatus(input:{orderId:$id}) { paymentIntentId clientSecret status } }' \
      "$(jq -cn --arg id "$order_id" '{id:$id}')" "$session_id")
    pi=$(printf '%s' "$resp" | jq -r '.data.paymentStatus.paymentIntentId // empty')
    [ -n "$pi" ] && break
    sleep 3; t=$((t+3))
  done
  [ -n "$pi" ] || fail "L4: paymentStatus(orderId=$order_id) served no intent to the checkout session after 30s — last: $(printf '%s' "${resp:-}" | head -c 300)"
  say "      L4: payment intent $pi served by the session-scoped guest paymentStatus read"

  # 3. Server-side confirm with the universal test card (frontend stand-in; TEST mode key only).
  case "${STRIPE_SECRET_KEY:-}" in
    sk_test_*) ;;
    *) fail "L4: STRIPE_SECRET_KEY is not a sk_test_ key — refusing to confirm a payment" ;;
  esac
  # return_url satisfies Stripe when redirect-based payment methods are enabled on the account;
  # pm_card_visa never redirects, so the URL is never visited.
  confirm=$(curl -sS -m 30 -X POST "$STRIPE_API/v1/payment_intents/$pi/confirm" \
    -u "$STRIPE_SECRET_KEY:" -d "payment_method=pm_card_visa" \
    -d "return_url=https://smoke-test.captain.food/checkout/return")
  # Manual capture (ADR-20260808-195315 s1.2): a confirmed intent is AUTHORIZED, not captured --
  # Stripe reports `requires_capture` (funds held). `succeeded` here would mean the deploy is
  # still creating automatic-capture intents: fail with the posture named.
  [ "$(printf '%s' "$confirm" | jq -r '.status // empty')" = "requires_capture" ] \
    || fail "L4: PaymentIntent confirm did not reach requires_capture (authorize-then-capture, ADR-20260808-195315): $(printf '%s' "$confirm" | jq -c '{status, error}' | head -c 500)"
  say "      L4: payment intent confirmed (requires_capture — funds held) — waiting for the webhook + saga"

  # 4. The inbound webhook (PaymentAuthorized) drives the saga: OrderPlaced + projection. Poll —
  #    AS THE CUSTOMER (#433): the token now carries this run's captain_food.customer_id (asserted on
  #    the token itself at mint), so this is the customer-POSITIVE production proof #430 could not
  #    give: the paying customer reads their own order through the membership guard.
  t=0; last="(never observed)"
  while [ "$t" -le "$SMOKE_ORDER_TIMEOUT" ]; do
    resp=$(storefront customer "$customer" \
      'query($id: OrderId!){ order(input:{id:$id}) { id status paymentStatus } }' \
      "$(jq -cn --arg id "$order_id" '{id:$id}')")
    status=$(printf '%s' "$resp" | jq -r '.data.order.status // empty')
    pay_status=$(printf '%s' "$resp" | jq -r '.data.order.paymentStatus // empty')
    last="status=${status:-<no order row>} paymentStatus=${pay_status:-<none>}"
    # AUTHORIZED, not CAPTURED (ADR-20260808-195315 s1.2): placement follows the authorization;
    # the money moves on delivered/picked up. The capture assertion moves to the future L5
    # fulfilment leg (ADR-20260813-191111 program).
    if [ "$pay_status" = "AUTHORIZED" ] && [ -n "$status" ]; then
      say "      L4: customer read their authorized order — asserting the read guard before declaring victory"
      l4_negative
      pass "L4 money path: order $order_id $last on the storefront host (intent $pi authorized via webhook; capture deferred to fulfilment; cart pair attributable, customer-positive + read guard held)"
      return 0
    fi
    sleep 5; t=$((t+5))
  done
  # Diagnosability on timeout (farley): ONE admin read separates "authorization never happened" from
  # "authorization landed but the customer's claim scope is broken" — otherwise both are the same 90s.
  admin=$(mint_token "$SMOKE_ADMIN_EMAIL" "ADMIN")
  resp=$(admin_gql admin "$admin" \
    'query($id: OrderId!){ order(input:{id:$id}) { id status paymentStatus } }' \
    "$(jq -cn --arg id "$order_id" '{id:$id}')")
  fail "L4: customer never read order $order_id after ${SMOKE_ORDER_TIMEOUT}s — last: $last; ADMIN sees: $(printf '%s' "$resp" | jq -c '.data.order' | head -c 200) (order present admin-side = claim/scope path broken, absent = authorization/projection broken)"
}

# --- Run ------------------------------------------------------------------------------------------
say "Captain.Food production smoke — marketplace $PUBLIC_BASE (L1 edge, L3 browse), storefront $TENANT_BASE (L2, L3b, ALL of L4 — #622), admin $ADMIN_BASE — Stripe TEST mode"
l1
l2
l3
l3b
l4
say "ALL LAYERS PASS"
