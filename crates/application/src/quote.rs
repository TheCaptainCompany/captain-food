//! The signed cart quote (PROP-20260831-134539 slice 3b, ADR-20260906-192007 D-A/D-F/D-H): a
//! server-minted, HMAC-signed, opaque token (`scalars.yaml#/CartQuote`) binding a cart's PRICES to
//! the exact catalog coordinate that produced them (`domain::catalog_as_of::CatalogVersion`) and to
//! the cart's own priceable content at mint time — so a cart edited after the quote was shown is
//! detectable at verify time without a second live read of "what did the customer see".
//!
//! **`hmac`/`sha2` live HERE, in `crates/application`, never in `crates/server`** (D-H, evans):
//! signing/verifying a token is business logic, not a framework concern.
//!
//! **Substitution note** (recorded here rather than silently): the dispatch names "the cart's own
//! stream VERSION" as the edit-detection anchor. The cart read projection
//! (`application::generated::rows::CartRow`) carries no stream-version column — adding one is a
//! `View_cart` schema change this deliverable does not make. [`QuotePayload::lines_digest`] is the
//! substitute: a SHA-256 over the SAME canonical JSON encoding of the cart's repricing inputs
//! (`Vec<CartLineItem>`) both the mint side (`CartRow::lines`, parsed) and the verify side
//! (`CartState::lines`, the write-side fold) already carry. It proves the identical fact — "has
//! this cart's priceable content changed since mint" — without a new stored column; it is NOT a
//! substitute for a real optimistic-concurrency version and must not be read as one anywhere else.
//!
//! **The charge is never the client-echoed `totalCents`.** The token's `total_cents`/`currency`
//! fields are carried for OBSERVABILITY only (`business.quoted_total_cents`) — the actual charge
//! amount is always freshly recomputed, server-side, via
//! [`crate::pricing::price_cart_at`] over the coordinate the token names
//! (`AsOfPriceAuthority::as_of`, never `at_head` — D-F). This is what makes
//! "the charged total comes from the fold" true even when a stale read-side projection disagrees
//! with the event log at the same coordinate.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use domain::catalog_as_of::CatalogVersion;
use domain::generated::entities::{CartLineItem, Money};
use domain::generated::scalars::{CartId, CartQuote, CatalogId, CurrencyCode, MoneyCents, RestaurantId};
use domain::shared::errors::DomainError;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::ports::AsOfPriceAuthority;
use crate::pricing::{price_cart_at, CatalogSnapshot};
use crate::queries::CatalogReadRepository;

/// The DEV-ONLY fallback signing key (`application::email_guard::DEV_ONLY_HMAC_KEY`'s own
/// precedent): reachable only when `QUOTE_SIGNING_KEY_HMAC_SECRET` is unset, which
/// staging/production refuse to boot with once the write door is open (see
/// [`QuoteGuard::resolve_at_boot`]) — this literal never signs a real customer's total there.
pub const DEV_ONLY_HMAC_KEY: &[u8] =
    b"captain-food-dev-only-quote-signing-hmac-key-DO-NOT-USE-IN-PRODUCTION";

/// The overlap window a retired key stays acceptable for verification (configuration.yaml
/// `QUOTE_SIGNING_KEY_PREVIOUS_HMAC_SECRET`'s own gates: 60 minutes = the 30-minute
/// `QUOTE-STALENESS` backstop + an UNVERIFIED skew term).
const MAX_QUOTE_AGE_SECONDS: i64 = 30 * 60;

/// A signing/verifying key: an opaque secret plus the `keyId` a token names so a verifier holding
/// MULTIPLE keys (current + previous, during a rotation's overlap window) knows which one to try.
/// The constructor is PRIVATE TO THIS MODULE (the `ActingRole` shape, ADR-20260803-234035 level
/// 4): the only paths that can produce one are [`SigningKey::from_resolved_secret`] (the real
/// config value, which staging/production require non-empty) and [`SigningKey::dev_only`] — there
/// is no third, "just pass any bytes" constructor a caller elsewhere could reach.
#[derive(Clone)]
pub struct SigningKey {
    id: String,
    secret: Vec<u8>,
}

impl SigningKey {
    fn new(id: impl Into<String>, secret: Vec<u8>) -> Self {
        Self { id: id.into(), secret }
    }

    /// The resolved configuration value (may be empty — an unset staging/production secret is
    /// caught by `required: [staging, production]` at boot before this is ever called with an
    /// empty string in a LIVE profile; development/test fall back to the DEV-ONLY key, matching
    /// `EmailSendPolicy::from_config`'s own precedent).
    pub fn from_resolved_secret(id: &str, secret: &str) -> Self {
        let trimmed = secret.trim();
        if trimmed.is_empty() {
            Self::dev_only(id)
        } else {
            Self::new(id, trimmed.as_bytes().to_vec())
        }
    }

    /// The fixed DEV-ONLY key, for development/test only.
    pub fn dev_only(id: &str) -> Self {
        Self::new(id, DEV_ONLY_HMAC_KEY.to_vec())
    }

    /// Whether this key's bytes are exactly the DEV-ONLY literal — the boot refusal's own check
    /// (never a per-request one). Comparing bytes, never the `id`, because an operator naming a
    /// custom `keyId` while still resolving the placeholder secret must still refuse.
    pub fn is_dev_only(&self) -> bool {
        self.secret == DEV_ONLY_HMAC_KEY
    }

    fn hmac(&self, message: &[u8]) -> Vec<u8> {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.secret)
            .expect("HMAC accepts a key of any length");
        mac.update(message);
        mac.finalize().into_bytes().to_vec()
    }
}

/// The token's signed payload — everything the verify guard checks against the LIVE command/cart/
/// catalog state. `total_cents`/`currency` are carried for OBSERVABILITY ONLY (never a verify-time
/// gate — see the module doc); the CHARGE is always the fresh [`price_cart_at`] recompute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotePayload {
    pub cart_id: uuid::Uuid,
    pub restaurant_id: uuid::Uuid,
    pub catalog_id: uuid::Uuid,
    /// The coordinate the fold was bounded at when this quote was minted
    /// (`domain::catalog_as_of::CatalogVersion::get`).
    pub catalog_version: i64,
    /// SHA-256 hex of the canonical JSON encoding of the cart's `Vec<CartLineItem>` at mint time —
    /// the edit-detection anchor (module doc's substitution note).
    pub lines_digest: String,
    pub total_cents: i64,
    pub currency: String,
    /// Unix seconds — the mint instant, checked against the 30-minute `QUOTE-STALENESS` backstop.
    pub minted_at: i64,
    pub key_id: String,
}

/// The base64url-decoded, JSON-deserialized envelope one [`CartQuote`] token carries: the payload
/// plus its own signature, so a verifier can recompute the HMAC over the SAME payload bytes it
/// deserialized from (never over the whole envelope, which would include the signature itself).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SignedEnvelope {
    payload: QuotePayload,
    signature: String,
}

/// The canonical bytes a signature covers: the payload alone, serialized once, deterministically
/// (a plain struct — never a `HashMap` — so field order is the declaration order every time).
fn payload_bytes(payload: &QuotePayload) -> Vec<u8> {
    serde_json::to_vec(payload).expect("QuotePayload always serializes")
}

fn lines_digest(lines: &[CartLineItem]) -> String {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(lines).expect("CartLineItem always serializes");
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    hex::encode(hasher.finalize())
}

fn encode_token(envelope: &SignedEnvelope) -> CartQuote {
    use base64::Engine as _;
    let json = serde_json::to_vec(envelope).expect("SignedEnvelope always serializes");
    CartQuote(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json))
}

fn decode_token(token: &CartQuote) -> Option<SignedEnvelope> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&token.0).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Mints signed quotes with exactly ONE current key (never the verifier's plural set — a minter
/// cannot accidentally verify, D-H).
pub struct QuoteMinter {
    key: SigningKey,
}

impl QuoteMinter {
    pub fn new(key: SigningKey) -> Self {
        Self { key }
    }

    /// Sign a quote for `lines`, priced at `catalog_version` (the coordinate `AsOfCatalog::coordinate`
    /// returned), for `total`. `minted_at` is a parameter — the same "no clock read inside" style as
    /// `place_order`'s `when_at` — so the mint site controls the instant and tests stay
    /// deterministic.
    pub fn mint(
        &self,
        cart_id: CartId,
        restaurant_id: RestaurantId,
        catalog_id: CatalogId,
        catalog_version: CatalogVersion,
        lines: &[CartLineItem],
        total: &Money,
        minted_at: DateTime<Utc>,
    ) -> CartQuote {
        let payload = QuotePayload {
            cart_id: cart_id.0,
            restaurant_id: restaurant_id.0,
            catalog_id: catalog_id.0,
            catalog_version: catalog_version.get(),
            lines_digest: lines_digest(lines),
            total_cents: total.amount_cents.0,
            currency: total.currency.0.clone(),
            minted_at: minted_at.timestamp(),
            key_id: self.key.id.clone(),
        };
        let signature = hex::encode(self.key.hmac(&payload_bytes(&payload)));
        encode_token(&SignedEnvelope { payload, signature })
    }
}

/// Why a submitted quote could not be verified — the caller (`verify_quote`) maps every variant
/// onto EXACTLY one of the two catalogued codes (D-D): [`QuoteRefusal::CartChanged`] and
/// [`QuoteRefusal::Expired`] are the ONE business error (`QuoteNoLongerHonoured`, quiet); every
/// other variant is the structural/technical path (`QuoteVerificationFailed`, LOUD — the caller
/// marks the `quote.verify` span ERROR). `reason()` is the span's own
/// `business.failure_reason` value, never surfaced to the customer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteRefusal {
    /// The token does not decode, or its signature does not verify under any live key.
    Tampered,
    /// The token names a `keyId` no live key (current or, within the overlap window, previous)
    /// resolves to.
    UnknownKey,
    /// The token names a different cart or restaurant than the command's.
    ForeignCart,
    /// The token names a catalog id that does not match the restaurant's live catalog.
    CatalogMismatch,
    /// The token's coordinate is absent or beyond the live stream head.
    CoordinateBeyondHead,
    /// The fold at the token's coordinate could not be read (transient/technical).
    FoldUnavailable,
    /// The cart's priceable content changed since the quote was minted.
    CartChanged,
    /// The quote is older than [`MAX_QUOTE_AGE_SECONDS`].
    Expired,
}

impl QuoteRefusal {
    /// The span's `business.failure_reason` value — never customer-facing.
    pub fn reason(&self) -> &'static str {
        match self {
            QuoteRefusal::Tampered => "tampered",
            QuoteRefusal::UnknownKey => "unknown_key",
            QuoteRefusal::ForeignCart => "foreign_cart",
            QuoteRefusal::CatalogMismatch => "catalog_mismatch",
            QuoteRefusal::CoordinateBeyondHead => "coordinate_beyond_head",
            QuoteRefusal::FoldUnavailable => "fold_unavailable",
            QuoteRefusal::CartChanged => "cart_changed",
            QuoteRefusal::Expired => "expired",
        }
    }

    /// `true` for the ONE business error's two causes (D-D ii); `false` for every structural cause
    /// (D-D i/iii), which the caller classifies LOUD (`quote_verify_total{outcome}`'s technical
    /// bucket, the `quote.verify` span marked ERROR).
    pub fn is_business(&self) -> bool {
        matches!(self, QuoteRefusal::CartChanged | QuoteRefusal::Expired)
    }

    /// The one catalogued error code this refusal maps onto (D-D): business causes always
    /// `QuoteNoLongerHonoured`; every structural/technical cause `QuoteVerificationFailed`.
    pub fn into_domain_error(self, cart_id: CartId) -> DomainError {
        let code = if self.is_business() { "QuoteNoLongerHonoured" } else { "QuoteVerificationFailed" };
        DomainError::rejected(code, json!({ "cartId": cart_id }))
    }
}

/// Verifies quotes against a key SET (current + an optional retired-but-in-overlap key) — a
/// DIFFERENT type from [`QuoteMinter`], so a verifier cannot accidentally mint (D-H).
pub struct QuoteVerifier {
    keys: Vec<SigningKey>,
}

impl QuoteVerifier {
    pub fn new(current: SigningKey, previous: Option<SigningKey>) -> Self {
        let mut keys = vec![current];
        keys.extend(previous);
        Self { keys }
    }

    /// Decode, verify the signature under the ONE live key the token names by `keyId`, and check
    /// the 30-minute staleness backstop. Returns the decoded payload on success — the caller
    /// (`verify_quote`) still owns every LIVE-STATE comparison (cartId/restaurantId/catalogId/
    /// lines digest/coordinate), because this method holds no cart or catalog state at all.
    /// `pub`: none of `QuotePayload`'s fields are secret (the signature itself never leaves this
    /// module), so a caller inspecting the decoded, ALREADY-VERIFIED fields (tests; a future
    /// observability span) needs no feature-gated test-only seam.
    pub fn decode_and_check_signature(
        &self,
        token: &CartQuote,
        now: DateTime<Utc>,
    ) -> Result<QuotePayload, QuoteRefusal> {
        let envelope = decode_token(token).ok_or(QuoteRefusal::Tampered)?;
        let Some(key) = self.keys.iter().find(|k| k.id == envelope.payload.key_id) else {
            return Err(QuoteRefusal::UnknownKey);
        };
        let expected = key.hmac(&payload_bytes(&envelope.payload));
        let given = hex::decode(&envelope.signature).map_err(|_| QuoteRefusal::Tampered)?;
        // Constant-time comparison: a signature check is exactly the kind of secret-dependent
        // branch a timing side-channel could exploit (dba/farley's spy-binary concern, D-I).
        use subtle::ConstantTimeEq as _;
        if expected.len() != given.len() || expected.ct_eq(&given).unwrap_u8() != 1 {
            return Err(QuoteRefusal::Tampered);
        }
        let age = now.timestamp() - envelope.payload.minted_at;
        if !(0..=MAX_QUOTE_AGE_SECONDS).contains(&age) {
            return Err(QuoteRefusal::Expired);
        }
        Ok(envelope.payload)
    }
}

/// The write door's witness (D-B interlock, compiler-first per ADR-20260803-234035 level 4): the
/// ONLY way to construct `Some` is [`WriteDoorOpen::resolve`] seeing BOTH doors open. The mixed
/// state — write door open, read door closed — is refused at [`QuoteGuard::resolve_at_boot`],
/// never merely checked per-request: there is no path from `(true, false)` to a live
/// `WriteDoorOpen` value anywhere in this module.
#[derive(Debug, Clone, Copy)]
pub struct WriteDoorOpen(());

impl WriteDoorOpen {
    fn resolve(
        run_quote_required_on_place_order: bool,
        run_fold_priced_cart_read: bool,
    ) -> Result<Option<Self>, String> {
        match (run_quote_required_on_place_order, run_fold_priced_cart_read) {
            (true, true) => Ok(Some(Self(()))),
            (false, _) => Ok(None),
            (true, false) => Err(
                "RUN_QUOTE_REQUIRED_ON_PLACE_ORDER is open while RUN_FOLD_PRICED_CART_READ is \
                 closed -- the write door would verify quotes no read ever mints; refusing to boot \
                 (ADR-20260906-192007 D-B)"
                    .to_string(),
            ),
        }
    }
}

/// Everything `application::commands::place_order`'s pre-payment guard needs, bundled into ONE
/// value (ADR-20260904-081527 §8 seventh carve-out condition (b): "one argument threaded") — the
/// write door's witness, the verifier (holding the key SET), and the fold-priced read authority
/// the guard reprices through (D-K's bulkhead pool, injected here rather than constructed here:
/// this module stays pool-agnostic).
pub struct QuoteGuard {
    door: Option<WriteDoorOpen>,
    verifier: QuoteVerifier,
    fold_authority: Arc<dyn AsOfPriceAuthority>,
}

impl QuoteGuard {
    /// Resolve the guard AT BOOT (never per-request): the interlock (D-B) and the dev-key boot
    /// refusal (D-H) both fire here, so a misconfigured process never serves a single request
    /// rather than refusing a fraction of them. `profile_is_live` is `true` in staging/production
    /// — the dev-key refusal never fires in development/test, where the DEV-ONLY fallback exists
    /// precisely so a database-less run still exercises the signed path.
    pub fn resolve_at_boot(
        run_quote_required_on_place_order: bool,
        run_fold_priced_cart_read: bool,
        profile_is_live: bool,
        current_key: SigningKey,
        previous_key: Option<SigningKey>,
        fold_authority: Arc<dyn AsOfPriceAuthority>,
    ) -> Result<Self, String> {
        let door = WriteDoorOpen::resolve(run_quote_required_on_place_order, run_fold_priced_cart_read)?;
        if door.is_some() && profile_is_live && current_key.is_dev_only() {
            return Err(
                "QUOTE_SIGNING_KEY_HMAC_SECRET resolves to the DEV-ONLY fallback while \
                 RUN_QUOTE_REQUIRED_ON_PLACE_ORDER is open in a live profile -- refusing to boot \
                 with a forgeable signing key on the money path (ADR-20260906-192007 D-H)"
                    .to_string(),
            );
        }
        Ok(Self { door, verifier: QuoteVerifier::new(current_key, previous_key), fold_authority })
    }

    /// `true` once the interlock proved BOTH doors open.
    pub fn is_open(&self) -> bool {
        self.door.is_some()
    }

    /// The CLOSED-door guard every OTHER crate's test harness needs (#510's `for_tests` shape,
    /// `queries::MailboxRequeueAccess::for_tests` precedent): both doors false, so
    /// `resolve_at_boot` can never refuse, over a fold authority that panics if ever called (the
    /// door stays CLOSED). Compiled only under `test-fixtures`, which only `[dev-dependencies]`
    /// may enable (`test_fixtures_feature_never_reaches_a_release_artifact`).
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn closed_for_tests() -> Self {
        struct UncalledFoldAuthority;
        #[async_trait::async_trait]
        impl AsOfPriceAuthority for UncalledFoldAuthority {
            async fn as_of(
                &self,
                _catalog_id: CatalogId,
                _version: CatalogVersion,
            ) -> Result<domain::catalog_as_of::AsOfCatalog, DomainError> {
                panic!("QuoteGuard::closed_for_tests -- the door is CLOSED, this must never be called");
            }
            async fn at_head(
                &self,
                _catalog_id: CatalogId,
                _correlation_id: uuid::Uuid,
            ) -> Result<domain::catalog_as_of::AsOfCatalog, DomainError> {
                panic!("QuoteGuard::closed_for_tests -- the door is CLOSED, this must never be called");
            }
        }
        Self::resolve_at_boot(
            false,
            false,
            false,
            SigningKey::from_resolved_secret("test", ""),
            None,
            Arc::new(UncalledFoldAuthority),
        )
        .expect("both doors closed can never refuse at boot")
    }
}

/// The write-side verify guard (D-F), called from `application::commands::place_order`'s
/// pre-payment block. CLOSED (`guard.is_open()` false): returns `Ok(None)` immediately — the
/// caller keeps charging `price_cart`'s HEAD-projection total, exactly today's behaviour, and
/// `quote`/`catalogs`/`lines` are not even inspected. OPEN: an absent `quote` refuses with
/// `QuoteVerificationFailed`; every other check maps through [`QuoteRefusal::into_domain_error`]. On success,
/// returns `Some(total)` — the FRESH [`price_cart_at`] recompute at the token's own coordinate,
/// which the caller charges INSTEAD OF the HEAD-projection total (never the token's own
/// `totalCents`, which is carried for observability only — see the module doc).
pub async fn verify_quote(
    guard: &QuoteGuard,
    quote: Option<&CartQuote>,
    cart_id: CartId,
    restaurant_id: RestaurantId,
    catalogs: &dyn CatalogReadRepository,
    lines: &[CartLineItem],
    now: DateTime<Utc>,
) -> Result<Option<Money>, DomainError> {
    if !guard.is_open() {
        return Ok(None);
    }
    let Some(token) = quote else {
        return Err(DomainError::rejected("QuoteVerificationFailed", json!({ "cartId": cart_id })));
    };
    let payload = guard
        .verifier
        .decode_and_check_signature(token, now)
        .map_err(|r| r.into_domain_error(cart_id))?;

    if payload.cart_id != cart_id.0 || payload.restaurant_id != restaurant_id.0 {
        return Err(QuoteRefusal::ForeignCart.into_domain_error(cart_id));
    }
    let snapshot = CatalogSnapshot::load(catalogs, restaurant_id).await?;
    let Some(catalog_id) = snapshot.catalog_id() else {
        return Err(QuoteRefusal::CatalogMismatch.into_domain_error(cart_id));
    };
    if payload.catalog_id != catalog_id.0 {
        return Err(QuoteRefusal::CatalogMismatch.into_domain_error(cart_id));
    }
    if payload.lines_digest != lines_digest(lines) {
        return Err(QuoteRefusal::CartChanged.into_domain_error(cart_id));
    }
    let Some(version) = CatalogVersion::try_new(payload.catalog_version) else {
        return Err(QuoteRefusal::CoordinateBeyondHead.into_domain_error(cart_id));
    };
    let as_of = guard.fold_authority.as_of(catalog_id, version).await.map_err(|_| {
        QuoteRefusal::CoordinateBeyondHead.into_domain_error(cart_id)
    })?;
    let priced = price_cart_at(catalogs, &as_of, cart_id, restaurant_id, lines).await.map_err(|_| {
        QuoteRefusal::FoldUnavailable.into_domain_error(cart_id)
    })?;
    let _ = (payload.total_cents, payload.currency); // observability only -- see module doc.
    Ok(Some(priced.total_amount))
}

// Silence "unused" for the plain re-export path some callers want (span attribute construction
// reads MoneyCents/CurrencyCode from the domain types directly, not from here).
#[allow(unused_imports)]
use MoneyCents as _MoneyCentsReexport;
#[allow(unused_imports)]
use CurrencyCode as _CurrencyCodeReexport;

#[cfg(test)]
mod tests {
    use super::*;
    use domain::generated::scalars::{CurrencyCode as Cur, MoneyCents as Cents, OfferId};

    fn uid(n: u128) -> uuid::Uuid {
        uuid::Uuid::from_u128(n)
    }
    fn cart() -> CartId {
        CartId(uid(1))
    }
    fn restaurant() -> RestaurantId {
        RestaurantId(uid(2))
    }
    fn catalog() -> CatalogId {
        CatalogId(uid(3))
    }
    fn v(n: i64) -> CatalogVersion {
        CatalogVersion::try_new(n).unwrap()
    }
    fn money(cents: i64) -> Money {
        Money { amount_cents: Cents(cents), currency: Cur("EUR".into()) }
    }
    fn line() -> CartLineItem {
        CartLineItem {
            cart_line_id: domain::generated::scalars::CartLineId(uid(10)),
            offer_id: OfferId(uid(20)),
            quantity: 2,
            selected_option_ids: vec![],
        }
    }
    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    /// beck (i): the minter/verifier round trip -- a quote minted with `key-1` decodes and
    /// signature-checks cleanly under a verifier holding the SAME key, with every field the
    /// verifier's live-state comparison reads (cartId/restaurantId/catalogId/lines digest) intact.
    #[test]
    fn a_minted_quote_round_trips_through_the_verifier() {
        let minter = QuoteMinter::new(SigningKey::from_resolved_secret("key-1", "s3cret-key-1"));
        let verifier =
            QuoteVerifier::new(SigningKey::from_resolved_secret("key-1", "s3cret-key-1"), None);
        let lines = vec![line()];
        let token = minter.mint(cart(), restaurant(), catalog(), v(5), &lines, &money(1500), at(1000));
        let payload = verifier.decode_and_check_signature(&token, at(1010)).expect("verifies");
        assert_eq!(payload.cart_id, cart().0);
        assert_eq!(payload.restaurant_id, restaurant().0);
        assert_eq!(payload.catalog_id, catalog().0);
        assert_eq!(payload.catalog_version, 5);
        assert_eq!(payload.lines_digest, lines_digest(&lines));
        assert_eq!(payload.key_id, "key-1");
    }

    /// A single flipped byte anywhere in the token (the payload OR the signature) fails
    /// signature verification -- both arms of the spy binary's "no bit of tamper survives".
    #[test]
    fn a_single_flipped_byte_in_the_payload_or_signature_fails_verification() {
        let minter = QuoteMinter::new(SigningKey::from_resolved_secret("key-1", "s3cret-key-1"));
        let verifier =
            QuoteVerifier::new(SigningKey::from_resolved_secret("key-1", "s3cret-key-1"), None);
        let lines = vec![line()];
        let token = minter.mint(cart(), restaurant(), catalog(), v(5), &lines, &money(1500), at(1000));

        use base64::Engine as _;
        let mut raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&token.0).unwrap();
        // Flip one byte inside the JSON body (not the first/last, which are structural braces).
        let mid = raw.len() / 2;
        raw[mid] ^= 0x01;
        let tampered =
            CartQuote(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&raw));
        assert_eq!(
            verifier.decode_and_check_signature(&tampered, at(1010)),
            Err(QuoteRefusal::Tampered)
        );
    }

    /// A quote signed by a key the verifier does not hold at all refuses `UnknownKey` -- distinct
    /// from a still-valid signature under a DIFFERENT secret (the checkpoint's "retired keyId ...
    /// incl. a still-valid MAC" case, exercised at the higher `verify_quote` level below).
    #[test]
    fn a_quote_under_an_unknown_key_id_is_refused() {
        let minter = QuoteMinter::new(SigningKey::from_resolved_secret("key-2", "s3cret-key-2"));
        let verifier =
            QuoteVerifier::new(SigningKey::from_resolved_secret("key-1", "s3cret-key-1"), None);
        let lines = vec![line()];
        let token = minter.mint(cart(), restaurant(), catalog(), v(5), &lines, &money(1500), at(1000));
        assert_eq!(
            verifier.decode_and_check_signature(&token, at(1010)),
            Err(QuoteRefusal::UnknownKey)
        );
    }

    /// A retired key STILL inside the verifier's overlap set signature-checks cleanly (two
    /// verifiers -- well, one verifier holding two keys -- agree; a DIFFERENT secret under the
    /// SAME keyId never would, since the signature covers the payload, not the keyId string).
    #[test]
    fn a_quote_under_the_previous_key_verifies_inside_the_overlap_set() {
        let minter = QuoteMinter::new(SigningKey::from_resolved_secret("old-key", "retired-secret"));
        let verifier = QuoteVerifier::new(
            SigningKey::from_resolved_secret("current-key", "current-secret"),
            Some(SigningKey::from_resolved_secret("old-key", "retired-secret")),
        );
        let lines = vec![line()];
        let token = minter.mint(cart(), restaurant(), catalog(), v(5), &lines, &money(1500), at(1000));
        assert!(verifier.decode_and_check_signature(&token, at(1010)).is_ok());
    }

    /// A quote older than the 30-minute backstop refuses `Expired` -- the business cause, never
    /// the structural one.
    #[test]
    fn a_quote_older_than_thirty_minutes_is_expired() {
        let minter = QuoteMinter::new(SigningKey::from_resolved_secret("key-1", "s3cret-key-1"));
        let verifier =
            QuoteVerifier::new(SigningKey::from_resolved_secret("key-1", "s3cret-key-1"), None);
        let lines = vec![line()];
        let token = minter.mint(cart(), restaurant(), catalog(), v(5), &lines, &money(1500), at(1000));
        let too_late = at(1000 + MAX_QUOTE_AGE_SECONDS + 1);
        assert_eq!(verifier.decode_and_check_signature(&token, too_late), Err(QuoteRefusal::Expired));
        assert!(QuoteRefusal::Expired.is_business());
    }

    /// D-B: the mixed state (write door open, read door closed) is UNREPRESENTABLE -- there is no
    /// path from these two bools to a live `WriteDoorOpen`, only a boot-time `Err`.
    #[test]
    fn the_write_door_cannot_open_while_the_read_door_is_closed() {
        assert_eq!(
            WriteDoorOpen::resolve(true, false),
            Err(
                "RUN_QUOTE_REQUIRED_ON_PLACE_ORDER is open while RUN_FOLD_PRICED_CART_READ is \
                 closed -- the write door would verify quotes no read ever mints; refusing to boot \
                 (ADR-20260906-192007 D-B)"
                    .to_string()
            )
        );
        assert!(WriteDoorOpen::resolve(true, true).unwrap().is_some());
        assert!(WriteDoorOpen::resolve(false, false).unwrap().is_none());
        assert!(WriteDoorOpen::resolve(false, true).unwrap().is_none());
    }

    impl PartialEq for WriteDoorOpen {
        fn eq(&self, _other: &Self) -> bool {
            true
        }
    }
    impl Eq for WriteDoorOpen {}

    struct NeverCalled;
    #[async_trait::async_trait]
    impl AsOfPriceAuthority for NeverCalled {
        async fn as_of(
            &self,
            _catalog_id: CatalogId,
            _version: CatalogVersion,
        ) -> Result<domain::catalog_as_of::AsOfCatalog, DomainError> {
            unreachable!("a boot refusal never reaches a read")
        }
        async fn at_head(
            &self,
            _catalog_id: CatalogId,
            _correlation_id: uuid::Uuid,
        ) -> Result<domain::catalog_as_of::AsOfCatalog, DomainError> {
            unreachable!("a boot refusal never reaches a read")
        }
    }

    /// D-H: the write door refuses to open AT BOOT when the resolved key is the DEV-ONLY
    /// fallback and the profile is LIVE (staging/production) -- never a per-request check.
    #[test]
    fn the_dev_signing_key_refuses_to_boot_in_a_live_profile_with_the_door_open() {
        let result = QuoteGuard::resolve_at_boot(
            true,
            true,
            true, // profile_is_live
            SigningKey::from_resolved_secret("current", ""), // unset -> DEV_ONLY fallback
            None,
            Arc::new(NeverCalled),
        );
        match result {
            Ok(_) => panic!("a live profile with the door open and the dev key must refuse to boot"),
            Err(err) => assert!(err.contains("DEV-ONLY"), "the refusal must name the dev key, got: {err}"),
        }
    }

    /// The SAME dev key never refuses in development/test (`profile_is_live: false`) -- the
    /// DEV-ONLY fallback exists precisely so a database-less run still exercises the signed
    /// path.
    #[test]
    fn the_dev_signing_key_boots_fine_outside_a_live_profile() {
        assert!(QuoteGuard::resolve_at_boot(
            true,
            true,
            false, // profile_is_live
            SigningKey::from_resolved_secret("current", ""),
            None,
            Arc::new(NeverCalled),
        )
        .is_ok());
    }

    /// A REAL (non-dev) key never refuses, live profile or not -- the refusal is specific to the
    /// dev-only literal, never a blanket "door open in a live profile" ban.
    #[test]
    fn a_real_signing_key_boots_fine_in_a_live_profile_with_the_door_open() {
        assert!(QuoteGuard::resolve_at_boot(
            true,
            true,
            true,
            SigningKey::from_resolved_secret("current", "a-real-provisioned-secret"),
            None,
            Arc::new(NeverCalled),
        )
        .is_ok());
    }
}
