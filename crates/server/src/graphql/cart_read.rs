//! The cart READ seam (#451 Phase 2, ADR-20260810-120531): every GraphQL cart read — `current`,
//! `cart`, `carts` — resolves its row(s) here and prices them through the ONE `price_cart`
//! authority. Hand-written (not generated) so the lookup semantics, the ownership narrowing and
//! the pricing/observability boundary are unit-testable behind a seam the generated resolver
//! literals only CALL — the emitter carries no pricing logic of its own.
//!
//! Three responsibilities, nothing else:
//! - [`current_open_cart`] — the TWO-LEG `current` lookup (claim, then session);
//! - [`readable_by`] — the by-id claim-ownership narrowing (#144/#434);
//! - [`priced`] — money-free row → priced GraphQL `Cart` under the `cart-price` observability
//!   contract (span + histogram + defect counter live HERE, the framework boundary — both pricers
//!   stay SDK-free): CLOSED (no witness, default everywhere) prices via `price_cart` over a
//!   one-read [`CatalogSnapshot`], unchanged since #451; OPEN ([`FoldPricedReadOpen`] present)
//!   prices via `price_cart_at` over the SAME snapshot plus ONE `AsOfPriceAuthority::at_head`
//!   range read, carrying no coordinate past this function (PROP-20260831-134539 slice 3a, D2-D4).
//!
//! **The `carts` fan-out narrowing** (ADR-20260906-192007 D-L, compiler-first per
//! ADR-20260803-234035 level 4): `priced`'s old `door_open: bool` parameter let EVERY cart read —
//! including the multi-restaurant `carts` LIST — open the fold-priced door, which would mean N
//! unbounded head folds inside one request (dba's saturation concern; 3b already doubles the fold
//! count per single-cart checkout, D-M). [`priced_list`] is the `carts` fan-out's only entry point
//! and its signature carries no witness parameter at all — there is no value the fan-out's
//! generated call site could pass to open the door, so the fan-out staying CLOSED is a fact the
//! type system states, not a runtime flag the fan-out happens to leave off. [`priced`] keeps the
//! witness parameter for the two SINGLE-cart-read entry points (`cart`, `current`) only.

use application::ports::AsOfPriceAuthority;
use application::pricing::{price_cart, price_cart_at, CatalogSnapshot};
use application::queries::{CartReadRepository, CartRow, CatalogReadRepository, ReadScope};
use domain::generated::entities::CartLineItem;
use domain::generated::scalars::{CartStatus, SessionId};
use domain::shared::errors::DomainError;
use tracing::Instrument;

use super::generated::scalars::{CurrencyCode, MoneyCents};
use super::generated::types::{Cart, Money, Restaurant};
use super::tenant::TenantScope;

/// The TWO-LEG `current` resolution (ADR-20260810-120531), **within ONE tenant** (#469). Carts are
/// built ANONYMOUSLY under a session id BEFORE any customer identity exists; CartBindingProcess
/// associates them on identification. So:
///
/// - **Leg 1 — claim**: a verified CUSTOMER claim resolves the claim-holder's most-recently-updated
///   OPEN cart AT THIS RESTAURANT (`open_by_customer_at` is `updated_at DESC`).
/// - **Leg 2 — session**: otherwise — anonymous, or the association not yet folded — a valid
///   `X-SESSION-ID` resolves the session's most-recently-updated OPEN cart at this restaurant WHERE
///   `customer_id IS NULL OR customer_id = <claim if present>`. The session id is an
///   UNAUTHENTICATED correlator (scoping only, never identity): the NULL-or-claim filter keeps a
///   cart already bound to someone ELSE invisible to whoever replays the session id.
///
/// **The tenant is not optional, and it comes from the `Host`** ([`TenantScope`], resolved once at
/// the transport boundary — never a client argument, which would let the caller assert which
/// restaurant's cart to serve). `current` is a STOREFRONT read: on `{slug}.captain.food` it answers
/// with that restaurant's cart or with nothing. Unbounded, leg 1 returned the claim-holder's newest
/// open cart ANYWHERE — so a customer with a cart at `b.captain.food` would have seen it, been
/// priced for it and paid for it on `a.captain.food`. No tenant in the host (the marketplace, an
/// unknown slug, a lookup that failed) ⇒ `None`, decided explicitly: null, never "newest anywhere",
/// which is the same bug wearing a fallback's clothes.
///
/// OPEN only (`CartStatus` is OPEN | CHECKED_OUT — the LOCKED lifecycle is #465). `None` = no
/// open cart: the client renders the empty state, never a fabricated 0,00 EUR payable.
pub async fn current_open_cart(
    carts: &dyn CartReadRepository,
    scope: &ReadScope,
    session: Option<uuid::Uuid>,
    tenant: TenantScope,
) -> Result<Option<CartRow>, DomainError> {
    // No tenant, no cart — decided BEFORE either leg, so neither is reachable unbounded.
    let TenantScope::Restaurant(restaurant_id) = tenant else {
        return Ok(None);
    };
    let claim = match scope {
        ReadScope::Customer(id) => Some(*id),
        _ => None,
    };
    // Leg 1 — the claim-holder's newest OPEN cart at THIS restaurant.
    if let Some(customer_id) = claim {
        let open = carts
            .open_by_customer_at(customer_id, restaurant_id)
            .await?
            .into_iter()
            .find(|r| r.status == CartStatus::OPEN);
        if let Some(row) = open {
            return Ok(Some(row));
        }
    }
    // Leg 2 — the session's newest OPEN cart at THIS restaurant, NULL-or-claim owned.
    let Some(session) = session else { return Ok(None) };
    Ok(carts
        .open_by_session_at(SessionId(session), restaurant_id)
        .await?
        .into_iter()
        .find(|r| r.customer_id.is_none() || r.customer_id == claim))
}

/// The by-id claim-ownership narrowing (#144/#434, the #451 DONE-WHEN): a CUSTOMER reads only a
/// cart bound to their own claim-resolved id — an unbound (session-only) cart or someone else's
/// resolves null through this predicate (no existence oracle); ADMIN (and SYSTEM) read any cart.
/// The guest path is NOT here: anonymous carts are read through [`current_open_cart`]'s session
/// leg. Fail-closed: an absent/unresolved scope reads nothing.
pub fn readable_by(row: &CartRow, scope: &ReadScope) -> bool {
    match scope {
        ReadScope::Admin | ReadScope::System => true,
        ReadScope::Customer(id) => row.customer_id.as_ref() == Some(id),
        _ => false,
    }
}

/// The fold-priced door's OPEN state — a WITNESS, not a `bool` (ADR-20260906-192007 D-L). The only
/// constructor is [`FoldPricedReadOpen::from_flag`]; a caller with `flag = false` gets `None`, and
/// there is no other way to produce `Some`. [`priced_list`] (the `carts` fan-out) never calls this
/// constructor and never receives a value of this type — its own signature has nowhere to put one
/// — so the multi-restaurant list cannot mint N unbounded head folds in one request regardless of
/// the door's runtime position, closed by the type system rather than by a convention the fan-out
/// could accidentally violate later.
#[derive(Debug, Clone, Copy)]
pub struct FoldPricedReadOpen(());

impl FoldPricedReadOpen {
    /// `Some` when the door's own config flag is open; `None` otherwise. The two single-cart-read
    /// entry points (`cart`, `current`) are the only emitted call sites for this constructor —
    /// `carts` reads no door flag at all (D-L: the `ctx.data::<RunFoldPricedCartRead>()` call the
    /// fan-out used to make is dead code once it calls [`priced_list`] instead, and is removed).
    pub fn from_flag(flag: bool) -> Option<Self> {
        flag.then_some(Self(()))
    }
}

/// Money-free row → priced GraphQL `Cart`, the ONE pricing path for every cart read
/// (rules.yaml#/CartPricedFromLiveCatalog): parse the row's repricing inputs, load the live
/// catalog ONCE ([`CatalogSnapshot`] — N lines, 1 read), price via `price_cart`, and map into
/// the API shape. Under the `cart-price` contract, emitted HERE at the framework boundary:
/// - span `cart.price` (business.aggregate_id = cartId; business.correlation_id = the REQUEST's
///   id, passed in — reads carry no command envelope, so the server mints one per request);
/// - histogram `cart_price_ms`;
/// - on an unresolvable price: counter `cart_price_unresolvable_total{reason}` AND OTel ERROR
///   status on the span — technical_error classification, never business_rejected
///   (ADR-20260810-112836): the customer asked to see their cart and must see no price rather
///   than a partial/wrong total.
///
/// **Every success emits the span and the histogram, including the empty cart.** An EMPTY open
/// cart (all lines removed) prices to the true sum of zero lines — 0 EUR, no breakdown — without
/// touching the catalog; that is arithmetic, not a fabricated payable (the platform is EUR-only in
/// V0, the same posture as the checkout's degenerate breakdown legs). It is nonetheless a priced
/// read: the contract's `status_rules.success.required_spans: ["cart.price"]` admits no
/// exceptions, and empty open carts are COMMON (every first storefront visit after a clear), so
/// short-circuiting before the span would have made the most frequent success in the flow a
/// contract-missing one — and dragged the p95 the histogram exists to watch downward with reads
/// that did no work.
pub async fn priced(
    catalogs: &dyn CatalogReadRepository,
    // PROP-20260831-134539 slice 3a (ADR-20260906-154419, D2/D4): the fold-priced read authority
    // and the door that gates it. CLOSED (`door` is `None`, default everywhere) = today's read,
    // below, unchanged — stated explicitly as its own arm, never as a fallback. OPEN (`door` is
    // `Some`) = the SAME snapshot resolves `catalog_id`, then ONE `at_head` call returns prices
    // carrying their own coordinate ([`AsOfCatalog::coordinate`], ADR-20260906-192007 D-L's tuple
    // collapse); the coordinate is used to price via `price_cart_at` and DROPPED here — it reaches
    // no reply field, no input, no command (D3).
    authority: &dyn AsOfPriceAuthority,
    door: Option<FoldPricedReadOpen>,
    row: CartRow,
    // The already clock-evaluated navigation target (RSO-1): built ONCE by the resolver via
    // `Restaurant::at` with the request clock — a `RestaurantRow` here would force this seam to
    // invent its own instant, and the cart's embedded serviceWindow could disagree with the
    // page's other cards.
    restaurant: Restaurant,
    correlation_id: uuid::Uuid,
) -> async_graphql::Result<Cart> {
    let span = telemetry::spans::cart_price(&row.cart_id.0.to_string());
    telemetry::spans::record_correlation_id(&span, &correlation_id.to_string());
    let started = std::time::Instant::now();

    // EVERY fallible step of the read lives inside the span, and the timing closes only after the
    // last of them. The span answers exactly one question — "did this customer get a price?" — so
    // any exit that returns a GraphQL error has to be inside it. Three used to sit outside (the
    // `lines` parse before it, and the two API-shape mappings after it), each returning an error to
    // the customer while the trace exported a clean SUCCESS. The serde mapping is inside the
    // customer's wait too, so leaving it outside the timer also understated `cart_price_ms`.
    let outcome = async {
        // The repricing inputs, as the money-free fold stores them. Malformed `lines` is a real
        // technical failure on a money path, not a parse detail.
        let lines: Vec<CartLineItem> = serde_json::from_value(row.lines.clone())
            .map_err(|e| async_graphql::Error::new(format!("cart lines are malformed: {e}")))?;

        // An EMPTY open cart: the true sum of zero lines, no catalog read needed. Still a priced
        // read, and still a success the contract requires a span for.
        if lines.is_empty() {
            return Ok(unpriced_empty(&row, restaurant));
        }

        let priced = async {
            // The ONE catalog read every arm performs (both today and behind the door): the
            // projected row, for `catalog_id` and (in the OPEN arm) display metadata — never
            // re-loaded, never a second `by_restaurant` call.
            let snapshot = CatalogSnapshot::load(catalogs, row.restaurant_id).await?;
            if door.is_some() {
                // The CLOSED arm's own "no catalog" case falls out of `price_cart`'s per-line
                // `offer_by_id` lookup naturally; mirrored here so the OPEN arm fails the SAME way
                // for the SAME input rather than inventing a second unresolvable shape.
                let Some(catalog_id) = snapshot.catalog_id() else {
                    return Err(DomainError::rejected(
                        "PriceUnresolvable",
                        serde_json::json!({ "cartId": row.cart_id, "offerId": lines[0].offer_id }),
                    ));
                };
                // THE mint (D2): one unbounded range read to head, returning prices carrying their
                // own coordinate (the tuple collapse, ADR-20260906-192007 D-L). `Err` here
                // (coordinate absent, stream unreadable) falls straight through to the SAME
                // `map_err` below as any other pricing failure — technical_error, never a
                // HEAD/projection fallback (D4, every lens's STOP).
                let as_of = authority.at_head(catalog_id, correlation_id).await?;
                price_cart_at(&snapshot, &as_of, row.cart_id, row.restaurant_id, &lines).await
            } else {
                price_cart(&snapshot, row.cart_id, row.restaurant_id, &lines).await
            }
        }
        .await
        .map_err(|e| {
            if e.code() == Some("PriceUnresolvable") {
                // Canonical reason set (specs/observability.yaml cart-price): offer_gone covers a
                // line's offer OR selected option no longer resolving (and the currency-clash
                // defect); policy_missing / stock_unknown are reserved for legs not yet on this
                // seam. Only THIS branch is reason-coded — a catalog-load failure is not an
                // unresolvable price, though it does fail the read (and marks the span ERROR
                // below, like every other failure).
                telemetry::meters::cart_price::unresolvable("offer_gone");
            }
            async_graphql::Error::new(e.to_string())
        })?;

        // Domain → API shapes share the serde spelling (camelCase) — the same round-trip the Order
        // read model uses for its jsonb items.
        let lines = serde_json::to_value(&priced.items)
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .ok_or_else(|| async_graphql::Error::new("priced lines failed to map to the API shape"))?;
        let total_amount = Money {
            amount_cents: MoneyCents(priced.total_amount.amount_cents.0),
            currency: CurrencyCode(priced.total_amount.currency.0.clone()),
        };
        let breakdown = serde_json::to_value(&priced.breakdown)
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .ok_or_else(|| async_graphql::Error::new("breakdown failed to map to the API shape"))?;
        Ok(Cart {
            id: row.cart_id.into(),
            restaurant_id: row.restaurant_id.into(),
            customer_id: row.customer_id.map(Into::into),
            status: row.status.into(),
            lines,
            total_amount,
            breakdown: Some(breakdown),
            // The Uber-comparison estimate is a policy read this seam does not perform yet (#463
            // owns the impure survivors; the degenerate shape ships — same as checkout).
            uber_comparison: None,
            // ADR-20260906-192007 D-A/D-J: the signed quote. `None` here for now -- the
            // `QuoteMinter` this field is meant to carry does not exist until a later phase of
            // this same PR (D-H); every read answers null in the meantime, one of `CartQuote`'s
            // three documented null causes (scalars.yaml#/CartQuote).
            quote: None,
            updated_at: row.updated_at,
            restaurant,
        })
    }
    .instrument(span.clone())
    .await;

    telemetry::meters::cart_price::duration(started.elapsed().as_secs_f64() * 1000.0);

    // ONE choke point decides the span's status for the whole read. Every failure above — malformed
    // lines, catalog load, unresolvable price, either API-shape mapping — marks the span ERROR, so
    // the contract's `technical_error: any_span_errors` rule classifies all of them and none can
    // export as a success. A new failure mode added inside the block inherits this by construction
    // rather than by remembering.
    outcome.map_err(|e| {
        telemetry::spans::record_cart_price_error(&span);
        e
    })
}

/// The `carts` FAN-OUT entry point (ADR-20260906-192007 D-L) — the customer's carts LIST, one row
/// per restaurant. Always CLOSED, structurally: this signature carries no [`FoldPricedReadOpen`]
/// parameter at all, so there is no value the generated `carts` resolver could pass to reach the
/// fold-priced arm, whatever the door's runtime config says. `authority` is threaded through only
/// because it delegates to [`priced`], which still declares one — it is never actually called on
/// this path, since `door` is always `None` below.
///
/// Why the fan-out is narrowed rather than left to the same door as the two single-cart reads
/// (dba, D-L, D-M): the door's own precondition register models ONE fold per read; `carts` returns
/// one row PER RESTAURANT, so opening the door here would multiply that by however many
/// restaurants the caller has an open cart at, inside a single request — the exact unbounded-fold
/// shape the dedicated fold pool (D-K) exists to bound, but for N reads at once rather than one.
pub async fn priced_list(
    catalogs: &dyn CatalogReadRepository,
    authority: &dyn AsOfPriceAuthority,
    row: CartRow,
    restaurant: Restaurant,
    correlation_id: uuid::Uuid,
) -> async_graphql::Result<Cart> {
    priced(catalogs, authority, None, row, restaurant, correlation_id).await
}

/// The zero-lines cart: the honest sum of nothing, not a fabricated payable. Shares one
/// construction with nothing else on purpose — the PRICED shape is built from `price_cart`'s
/// output and must never be reachable from a path that did not price.
fn unpriced_empty(row: &CartRow, restaurant: Restaurant) -> Cart {
    Cart {
        id: row.cart_id.into(),
        restaurant_id: row.restaurant_id.into(),
        customer_id: row.customer_id.map(Into::into),
        status: row.status.into(),
        lines: Vec::new(),
        total_amount: Money {
            amount_cents: MoneyCents(0),
            currency: CurrencyCode("EUR".into()),
        },
        breakdown: None,
        uber_comparison: None,
        // ADR-20260906-192007 D-A/D-J: no lines, no mint -- null, same as every other arm before
        // `QuoteMinter` lands (D-H, a later phase of this same PR).
        quote: None,
        updated_at: row.updated_at,
        restaurant,
    }
}

#[cfg(test)]
mod tests {
    //! The two-leg lookup and the ownership narrowing, unit-tested at the seam (beck's ownership
    //! set): anonymous session A never sees session B's cart; a cart bound to someone else is
    //! invisible through the session leg; the by-id narrowing admits the owner and ADMIN only.
    use application::queries::CartReadRepository;
    use async_trait::async_trait;
    use domain::generated::scalars::{CartId, CustomerId, RestaurantId};

    use super::*;

    fn uid(n: u8) -> uuid::Uuid {
        uuid::Uuid::from_u128(n as u128)
    }

    /// The tenant the request's Host resolved to (#469) — restaurant 90 unless a test names
    /// another, so the pre-existing lookup assertions read as "on this restaurant's storefront".
    fn tenant(restaurant: u8) -> TenantScope {
        TenantScope::Restaurant(RestaurantId(uid(restaurant)))
    }

    fn row(cart: u8, session: u8, customer: Option<u8>, status: CartStatus, at: i64) -> CartRow {
        row_at(cart, 90, session, customer, status, at)
    }

    /// A cart AT a named restaurant — the axis #469 turned into a scoping boundary.
    fn row_at(
        cart: u8,
        restaurant: u8,
        session: u8,
        customer: Option<u8>,
        status: CartStatus,
        at: i64,
    ) -> CartRow {
        CartRow {
            cart_id: CartId(uid(cart)),
            restaurant_id: RestaurantId(uid(restaurant)),
            session_id: SessionId(uid(session)),
            customer_id: customer.map(|c| CustomerId(uid(c))),
            status,
            lines: serde_json::json!([]),
            created_at: chrono::DateTime::from_timestamp(at, 0).unwrap(),
            updated_at: chrono::DateTime::from_timestamp(at, 0).unwrap(),
        }
    }

    /// A cart store over a plain Vec, serving both lookups newest-first (the Pg contract).
    struct MemCarts(Vec<CartRow>);

    #[async_trait]
    impl CartReadRepository for MemCarts {
        async fn by_customer(&self, customer_id: CustomerId) -> Result<Vec<CartRow>, DomainError> {
            let mut rows: Vec<CartRow> = self
                .0
                .iter()
                .filter(|r| r.customer_id == Some(customer_id))
                .cloned()
                .collect();
            rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            Ok(rows)
        }
        /// OPEN-only, like the Pg adapter (#451) -- a double that answers a question the real
        /// port refuses would make every by-id test above vacuous.
        async fn by_id(&self, id: CartId) -> Result<Option<CartRow>, DomainError> {
            Ok(self
                .0
                .iter()
                .find(|r| r.cart_id == id && r.status == CartStatus::OPEN)
                .cloned())
        }
        async fn open_by_session(&self, session_id: SessionId) -> Result<Vec<CartRow>, DomainError> {
            let mut rows: Vec<CartRow> = self
                .0
                .iter()
                .filter(|r| r.session_id == session_id && r.status == CartStatus::OPEN)
                .cloned()
                .collect();
            rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            Ok(rows)
        }
        /// Tenant-scoped, like the Pg adapter (#469) — for the same reason the OPEN-only note above
        /// gives: a double that answers a question the real port refuses makes the tests vacuous.
        async fn open_by_customer_at(
            &self,
            customer_id: CustomerId,
            restaurant_id: RestaurantId,
        ) -> Result<Vec<CartRow>, DomainError> {
            let mut rows: Vec<CartRow> = self
                .0
                .iter()
                .filter(|r| {
                    r.customer_id == Some(customer_id)
                        && r.restaurant_id == restaurant_id
                        && r.status == CartStatus::OPEN
                })
                .cloned()
                .collect();
            rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            Ok(rows)
        }
        async fn open_by_session_at(
            &self,
            session_id: SessionId,
            restaurant_id: RestaurantId,
        ) -> Result<Vec<CartRow>, DomainError> {
            let mut rows: Vec<CartRow> = self
                .0
                .iter()
                .filter(|r| {
                    r.session_id == session_id
                        && r.restaurant_id == restaurant_id
                        && r.status == CartStatus::OPEN
                })
                .cloned()
                .collect();
            rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            Ok(rows)
        }
    }

    /// Leg 1: the claim picks the claim-holder's most-recently-updated OPEN cart — never the
    /// CHECKED_OUT one above it, never another customer's.
    #[tokio::test]
    async fn the_claim_leg_resolves_the_newest_open_cart_of_the_claim_holder() {
        let carts = MemCarts(vec![
            row(1, 10, Some(5), CartStatus::CHECKED_OUT, 300),
            row(2, 10, Some(5), CartStatus::OPEN, 200),
            row(3, 10, Some(5), CartStatus::OPEN, 100),
            row(4, 11, Some(6), CartStatus::OPEN, 400),
        ]);
        let found =
            current_open_cart(&carts, &ReadScope::Customer(CustomerId(uid(5))), None, tenant(90))
                .await
                .unwrap();
        assert_eq!(found.map(|r| r.cart_id), Some(CartId(uid(2))));
    }

    /// Leg 2 (anonymous): session A resolves ITS newest OPEN unbound cart; session B's carts are
    /// structurally out of reach — the lookup is keyed by the session id.
    #[tokio::test]
    async fn the_session_leg_resolves_only_the_callers_session() {
        let carts = MemCarts(vec![
            row(1, 10, None, CartStatus::OPEN, 100),
            row(2, 11, None, CartStatus::OPEN, 200),
        ]);
        let a = current_open_cart(&carts, &ReadScope::Public, Some(uid(10)), tenant(90)).await.unwrap();
        assert_eq!(a.map(|r| r.cart_id), Some(CartId(uid(1))), "session A sees its own cart");
        let b = current_open_cart(&carts, &ReadScope::Public, Some(uid(11)), tenant(90)).await.unwrap();
        assert_eq!(b.map(|r| r.cart_id), Some(CartId(uid(2))), "session B sees its own cart");
        let none = current_open_cart(&carts, &ReadScope::Public, Some(uid(12)), tenant(90)).await.unwrap();
        assert!(none.is_none(), "an unknown session sees nothing");
        let no_header = current_open_cart(&carts, &ReadScope::Public, None, tenant(90)).await.unwrap();
        assert!(no_header.is_none(), "no claim and no session header resolves nothing");
    }

    /// Leg 2's NULL-or-claim filter: a cart ALREADY BOUND to customer 5 is invisible to an
    /// anonymous replay of the same session id, and to a DIFFERENT customer riding that session —
    /// but stays visible to its own customer through the session leg (association already folded,
    /// claim leg would have found it anyway).
    #[tokio::test]
    async fn a_bound_cart_is_invisible_to_the_session_leg_unless_the_claim_matches() {
        let carts = MemCarts(vec![row(1, 10, Some(5), CartStatus::OPEN, 100)]);
        let anon = current_open_cart(&carts, &ReadScope::Public, Some(uid(10)), tenant(90)).await.unwrap();
        assert!(anon.is_none(), "anonymous replay of the session id sees a bound cart NEVER");
        let other = current_open_cart(&carts, &ReadScope::Customer(CustomerId(uid(6))), Some(uid(10)), tenant(90))
            .await
            .unwrap();
        assert!(other.is_none(), "another customer's claim on the same session sees nothing");
        let owner = current_open_cart(&carts, &ReadScope::Customer(CustomerId(uid(5))), Some(uid(10)), tenant(90))
            .await
            .unwrap();
        assert_eq!(owner.map(|r| r.cart_id), Some(CartId(uid(1))), "the owner still resolves it");
    }

    /// The association-not-yet-landed window (the PM lag FACT 1 names): a just-identified
    /// customer whose cart is still unbound resolves it through the session leg.
    #[tokio::test]
    async fn a_just_identified_customer_falls_through_to_their_session_cart() {
        let carts = MemCarts(vec![row(1, 10, None, CartStatus::OPEN, 100)]);
        let found = current_open_cart(&carts, &ReadScope::Customer(CustomerId(uid(5))), Some(uid(10)), tenant(90))
            .await
            .unwrap();
        assert_eq!(
            found.map(|r| r.cart_id),
            Some(CartId(uid(1))),
            "claim leg empty → session leg resolves the unbound cart"
        );
    }

    /// #469, the tenant boundary: the SAME customer, the SAME claim, TWO restaurants — each host
    /// serves its own cart. Leg 1 was `by_customer` newest-OPEN-anywhere, so on restaurant A this
    /// returned the cart from restaurant B whenever B's was more recently touched.
    #[tokio::test]
    async fn the_claim_leg_serves_the_cart_of_the_host_restaurant() {
        let carts = MemCarts(vec![
            // B's cart is NEWER — an unscoped "newest OPEN cart" resolves it on every host.
            row_at(2, 91, 10, Some(5), CartStatus::OPEN, 300),
            row_at(1, 90, 10, Some(5), CartStatus::OPEN, 100),
        ]);
        let scope = ReadScope::Customer(CustomerId(uid(5)));
        let on_a = current_open_cart(&carts, &scope, None, tenant(90)).await.unwrap();
        assert_eq!(
            on_a.map(|r| r.cart_id),
            Some(CartId(uid(1))),
            "restaurant A's storefront serves A's cart, never B's newer one"
        );
        let on_b = current_open_cart(&carts, &scope, None, tenant(91)).await.unwrap();
        assert_eq!(on_b.map(|r| r.cart_id), Some(CartId(uid(2))), "and B's serves B's");
    }

    /// The DECOY-FREE case (mob, testing lens 3): "this host's cart, ELSE the newest anywhere"
    /// passes the two-restaurant test above and is still the bug. With a cart at A **only**, a
    /// request on B's host must resolve NOTHING — no fallback, on either leg.
    #[tokio::test]
    async fn a_cart_at_another_restaurant_is_invisible_with_nothing_to_fall_back_to() {
        let carts = MemCarts(vec![row_at(1, 90, 10, Some(5), CartStatus::OPEN, 100)]);
        let claimed = current_open_cart(
            &carts,
            &ReadScope::Customer(CustomerId(uid(5))),
            Some(uid(10)),
            tenant(91),
        )
        .await
        .unwrap();
        assert!(claimed.is_none(), "no cart at THIS restaurant is null, never another's cart");

        // The same, unbound, through the session leg.
        let anon_carts = MemCarts(vec![row_at(1, 90, 10, None, CartStatus::OPEN, 100)]);
        let anon =
            current_open_cart(&anon_carts, &ReadScope::Public, Some(uid(10)), tenant(91)).await.unwrap();
        assert!(anon.is_none(), "the session leg is bounded by the host too");
    }

    /// The marketplace host, decided explicitly (#469, API lens): `live.captain.food` and every
    /// other non-tenant host name NO restaurant, so `current` is null there — not "newest
    /// anywhere", which is the unbounded read with a friendlier name. Holds with a claim, with a
    /// session id, and with both.
    #[tokio::test]
    async fn no_tenant_in_the_host_resolves_no_cart_at_all() {
        let carts = MemCarts(vec![
            row_at(1, 90, 10, Some(5), CartStatus::OPEN, 100),
            row_at(2, 91, 11, None, CartStatus::OPEN, 200),
        ]);
        for (case, scope, session) in [
            ("claim", ReadScope::Customer(CustomerId(uid(5))), None),
            ("session", ReadScope::Public, Some(uid(10))),
            ("both", ReadScope::Customer(CustomerId(uid(5))), Some(uid(10))),
        ] {
            let found = current_open_cart(&carts, &scope, session, TenantScope::None).await.unwrap();
            assert!(found.is_none(), "{case}: no tenant in the host is no cart");
        }
    }

    /// The by-id narrowing (#144/#434, the DONE-WHEN): owner yes, stranger no, unbound no
    /// (session carts are not by-id readable), ADMIN/SYSTEM yes, Public/unresolved never.
    #[test]
    fn readable_by_admits_the_owner_and_admin_only() {
        let bound = row(1, 10, Some(5), CartStatus::OPEN, 100);
        let unbound = row(2, 10, None, CartStatus::OPEN, 100);
        assert!(readable_by(&bound, &ReadScope::Customer(CustomerId(uid(5)))));
        assert!(!readable_by(&bound, &ReadScope::Customer(CustomerId(uid(6)))));
        assert!(!readable_by(&unbound, &ReadScope::Customer(CustomerId(uid(5)))));
        assert!(readable_by(&bound, &ReadScope::Admin));
        assert!(readable_by(&bound, &ReadScope::System));
        assert!(!readable_by(&bound, &ReadScope::Public));
    }
}
