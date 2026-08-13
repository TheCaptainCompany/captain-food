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
#                      pm_card_visa -> webhook -> inbound_events drain -> order-tracking read model
#                      shows the order PLACED with paymentStatus CAPTURED (bounded polling).
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
#   SMOKE_BASE_DOMAIN     default captain.food. May carry a port for a local rehearsal
#                         (`captain.local:8080`) — host classification ignores it.
#   SMOKE_SCHEME          default https; set `http` when smoking a port-forwarded local stack.
#   SMOKE_TENANT_SLUG     default smoke-test
#   SMOKE_APP_PROFILE     which baked config profile the deployment runs (default production)
#   SMOKE_ORDER_TIMEOUT   seconds to wait for the captured order (default 90)
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

# Fixed fixture ids => idempotent creation (register/create-catalog replays are no-ops server-side,
# addProduct is guarded by an existence check below).
FIX_RESTAURANT_ID="e2e50000-0000-4000-8000-000000000001"
FIX_CATALOG_ID="e2e50000-0000-4000-8000-000000000002"
FIX_PRODUCT_ID="e2e50000-0000-4000-8000-000000000003"
FIX_OFFER_ID="e2e50000-0000-4000-8000-000000000004"
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
  resp=$(gql_ok "L2" "$TENANT_BASE/public/graphql" "" '{ __schema { queryType { name } } }')
  [ "$(printf '%s' "$resp" | jq -r '.data.__schema.queryType.name')" = "Query" ] \
    || fail "L2: unexpected introspection payload: $resp"
  pass "L2 public API: introspection OK on $TENANT_BASE/public/graphql"
}

# --- L3: idempotent smoke fixture -----------------------------------------------------------------
RESTAURANT_QUERY='query($slug: Slug!){ restaurant(input:{slug:$slug}) { id status orderAcceptance defaultCurrency } }'
CATALOG_QUERY='query($rid: RestaurantId!){ catalog(input:{restaurantId:$rid}) { id products { id offers { id availability } } } }'

fixture_state() { # prints: restaurant-status|offer-present (e.g. "ACTIVE|yes", "absent|no")
  local r c status offer
  r=$(gql "$PUBLIC_BASE/public/graphql" "" "$RESTAURANT_QUERY" "$(jq -cn --arg s "$SMOKE_TENANT_SLUG" '{slug:$s}')")
  status=$(printf '%s' "$r" | jq -r '.data.restaurant.status // "absent"')
  c=$(gql "$PUBLIC_BASE/public/graphql" "" "$CATALOG_QUERY" "$(jq -cn --arg r "$FIX_RESTAURANT_ID" '{rid:$r}')")
  offer=$(printf '%s' "$c" | jq -r --arg o "$FIX_OFFER_ID" '[.data.catalog.products[]?.offers[]? | select(.id==$o and .availability=="AVAILABLE")] | if length>0 then "yes" else "no" end')
  printf '%s|%s' "$status" "$offer"
}

wait_for() { # wait_for <layer> <description> <deadline-secs> <cmd producing "ok" on success>
  local layer="$1" what="$2" deadline="$3" checker="$4" t=0 last=""
  while [ "$t" -le "$deadline" ]; do
    last=$("$checker" 2>/dev/null || true)
    [ "$last" = "ok" ] && return 0
    sleep 3; t=$((t+3))
  done
  fail "$layer: timed out (${deadline}s) waiting for $what — last state: $last"
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
  gql_ok "L3" "$ADMIN_BASE/admin/graphql" "$admin" \
    'mutation($i: RegisterRestaurantInput!){ registerRestaurant(input:$i){ correlationId } }' \
    "$(jq -cn --arg id "$FIX_RESTAURANT_ID" '{i:{
        mode:"TEST", restaurantId:$id, displayName:"Smoke Test Restaurant",
        address:{line1:"1 rue du Test", postalCode:"37000", city:"Tours", country:"FR"},
        timezone:"Europe/Paris"}}')" >/dev/null

  # 1b. Configure the storefront slug (idempotent: re-submitting the current slug is a no-op). Same
  #     aggregate as the registration above, so the write-side ordering holds without a wait; the
  #     projection wait below then observes the slug becoming resolvable.
  gql_ok "L3" "$ADMIN_BASE/admin/graphql" "$admin" \
    'mutation($i: ConfigureRestaurantSlugInput!){ configureRestaurantSlug(input:$i){ correlationId } }' \
    "$(jq -cn --arg id "$FIX_RESTAURANT_ID" --arg slug "$SMOKE_TENANT_SLUG" '{i:{restaurantId:$id, slug:$slug}}')" >/dev/null

  # The registration + slug must be projected before createCatalog (RestaurantNotFound guard reads
  # the view) and before the slug resolves the tenant host.
  check_restaurant_projected() {
    local r; r=$(gql "$PUBLIC_BASE/public/graphql" "" "$RESTAURANT_QUERY" "$(jq -cn --arg s "$SMOKE_TENANT_SLUG" '{slug:$s}')")
    [ "$(printf '%s' "$r" | jq -r '.data.restaurant.id // empty')" = "$FIX_RESTAURANT_ID" ] && echo ok || printf '%s' "$r" | jq -c '.data' 2>/dev/null
  }
  wait_for "L3" "restaurant projection" 60 check_restaurant_projected

  # 2. Activate (idempotent: activating an ACTIVE restaurant is a no-op).
  gql_ok "L3" "$ADMIN_BASE/admin/graphql" "$admin" \
    'mutation($i: ActivateRestaurantInput!){ activateRestaurant(input:$i){ correlationId } }' \
    "$(jq -cn --arg id "$FIX_RESTAURANT_ID" '{i:{restaurantId:$id, reason:"prod smoke fixture"}}')" >/dev/null

  # 3. Catalog (idempotent server-side) + one product/offer (guarded by the offer existence check).
  gql_ok "L3" "$ADMIN_BASE/admin/graphql" "$admin" \
    'mutation($i: CreateCatalogInput!){ createCatalog(input:$i){ correlationId } }' \
    "$(jq -cn --arg c "$FIX_CATALOG_ID" --arg r "$FIX_RESTAURANT_ID" '{i:{catalogId:$c, restaurantId:$r, name:"Smoke Catalog"}}')" >/dev/null
  if [ "${state##*|}" != "yes" ]; then
    local cat offer_known
    cat=$(gql "$PUBLIC_BASE/public/graphql" "" "$CATALOG_QUERY" "$(jq -cn --arg r "$FIX_RESTAURANT_ID" '{rid:$r}')")
    offer_known=$(printf '%s' "$cat" | jq -r --arg o "$FIX_OFFER_ID" '[.data.catalog.products[]?.offers[]? | select(.id==$o)] | length')
    if [ "${offer_known:-0}" = "0" ]; then
      gql_ok "L3" "$ADMIN_BASE/admin/graphql" "$admin" \
        'mutation($i: AddProductInput!){ addProduct(input:$i){ correlationId } }' \
        "$(jq -cn --arg p "$FIX_PRODUCT_ID" --arg c "$FIX_CATALOG_ID" --arg r "$FIX_RESTAURANT_ID" --arg o "$FIX_OFFER_ID" '{i:{
            productId:$p, catalogId:$c, restaurantId:$r, name:"Smoke Pizza",
            taxRate:{delivery:10.0, collection:10.0},
            offers:[{id:$o, productId:$p, name:"Default", price:{amountCents:1200, currency:"EUR"}, availability:"AVAILABLE"}]}}')" >/dev/null
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
l4_negative() {
  local resp others stranger stranger_id
  stranger_id=$(uuid)
  [ "$stranger_id" != "$customer_id" ] || fail "L4: stranger uuid collided with the customer id — probe would false-alarm"
  stranger=$(mint_token "$SMOKE_STRANGER_EMAIL" "CUSTOMER" "$stranger_id")
  # Outage-honest (post-#430 review): an errored OR malformed response must FAIL the proof — a
  # transport failure collapses to `{}` in gql(), which has no `errors` key and null-chains
  # `.data.order` to null, so has("data") is load-bearing on every probe.
  resp=$(gql "$PUBLIC_BASE/customer/graphql" "$stranger" \
    'query($id: OrderId!){ order(input:{id:$id}) { id } }' \
    "$(jq -cn --arg id "$order_id" '{id:$id}')")
  [ "$(printf '%s' "$resp" | jq -r 'has("errors")')" = "false" ] \
    || fail "L4: read-guard probe ERRORED (cannot prove the guard): $(printf '%s' "$resp" | head -c 300)"
  [ "$(printf '%s' "$resp" | jq -r 'has("data")')" = "true" ] \
    || fail "L4: read-guard probe returned no data envelope (outage, not a deny — cannot prove the guard): $(printf '%s' "$resp" | head -c 300)"
  [ "$(printf '%s' "$resp" | jq -r '.data.order')" = "null" ] \
    || fail "L4: READ GUARD BREACH — a bridged non-member customer read order $order_id: $(printf '%s' "$resp" | head -c 300)"
  others=$(gql "$PUBLIC_BASE/customer/graphql" "$stranger" \
    'query{ orders { id } }' '{}')
  [ "$(printf '%s' "$others" | jq -r 'has("errors")')" = "false" ] \
    || fail "L4: read-guard list probe ERRORED (cannot prove the guard): $(printf '%s' "$others" | head -c 300)"
  [ "$(printf '%s' "$others" | jq -r 'has("data")')" = "true" ] \
    || fail "L4: read-guard list probe returned no data envelope: $(printf '%s' "$others" | head -c 300)"
  [ "$(printf '%s' "$others" | jq -r '.data.orders == []')" = "true" ] \
    || fail "L4: READ GUARD BREACH — a bridged non-member customer listed orders (the pre-#144 full-table dump): $(printf '%s' "$others" | head -c 300)"
  say "      L4: read guard held — bridged non-member by-id resolved null, list came back empty"
}

# --- L4: full money path (TEST mode) --------------------------------------------------------------
l4() {
  local cart_id line_id session_id order_id customer_id customer admin resp message_id op_status pi secret confirm status pay_status deadline t last
  cart_id=$(uuid); line_id=$(uuid); session_id=$(uuid); order_id=$(uuid); customer_id=$(uuid)

  # 1. Build the cart (PUBLIC role — guest carts by design). Acceptance-first: the mutation only
  #    acknowledges (MutationAcceptance); the cart-projection wait below observes the completion.
  gql_ok "L4" "$PUBLIC_BASE/public/graphql" "" \
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
  # The assertion is the WHOLE priced shape in one predicate, not just "a row exists". Before #451
  # the projection carried a stubbed 0/NULL price, so a cart that projected but priced to nothing
  # would have passed a status-only check while the customer saw no payable amount — the silent
  # conversion failure this smoke exists to catch. amountCents MUST be > 0 for the seeded fixture.
  check_cart_projected() {
    local r; r=$(gql "$PUBLIC_BASE/public/graphql" "" \
      'query{ current { id status totalAmount { amountCents currency } } }' '{}' "$session_id")
    [ "$(printf '%s' "$r" | jq -r '
        (.data.current.status == "OPEN")
        and (.data.current.totalAmount.amountCents > 0)
        and (.data.current.totalAmount.currency == "EUR")
      ' 2>/dev/null)" = "true" ] && echo ok || printf '%s' "$r" | jq -c '.data // .errors' 2>/dev/null
  }
  wait_for "L4" "cart projected and priced live" 60 check_cart_projected

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
  resp=$(gql_ok "L4" "$PUBLIC_BASE/customer/graphql" "$customer" \
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
    resp=$(gql "$PUBLIC_BASE/customer/graphql" "$customer" \
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
    resp=$(gql "$PUBLIC_BASE/public/graphql" "" \
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
  [ "$(printf '%s' "$confirm" | jq -r '.status // empty')" = "succeeded" ] \
    || fail "L4: PaymentIntent confirm did not succeed: $(printf '%s' "$confirm" | jq -c '{status, error}' | head -c 500)"
  say "      L4: payment intent confirmed (succeeded) — waiting for the webhook + saga"

  # 4. The inbound webhook (PaymentCaptured) drives the saga: OrderPlaced + projection. Poll —
  #    AS THE CUSTOMER (#433): the token now carries this run's captain_food.customer_id (asserted on
  #    the token itself at mint), so this is the customer-POSITIVE production proof #430 could not
  #    give: the paying customer reads their own order through the membership guard.
  t=0; last="(never observed)"
  while [ "$t" -le "$SMOKE_ORDER_TIMEOUT" ]; do
    resp=$(gql "$PUBLIC_BASE/customer/graphql" "$customer" \
      'query($id: OrderId!){ order(input:{id:$id}) { id status paymentStatus } }' \
      "$(jq -cn --arg id "$order_id" '{id:$id}')")
    status=$(printf '%s' "$resp" | jq -r '.data.order.status // empty')
    pay_status=$(printf '%s' "$resp" | jq -r '.data.order.paymentStatus // empty')
    last="status=${status:-<no order row>} paymentStatus=${pay_status:-<none>}"
    if [ "$pay_status" = "CAPTURED" ] && [ -n "$status" ]; then
      say "      L4: customer read their captured order — asserting the read guard before declaring victory"
      l4_negative
      pass "L4 money path: order $order_id $last (intent $pi captured via webhook; customer-positive + read guard held)"
      return 0
    fi
    sleep 5; t=$((t+5))
  done
  # Diagnosability on timeout (farley): ONE admin read separates "capture never happened" from
  # "capture landed but the customer's claim scope is broken" — otherwise both are the same 90s.
  admin=$(mint_token "$SMOKE_ADMIN_EMAIL" "ADMIN")
  resp=$(gql "$ADMIN_BASE/admin/graphql" "$admin" \
    'query($id: OrderId!){ order(input:{id:$id}) { id status paymentStatus } }' \
    "$(jq -cn --arg id "$order_id" '{id:$id}')")
  fail "L4: customer never read order $order_id after ${SMOKE_ORDER_TIMEOUT}s — last: $last; ADMIN sees: $(printf '%s' "$resp" | jq -c '.data.order' | head -c 200) (order present admin-side = claim/scope path broken, absent = capture/projection broken)"
}

# --- Run ------------------------------------------------------------------------------------------
say "Captain.Food production smoke — public $PUBLIC_BASE, admin $ADMIN_BASE, tenant $TENANT_BASE — Stripe TEST mode"
l1
l2
l3
l3b
l4
say "ALL LAYERS PASS"
