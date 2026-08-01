#!/usr/bin/env bash
# Production E2E smoke test — Stripe TEST mode only.
#
# Layers (each logs PASS/FAIL; the script exits non-zero at the first failing layer):
#   L1  edge         — GET /ping == "pong", GET /health == 200
#   L2  public API   — public GraphQL introspection on a tenant host ({slug}.captain.food)
#   L3  fixture      — a dedicated TEST-mode smoke restaurant (slug `smoke-test`) with one
#                      product/offer exists; created via the real GraphQL mutations (ADMIN role)
#                      when missing. Idempotent: fixed fixture UUIDs, existence-checked first.
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
#   STRIPE_SECRET_KEY   sk_test_... (refused otherwise — this script must never move live money).
#                       CI supplies it from the repo secret STRIPE_SECRET_KEY_TEST; the unsuffixed
#                       repo secret was retired 2026-07-29 because its mode was not visible in its name.
#   RENDER_API_KEY      used to read the deployed Supabase SECRET key (SUPABASE_SECRET_KEY, which stays
#                       dashboard-managed on the Render service) so role JWTs can be minted through the
#                       deployment's own auth provider (Supabase admin API). The Supabase URL is NOT read
#                       from Render: it is a non-secret that RIDES THE ARTIFACT (baked per-profile,
#                       ADR-20260729-020000) and was deliberately removed from the Render env, so it is
#                       read from the baked source of truth specs/configuration.yaml. Set SUPABASE_URL +
#                       SUPABASE_SECRET_KEY directly to override both (RENDER_API_KEY then optional).
# Optional env:
#   SMOKE_BASE_DOMAIN     default captain.food
#   SMOKE_TENANT_SLUG     default smoke-test
#   RENDER_SERVICE_NAME   default captain-food
#   SMOKE_APP_PROFILE     which baked config profile the deployment runs (default production)
#   SMOKE_ORDER_TIMEOUT   seconds to wait for the captured order (default 90)
set -euo pipefail

# --- Config ---------------------------------------------------------------------------------------
SMOKE_BASE_DOMAIN="${SMOKE_BASE_DOMAIN:-captain.food}"
SMOKE_TENANT_SLUG="${SMOKE_TENANT_SLUG:-smoke-test}"
RENDER_SERVICE_NAME="${RENDER_SERVICE_NAME:-captain-food}"
SMOKE_APP_PROFILE="${SMOKE_APP_PROFILE:-production}"
SMOKE_ORDER_TIMEOUT="${SMOKE_ORDER_TIMEOUT:-90}"
# Repo root, so we can read baked (non-secret) config from specs/configuration.yaml (ADR-20260729-020000).
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
API_BASE="https://api.${SMOKE_BASE_DOMAIN}"
TENANT_BASE="https://${SMOKE_TENANT_SLUG}.${SMOKE_BASE_DOMAIN}"
STRIPE_API="https://api.stripe.com"

# Fixed fixture ids => idempotent creation (register/create-catalog replays are no-ops server-side,
# addProduct is guarded by an existence check below).
FIX_RESTAURANT_ID="e2e50000-0000-4000-8000-000000000001"
FIX_CATALOG_ID="e2e50000-0000-4000-8000-000000000002"
FIX_PRODUCT_ID="e2e50000-0000-4000-8000-000000000003"
FIX_OFFER_ID="e2e50000-0000-4000-8000-000000000004"
SMOKE_ADMIN_EMAIL="smoke-admin@${SMOKE_BASE_DOMAIN}"
SMOKE_CUSTOMER_EMAIL="smoke-customer@${SMOKE_BASE_DOMAIN}"

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
#                           REMOVED from the Render env (env > baked precedence means a leftover dashboard
#                           value would silently win over the digest). Read it from the baked source of
#                           truth specs/configuration.yaml, NOT from the Render service. Before this fix
#                           the script still read it from Render and failed L3 once the key was removed.
#   * SUPABASE_SECRET_KEY — a real secret; stays on the Render service, read via RENDER_API_KEY.
# An explicit SUPABASE_URL / SUPABASE_SECRET_KEY env still overrides either lookup.
SB_URL="${SUPABASE_URL:-}"
SB_KEY="${SUPABASE_SECRET_KEY:-}"

# Read a baked per-profile config value straight from the DSL source of truth, coreutils only: the CI
# runner ships mikefarah yq while dev boxes often carry python-yq, and their query syntaxes differ, so a
# yq invocation is not portable here. Blocks are 2-space-indented keys; `deploy:` holds per-profile values.
baked_config() {
  local key="$1" prof="$2" cfg="${REPO_ROOT}/specs/configuration.yaml"
  [ -f "$cfg" ] || return 0
  awk -v key="$key" -v prof="$prof" '
    $0 ~ "^  " key ":" {inkey=1; next}
    inkey && /^  [A-Za-z_]/ {inkey=0}
    inkey && $1=="deploy:" {indeploy=1; next}
    indeploy && $1==prof":" {gsub(/^[^:]*:[[:space:]]*/,""); gsub(/^"|"$/,""); print; exit}
  ' "$cfg"
}

load_supabase_creds() {
  [ -n "$SB_URL" ] && [ -n "$SB_KEY" ] && return 0
  if [ -z "$SB_URL" ]; then
    SB_URL=$(baked_config SUPABASE_URL "$SMOKE_APP_PROFILE")
    [ -n "$SB_URL" ] || fail "L3: SUPABASE_URL not set and no baked default for profile '${SMOKE_APP_PROFILE}' in specs/configuration.yaml (ADR-20260729-020000); set SUPABASE_URL to override"
  fi
  if [ -z "$SB_KEY" ]; then
    [ -n "${RENDER_API_KEY:-}" ] || fail "L3: need RENDER_API_KEY (or SUPABASE_SECRET_KEY) to read the Supabase service key for role-token minting"
    local sid ev vars
    sid=$(curl -sS -m 20 "https://api.render.com/v1/services?name=${RENDER_SERVICE_NAME}&limit=1" \
      -H "Authorization: Bearer $RENDER_API_KEY" | jq -r '.[0].service.id // empty')
    [ -n "$sid" ] || fail "L3: Render service '${RENDER_SERVICE_NAME}' not found"
    ev=$(curl -sS -m 20 "https://api.render.com/v1/services/${sid}/env-vars?limit=100" \
      -H "Authorization: Bearer $RENDER_API_KEY")
    # SHAPE-AGNOSTIC (2026-07-29): this used `.[].envVar | select(...)`, which assumes Render returns an
    # array of {envVar:{key,value}}. It also returns an object wrapper over the same records, and the docs
    # publish neither shape — the assumption died with `Cannot index string with string "envVar"` the first
    # time render-config-sync.yml ran against production. Pull any object carrying key+value, wherever it
    # sits, so either shape works.
    vars=$(printf '%s' "$ev" | jq -c '[.. | objects | select(has("key") and has("value"))]')
    SB_KEY=$(printf '%s' "$vars" | jq -r 'map(select(.key=="SUPABASE_SECRET_KEY")) | .[0].value // empty')
    [ -n "$SB_KEY" ] || fail "L3: SUPABASE_SECRET_KEY not configured on the Render service"
  fi
}

# mint_token <email> <captain_role> — ensure the smoke user exists with the role, then magic-link
# verify to a session. Prints the access token. Nothing is emailed (admin generate_link only).
mint_token() {
  local email="$1" role="$2" link th sess tok uid have_role
  load_supabase_creds
  # Idempotent create (an already-registered email errors; ignored).
  curl -sS -m 20 -o /dev/null -X POST "$SB_URL/auth/v1/admin/users" \
    -H "apikey: $SB_KEY" -H "Authorization: Bearer $SB_KEY" -H "Content-Type: application/json" \
    -d "$(jq -cn --arg e "$email" --arg r "$role" '{email:$e, email_confirm:true, app_metadata:{captain_role:$r}}')" || true
  link=$(curl -sS -m 20 -X POST "$SB_URL/auth/v1/admin/generate_link" \
    -H "apikey: $SB_KEY" -H "Authorization: Bearer $SB_KEY" -H "Content-Type: application/json" \
    -d "$(jq -cn --arg e "$email" '{type:"magiclink", email:$e}')")
  th=$(printf '%s' "$link" | jq -r '.hashed_token // empty')
  [ -n "$th" ] || fail "L3: could not generate a sign-in link for $email: $(printf '%s' "$link" | jq -c 'del(.action_link, .email_otp, .hashed_token)' | head -c 400)"
  # Repair the role claim if the (pre-existing) user lacks it, then re-link.
  have_role=$(printf '%s' "$link" | jq -r '.app_metadata.captain_role // .user.app_metadata.captain_role // empty')
  if [ "$have_role" != "$role" ]; then
    uid=$(printf '%s' "$link" | jq -r '.id // .user.id')
    curl -sS -m 20 -o /dev/null -X PUT "$SB_URL/auth/v1/admin/users/$uid" \
      -H "apikey: $SB_KEY" -H "Authorization: Bearer $SB_KEY" -H "Content-Type: application/json" \
      -d "$(jq -cn --arg r "$role" '{app_metadata:{captain_role:$r}}')"
    link=$(curl -sS -m 20 -X POST "$SB_URL/auth/v1/admin/generate_link" \
      -H "apikey: $SB_KEY" -H "Authorization: Bearer $SB_KEY" -H "Content-Type: application/json" \
      -d "$(jq -cn --arg e "$email" '{type:"magiclink", email:$e}')")
    th=$(printf '%s' "$link" | jq -r '.hashed_token // empty')
  fi
  sess=$(curl -sS -m 20 -X POST "$SB_URL/auth/v1/verify" \
    -H "apikey: $SB_KEY" -H "Content-Type: application/json" \
    -d "$(jq -cn --arg t "$th" '{type:"magiclink", token_hash:$t}')")
  tok=$(printf '%s' "$sess" | jq -r '.access_token // empty')
  [ -n "$tok" ] || fail "L3: magic-link verification for $email yielded no session: $(printf '%s' "$sess" | jq -c 'del(.user, .access_token, .refresh_token)' | head -c 400)"
  printf '%s' "$tok"
}

# --- L1: edge -------------------------------------------------------------------------------------
l1() {
  local ping health
  ping=$(curl -sS -m 15 "$API_BASE/ping" || true)
  [ "$ping" = "pong" ] || fail "L1: $API_BASE/ping returned '$ping' (expected 'pong')"
  health=$(curl -sS -m 15 -o /dev/null -w '%{http_code}' "$API_BASE/health" || true)
  [ "$health" = "200" ] || fail "L1: $API_BASE/health returned HTTP $health — body: $(curl -sS -m 15 "$API_BASE/health" || true)"
  pass "L1 edge: /ping=pong, /health=200"
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
  r=$(gql "$API_BASE/public/graphql" "" "$RESTAURANT_QUERY" "$(jq -cn --arg s "$SMOKE_TENANT_SLUG" '{slug:$s}')")
  status=$(printf '%s' "$r" | jq -r '.data.restaurant.status // "absent"')
  c=$(gql "$API_BASE/public/graphql" "" "$CATALOG_QUERY" "$(jq -cn --arg r "$FIX_RESTAURANT_ID" '{rid:$r}')")
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
  gql_ok "L3" "$API_BASE/admin/graphql" "$admin" \
    'mutation($i: RegisterRestaurantInput!){ registerRestaurant(input:$i){ correlationId } }' \
    "$(jq -cn --arg id "$FIX_RESTAURANT_ID" '{i:{
        mode:"TEST", restaurantId:$id, displayName:"Smoke Test Restaurant",
        address:{line1:"1 rue du Test", postalCode:"37000", city:"Tours", country:"FR"},
        timezone:"Europe/Paris"}}')" >/dev/null

  # 1b. Configure the storefront slug (idempotent: re-submitting the current slug is a no-op). Same
  #     aggregate as the registration above, so the write-side ordering holds without a wait; the
  #     projection wait below then observes the slug becoming resolvable.
  gql_ok "L3" "$API_BASE/admin/graphql" "$admin" \
    'mutation($i: ConfigureRestaurantSlugInput!){ configureRestaurantSlug(input:$i){ correlationId } }' \
    "$(jq -cn --arg id "$FIX_RESTAURANT_ID" --arg slug "$SMOKE_TENANT_SLUG" '{i:{restaurantId:$id, slug:$slug}}')" >/dev/null

  # The registration + slug must be projected before createCatalog (RestaurantNotFound guard reads
  # the view) and before the slug resolves the tenant host.
  check_restaurant_projected() {
    local r; r=$(gql "$API_BASE/public/graphql" "" "$RESTAURANT_QUERY" "$(jq -cn --arg s "$SMOKE_TENANT_SLUG" '{slug:$s}')")
    [ "$(printf '%s' "$r" | jq -r '.data.restaurant.id // empty')" = "$FIX_RESTAURANT_ID" ] && echo ok || printf '%s' "$r" | jq -c '.data' 2>/dev/null
  }
  wait_for "L3" "restaurant projection" 60 check_restaurant_projected

  # 2. Activate (idempotent: activating an ACTIVE restaurant is a no-op).
  gql_ok "L3" "$API_BASE/admin/graphql" "$admin" \
    'mutation($i: ActivateRestaurantInput!){ activateRestaurant(input:$i){ correlationId } }' \
    "$(jq -cn --arg id "$FIX_RESTAURANT_ID" '{i:{restaurantId:$id, reason:"prod smoke fixture"}}')" >/dev/null

  # 3. Catalog (idempotent server-side) + one product/offer (guarded by the offer existence check).
  gql_ok "L3" "$API_BASE/admin/graphql" "$admin" \
    'mutation($i: CreateCatalogInput!){ createCatalog(input:$i){ correlationId } }' \
    "$(jq -cn --arg c "$FIX_CATALOG_ID" --arg r "$FIX_RESTAURANT_ID" '{i:{catalogId:$c, restaurantId:$r, name:"Smoke Catalog"}}')" >/dev/null
  if [ "${state##*|}" != "yes" ]; then
    local cat offer_known
    cat=$(gql "$API_BASE/public/graphql" "" "$CATALOG_QUERY" "$(jq -cn --arg r "$FIX_RESTAURANT_ID" '{rid:$r}')")
    offer_known=$(printf '%s' "$cat" | jq -r --arg o "$FIX_OFFER_ID" '[.data.catalog.products[]?.offers[]? | select(.id==$o)] | length')
    if [ "${offer_known:-0}" = "0" ]; then
      gql_ok "L3" "$API_BASE/admin/graphql" "$admin" \
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

# --- L4: full money path (TEST mode) --------------------------------------------------------------
l4() {
  local cart_id line_id session_id order_id customer resp message_id op_status pi secret confirm status pay_status deadline t last
  cart_id=$(uuid); line_id=$(uuid); session_id=$(uuid); order_id=$(uuid)

  # 1. Build the cart (PUBLIC role — guest carts by design). Acceptance-first: the mutation only
  #    acknowledges (MutationAcceptance); the cart-projection wait below observes the completion.
  gql_ok "L4" "$API_BASE/public/graphql" "" \
    'mutation($i: AddCartLineInput!){ addCartLine(input:$i){ messageId operationStatus } }' \
    "$(jq -cn --arg c "$cart_id" --arg r "$FIX_RESTAURANT_ID" --arg l "$line_id" --arg o "$FIX_OFFER_ID" --arg s "$session_id" \
      '{i:{cartId:$c, restaurantId:$r, sessionId:$s, line:{cartLineId:$l, offerId:$o, quantity:1}}}')" >/dev/null

  # placeOrder reads the cart PROJECTION — wait for it.
  check_cart_projected() {
    local r; r=$(gql "$API_BASE/public/graphql" "" 'query($id: CartId!){ cart(input:{id:$id}) { id status totalAmount { amountCents currency } } }' "$(jq -cn --arg id "$cart_id" '{id:$id}')")
    [ "$(printf '%s' "$r" | jq -r '.data.cart.status // empty')" = "OPEN" ] && echo ok || printf '%s' "$r" | jq -c '.data' 2>/dev/null
  }
  wait_for "L4" "cart projection" 60 check_cart_projected

  # 2. Checkout as the smoke CUSTOMER (TEST mode order against the TEST restaurant).
  #    Acceptance-first (ADR-20260720-015500): placeOrder returns only the acceptance envelope; the
  #    outcome is read by polling operationStatus(messageId) — owned by this customer's JWT subject
  #    (the journal row's user_id) — until it leaves PENDING. The cart's session id rides along as
  #    the X-SESSION-ID envelope header: the run row stamps it (#12), so the session-scoped
  #    paymentStatus read below works exactly like a real guest checkout.
  customer=$(mint_token "$SMOKE_CUSTOMER_EMAIL" "CUSTOMER")
  resp=$(gql_ok "L4" "$API_BASE/customer/graphql" "$customer" \
    'mutation($i: PlaceOrderInput!){ placeOrder(input:$i){ messageId operationStatus duplicate } }' \
    "$(jq -cn --arg o "$order_id" --arg r "$FIX_RESTAURANT_ID" --arg c "$cart_id" '{i:{
        mode:"TEST", orderId:$o, restaurantId:$r, cartId:$c,
        customerContact:{displayName:"Smoke Customer", phone:"+33600000000"},
        serviceType:"COLLECTION", paymentMethodId:"pm_card_visa"}}')" "$session_id")
  message_id=$(printf '%s' "$resp" | jq -r '.data.placeOrder.messageId // empty')
  [ -n "$message_id" ] || fail "L4: placeOrder returned no messageId (acceptance): $resp"
  say "      L4: placeOrder accepted (messageId $message_id) — polling operationStatus"

  op_status=""; t=0; last="(never observed)"
  while [ "$t" -le 60 ]; do
    resp=$(gql "$API_BASE/customer/graphql" "$customer" \
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
    resp=$(gql "$API_BASE/public/graphql" "" \
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

  # 4. The inbound webhook (PaymentCaptured) drives the saga: OrderPlaced + projection. Poll.
  t=0; last="(never observed)"
  while [ "$t" -le "$SMOKE_ORDER_TIMEOUT" ]; do
    resp=$(gql "$API_BASE/customer/graphql" "$customer" \
      'query($id: OrderId!){ order(input:{id:$id}) { id status paymentStatus } }' \
      "$(jq -cn --arg id "$order_id" '{id:$id}')")
    status=$(printf '%s' "$resp" | jq -r '.data.order.status // empty')
    pay_status=$(printf '%s' "$resp" | jq -r '.data.order.paymentStatus // empty')
    last="status=${status:-<no order row>} paymentStatus=${pay_status:-<none>}"
    if [ "$pay_status" = "CAPTURED" ] && [ -n "$status" ]; then
      pass "L4 money path: order $order_id $last (intent $pi captured via webhook)"
      return 0
    fi
    sleep 5; t=$((t+5))
  done
  fail "L4: order $order_id not captured after ${SMOKE_ORDER_TIMEOUT}s — last observed: $last"
}

# --- Run ------------------------------------------------------------------------------------------
say "Captain.Food production smoke — $API_BASE (tenant $TENANT_BASE) — Stripe TEST mode"
l1
l2
l3
l4
say "ALL LAYERS PASS"
