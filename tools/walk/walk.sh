#!/usr/bin/env bash
# The walk: one DELIVERY order, browse to captured, against one database.
#
# This is a READING, not a certificate. What a green walk does NOT prove is printed at the end of
# every run and repeated in tools/walk/README.md -- read it before quoting a green.
#
# TARGET-INDEPENDENCE IS A HARD CONSTRAINT. The same script must run against the local bare
# process, a k3s rehearsal and later MKS by changing WALK_BASE_DOMAIN / WALK_SCHEME only. Nothing
# below may hard-code a port, a loopback address or a local path.
set -uo pipefail

WALK_BASE_DOMAIN="${WALK_BASE_DOMAIN:-captain.food:8080}"
WALK_SCHEME="${WALK_SCHEME:-http}"
WALK_TENANT_SLUG="${WALK_TENANT_SLUG:-walk-test}"
WALK_STATE="${WALK_STATE:-/tmp/captain-walk}"
WALK_SUPABASE_URL="${WALK_SUPABASE_URL:-https://zcshlzhiinwmpzujuiep.supabase.co}"
WALK_DATABASE_URL="${WALK_DATABASE_URL:-postgres://walk:walk@127.0.0.1:5432/captain_walk}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

PUBLIC_BASE="${WALK_SCHEME}://live.${WALK_BASE_DOMAIN}"
ADMIN_BASE="${WALK_SCHEME}://system.${WALK_BASE_DOMAIN}"
TENANT_BASE="${WALK_SCHEME}://${WALK_TENANT_SLUG}.${WALK_BASE_DOMAIN}"

# Fixed fixture ids => idempotent re-runs.
FIX_RESTAURANT_ID="a1c00000-0000-4000-8000-000000000001"
FIX_CATALOG_ID="a1c00000-0000-4000-8000-000000000002"
FIX_PRODUCT_ID="a1c00000-0000-4000-8000-000000000003"
FIX_OFFER_ID="a1c00000-0000-4000-8000-000000000004"

# Per-run ids.
CART_ID=""; LINE_ID=""; SESSION_ID=""; ORDER_ID=""; CUSTOMER_ID=""; RIDER_ID=""
CUSTOMER_TOKEN=""; RESTAURANT_TOKEN=""; ADMIN_TOKEN=""; RIDER_TOKEN=""

say()  { printf '%s\n' "$*" >&2; }
need() { command -v "$1" >/dev/null || { say "FATAL: missing required tool '$1'"; exit 2; }; }
need curl; need jq; need psql; need python3

uuid() { cat /proc/sys/kernel/random/uuid; }

# --- verdict bookkeeping --------------------------------------------------------------------------
# THE COMPLETENESS MACHINERY (card s6, beck's M10). The run loop is driven BY the declaration
# below, and every declared leg must report a verdict or the run fails as INCOMPLETE. Declaring a
# leg IS running it: there is no second flat list to forget a line in, which is the defect that
# lets a script print ALL PASS while a leg it claims to cover never executed.
declare -A VERDICT=()   # leg -> PASS | RED | BLOCKED
declare -A DETAIL=()    # leg -> one-line explanation
declare -A PROVENANCE=()  # leg -> WALKED | INJECTED | SIMULATED

pass()    { VERDICT["$1"]=PASS;    DETAIL["$1"]="${2:-}"; say "PASS     $1 -- ${2:-}"; }
red()     { VERDICT["$1"]=RED;     DETAIL["$1"]="${2:-}"; say "RED      $1 -- ${2:-}"; }
blocked() { VERDICT["$1"]=BLOCKED; DETAIL["$1"]="${2:-}"; say "BLOCKED  $1 -- ${2:-}"; }
prov()    { PROVENANCE["$1"]="$2"; }

# --- HTTP / GraphQL -------------------------------------------------------------------------------
# Host-header addressing: the walk speaks to ONE listener and selects the tenant by Host, so it
# needs no DNS and no ingress. Against k3s/MKS the same URLs resolve for real.
resolve_args() {
  # Only used when WALK_RESOLVE is set (the local bare process). Empty otherwise, so the identical
  # script reaches a real cluster unchanged.
  [ -n "${WALK_RESOLVE:-}" ] && printf '%s' "--resolve"
}

gql_raw() { # <endpoint> <token> <query> [variables] [session]
  local endpoint="$1" token="$2" query="$3" variables="${4:-null}" session="${5:-}"
  local body auth=() sess=() out
  [ -n "$token" ] && auth=(-H "Authorization: Bearer $token")
  [ -n "$session" ] && sess=(-H "X-SESSION-ID: $session")
  body=$(jq -cn --arg q "$query" --argjson v "$variables" '{query:$q, variables:$v}')
  out=$(curl -sS -m 30 -w $'\n%{http_code}' -X POST "$endpoint" \
    -H "Content-Type: application/json" "${auth[@]}" "${sess[@]}" -d "$body" 2>/dev/null) \
    || { printf '{"errors":[{"message":"transport-error"}]}\n000'; return 0; }
  printf '%s' "$out"
}
gql()  { local o; o=$(gql_raw "$@"); printf '%s' "${o%$'\n'*}"; }
code() { local o; o=$(gql_raw "$@"); printf '%s' "${o##*$'\n'}"; }

# gql_check <query...> -> prints body, returns 1 if HTTP!=200 or GraphQL errors present.
gql_check() {
  local out c b
  out=$(gql_raw "$@"); c="${out##*$'\n'}"; b="${out%$'\n'*}"
  printf '%s' "$b"
  [ "$c" = "200" ] || return 1
  [ "$(printf '%s' "$b" | jq -r 'has("errors")' 2>/dev/null)" = "false" ] || return 1
  return 0
}

# --- domain_events assertions ---------------------------------------------------------------------
# ASSERT ON THE LOG, AND KEEP THE READ MODEL ALONGSIDE -- NOT INSTEAD (card, beck).
# This is a live defect, not a preference: the projector's OrderPlaced arm writes
# payment_status='AUTHORIZED' at BIRTH, with no payment fact anywhere. So `paymentStatus ==
# AUTHORIZED` proves the order was BORN, not that an authorization was recorded -- one safe
# indirection today, and fully vacuous the moment anything else births an order.
sql() { PGPASSWORD="${WALK_DB_PASSWORD:-walk}" psql "$WALK_DATABASE_URL" -tAc "$1" 2>&1; }

event_count() { # <event_type> [stream_name]
  local t="$1" s="${2:-}"
  if [ -n "$s" ]; then
    sql "SELECT count(*) FROM domain_events WHERE event_type='$t' AND stream_name='$s';"
  else
    sql "SELECT count(*) FROM domain_events WHERE event_type='$t';"
  fi
}

# --- waiting, with the three-way diagnostic --------------------------------------------------------
# A TIMEOUT IS NOT A LEG'S RED (card s6). A timeout is compatible with three worlds and the
# transcript must say which is which, or the next reader debugs the wrong one:
#   (a) the mutation was never accepted   (b) the stack is slow   (c) the previous leg never fired
# The checker's stderr is NEVER swallowed: a broken jq filter must not be byte-identical to
# "state not arrived". A checker prints `ok`, or `ERR:<diagnosis>`, or the last observed state.
wait_for() { # <leg> <what> <deadline> <checker-fn>
  local leg="$1" what="$2" deadline="$3" checker="$4" t=0 last="(never observed)" out
  while [ "$t" -le "$deadline" ]; do
    out=$("$checker" 2>&1)
    case "$out" in
      ok) return 0 ;;
      ERR:*) red "$leg" "$what: the CHECKER ITSELF failed (not a missing state) -- ${out#ERR:}"; return 1 ;;
      *) last="$out" ;;
    esac
    sleep 2; t=$((t+2))
  done
  red "$leg" "TIMEOUT after ${deadline}s waiting for $what.
             THREE-WAY DIAGNOSTIC -- a timeout alone does not say which of these it is:
               (a) the mutation was never accepted (check the acceptance envelope above),
               (b) the stack is slow / a worker is not draining (check the server log),
               (c) a PRECEDING leg never actually fired (check its PASS line in THIS run).
             Last observed state: $last"
  return 1
}

# --- token minting, with the decode-and-assert discipline ------------------------------------------
# mint_token <role> <sub> [claims-json-without-role]
#
# The claims argument exists because the walk needs restaurant_id and rider_id stamped, not only
# customer_id. The DECODE-AND-ASSERT is extended to all of them: today only customer_id was
# asserted, so a restaurant_id that fails to reach the token surfaces as a mysterious accept
# failure rather than a named one. That assert exists because it already happened once.
mint_token() {
  local role="$1" sub="$2" extra="${3:-{\}}" tok payload got want k
  local claims; claims=$(jq -cn --arg r "$role" --argjson e "$extra" '{role:$r} + $e')
  tok=$(python3 "$REPO_ROOT/tools/walk/jwks.py" mint \
          --dir "$WALK_STATE" --issuer "$WALK_SUPABASE_URL" --sub "$sub" --claims "$claims") \
    || { say "FATAL: mint failed for role $role"; return 1; }
  payload=$(python3 "$REPO_ROOT/tools/walk/jwks.py" decode --dir "$WALK_STATE" --token "$tok")
  for k in $(printf '%s' "$claims" | jq -r 'keys[]'); do
    want=$(printf '%s' "$claims"  | jq -r --arg k "$k" '.[$k]')
    got=$(printf  '%s' "$payload" | jq -r --arg k "$k" '.app_metadata.captain_food[$k] // empty')
    [ "$got" = "$want" ] || {
      say "FATAL: minted $role token carries captain_food.$k='$got', expected '$want' -- the stamp did not reach the issued token"
      return 1
    }
  done
  printf '%s' "$tok"
}

# ====================================================================================================
# THE LEGS
# ====================================================================================================

leg_target() {
  local leg=L01_target; prov "$leg" WALKED
  local ping health body req
  ping=$(curl -sS -m 15 "$PUBLIC_BASE/ping" 2>/dev/null)
  [ "$ping" = "pong" ] || { red "$leg" "/ping returned '$ping' (expected pong)"; return 1; }
  body=$(curl -sS -m 15 -w $'\n%{http_code}' "$PUBLIC_BASE/health" 2>/dev/null)
  health="${body##*$'\n'}"; body="${body%$'\n'*}"
  [ "$health" = "200" ] || { red "$leg" "/health HTTP $health -- $(printf '%s' "$body" | head -c 200)"; return 1; }
  req=$(printf '%s' "$body" | jq -r '.requiredSchemaVersion // 0')
  pass "$leg" "/ping=pong, /health=200, schema $req"
}

leg_browse() {
  local leg=L02_browse; prov "$leg" WALKED
  # PUBLIC, NO TOKEN. The first walked leg, and the one a real customer starts on.
  local r n
  r=$(gql_check "$PUBLIC_BASE/public/graphql" "" '{ restaurants { id slug displayName orderable } }') || {
    red "$leg" "public browse errored: $(printf '%s' "$r" | head -c 300)"; return 1; }
  n=$(printf '%s' "$r" | jq -r '.data.restaurants | length')
  pass "$leg" "public catalog browsable with no token ($n restaurant(s) listed)"
}

leg_fixture() {
  local leg=L03_fixture; prov "$leg" WALKED
  local r
  ADMIN_TOKEN=$(mint_token ADMIN "00000000-0000-4000-8000-00000000ad11") || { red "$leg" "admin mint failed"; return 1; }

  r=$(gql_check "$ADMIN_BASE/admin/graphql" "$ADMIN_TOKEN" \
      'mutation($i: RegisterRestaurantInput!){ registerRestaurant(input:$i){ correlationId } }' \
      "$(jq -cn --arg id "$FIX_RESTAURANT_ID" '{i:{mode:"TEST", restaurantId:$id,
          displayName:"Walk Test Restaurant",
          address:{line1:"1 rue du Test", postalCode:"37000", city:"Tours", country:"FR"},
          timezone:"Europe/Paris"}}')") || { red "$leg" "registerRestaurant: $(printf '%s' "$r" | jq -c '.errors' | head -c 300)"; return 1; }

  r=$(gql_check "$ADMIN_BASE/admin/graphql" "$ADMIN_TOKEN" \
      'mutation($i: ConfigureRestaurantSlugInput!){ configureRestaurantSlug(input:$i){ correlationId } }' \
      "$(jq -cn --arg id "$FIX_RESTAURANT_ID" --arg s "$WALK_TENANT_SLUG" '{i:{restaurantId:$id, slug:$s}}')") \
    || { red "$leg" "configureRestaurantSlug: $(printf '%s' "$r" | jq -c '.errors' | head -c 300)"; return 1; }

  check_restaurant() {
    local x; x=$(gql "$PUBLIC_BASE/public/graphql" "" \
      'query($s: Slug!){ restaurant(input:{slug:$s}) { id status } }' \
      "$(jq -cn --arg s "$WALK_TENANT_SLUG" '{s:$s}')")
    printf '%s' "$x" | jq -e . >/dev/null 2>&1 || { printf 'ERR:unparseable response: %s' "$(printf '%s' "$x" | head -c 160)"; return; }
    [ "$(printf '%s' "$x" | jq -r '.data.restaurant.id // empty')" = "$FIX_RESTAURANT_ID" ] \
      && echo ok || printf '%s' "$(printf '%s' "$x" | jq -c '.data // .errors')"
  }
  wait_for "$leg" "restaurant projection" 60 check_restaurant || return 1

  r=$(gql_check "$ADMIN_BASE/admin/graphql" "$ADMIN_TOKEN" \
      'mutation($i: ActivateRestaurantInput!){ activateRestaurant(input:$i){ correlationId } }' \
      "$(jq -cn --arg id "$FIX_RESTAURANT_ID" '{i:{restaurantId:$id, reason:"walk fixture"}}')") \
    || { red "$leg" "activateRestaurant: $(printf '%s' "$r" | jq -c '.errors' | head -c 300)"; return 1; }

  r=$(gql_check "$ADMIN_BASE/admin/graphql" "$ADMIN_TOKEN" \
      'mutation($i: CreateCatalogInput!){ createCatalog(input:$i){ correlationId } }' \
      "$(jq -cn --arg c "$FIX_CATALOG_ID" --arg r "$FIX_RESTAURANT_ID" '{i:{catalogId:$c, restaurantId:$r, name:"Walk Catalog"}}')") \
    || { red "$leg" "createCatalog: $(printf '%s' "$r" | jq -c '.errors' | head -c 300)"; return 1; }

  r=$(gql_check "$ADMIN_BASE/admin/graphql" "$ADMIN_TOKEN" \
      'mutation($i: AddProductInput!){ addProduct(input:$i){ correlationId } }' \
      "$(jq -cn --arg p "$FIX_PRODUCT_ID" --arg c "$FIX_CATALOG_ID" --arg r "$FIX_RESTAURANT_ID" --arg o "$FIX_OFFER_ID" \
        '{i:{productId:$p, catalogId:$c, restaurantId:$r, name:"Walk Pizza",
             taxRate:{delivery:10.0, collection:10.0},
             offers:[{id:$o, productId:$p, name:"Default",
                      price:{amountCents:1200, currency:"EUR"}, availability:"AVAILABLE"}]}}')") \
    || { red "$leg" "addProduct: $(printf '%s' "$r" | jq -c '.errors' | head -c 300)"; return 1; }

  check_offer() {
    local x; x=$(gql "$PUBLIC_BASE/public/graphql" "" \
      'query($r: RestaurantId!){ catalog(input:{restaurantId:$r}) { id products { id offers { id availability } } } }' \
      "$(jq -cn --arg r "$FIX_RESTAURANT_ID" '{r:$r}')")
    printf '%s' "$x" | jq -e . >/dev/null 2>&1 || { printf 'ERR:unparseable response: %s' "$(printf '%s' "$x" | head -c 160)"; return; }
    [ "$(printf '%s' "$x" | jq -r --arg o "$FIX_OFFER_ID" '[.data.catalog.products[]?.offers[]? | select(.id==$o and .availability=="AVAILABLE")] | length')" != "0" ] \
      && echo ok || printf '%s' "$(printf '%s' "$x" | jq -c '.data // .errors')"
  }
  wait_for "$leg" "AVAILABLE offer in the catalog read model" 90 check_offer || return 1
  pass "$leg" "restaurant ACTIVE with an AVAILABLE offer, created through the real ADMIN mutations"
}

leg_cart() {
  local leg=L04_cart; prov "$leg" WALKED
  # THE 2026-08-04 LEG. The last honest reading this system gave of its own money path died here
  # at {"cart":null} -- the cart never projected. Topology-independent, never diagnosed.
  CART_ID=$(uuid); LINE_ID=$(uuid); SESSION_ID=$(uuid)
  local r
  # THE TENANT HOST IS LOAD-BEARING FOR EVERY CART READ (#469). `current` resolves its tenant from
  # the Host header and returns None when the host is not a restaurant slug -- so the SAME query
  # that works on walk-test.captain.food returns {"current":null} on the marketplace host
  # live.captain.food, by design and with no error. Read the finding in the PR body before
  # "simplifying" these to $PUBLIC_BASE.
  r=$(gql_check "$TENANT_BASE/public/graphql" "" \
      'mutation($i: AddCartLineInput!){ addCartLine(input:$i){ messageId operationStatus } }' \
      "$(jq -cn --arg c "$CART_ID" --arg r "$FIX_RESTAURANT_ID" --arg l "$LINE_ID" --arg o "$FIX_OFFER_ID" --arg s "$SESSION_ID" \
        '{i:{cartId:$c, restaurantId:$r, sessionId:$s, line:{cartLineId:$l, offerId:$o, quantity:1}}}')") \
    || { red "$leg" "addCartLine rejected: $(printf '%s' "$r" | jq -c '.errors' | head -c 300)"; return 1; }

  # The WHOLE priced shape in one predicate, not "a row exists": a cart that projects but prices to
  # nothing passes a status-only check while the customer sees no payable amount.
  check_cart() {
    local x; x=$(gql "$TENANT_BASE/public/graphql" "" \
      'query{ current { id status totalAmount { amountCents currency } } }' '{}' "$SESSION_ID")
    printf '%s' "$x" | jq -e . >/dev/null 2>&1 || { printf 'ERR:unparseable response: %s' "$(printf '%s' "$x" | head -c 160)"; return; }
    [ "$(printf '%s' "$x" | jq -r '(.data.current.status=="OPEN") and (.data.current.totalAmount.amountCents>0) and (.data.current.totalAmount.currency=="EUR")' 2>/dev/null)" = "true" ] \
      && echo ok || printf '%s' "$(printf '%s' "$x" | jq -c '.data // .errors')"
  }
  wait_for "$leg" "cart projected AND priced live through the guest session path" 60 check_cart || return 1
  pass "$leg" "guest cart $CART_ID projected and priced > 0 via session $SESSION_ID"
}

leg_register() {
  local leg=L05_register; prov "$leg" WALKED
  # DECLARED RED AND NAMED, not routed around. Two lenses independently found this may be
  # unwalkable regardless of the JWKS stub: phone-OTP has no admin API that hands you the code, and
  # the honest mechanism is a Supabase test phone number, which is project configuration and may be
  # admin-gated. Supabase is ALSO unreachable from this environment (proxy 502 on CONNECT).
  #
  # NO GoTrue FAKE. A fake that minted convincing customers would make the walk's first leg LOOK
  # real while proving nothing -- precisely the failure this whole chunk exists to avoid.
  local r national; national="60000$(shuf -i 1000-9999 -n1)"
  r=$(gql "$TENANT_BASE/public/graphql" "" \
      'mutation($i: RequestPhoneVerificationInput!){ requestPhoneVerification(input:$i){ messageId operationStatus } }' \
      "$(jq -cn --arg n "$national" '{i:{dialingCode:"+33", nationalNumber:$n, locale:"fr"}}')")
  if [ "$(printf '%s' "$r" | jq -r 'has("errors")')" = "true" ]; then
    red "$leg" "requestPhoneVerification errored (EXPECTED -- Supabase identity unreachable from here): $(printf '%s' "$r" | jq -c '.errors[0].message' | head -c 240)"
  else
    red "$leg" "requestPhoneVerification was accepted, but the OTP CODE cannot be obtained: phone-OTP exposes no admin API that returns it, and Supabase is unreachable (proxy 502 on CONNECT). verifyPhone therefore cannot be called, so CustomerRegistered and CartBoundToCustomer cannot be asserted. Response: $(printf '%s' "$r" | jq -c '.data' | head -c 200)"
  fi
  # ux-designer's finding, recorded so the red does not hide it: CartBindingProcess reacts to
  # CustomerIdentified ONLY, while a first-time phone emits CustomerRegistered -- so the FIRST
  # order a customer ever places may be the only one whose cart is never bound to them. Unwalkable
  # here, so it stays an open question, NOT an assertion anyone can mistake for evidence.
  say "         NOTE: CartBoundToCustomer remains UNVERIFIED (see #TBD -- CartBindingProcess reacts to CustomerIdentified only)."
  return 1
}

leg_place() {
  local leg=L06_place; prov "$leg" WALKED
  ORDER_ID=$(uuid); CUSTOMER_ID=$(uuid)
  CUSTOMER_TOKEN=$(mint_token CUSTOMER "00000000-0000-4000-8000-00000000c001" \
      "$(jq -cn --arg c "$CUSTOMER_ID" '{customer_id:$c}')") || { red "$leg" "customer mint failed"; return 1; }
  local r mid
  # DELIVERY, never COLLECTION (three lenses, independently): capture-at-READY for collection is
  # decided but unimplemented, so a COLLECTION walk cannot reach capture honestly and would pin
  # the decided-against behaviour.
  r=$(gql "$TENANT_BASE/customer/graphql" "$CUSTOMER_TOKEN" \
      'mutation($i: PlaceOrderInput!){ placeOrder(input:$i){ messageId operationStatus duplicate } }' \
      "$(jq -cn --arg o "$ORDER_ID" --arg r "$FIX_RESTAURANT_ID" --arg c "$CART_ID" --arg u "$CUSTOMER_ID" \
        '{i:{mode:"TEST", orderId:$o, restaurantId:$r, cartId:$c, customerId:$u,
             customerContact:{displayName:"Walk Customer", phone:"+33600000000"},
             deliveryAddress:{line1:"2 rue du Client", postalCode:"37000", city:"Tours", country:"FR"},
             serviceType:"DELIVERY", paymentMethodId:"pm_card_visa"}}')" "$SESSION_ID")
  mid=$(printf '%s' "$r" | jq -r '.data.placeOrder.messageId // empty')
  [ -n "$mid" ] || { red "$leg" "placeOrder returned no messageId: $(printf '%s' "$r" | jq -c '.errors // .' | head -c 400)"; return 1; }

  local t=0 st last="(no operation row)"
  while [ "$t" -le 60 ]; do
    local x; x=$(gql "$TENANT_BASE/customer/graphql" "$CUSTOMER_TOKEN" \
      'query($m: MessageId!){ operationStatus(input:{messageId:$m}) { status errorCode message } }' \
      "$(jq -cn --arg m "$mid" '{m:$m}')")
    st=$(printf '%s' "$x" | jq -r '.data.operationStatus.status // empty')
    case "$st" in
      SUCCEEDED) break ;;
      REJECTED|FAILED)
        # Say what is known and what is NOT. `Internal` with an empty context (issue #623) cannot
        # be attributed from the record, so the transcript must not pretend otherwise: name the
        # most probable proximate cause AND the fact that it is unconfirmed.
        red "$leg" "placeOrder $st: $(printf '%s' "$x" | jq -c '.data.operationStatus' | head -c 240)
             ATTRIBUTION: unattributable from the record -- inbound_messages.error is
             {\"code\":\"Internal\",\"context\":{}} and nothing is logged at ERROR/WARN (issue #623).
             MOST PROBABLE proximate cause when running without a usable sk_test_ key:
             application::commands::place_order calls payments.request (Stripe intent-create)
             BEFORE any append, so a gateway 401 fails the command with no Order stream created.
             UNCONFIRMED -- do not record this as 'Stripe was the cause', only as consistent with it."
        return 1 ;;
    esac
    last="status=${st:-<no operation row>}"; sleep 2; t=$((t+2))
  done
  [ "$st" = "SUCCEEDED" ] || {
    red "$leg" "TIMEOUT: placeOrder operation not terminal after 60s.
             THREE-WAY DIAGNOSTIC: (a) never accepted, (b) stack slow / worker not draining,
             (c) a preceding leg never fired. Last: $last"; return 1; }

  # ON THE LOG, and the read model alongside -- not instead.
  local n; n=$(event_count OrderPlaced "Order-$ORDER_ID")
  [ "$n" = "1" ] || { red "$leg" "operationStatus SUCCEEDED but domain_events holds $n OrderPlaced rows for Order-$ORDER_ID"; return 1; }
  pass "$leg" "placeOrder SUCCEEDED and OrderPlaced is on the log for Order-$ORDER_ID"
}

# --- The Stripe-dependent back half ----------------------------------------------------------------
# These legs have NEVER RUN. They are declared here so the run loop must account for them: a leg
# that is not reached is BLOCKED and says WHY, which is a cascade rather than evidence (card s6:
# "the previous leg is not written yet is a cascade, not evidence"). A BLOCKED leg is never a PASS
# and never counts toward a green.
stripe_key_usable() {
  case "${STRIPE_SECRET_KEY:-}" in
    sk_test_WALKPLACEHOLDER*|"") return 1 ;;
    sk_test_*) return 0 ;;
    *) return 1 ;;
  esac
}

leg_authorize() {
  local leg=L07_authorize; prov "$leg" WALKED
  if ! stripe_key_usable; then
    blocked "$leg" "no usable sk_test_ key in this environment. The repo secret STRIPE_SECRET_KEY_TEST exists but is readable only inside CI; api.stripe.com IS reachable from here (a dummy key returns a Stripe-shaped 401), so this is an AUTHENTICATION gap, not a connectivity one. Without it no PaymentIntent can be confirmed, so PaymentAuthorized never occurs."
    return 1
  fi
  blocked "$leg" "not implemented in this change -- see the PR body for the CI-first plan"
  return 1
}

blocked_on_authorization() { # <leg> <what it would have proved>
  blocked "$1" "not reached: requires PaymentAuthorized (L07), which is blocked. CASCADE, NOT EVIDENCE -- this leg has never run, and nothing here should be read as either working or broken. It would have proved: $2"
  return 1
}

leg_visibility()  { prov L08_visibility  WALKED; blocked_on_authorization L08_visibility  "the owning restaurant reads its OWN orders and the new order is there -- the kitchen reloading, which is the shipped product's real notification mechanism"; }
leg_accept()      { prov L09_accept      SIMULATED; blocked_on_authorization L09_accept      "accept performed by the OWNING restaurant's identity (never ADMIN, never the path role alone)"; }
leg_ready()       { prov L10_ready       WALKED; blocked_on_authorization L10_ready       "OrderReady on the log"; }
leg_dispatch()    { prov L11_dispatch    WALKED; blocked_on_authorization L11_dispatch    "a DeliveryJob offered for the order"; }
leg_acceptdel()   { prov L12_acceptdel   WALKED; blocked_on_authorization L12_acceptdel   "the rider accepts the delivery"; }
leg_pickup()      { prov L13_pickup      WALKED; blocked_on_authorization L13_pickup      "confirmPickup -- the hop the first version of the card omitted, without which completeDelivery throws InvalidDeliveryStatus"; }
leg_delivered()   { prov L14_delivered   WALKED; blocked_on_authorization L14_delivered   "completeDelivery producing BOTH DeliveryCompleted and, via the PM bridge, OrderDelivered -- the hop that can silently never fire"; }
leg_captured()    { prov L15_captured    WALKED; blocked_on_authorization L15_captured    "PaymentCaptured -- the walk's finish line"; }

leg_replay() {
  local leg=L16_replay; prov "$leg" WALKED
  # Runs AFTER the money legs by design, and refuses to run against anything but a local,
  # single-row target: `DELETE FROM ...` against a populated database is a read-model wipe.
  if [ -z "$ORDER_ID" ] || [ "$(event_count OrderPlaced "Order-$ORDER_ID")" != "1" ]; then
    blocked "$leg" "not reached: no order was born, so there is no projection to replay. CASCADE, NOT EVIDENCE."
    return 1
  fi
  blocked "$leg" "not implemented in this change"
  return 1
}

# ====================================================================================================
# THE DECLARATION -- this array IS the run loop. Adding a leg here runs it; there is no second list.
# ====================================================================================================
LEGS=(
  L01_target:leg_target
  L02_browse:leg_browse
  L03_fixture:leg_fixture
  L04_cart:leg_cart
  L05_register:leg_register
  L06_place:leg_place
  L07_authorize:leg_authorize
  L08_visibility:leg_visibility
  L09_accept:leg_accept
  L10_ready:leg_ready
  L11_dispatch:leg_dispatch
  L12_acceptdel:leg_acceptdel
  L13_pickup:leg_pickup
  L14_delivered:leg_delivered
  L15_captured:leg_captured
  L16_replay:leg_replay
)

# --- resolved configuration, printed so the reading is self-describing -----------------------------
print_config() {
  local h; h=$(curl -sS -m 15 "$PUBLIC_BASE/health" 2>/dev/null)
  say "=================================================================================="
  say "THE WALK -- one DELIVERY order, browse to captured, on one database"
  say "=================================================================================="
  say "target        : $PUBLIC_BASE (tenant $TENANT_BASE)"
  say "database      : ${WALK_DATABASE_URL%%\?*}"
  say "schema        : $(printf '%s' "$h" | jq -r '.schemaVersion // "?"') (binary requires $(printf '%s' "$h" | jq -r '.requiredSchemaVersion // "?"'))"
  say "issuer        : ${WALK_SUPABASE_URL}/auth/v1   (real; only JWKS is stubbed)"
  say "stripe key    : $(stripe_key_usable && echo 'sk_test_ (usable)' || echo 'ABSENT/placeholder -- the money legs cannot run')"
  # The three flags all default OFF and the chain still completes; the walk therefore certifies the
  # PRE-FLIP configuration, and the evidence does not transfer the day a default flips.
  say "flags         : ROUTE_ORDER_BIRTH_THROUGH_LANE / ENFORCE_ACCEPTANCE_TIMEOUT / ENFORCE_SERVICE_HOURS_GUARD all at their defaults (OFF)"
  say "=================================================================================="
}

not_proven() {
  say ""
  say "WHAT THIS WALK DOES NOT PROVE (verbatim, every run -- the label is READING, not certificate)"
  say "  No deploy path, image, TLS, DNS or ingress. Stripe TEST with one card: no 3DS/SCA -- in"
  say "  France the common path, not an edge case -- no decline, no partial capture, no dispute."
  say "  NOBODY WAS TOLD: the accept leg is script-issued. One instance, no WAL archiving, no base"
  say "  backup, restore drill never executed against this stack. One order, one user: nothing"
  say "  about Friday peak. One database, so the boundary rows pass because the wall is not"
  say "  physical. Flags OFF, so this certifies the pre-flip configuration and the evidence does"
  say "  not transfer the day a default flips."
}

main() {
  print_config
  local entry leg fn
  for entry in "${LEGS[@]}"; do
    leg="${entry%%:*}"; fn="${entry##*:}"
    if ! declare -F "$fn" >/dev/null; then
      say "FATAL: leg $leg declares function '$fn', which does not exist."
      exit 2
    fi
    "$fn" || true
  done

  # THE COMPLETENESS ASSERTION (beck's M10). A declared leg that never reported a verdict is the
  # single most likely defect in this harness, and it is exactly the shape that prints ALL PASS
  # while covering nothing. It is a HARD FAILURE, never a warning.
  local missing=()
  for entry in "${LEGS[@]}"; do
    leg="${entry%%:*}"
    [ -n "${VERDICT[$leg]:-}" ] || missing+=("$leg")
  done

  say ""
  say "=================================================================================="
  say "TRANSCRIPT"
  say "=================================================================================="
  printf '%-16s %-8s %-10s %s\n' "LEG" "VERDICT" "PROVENANCE" "DETAIL" >&2
  local p r b=0 np=0 nr=0 nb=0
  for entry in "${LEGS[@]}"; do
    leg="${entry%%:*}"
    printf '%-16s %-8s %-10s %s\n' "$leg" "${VERDICT[$leg]:-<NO VERDICT>}" "${PROVENANCE[$leg]:-?}" \
      "$(printf '%s' "${DETAIL[$leg]:-}" | head -1 | cut -c1-96)" >&2
    case "${VERDICT[$leg]:-}" in
      PASS) np=$((np+1)) ;; RED) nr=$((nr+1)) ;; BLOCKED) nb=$((nb+1)) ;;
    esac
  done
  say ""
  say "PASS=$np  RED=$nr  BLOCKED=$nb  DECLARED=${#LEGS[@]}"
  not_proven

  if [ "${#missing[@]}" -gt 0 ]; then
    say ""
    say "INCOMPLETE: ${#missing[@]} declared leg(s) reported NO verdict: ${missing[*]}"
    say "The run is void -- a declared leg that never runs while the script prints a summary is the"
    say "defect this assertion exists to catch. Fix the leg, not this check."
    exit 3
  fi
  [ "$nr" = "0" ] && [ "$nb" = "0" ] && { say ""; say "WALK GREEN: all ${#LEGS[@]} declared legs PASSED."; exit 0; }
  say ""
  say "WALK NOT GREEN: $nr red, $nb blocked. Every one is named above."
  exit 1
}

main "$@"
