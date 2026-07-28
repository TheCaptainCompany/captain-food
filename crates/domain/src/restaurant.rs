//! Restaurant aggregate — the PURE write-side state fold (ADR-0035). Command handlers rehydrate a
//! [`RestaurantState`] by folding the stream's events (loaded through the `EventStore` port) and then
//! enforce the invariants declared in `specs/actors.yaml`/`specs/errors.yaml` against it. No I/O, no
//! serialization logic (dependency rule).
//!
//! Scope: the fold carries the fields the invariants read, PLUS — since ADR-20260728-011344 — the
//! editable location fields, so the aggregate can decide whether an inbound registry record actually
//! changes anything without asking the read side. It is still not the read model: the projected
//! `Restaurant` row (ADR-0040) holds the query-shaped view, including the enrichment columns
//! (rating, reviews, GBP data) no write-side decision depends on.
//!
//! The status machine is the DECLARED lifecycle (`specs/actors.yaml#/Restaurant/lifecycle`,
//! ADR-20260721-093027): the fold moves `status` exclusively through the GENERATED tables
//! ([`lifecycle::initial`] births it DRAFT, [`lifecycle::target`] applies `RestaurantActivated` →
//! ACTIVE and `RestaurantDeactivated`/`RestaurantRemoved`/`RestaurantMarkedClosed` → INACTIVE), so
//! write-side decisions and the projected `status` column can never disagree.

use crate::generated::entities::{Address, GeoPoint, OpeningHoursSlot, RestaurantContact};
use crate::generated::events::DomainEvent;
pub use crate::generated::lifecycles::restaurant as lifecycle;
use crate::generated::scalars::{
    CuisineCategory, ExternalReference, MarginPercent, OrderAcceptanceMode, RestaurantDisplayName,
    RestaurantListingStatus, RestaurantStatus, Slug, Tag, TimeZone, WebUrl,
};

/// What the Restaurant command handlers need to know about the aggregate to accept or reject a
/// command. `None` (from [`fold`]) means the aggregate does not exist → `RestaurantNotFound`.
#[derive(Debug, Clone, PartialEq)]
pub struct RestaurantState {
    /// Operational lifecycle (DRAFT → ACTIVE ⇄ INACTIVE) — `RestaurantNotActive`, activate/deactivate
    /// idempotency.
    pub status: RestaurantStatus,
    /// Live acceptance mode (NORMAL until changed) — `AcceptanceModeUnchanged`.
    pub order_acceptance: OrderAcceptanceMode,
    /// Partnership funnel position (NON_PARTNER → PASSIVE_PARTNER → ACTIVE_PARTNER).
    pub listing_status: RestaurantListingStatus,
    /// Whether an owner already claimed this listing — `ListingAlreadyClaimed`.
    pub listing_claimed: bool,
    /// The configured GBP 'Order online' link, if any — `GbpOrderLinkNotConfigured` (ADR-0021).
    pub gbp_order_url: Option<WebUrl>,
    /// The storefront host label, or `None` while the restaurant has no storefront
    /// (ADR-20260728-011344). Registration never sets it; `RestaurantSlugConfigured` does. This is
    /// what the activation gate reads — `SlugNotConfigured` is decidable from the fold alone.
    pub slug: Option<Slug>,
    /// Display name, carried into rejection contexts (errors.yaml `restaurantName`).
    pub display_name: RestaurantDisplayName,
    /// External idempotent import key, when seeded from an external source.
    pub r#ref: Option<ExternalReference>,

    // --- The editable LOCATION fields (ADR-20260728-011344) ---
    //
    // These are folded for ONE reason: so the aggregate can answer "does this incoming registry
    // record actually change anything?" WITHOUT consulting the read model. That question used to be
    // asked of the `Restaurant` projection through an unindexed JSONB containment scan, once per
    // staged SIRET; now it is a pure comparison against the fold. The set is exactly the fields
    // `RestaurantUpdated` can carry — if that event grows a field, it belongs here too, or an
    // inbound registry change to it would be silently judged a no-op.
    /// Contact details (phone/email) as last recorded.
    pub contact: Option<RestaurantContact>,
    /// Public website, when known.
    pub website: Option<WebUrl>,
    /// Free-form tags. Vec rather than Option: the event carries `#[serde(default)]`, so "absent"
    /// and "empty" are indistinguishable on the wire (see [`replaced_vec`]).
    pub tags: Vec<Tag>,
    /// Commission margin override, when set.
    pub margin_rate: Option<MarginPercent>,
    /// Cuisine classification, when known.
    pub cuisine_category: Option<CuisineCategory>,
    /// Uber-prices opt-in flag, when set.
    pub uber_prices_opt_in: Option<bool>,
    /// Postal address — required at registration, so never absent once the aggregate exists.
    pub address: Address,
    /// Geocoded position, when known.
    pub location: Option<GeoPoint>,
    /// IANA timezone — the clock every opening-hours decision must use.
    pub timezone: Option<TimeZone>,
    /// Declared preparation time in minutes, when known.
    pub preparation_time_minutes: Option<i64>,
    /// Weekly opening slots. Vec for the same reason as [`Self::tags`].
    pub opening_hours: Vec<OpeningHoursSlot>,
}

/// Replace-semantics helper for the array fields of `RestaurantUpdated`.
///
/// The spec marks `tags`/`openingHours` nullable, but the emitter renders them as `Vec` with
/// `#[serde(default)]` — so an omitted array and an explicitly-empty one arrive identically. We take
/// the least destructive reading: a non-empty array replaces, an empty one means "not provided".
///
/// Residual (ADR-20260728-011344): this makes "clear every tag" inexpressible through
/// `RestaurantUpdated`. Fixing it needs the array fields to become genuinely optional in the spec and
/// the emitter — out of scope here, and recorded rather than silently worked around.
fn replaced_vec<T: Clone>(current: &[T], incoming: &[T]) -> Vec<T> {
    if incoming.is_empty() {
        current.to_vec()
    } else {
        incoming.to_vec()
    }
}

/// Decide what an inbound REGISTRY registration changes about a restaurant we already hold
/// (ADR-20260728-011344 D4).
///
/// This is the decision the SIRENE path used to delegate to a Postgres unique-constraint violation.
/// The registry ACL stages `RestaurantRegistered` unconditionally — it never branches — and the
/// aggregate answers one of three ways: `None` here means *nothing changed*, so the drain records the
/// delivery as IGNORED and writes no event at all; `Some(update)` means emit `RestaurantUpdated` with
/// exactly the fields that moved.
///
/// **Only fields the report actually carries are considered.** A registry is a PARTIAL source: SIRENE
/// knows an établissement's name, address and NAF, and knows nothing about its website, tags, margin or
/// opening hours. Treating the report's `None` as "clear this field" would let every weekly sweep wipe
/// data the restaurant's own staff had entered — so an absent field means *no opinion*, never *empty*.
/// The arrays follow the same rule for the same reason (empty = no opinion), matching
/// [`replaced_vec`]'s reading on the fold side.
///
/// `address` is required on the event, so it is always compared.
pub fn changes_from_registry(
    state: &RestaurantState,
    reported: &crate::generated::events::RestaurantRegistered,
) -> Option<crate::generated::events::RestaurantUpdated> {
    fn moved<T: PartialEq + Clone>(current: &Option<T>, report: &Option<T>) -> Option<T> {
        match report {
            Some(v) if Some(v) != current.as_ref() => Some(v.clone()),
            _ => None,
        }
    }

    let display_name =
        (reported.display_name != state.display_name).then(|| reported.display_name.clone());
    let address = (reported.address != state.address).then(|| reported.address.clone());
    let contact = moved(&state.contact, &reported.contact);
    let website = moved(&state.website, &reported.website);
    let margin_rate = moved(&state.margin_rate, &reported.margin_rate);
    let cuisine_category = moved(&state.cuisine_category, &reported.cuisine_category);
    let uber_prices_opt_in = moved(&state.uber_prices_opt_in, &reported.uber_prices_opt_in);
    let location = moved(&state.location, &reported.location);
    let timezone = moved(&state.timezone, &reported.timezone);
    let preparation_time_minutes =
        moved(&state.preparation_time_minutes, &reported.preparation_time_minutes);
    // Arrays: a non-empty report that differs replaces; empty means the source has no opinion.
    let tags = (!reported.tags.is_empty() && reported.tags != state.tags)
        .then(|| reported.tags.clone())
        .unwrap_or_default();
    let opening_hours = (!reported.opening_hours.is_empty()
        && reported.opening_hours != state.opening_hours)
        .then(|| reported.opening_hours.clone())
        .unwrap_or_default();

    let nothing_moved = display_name.is_none()
        && address.is_none()
        && contact.is_none()
        && website.is_none()
        && margin_rate.is_none()
        && cuisine_category.is_none()
        && uber_prices_opt_in.is_none()
        && location.is_none()
        && timezone.is_none()
        && preparation_time_minutes.is_none()
        && tags.is_empty()
        && opening_hours.is_empty();
    if nothing_moved {
        return None;
    }

    Some(crate::generated::events::RestaurantUpdated {
        restaurant_id: reported.restaurant_id,
        display_name,
        contact,
        website,
        tags,
        margin_rate,
        cuisine_category,
        uber_prices_opt_in,
        address,
        location,
        timezone,
        preparation_time_minutes,
        opening_hours,
    })
}

/// Fold a Restaurant stream (events in version order) into its current state. `None` ⇔ the stream has
/// no `RestaurantRegistered` yet, i.e. the aggregate does not exist.
pub fn fold(events: &[DomainEvent]) -> Option<RestaurantState> {
    events.iter().fold(None, apply)
}

/// Apply one event to the state — a pure transition, total over the whole event union (events not
/// touching the folded fields are no-ops, so a fatter stream never breaks rehydration).
fn apply(state: Option<RestaurantState>, event: &DomainEvent) -> Option<RestaurantState> {
    if let Some(status) = lifecycle::initial(event) {
        if let DomainEvent::RestaurantRegistered(e) = event {
            return Some(RestaurantState {
                status,
                order_acceptance: OrderAcceptanceMode::NORMAL,
                listing_status: e.listing_status,
                listing_claimed: false,
                gbp_order_url: None,
                slug: None,
                display_name: e.display_name.clone(),
                r#ref: e.r#ref.clone(),
                contact: e.contact.clone(),
                website: e.website.clone(),
                tags: e.tags.clone(),
                margin_rate: e.margin_rate.clone(),
                cuisine_category: e.cuisine_category,
                uber_prices_opt_in: e.uber_prices_opt_in,
                address: e.address.clone(),
                location: e.location.clone(),
                timezone: e.timezone.clone(),
                preparation_time_minutes: e.preparation_time_minutes,
                opening_hours: e.opening_hours.clone(),
            });
        }
    }
    let mut s = state?;
    // The recorded fact wins at fold time: `target` maps a lifecycle event to its state regardless
    // of the current one (legality is `transition`'s job at append time).
    if let Some(next) = lifecycle::target(event) {
        s.status = next;
    }
    match event {
        DomainEvent::RestaurantAcceptanceModeChanged(e) => s.order_acceptance = e.mode,
        // Replace semantics on the fields actually provided (events.yaml: `*Updated` replaces).
        // `None` = not provided, so it must NEVER clear a currently-set value — otherwise a partial
        // update would silently wipe fields the caller never mentioned.
        DomainEvent::RestaurantUpdated(e) => {
            if let Some(name) = &e.display_name {
                s.display_name = name.clone();
            }
            if e.contact.is_some() {
                s.contact = e.contact.clone();
            }
            if e.website.is_some() {
                s.website = e.website.clone();
            }
            s.tags = replaced_vec(&s.tags, &e.tags);
            if e.margin_rate.is_some() {
                s.margin_rate = e.margin_rate.clone();
            }
            if e.cuisine_category.is_some() {
                s.cuisine_category = e.cuisine_category;
            }
            if e.uber_prices_opt_in.is_some() {
                s.uber_prices_opt_in = e.uber_prices_opt_in;
            }
            if let Some(address) = &e.address {
                s.address = address.clone();
            }
            if e.location.is_some() {
                s.location = e.location.clone();
            }
            if e.timezone.is_some() {
                s.timezone = e.timezone.clone();
            }
            if e.preparation_time_minutes.is_some() {
                s.preparation_time_minutes = e.preparation_time_minutes;
            }
            s.opening_hours = replaced_vec(&s.opening_hours, &e.opening_hours);
        }
        // The storefront address lifecycle. `Reconfigured` also carries `previousSlug`, but the
        // aggregate does not need to remember the chain — the released labels live in the slug
        // reservation table (uniqueness) and the alias read model (redirects), both of which are fed
        // from the event. The fold only needs the CURRENT label.
        DomainEvent::RestaurantSlugConfigured(e) => s.slug = Some(e.slug.clone()),
        DomainEvent::RestaurantSlugReconfigured(e) => s.slug = Some(e.slug.clone()),
        DomainEvent::RestaurantListingClaimed(_) => s.listing_claimed = true,
        DomainEvent::RestaurantListingStatusChanged(e) => s.listing_status = e.listing_status,
        DomainEvent::RestaurantGoogleBusinessProfileOrderLinkConfigured(e) => {
            s.gbp_order_url = Some(e.gbp_order_url.clone())
        }
        _ => {}
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::events::{RestaurantRegistered, RestaurantUpdated};
    use crate::generated::scalars::{AddressLine, CityName, CountryCode, PostalCode, RestaurantId};

    fn address(line1: &str) -> Address {
        Address {
            line1: AddressLine(line1.into()),
            line2: None,
            postal_code: PostalCode("37000".into()),
            city: CityName("Tours".into()),
            country: CountryCode("FR".into()),
        }
    }

    fn registered() -> DomainEvent {
        DomainEvent::RestaurantRegistered(RestaurantRegistered {
            mode: None,
            restaurant_id: RestaurantId(uuid::Uuid::nil()),
            account_id: None,
            listing_status: RestaurantListingStatus::NON_PARTNER,
            r#ref: None,
            external_identifiers: vec![],
            display_name: RestaurantDisplayName("Chez Marco".into()),
            contact: None,
            website: None,
            tags: vec![Tag("bistrot".into())],
            margin_rate: None,
            cuisine_category: None,
            uber_prices_opt_in: None,
            address: address("12 rue Nationale"),
            location: None,
            timezone: Some(TimeZone("Europe/Paris".into())),
            preparation_time_minutes: Some(20),
            opening_hours: vec![],
        })
    }

    /// Registration seeds every folded location field — this is what makes "did the registry record
    /// change anything?" answerable without touching the read model (ADR-20260728-011344).
    #[test]
    fn registration_seeds_the_editable_location_fields() {
        let s = fold(&[registered()]).unwrap();
        assert_eq!(s.address.line1.0, "12 rue Nationale");
        assert_eq!(s.timezone.as_ref().unwrap().0, "Europe/Paris");
        assert_eq!(s.preparation_time_minutes, Some(20));
        assert_eq!(s.tags.len(), 1);
        assert_eq!(s.contact, None);
    }

    /// A partial update replaces ONLY what it carries. A field left `None` must never clear a value
    /// the caller never mentioned — the whole point of replace-on-provided semantics.
    #[test]
    fn a_partial_update_never_clears_unmentioned_fields() {
        let update = DomainEvent::RestaurantUpdated(RestaurantUpdated {
            restaurant_id: RestaurantId(uuid::Uuid::nil()),
            display_name: None,
            contact: None,
            website: None,
            tags: vec![],
            margin_rate: None,
            cuisine_category: None,
            uber_prices_opt_in: None,
            address: Some(address("3 place Plumereau")),
            location: None,
            timezone: None,
            preparation_time_minutes: None,
            opening_hours: vec![],
        });
        let s = fold(&[registered(), update]).unwrap();
        // The provided field moved...
        assert_eq!(s.address.line1.0, "3 place Plumereau");
        // ...and nothing else did, including the arrays (empty = not provided, see `replaced_vec`).
        assert_eq!(s.display_name.0, "Chez Marco");
        assert_eq!(s.timezone.as_ref().unwrap().0, "Europe/Paris");
        assert_eq!(s.preparation_time_minutes, Some(20));
        assert_eq!(s.tags, vec![Tag("bistrot".into())]);
    }

    /// The property the whole inbound-event design rests on: a re-delivered UNCHANGED registration
    /// produces no update, so the drain can record it IGNORED and write nothing.
    #[test]
    fn an_unchanged_registry_report_changes_nothing() {
        let state = fold(&[registered()]).unwrap();
        let DomainEvent::RestaurantRegistered(reported) = registered() else { unreachable!() };
        assert_eq!(changes_from_registry(&state, &reported), None);
    }

    /// A real INSEE rename must reach the domain — the failure this whole change exists to fix was that
    /// it silently did not.
    #[test]
    fn a_renamed_establishment_produces_an_update() {
        let state = fold(&[registered()]).unwrap();
        let DomainEvent::RestaurantRegistered(mut reported) = registered() else { unreachable!() };
        reported.display_name = RestaurantDisplayName("Chez Marco et Fils".into());
        let update = changes_from_registry(&state, &reported).expect("a rename is a change");
        assert_eq!(update.display_name.unwrap().0, "Chez Marco et Fils");
        // Only what moved is carried: everything else stays `None` so the fold leaves it alone.
        assert_eq!(update.address, None);
        assert_eq!(update.timezone, None);
    }

    /// The data-loss guard. A registry is a PARTIAL source: SIRENE has no idea about a website or tags,
    /// so its `None` must never be read as "clear it". Without this, every weekly sweep would wipe
    /// whatever the restaurant's own staff had entered.
    #[test]
    fn a_partial_report_never_clears_fields_the_source_knows_nothing_about() {
        // Staff have since added a website and richer tags.
        let mut state = fold(&[registered()]).unwrap();
        state.website = Some(WebUrl("https://chez-marco.fr".into()));
        state.tags = vec![Tag("bistrot".into()), Tag("terrasse".into())];

        // SIRENE re-reports the same établissement, knowing nothing of either.
        let DomainEvent::RestaurantRegistered(mut reported) = registered() else { unreachable!() };
        reported.website = None;
        reported.tags = vec![];

        // Nothing moved -- the sweep is a no-op, and the staff's data survives.
        assert_eq!(changes_from_registry(&state, &reported), None);
    }

    /// The comparison the SIRENE inbound path relies on: folded state equals the record it came from,
    /// so a re-delivered unchanged registration is decidably a no-op.
    #[test]
    fn refolding_the_same_registration_is_stable() {
        assert_eq!(fold(&[registered()]), fold(&[registered(), registered()]));
    }
}
