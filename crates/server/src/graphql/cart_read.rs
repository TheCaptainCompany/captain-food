//! The cart READ seam (#451 Phase 2, ADR-20260810-120531): every GraphQL cart read — `current`,
//! `cart`, `carts` — resolves its row(s) here and prices them through the ONE `price_cart`
//! authority. Hand-written (not generated) so the lookup semantics, the ownership narrowing and
//! the pricing/observability boundary are unit-testable behind a seam the generated resolver
//! literals only CALL — the emitter carries no pricing logic of its own.
//!
//! Three responsibilities, nothing else:
//! - [`current_open_cart`] — the TWO-LEG `current` lookup (claim, then session);
//! - [`readable_by`] — the by-id claim-ownership narrowing (#144/#434);
//! - [`priced`] — money-free row → priced GraphQL `Cart` via `price_cart` over a one-read
//!   [`CatalogSnapshot`], under the `cart-price` observability contract (span + histogram +
//!   defect counter live HERE, the framework boundary — the pricer stays SDK-free).

use application::pricing::{price_cart, CatalogSnapshot};
use application::projections::RestaurantRow;
use application::queries::{CartReadRepository, CartRow, CatalogReadRepository, ReadScope};
use domain::generated::entities::CartLineItem;
use domain::generated::scalars::{CartStatus, SessionId};
use domain::shared::errors::DomainError;
use tracing::Instrument;

use super::generated::scalars::{CurrencyCode, MoneyCents};
use super::generated::types::{Cart, Money};

/// The TWO-LEG `current` resolution (ADR-20260810-120531). Carts are built ANONYMOUSLY under a
/// session id BEFORE any customer identity exists; CartBindingProcess associates them on
/// identification. So:
///
/// - **Leg 1 — claim**: a verified CUSTOMER claim resolves the claim-holder's most-recently-updated
///   OPEN cart (`by_customer` is `updated_at DESC`).
/// - **Leg 2 — session**: otherwise — anonymous, or the association not yet folded — a valid
///   `X-SESSION-ID` resolves the session's most-recently-updated OPEN cart WHERE `customer_id IS
///   NULL OR customer_id = <claim if present>`. The session id is an UNAUTHENTICATED correlator
///   (scoping only, never identity): the NULL-or-claim filter keeps a cart already bound to
///   someone ELSE invisible to whoever replays the session id.
///
/// OPEN only (`CartStatus` is OPEN | CHECKED_OUT — the LOCKED lifecycle is #465). `None` = no
/// open cart: the client renders the empty state, never a fabricated 0,00 EUR payable.
pub async fn current_open_cart(
    carts: &dyn CartReadRepository,
    scope: &ReadScope,
    session: Option<uuid::Uuid>,
) -> Result<Option<CartRow>, DomainError> {
    let claim = match scope {
        ReadScope::Customer(id) => Some(*id),
        _ => None,
    };
    // Leg 1 — the claim-holder's newest OPEN cart.
    if let Some(customer_id) = claim {
        let open = carts
            .by_customer(customer_id)
            .await?
            .into_iter()
            .find(|r| r.status == CartStatus::OPEN);
        if let Some(row) = open {
            return Ok(Some(row));
        }
    }
    // Leg 2 — the session's newest OPEN cart, NULL-or-claim owned.
    let Some(session) = session else { return Ok(None) };
    Ok(carts
        .open_by_session(SessionId(session))
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
    row: CartRow,
    restaurant: RestaurantRow,
    correlation_id: uuid::Uuid,
) -> async_graphql::Result<Cart> {
    let lines: Vec<CartLineItem> = serde_json::from_value(row.lines.clone())
        .map_err(|e| async_graphql::Error::new(format!("cart lines are malformed: {e}")))?;

    let span = telemetry::spans::cart_price(&row.cart_id.0.to_string());
    telemetry::spans::record_correlation_id(&span, &correlation_id.to_string());
    let started = std::time::Instant::now();
    let result = async {
        if lines.is_empty() {
            return Ok(None);
        }
        let snapshot = CatalogSnapshot::load(catalogs, row.restaurant_id).await?;
        price_cart(&snapshot, row.cart_id, row.restaurant_id, &lines).await.map(Some)
    }
    .instrument(span.clone())
    .await;
    telemetry::meters::cart_price::duration(started.elapsed().as_secs_f64() * 1000.0);

    let priced = result.map_err(|e| {
        if e.code() == Some("PriceUnresolvable") {
            // Canonical reason set (specs/observability.yaml cart-price): offer_gone covers a
            // line's offer OR selected option no longer resolving (and the currency-clash defect);
            // policy_missing / stock_unknown are reserved for legs not yet on this seam.
            telemetry::meters::cart_price::unresolvable("offer_gone");
        }
        // The span's ERROR status is what makes the contract's `technical_error: any_span_errors`
        // rule fire. Without it the counter above would tick while the trace exported a SUCCESS.
        telemetry::spans::record_cart_price_error(&span);
        async_graphql::Error::new(e.to_string())
    })?;

    let Some(priced) = priced else {
        return Ok(Cart {
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
            updated_at: row.updated_at,
            restaurant: restaurant.into(),
        });
    };

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
        updated_at: row.updated_at,
        restaurant: restaurant.into(),
    })
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

    fn row(cart: u8, session: u8, customer: Option<u8>, status: CartStatus, at: i64) -> CartRow {
        CartRow {
            cart_id: CartId(uid(cart)),
            restaurant_id: RestaurantId(uid(90)),
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
        async fn by_id(&self, id: CartId) -> Result<Option<CartRow>, DomainError> {
            Ok(self.0.iter().find(|r| r.cart_id == id).cloned())
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
            current_open_cart(&carts, &ReadScope::Customer(CustomerId(uid(5))), None).await.unwrap();
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
        let a = current_open_cart(&carts, &ReadScope::Public, Some(uid(10))).await.unwrap();
        assert_eq!(a.map(|r| r.cart_id), Some(CartId(uid(1))), "session A sees its own cart");
        let b = current_open_cart(&carts, &ReadScope::Public, Some(uid(11))).await.unwrap();
        assert_eq!(b.map(|r| r.cart_id), Some(CartId(uid(2))), "session B sees its own cart");
        let none = current_open_cart(&carts, &ReadScope::Public, Some(uid(12))).await.unwrap();
        assert!(none.is_none(), "an unknown session sees nothing");
        let no_header = current_open_cart(&carts, &ReadScope::Public, None).await.unwrap();
        assert!(no_header.is_none(), "no claim and no session header resolves nothing");
    }

    /// Leg 2's NULL-or-claim filter: a cart ALREADY BOUND to customer 5 is invisible to an
    /// anonymous replay of the same session id, and to a DIFFERENT customer riding that session —
    /// but stays visible to its own customer through the session leg (association already folded,
    /// claim leg would have found it anyway).
    #[tokio::test]
    async fn a_bound_cart_is_invisible_to_the_session_leg_unless_the_claim_matches() {
        let carts = MemCarts(vec![row(1, 10, Some(5), CartStatus::OPEN, 100)]);
        let anon = current_open_cart(&carts, &ReadScope::Public, Some(uid(10))).await.unwrap();
        assert!(anon.is_none(), "anonymous replay of the session id sees a bound cart NEVER");
        let other = current_open_cart(&carts, &ReadScope::Customer(CustomerId(uid(6))), Some(uid(10)))
            .await
            .unwrap();
        assert!(other.is_none(), "another customer's claim on the same session sees nothing");
        let owner = current_open_cart(&carts, &ReadScope::Customer(CustomerId(uid(5))), Some(uid(10)))
            .await
            .unwrap();
        assert_eq!(owner.map(|r| r.cart_id), Some(CartId(uid(1))), "the owner still resolves it");
    }

    /// The association-not-yet-landed window (the PM lag FACT 1 names): a just-identified
    /// customer whose cart is still unbound resolves it through the session leg.
    #[tokio::test]
    async fn a_just_identified_customer_falls_through_to_their_session_cart() {
        let carts = MemCarts(vec![row(1, 10, None, CartStatus::OPEN, 100)]);
        let found = current_open_cart(&carts, &ReadScope::Customer(CustomerId(uid(5))), Some(uid(10)))
            .await
            .unwrap();
        assert_eq!(
            found.map(|r| r.cart_id),
            Some(CartId(uid(1))),
            "claim leg empty → session leg resolves the unbound cart"
        );
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
