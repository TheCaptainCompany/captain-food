//! API authentication + role authorization for the role-as-path GraphQL endpoints (ADR-0047, realizing the
//! deferred guard of ADR-0006 over ADR-0015 Supabase Auth).
//!
//! `/public/graphql` is open. Every other role path requires a valid Supabase **JWT**
//! (`Authorization: Bearer <token>`), verified against the project's **public** keys fetched from
//! `SUPABASE_JWKS_URL` — the signing secret never touches this server. Missing/invalid token on a
//! non-public path ⇒ `401`; if the verifier is unconfigured or JWKS is unreachable we **fail closed**
//! (`503`) rather than allow.
//!
//! **What a token must PROVE (#519), in order — every step is necessary and none is skippable:**
//! 1. **signature**, against a key from our JWKS, asymmetric only;
//! 2. **`iss`**, equal to `{SUPABASE_URL}/auth/v1`. Mandatory: with no issuer configured there is no
//!    [`Verifier`] at all and the path answers `503`. There is no "skip the check" state left to be
//!    in — see [`Verifier`] for why that is a type property rather than a branch;
//! 3. **`aud`**, `authenticated` — a shape check only. Every Supabase user of every project carries
//!    it, so it is evidence of nothing about who minted the token;
//! 4. **`app_metadata.captain_food`**, present, carrying a role this product recognises. This is the
//!    only separator that survives ONE identity project serving several products of a group: at that
//!    point `iss`, `aud` and the signing key are identical across siblings, and a token with no
//!    Captain Food claims is a stranger's — refused (`403`), never defaulted to CUSTOMER;
//! 5. the granted role must **equal the path role**, else `403`.
//!
//! **Open is not credential-blind (#469).** `/public` also READS whatever credential the request
//! carries, because the storefront IS the open path (`web::router` pins the customer surfaces to
//! `Role::Public`): without that, an identified customer arrives as `ReadScope::Public` and
//! `cart.current`'s claim leg is unreachable from a browser. It is the ONLY path that degrades
//! instead of refusing — invalid, expired, absent-verifier and non-CUSTOMER credentials all serve
//! the anonymous view with a `200` (see [`AuthContext::public_principal`]) — and it grants at most
//! the CUSTOMER identity: a staff token there is anonymous, never elevated.
//!
//! Security notes: the verification algorithm is taken from the matched **JWK** (not the attacker-controlled
//! header) and restricted to asymmetric families, closing the classic `alg`-confusion hole (an attacker
//! can't downgrade to HS256 and sign with the public key).

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use jsonwebtoken::{
    decode, decode_header,
    jwk::{Jwk, JwkSet, KeyAlgorithm},
    Algorithm, DecodingKey, Validation,
};
use serde::Deserialize;
use tokio::sync::RwLock;
use tracing::Instrument;

use crate::graphql::acl::RequestRole;

/// How long a fetched JWKS is trusted before a refresh (key rotation is also handled by a forced refetch
/// when a token's `kid` is not in the cached set).
const JWKS_TTL: Duration = Duration::from_secs(3600);
/// Hard ceiling on ONE JWKS fetch (see [`jwks_client`]): the open path now verifies credentials, so
/// an unbounded fetch would be an unbounded storefront request.
const JWKS_FETCH_TIMEOUT: Duration = Duration::from_secs(3);
/// After a FAILED fetch, no other request re-attempts one for this long (#469 review round 2, peak
/// risk). Without it, a Supabase blip at Friday 19:00 costs EVERY cookie-carrying storefront
/// request the full [`JWKS_FETCH_TIMEOUT`] before it degrades — a 3 s tax on the whole storefront,
/// repeated per request. With it, one request pays and everyone else degrades instantly, so the
/// outage costs a lost identity (a cart that reads anonymous) rather than a lost dinner service.
const JWKS_FAILURE_BACKOFF: Duration = Duration::from_secs(10);
/// Minimum spacing between two ROTATION-driven refetches (an unknown `kid`). A `kid` is
/// attacker-supplied on the open path, so "unknown kid ⇒ refetch" is an unauthenticated request
/// amplifier: one forged token, one outbound JWKS fetch. Rotation still absorbs within this window
/// — the price is that a token signed with a brand-new key can be refused for at most this long
/// after the key appears, which is bounded and self-healing; the alternative is not.
const JWKS_ROTATION_REFETCH_MIN_INTERVAL: Duration = Duration::from_secs(5);
/// Supabase issues user tokens with this audience — and so does every OTHER Supabase project, for
/// every one of its users. It is a shape check, not a proof of anything: keep validating it (a token
/// that is not a user token is still refused), but never treat it as evidence about WHICH product or
/// environment minted the token. That job belongs to [`Verifier::issuer`] and, once one identity
/// project serves several products, to [`ProductClaims`] (#519).
const SUPABASE_AUDIENCE: &str = "authenticated";

// The `app_metadata` key holding THIS product's claims is `captain_food`, declared once by the ACL
// that WRITES it (`infrastructure::integrations::supabase_auth::PRODUCT_CLAIM_KEY`). Supabase merges
// `app_metadata` SHALLOWLY (`specs/services.yaml`), which is why the claims are NESTED under one
// product-owned object rather than renamed as a set of flat `captain_*` keys: the merge sees one key,
// and it is this one.
//
// `serde` needs a literal for the field name, so the reader spells it out and its agreement with the
// writer's constant is proved by `tests::the_verifier_reads_what_the_claim_stamp_writes` — the two
// sit in different crates with no shared type, and a transposition between them would otherwise
// first surface as a production smoke timeout.

/// The authenticated caller injected into the GraphQL context: ONE private field, an [`Identity`],
/// with the role and its domain binding travelling together.
///
/// **Why a wrapper and not a bag of fields** (reviewer round 2 on #469): the previous shape was
/// `pub struct Principal { pub role, pub customer_id, … }`, and a struct literal is not a
/// constructor — `Principal { role: RequestRole::Customer, customer_id: None, .. }` compiled
/// inside AND outside this crate, so the "escalation is unspellable" claim documented on
/// [`Principal::public_customer`] constrained exactly one helper and nothing else. The field is
/// private and every constructor is `pub(crate)`, so the type — not a review, not a doc comment —
/// is what makes the illegal states illegal.
#[derive(Clone, Debug)]
pub struct Principal {
    identity: Identity,
}

/// Who the caller IS, as ONE value. A role never travels without the claim that gives it meaning,
/// so "role says CUSTOMER, claim absent" is not a field combination anybody can spell by accident:
/// it is [`Identity::Unbound`], a NAMED state that reads as what it is — an authenticated caller on
/// a ROLE path whose token carries no domain binding, i.e. the population
/// `read_authorization_bridge_unresolved_total` exists to count.
///
/// Module-private on purpose: the whole point is that the variants are reachable only through the
/// four constructors below, which are the only places a claim becomes an identity.
///
/// Tenant claims (#144/#433) are verified with the rest of the token — the login-to-domain bridge
/// lives in JWT claims for EVERY role (ADR-20260809-050000 CARD-11; product-owner correction on
/// #430: "this information is provided in the jwt"). The claim IS the domain id; the auth subject
/// stays `sub` (PROP-20260725-185140 §3.2.1 — a binding, not a subject swap). No per-request lookup
/// resolves read scope anywhere.
///
/// KNOWN LIMITATION (proposal §6.4, register row "claim staleness", open by explicit decision):
/// a claim is frozen until token refresh. For customers/riders the only transition is
/// null -> set at mint time, so the #429 bearer-token item carries the BLOCKING precondition
/// that the client obtains its token AFTER the claim stamp (or forces one refresh) — otherwise
/// the first paid session is the one denied its tracking screen.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Identity {
    /// No credential — or one that did not survive the open path's verification. Reads exactly what
    /// an anonymous browser reads.
    Anonymous,
    /// A machine caller on `/external`: the pre-shared service token (no subject) or a Supabase
    /// token whose `captain_food.role` is EXTERNAL.
    External { sub: Option<String> },
    /// Platform staff. ADMIN carries no domain binding — its scope IS the role.
    Admin { sub: String },
    Customer { sub: String, customer_id: uuid::Uuid },
    Restaurant { sub: String, restaurant_id: uuid::Uuid },
    RestaurantAccount { sub: String, restaurant_account_id: uuid::Uuid },
    /// A RIDER the request seam RESOLVED — the `Rider` read model answered a row for this subject
    /// (#639 part C step 2b). The SUBJECT and nothing else, by construction: the rider's domain id
    /// is carried by the `ReadScope` the same resolution returned, never by a claim, so there is
    /// no field here for a claim to bind into. **The only producer is the seam**
    /// ([`resolve_rider_scope`]; [`Principal::role_binding`] mints one for tests) — the token
    /// verifier ([`Principal::role_path`]) yields [`Identity::Unbound`] for every RIDER token,
    /// because a token cannot prove a binding it never carries. That is what makes an
    /// `ActingRole(Rider)` unspellable for a subject with no row: the identity that mints it does
    /// not exist until Postgres said so (the #849 re-presentation — as first pushed, this variant
    /// was minted at the verifier, and a bare `role: RIDER` token acted RIDER while reading
    /// `Public`).
    Rider { sub: String },
    /// Verified on a ROLE path, with no domain binding. For CUSTOMER / RESTAURANT /
    /// RESTAURANT_ACCOUNT: the token carries no usable `captain_*` claim for that role (absent, or
    /// malformed — indistinguishable by design). For RIDER: EVERY verified token, until the seam
    /// binds it — a rider's binding is a Postgres row, never a claim, and a caller the seam did
    /// not resolve (no row, or the seam could not answer) stays here. Denies everything scoped,
    /// on both halves: acts as PUBLIC ([`ActingRole::of`]) and records PUBLIC
    /// ([`Principal::recorded_role`]). Counted as a provisioning gap by [`read_scope`] when reached
    /// as a claims question. Unreachable from `/public`, which degrades such a caller to
    /// [`Identity::Anonymous`] instead (see [`AuthContext::public_principal`]).
    Unbound { sub: String, role: RequestRole },
}

/// The [`ActingRole`] witness and its ONE producer, in a child module for the same reason
/// [`fetch_intent`] is: **privacy is MODULE-scoped, not type-scoped**. A private field declared
/// beside `Principal` would still leave `ActingRole(RequestRole::Restaurant)` spellable everywhere
/// in `auth`, including in the very function whose mistake this type exists to make impossible.
/// Down here the tuple field is out of scope at every call site in the file, and
/// [`ActingRole::of`] — which owns the [`Identity`] match itself — is the only door.
mod acting_role {
    use super::{Identity, RequestRole};

    /// **The only value a role guard accepts**: the role a verified caller may ACT as, as opposed
    /// to the role their token merely asserts or their URL path merely names.
    ///
    /// **What it buys, concretely** (#639 part B, ADR-20260818-004646 Correction 3): the guard on
    /// `approveRefund` was a membership test on the PATH role, which is attacker-chosen text that
    /// any code can re-derive — so a token asserting `captain_food.role = "RESTAURANT"` and
    /// carrying no `restaurant_id` reached the money path and could approve ANY pending refund.
    /// An `ActingRole` cannot be spelled at all without an [`Identity`] in front of it, and
    /// [`ActingRole::of`]'s [`Identity::Unbound`] arm yields PUBLIC. The privileged value does not
    /// exist for that caller: the refusal is a property of the type, not of a check.
    ///
    /// Deliberately NOT `Default`, NOT `From<RequestRole>`, and with no public constructor and no
    /// test-only one — each would be exactly the escape hatch this type exists to close. A test
    /// that needs one builds the [`super::Principal`] it belongs to
    /// ([`super::Principal::role_binding`]) and asks it, which is also the only honest way to
    /// assert the unbound case.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ActingRole(RequestRole);

    impl ActingRole {
        /// Mint the acting role of an identity on a given path. `pub(super)` so the whole `auth`
        /// module can reach it, which costs nothing: every caller must already hold an `Identity`,
        /// and the arm that decides is right here.
        ///
        /// `path_role` decides for every bound identity rather than the identity's own role, and
        /// that is deliberate: the ACL is PATH authorization (ADR-0006), and on `/public` an
        /// identified customer ([`super::Principal::public_customer`]) must still evaluate as
        /// PUBLIC — otherwise every operation with `roles: [CUSTOMER]` becomes reachable, and
        /// introspectable, from the one path anyone can reach. For every other path
        /// [`super::AuthContext::authorize`] has already checked the granted role equals the path
        /// role, so the two agree by the time we get here.
        ///
        /// The match is exhaustive on purpose: a new [`Identity`] variant is `rustc` E0004 here, so
        /// nobody can add an identity that silently inherits the "may act" answer.
        pub(super) fn of(identity: &Identity, path_role: RequestRole) -> Self {
            match identity {
                // The ONE arm that cannot act. Verified on a role path, no domain binding — so
                // there is no restaurant, rider or account it could be acting FOR, and every
                // scoped operation would resolve its target from the request payload instead of
                // from the caller. It degrades to PUBLIC rather than erroring, and the posture is
                // ONE per caller because it is decided ONCE — by the seam's resolution
                // (`super::resolve_read_scope`), which hands back the principal whose identity IS
                // its outcome: an unresolved rider is `Unbound`, a resolved one is `Rider`. The
                // read half (the `ReadScope` returned beside it) and the write half (this witness,
                // minted from that principal AFTER the seam answered, and `recorded_role`, stamped
                // from the same principal) both read that one outcome. The #849 re-presentation
                // restored this: as first pushed the witness was minted from `Identity::Rider`
                // BEFORE the seam ran, so a bare `role: RIDER` token with no row read `Public` and
                // acted RIDER.
                Identity::Unbound { .. } => ActingRole(RequestRole::Public),
                Identity::Anonymous
                | Identity::External { .. }
                | Identity::Admin { .. }
                | Identity::Customer { .. }
                | Identity::Restaurant { .. }
                | Identity::RestaurantAccount { .. }
                | Identity::Rider { .. } => ActingRole(path_role),
            }
        }

        /// The role this caller may act as. Read-only: handing the inner role out lets callers
        /// COMPARE it, never mint one.
        pub fn get(self) -> RequestRole {
            self.0
        }
    }
}
pub use acting_role::ActingRole;

impl Principal {
    /// The unauthenticated PUBLIC identity — also what the SSR page renderer executes reads as
    /// (#92: a document GET carries no credentials, so server-side data resolution is by
    /// construction the anonymous view).
    pub(crate) fn anonymous() -> Self {
        Self { identity: Identity::Anonymous }
    }

    /// A machine caller that presented a valid pre-shared `X-External-Api-Key` — authenticated, but
    /// no Supabase subject exists for it.
    pub(crate) fn external_service() -> Self {
        Self { identity: Identity::External { sub: None } }
    }

    /// The identified CUSTOMER on the OPEN path (#469): a verified `captain_auth` cookie / bearer
    /// presented to `/public/graphql`, which the storefront is pinned to (`web::router` serves the
    /// customer surfaces as `Role::Public`). Before #469 the open path skipped credential reading
    /// entirely, so `cart.current`'s claim leg could never fire from a browser.
    ///
    /// **Escalation is unspellable here by CONSTRUCTION**: the only identity this constructor can
    /// produce is [`Identity::Customer`], which has room for the customer claim and for nothing
    /// else — there is no `restaurant_id` to set, correctly or otherwise. A token carrying
    /// `captain_food.restaurant_id` cannot leak a `ReadScope::Restaurant` onto the one path everyone
    /// can reach, whatever the caller sends, and a non-CUSTOMER role never reaches this
    /// constructor at all (see [`AuthContext::public_principal`], which degrades it to
    /// [`Principal::anonymous`]). The principal's role is who the caller IS; the per-field ACL keeps
    /// running against the PATH role (`RequestRole::Public`), injected separately, so this widens no
    /// field's visibility — only the row scope of reads the open path already served.
    ///
    /// `customer_id` is **not optional** (reviewer S3), and now the type agrees: a CUSTOMER without
    /// its domain claim resolves to `ReadScope::Public` anyway — it serves no cart, matches no
    /// ownership and fires no leg — so on the open path that is not a weaker identity, it is a
    /// DEGRADE, handled as one (`claim_absent`). [`Identity::Unbound`] is the state that spells it,
    /// and this path cannot reach it.
    pub(crate) fn public_customer(sub: String, customer_id: uuid::Uuid) -> Self {
        Self { identity: Identity::Customer { sub, customer_id } }
    }

    /// The verified principal of a ROLE path (`/customer`, `/restaurant`, `/rider`, …): the claim
    /// that MATCHES the path role becomes the identity, and every other claim in the token is
    /// dropped rather than carried along. A `/restaurant` token's `captain_food.customer_id` was never
    /// read by anything — now it cannot even be held.
    ///
    /// `path_role` has already been checked equal to the token's granted role by
    /// [`AuthContext::authorize`]. `Public` cannot arrive here (the open path returns before), and
    /// maps to the anonymous identity if it ever did — fail closed, never a silent elevation.
    ///
    /// Module-private (not even `pub(crate)`): it takes THIS PRODUCT's verified claim object, and
    /// the only thing that yields one is [`AppMetadata::grant`] — so a caller cannot reach this
    /// constructor with a token that proved no Captain Food role (#519).
    fn role_path(path_role: RequestRole, sub: String, claims: &ProductClaims) -> Self {
        let bind = |claim: &Option<String>, f: fn(String, uuid::Uuid) -> Identity| match claim_uuid(
            claim,
        ) {
            Some(id) => f(sub.clone(), id),
            None => Identity::Unbound { sub: sub.clone(), role: path_role },
        };
        let identity = match path_role {
            RequestRole::Admin => Identity::Admin { sub },
            RequestRole::External => Identity::External { sub: Some(sub) },
            RequestRole::Customer => bind(&claims.customer_id, |sub, customer_id| {
                Identity::Customer { sub, customer_id }
            }),
            RequestRole::Restaurant => bind(&claims.restaurant_id, |sub, restaurant_id| {
                Identity::Restaurant { sub, restaurant_id }
            }),
            RequestRole::RestaurantAccount => {
                bind(&claims.restaurant_account_id, |sub, restaurant_account_id| {
                    Identity::RestaurantAccount { sub, restaurant_account_id }
                })
            }
            // No `bind`, and no `Identity::Rider` either: a RIDER token proves the subject and the
            // role, and the domain binding is the request seam's to resolve (#639 part C step 2b)
            // — so at the verifier a rider is UNBOUND, exactly like a RESTAURANT token with no
            // `restaurant_id`, and only the seam's `Resolved` outcome upgrades it
            // (`resolve_rider_scope`). `ProductClaims` has no rider field at all, so a
            // `captain_food.rider_id` key in a token is a stranger's key.
            RequestRole::Rider => Identity::Unbound { sub, role: path_role },
            RequestRole::Public => Identity::Anonymous,
        };
        Self { identity }
    }

    /// The Supabase `sub` — the AUTH subject, never a domain identity (#433). `None` for an
    /// anonymous caller and for a service-token EXTERNAL one.
    pub fn user_id(&self) -> Option<&str> {
        match &self.identity {
            Identity::Anonymous => None,
            Identity::External { sub } => sub.as_deref(),
            Identity::Admin { sub }
            | Identity::Customer { sub, .. }
            | Identity::Restaurant { sub, .. }
            | Identity::RestaurantAccount { sub, .. }
            | Identity::Rider { sub, .. }
            | Identity::Unbound { sub, .. } => Some(sub),
        }
    }

    /// The verified role this caller IS — DERIVED from the identity, so it can never disagree with
    /// the claim beside it. This is the role the mutation envelope stamps into
    /// `domain_events.user_type` (ADR-0041: the acting user is envelope metadata).
    ///
    /// **Not the same question as [`acting_role`](Self::acting_role), and the two must not be
    /// merged.** This one answers *"whose act was this?"* and follows the IDENTITY, which is why
    /// it takes no path: an identified customer on `/public` — the storefront's own path (#469) —
    /// records CUSTOMER, while the ACL evaluates them as PUBLIC. `acting_role` answers *"what may
    /// they do?"* and follows the PATH. Using either for the other's job is a regression in one
    /// direction and a hole in the other.
    ///
    /// **Why Unbound records as PUBLIC** (#639 part B): a token asserting RESTAURANT with no
    /// `restaurant_id` is an authenticated stranger. Stamping *"RESTAURANT did this"* on its
    /// commands writes a **false author into an immutable log** — worse than the authorization hole
    /// it accompanies, because the log is what we would later reason from and events are never
    /// rewritten. The declared role survives where it is a DIAGNOSIS rather than an attribution:
    /// [`read_scope`] destructures `Identity::Unbound { role, .. }` itself to label
    /// `read_authorization_bridge_unresolved_total{role}`, so the provisioning gap stays
    /// attributable without anything downstream believing the role.
    ///
    /// Note for anyone reading `domain_events`: PUBLIC in `user_type` no longer implies a NULL
    /// `user_id`. An Unbound caller keeps its auth subject (that is honest — it did authenticate)
    /// and records PUBLIC, so the pair `(user_type = PUBLIC, user_id = <sub>)` now means *"a
    /// credential proved no usable role"*. `specs/common/scalars.yaml#/UserType` carries the same
    /// correction.
    pub fn recorded_role(&self) -> RequestRole {
        match &self.identity {
            Identity::Anonymous => RequestRole::Public,
            Identity::External { .. } => RequestRole::External,
            Identity::Admin { .. } => RequestRole::Admin,
            Identity::Customer { .. } => RequestRole::Customer,
            Identity::Restaurant { .. } => RequestRole::Restaurant,
            Identity::RestaurantAccount { .. } => RequestRole::RestaurantAccount,
            Identity::Rider { .. } => RequestRole::Rider,
            Identity::Unbound { .. } => RequestRole::Public,
        }
    }

    /// **The [`ActingRole`] the role guards run against.** The whole decision lives in
    /// [`ActingRole::of`], in the child module that owns the type; this is the door onto it, and
    /// the only reason it is `pub` is that `routes.rs` and the tests hold a `Principal`, not an
    /// `Identity`.
    pub fn acting_role(&self, path_role: RequestRole) -> ActingRole {
        ActingRole::of(&self.identity, path_role)
    }

    /// Whether this caller's login-to-domain bridge RESOLVED — the `auth.read_scope` span's
    /// `bridge_resolved` attribute (#451).
    ///
    /// **It takes BOTH the identity and the scope that was actually resolved**, because neither
    /// alone answers the question. The form this replaced was
    /// `scope != Public || role == Public || role == External`, evaluated at the call site: correct
    /// while `role()` reported an Unbound caller's declared role, and silently **always true** the
    /// moment it stopped (#639 part B) — turning the one population the attribute exists to surface
    /// into a healthy reading. Restating it purely on the identity fixed that and broke the other
    /// end: a CUSTOMER under `CustomerIdentitySource::Postgres` whose lookup returns `NoMapping` or
    /// `LookupFailed` **is** a bound identity, and degrades to `Public` anyway (#641) — so an
    /// identity-only predicate reports the seam's own outage as resolved, and `LookupFailed` is the
    /// PAGE-classed one. Asking both questions is the only form that is right at both ends.
    pub fn bridge_resolved(&self, scope: &application::queries::ReadScope) -> bool {
        match &self.identity {
            // Nothing to resolve: their scope IS their role (ADMIN), or they have none by design.
            Identity::Anonymous | Identity::External { .. } | Identity::Admin { .. } => true,
            // A binding was presented — did it actually resolve to a scope? Under the default
            // claim path this is a foregone `true`; under Postgres resolution it is the real
            // question, and `Public` here means the seam said no or could not answer.
            Identity::Customer { .. }
            | Identity::Restaurant { .. }
            | Identity::RestaurantAccount { .. }
            | Identity::Rider { .. } => {
                !matches!(scope, application::queries::ReadScope::Public)
            }
            // No binding was presented at all: nothing could have resolved.
            Identity::Unbound { .. } => false,
        }
    }

    /// Build a verified principal from a role and its domain binding — the same match
    /// [`AuthContext::authorize`] runs, exposed for tests and for any future non-HTTP driver that
    /// must present a caller to the schema.
    ///
    /// `binding: None` yields [`Identity::Unbound`], which is the point: the unbound case is the
    /// one a guard must refuse, so it has to be spellable in a test. What is NOT spellable through
    /// here — or anywhere — is a *lying* principal: a bound identity carrying no binding, or a
    /// binding that disagrees with the role. The identity stays one private value and this
    /// constructor cannot assemble a pair the real path would reject.
    pub fn role_binding(role: RequestRole, sub: String, binding: Option<uuid::Uuid>) -> Self {
        // RIDER: the binding is REAL but never a claim — it is a Postgres row the request seam
        // resolves (#639 part C step 2b), so `role_path` cannot mint a bound rider and this
        // constructor stands in for the seam's outcome instead: `Some(_)` is the identity the seam
        // returns for a row (the id itself lives in the `ReadScope` a test injects beside it),
        // `None` is the unbound caller a guard must refuse — same contract as every other role.
        if role == RequestRole::Rider {
            let identity = match binding {
                Some(_) => Identity::Rider { sub },
                None => Identity::Unbound { sub, role },
            };
            return Self { identity };
        }
        let binding = binding.map(|id| id.to_string());
        let claims = match role {
            RequestRole::Customer => ProductClaims { customer_id: binding, ..Default::default() },
            RequestRole::Restaurant => {
                ProductClaims { restaurant_id: binding, ..Default::default() }
            }
            RequestRole::RestaurantAccount => {
                ProductClaims { restaurant_account_id: binding, ..Default::default() }
            }
            // ADMIN, EXTERNAL and PUBLIC carry no domain binding — their arms in `role_path` ignore
            // the claim object entirely, so an argument here would be dropped, not honoured. RIDER
            // returned above.
            RequestRole::Admin
            | RequestRole::External
            | RequestRole::Public
            | RequestRole::Rider => ProductClaims::default(),
        };
        Self::role_path(role, sub, &claims)
    }
}

/// Why authorization failed, mapped to an HTTP status at the edge.
#[derive(Debug)]
pub enum AuthError {
    /// No/!malformed/invalid token on a non-public path.
    Unauthorized,
    /// Valid token, but its role is not permitted for this path.
    Forbidden,
    /// Auth cannot be performed (JWKS not configured or unreachable) — fail closed.
    Unavailable,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        match self {
            AuthError::Unauthorized => {
                (StatusCode::UNAUTHORIZED, "unauthorized: valid bearer token required").into_response()
            }
            AuthError::Forbidden => {
                (StatusCode::FORBIDDEN, "forbidden: token role not permitted for this path").into_response()
            }
            AuthError::Unavailable => {
                (StatusCode::SERVICE_UNAVAILABLE, "auth unavailable").into_response()
            }
        }
    }
}

/// Only the claims we consume. Reserved claims (`exp`/`aud`/`iss`) are validated by `jsonwebtoken` from the
/// raw payload via [`Validation`], so they need not appear here.
#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    #[serde(default)]
    app_metadata: AppMetadata,
}

/// The provider's `app_metadata` bag, of which we read exactly ONE key (#519).
///
/// Everything else in there belongs to the provider or to a sibling product of the group, and this
/// type deliberately cannot see it: a claim that is not inside [`PRODUCT_CLAIM_KEY`](infrastructure::integrations::supabase_auth::PRODUCT_CLAIM_KEY) is not a claim
/// about us. That is also why the pre-#519 flat `captain_*` keys need no read-side tolerance —
/// there is nowhere for them to land.
#[derive(Debug, Default, Deserialize)]
struct AppMetadata {
    /// **The positive product proof.** Absent ⇒ the token belongs to some other product (or predates
    /// the nesting), and no amount of valid signature, issuer or audience turns it into a principal
    /// here. The field name IS the wire key [`PRODUCT_CLAIM_KEY`](infrastructure::integrations::supabase_auth::PRODUCT_CLAIM_KEY).
    #[serde(default)]
    captain_food: Option<ProductClaims>,
}

/// Captain Food's own claims — the object Supabase's shallow `app_metadata` merge treats as ONE
/// value, so a write of ours replaces it wholesale and a sibling product's write cannot reach into
/// it. Server-controlled (the admin-key stamp, `identity.stamp_customer_claim`), never user-editable,
/// which is what makes them trustworthy without a per-request database lookup.
#[derive(Debug, Default, Deserialize, Clone, PartialEq, Eq)]
struct ProductClaims {
    /// The role this token acts as. Parsed by [`parse_role`], which FAILS CLOSED: absent or
    /// unrecognised is no role at all, never a CUSTOMER baseline.
    #[serde(default)]
    role: Option<String>,
    /// The single location a RESTAURANT principal acts for (#144).
    #[serde(default)]
    restaurant_id: Option<String>,
    /// The account a RESTAURANT_ACCOUNT principal acts for (#144) — a chain's manager reaches every
    /// location under it. This is why the two roles exist as distinct UserTypes rather than one
    /// restaurant role with a multiplicity problem.
    #[serde(default)]
    restaurant_account_id: Option<String>,
    /// The CUSTOMER's domain id (#433) — stamped when the Customer is registered/resolved
    /// (verifyPhone). Replaces the per-request `auth_ref` bridge for read scope; a token without it
    /// fails closed to Public.
    #[serde(default)]
    customer_id: Option<String>,
    // Deliberately NO `rider_id` (#639 part C step 2b): a rider's binding lives in the `Rider` read
    // model and is resolved per request at the seam (ADR-20260818-004646 — no business identifier
    // in the identity provider). Not parsing the key is what makes "bind the claim" unspellable:
    // there is no field for it to arrive in. `serde` ignores unknown keys, so a token carrying one
    // is inert rather than refused.
}

/// What a verified token PROVES about this product: a role that parsed, travelling with the claim
/// object it came from. There is no constructor that yields a grant without a role, so
/// "authenticated, role unknown, treat as customer" is not a value anybody downstream can be handed
/// — [`Principal::role_path`] takes a `&ProductClaims` that only [`AppMetadata::grant`] produces.
struct Grant<'a> {
    role: RequestRole,
    claims: &'a ProductClaims,
}

impl AppMetadata {
    /// The ONE place a verified token becomes a grant, and the only gate between `/{role}/graphql`
    /// and an authenticated principal. `None` means the token proves nothing about Captain Food:
    /// either it carries no [`PRODUCT_CLAIM_KEY`](infrastructure::integrations::supabase_auth::PRODUCT_CLAIM_KEY) object (a sibling product's user under a shared
    /// identity project, or a pre-#519 flat-claims token), or the object carries no role we
    /// recognise. Both are refusals, never defaults.
    fn grant(&self) -> Option<Grant<'_>> {
        let claims = self.captain_food.as_ref()?;
        let role = parse_role(claims.role.as_deref()?)?;
        Some(Grant { role, claims })
    }
}

struct CachedJwks {
    set: JwkSet,
    fetched: Instant,
}

/// The [`FetchIntent`] witness, in a module of its own so that Rust's MODULE-scoped privacy does
/// the enforcing: `formed_at` is out of scope at every call site in `auth`, so neither the struct
/// literal nor a late mint is spellable there. See the type's doc for the invariant.
mod fetch_intent {
    use super::CachedJwks;
    use std::time::Instant;
    use tokio::sync::RwLock;

    /// A witness that the decision to fetch was formed BEFORE the cache read that justified it.
    ///
    /// `AuthContext::refresh`'s single-flight test is "someone else's fetch completed after this
    /// intent formed: it is our fetch" — which is only sound if the intent's instant predates the
    /// read that concluded a fetch was needed. #683 was exactly that ordering minted wrong (the
    /// instant taken after the read), and it looked like a flake for a week.
    ///
    /// Three shapes converged here, each retired by a review round on #692/#693:
    /// a free `formed_now()` with a "MUST call before the read" doc line (spellable wherever the
    /// call sites live); then `decide(read)` over an opaque async closure (the struct literal was
    /// still spellable in `auth` — privacy is MODULE-scoped, not type-scoped — and nothing forced
    /// the read to happen inside the closure). Now BOTH doors are the type's own: the struct
    /// lives in this child module, so `formed_at` is out of scope at every call site, and
    /// [`FetchIntent::decide`] owns the cache read itself — the caller hands over a SYNCHRONOUS
    /// predicate, which cannot `await` a read of its own before the instant exists. The
    /// `#[cfg(test)]` constructor is the one deliberate forgery point, for building #683's
    /// ordering deterministically from `auth::tests`.
    pub(super) struct FetchIntent {
        formed_at: Instant,
    }

    impl FetchIntent {
        /// Form the intent, THEN run the deciding read over the cache. Returns `Some` when the
        /// predicate says a fetch is needed (`true`), carrying an instant that provably predates
        /// the read the predicate saw.
        pub(super) async fn decide(
            cache: &RwLock<Option<CachedJwks>>,
            needed: impl FnOnce(Option<&CachedJwks>) -> bool,
        ) -> Option<Self> {
            let intent = Self { formed_at: Instant::now() };
            needed(cache.read().await.as_ref()).then_some(intent)
        }

        pub(super) fn formed_at(&self) -> Instant {
            self.formed_at
        }

        /// Tests only: mint a witness at an arbitrary moment, to construct #683's ordering
        /// directly.
        #[cfg(test)]
        pub(super) fn formed_now() -> Self {
            Self { formed_at: Instant::now() }
        }
    }
}
use fetch_intent::FetchIntent;

/// **The verification contract, as ONE value** (#519): the JWKS endpoint that supplies the signing
/// keys AND the issuer those keys are trusted to have signed for.
///
/// Both halves are `String`, not `Option<String>`, and the pair is constructed only by
/// [`Verifier::new`]. That is the whole point: BEFORE #519 the issuer was an `Option` derived from a
/// possibly-empty `SUPABASE_URL`, and `verify` read it as *"`Some` ⇒ check it, `None` ⇒ skip"* — so
/// the fail-open configuration was not a bug in a branch, it was a state the type permitted. A
/// verifier that skips issuer validation is now unspellable: there is no `None` to take that branch,
/// [`Verifier::validation`] is the only `Validation` this module builds, and it always sets both the
/// issuer and the audience. With the configuration absent, the whole verifier is absent and every
/// role path answers `503` — refusing, not skipping.
struct Verifier {
    jwks_url: String,
    /// `{SUPABASE_URL}/auth/v1` — the `iss` a token must carry. Necessary and, under a group-wide
    /// identity project, NOT sufficient: sibling products share it, which is why
    /// [`AppMetadata::grant`] exists.
    issuer: String,
}

impl Verifier {
    /// The only constructor: both halves present and non-empty, or no verifier at all. Empty is
    /// unset — a resolved `Config` supplies `""` for a key with no baked value and no env override.
    fn new(jwks_url: String, supabase_url: String) -> Option<Self> {
        let jwks_url = Some(jwks_url).filter(|s| !s.is_empty())?;
        let issuer = Some(supabase_url)
            .filter(|s| !s.is_empty())
            .map(|u| format!("{}/auth/v1", u.trim_end_matches('/')))?;
        Some(Self { jwks_url, issuer })
    }

    /// The only [`Validation`] built in this module, so "forgot to set the issuer" is not an edit a
    /// call site can make.
    ///
    /// **In `jsonwebtoken`, MATCHING a reserved claim does not REQUIRE it** — and getting that
    /// backwards is how an issuer check becomes a no-op (review round 1 on #519; the previous
    /// version of this comment asserted the opposite and was wrong). In the pinned `10.3.0`:
    /// `Validation::new` seeds `required_spec_claims` with `{"exp"}` alone (`validation.rs:112-115`);
    /// `set_issuer`/`set_audience` only assign the matcher (`:143-145`); and `validate()`'s `iss`
    /// and `aud` arms both end in `_ => {}` (`:308-320`, `:325-349`), so an ABSENT claim — or a
    /// non-string one, which deserializes to `TryParse::FailedToParse` — falls through and passes
    /// **vacuously**. The crate documents it: *"Validation only happens if `iss` claim is present in
    /// the token."*
    ///
    /// So the requirement is **derived from the matchers we actually set**, not written out beside
    /// them: adding a matcher without requiring it is not a pair anyone can spell here, and removing
    /// one keeps the two in step. `required_spec_claims` demands `TryParse::Parsed`, so the same
    /// line covers the retyped-claim road as well as the absent-claim one
    /// (`validation.rs:258-272`). Pinned by
    /// `tests::every_reserved_claim_the_verifier_matches_is_also_required` on the produced value and
    /// end-to-end by `a_token_that_omits_or_retypes_iss_or_aud_is_refused_not_passed_vacuously`.
    fn validation(&self, alg: Algorithm) -> Validation {
        let mut validation = Validation::new(alg);
        validation.set_audience(&[SUPABASE_AUDIENCE]);
        validation.set_issuer(&[self.issuer.as_str()]);

        // `exp` is the library's own default and is validated by time rather than matched; every
        // OTHER reserved claim is required exactly when we matched it.
        let mut required = vec!["exp"];
        if validation.iss.is_some() {
            required.push("iss");
        }
        if validation.aud.is_some() {
            required.push("aud");
        }
        validation.set_required_spec_claims(&required);
        validation
    }
}

/// Verifier state: the JWKS endpoint + expected issuer, an HTTP client, the cached key set,
/// and the pre-shared EXTERNAL service tokens (machine callers to `/external`).
pub struct AuthContext {
    /// `None` ⇒ this process cannot verify a token at all: `/public` degrades to anonymous and every
    /// role path returns `503`. It is one field rather than two because a JWKS URL without an issuer
    /// is not a weaker verifier, it is a verifier that cannot tell one project's tokens from
    /// another's.
    verifier: Option<Verifier>,
    /// Pre-shared secrets for EXTERNAL machine callers (Stripe/HubRise/Avelo37 ACLs), presented via the
    /// `X-External-Api-Key` header. Loaded from `EXTERNAL_API_TOKENS` (comma-separated). Empty ⇒ no
    /// service-token access to `/external` (a Supabase JWT with a Captain Food EXTERNAL role still works).
    external_tokens: Vec<String>,
    http: reqwest::Client,
    cache: RwLock<Option<CachedJwks>>,
    /// Single-flight gate: at most ONE JWKS fetch is in flight per process. Everything else that
    /// wants fresh keys queues here and takes the winner's result. Before this, the hourly TTL
    /// boundary let every concurrent request fetch independently — a self-inflicted burst at
    /// exactly the moment the storefront is busiest.
    refresh_lock: tokio::sync::Mutex<()>,
    /// When the last fetch FAILED (negative cache, [`JWKS_FAILURE_BACKOFF`]). `None` = no failure
    /// standing.
    last_failure: RwLock<Option<Instant>>,
}

impl AuthContext {
    /// Build from the **resolved** configuration: `jwks_url` (public keys) and `supabase_url` (used to
    /// derive the expected `iss = {SUPABASE_URL}/auth/v1`) come from the generated `Config`, which applies
    /// precedence env > baked profile > default (ADR-20260729-020000). With no JWKS URL, only `/public`
    /// works; other paths return `503`.
    ///
    /// These two are **non-secret baked** config: on the deployed service they live inside the image, NOT
    /// in the process environment. Reading them straight from `std::env` here (as this did) made every
    /// authenticated path fail closed with `503 "auth unavailable"` in production, where the JWKS URL is
    /// baked into the digest, not set as a Render env var — the same trap `263f2a2` fixed for the smoke
    /// script. `EXTERNAL_API_TOKENS` stays an env read: it is a **secret**, delivered by CI into the
    /// service environment, and carries no baked value.
    ///
    /// **Both are now required together** (#519): `SUPABASE_URL` is not an optional refinement of
    /// `SUPABASE_JWKS_URL`, it is half of the same contract, and both are already
    /// `required: [staging, production]` in `specs/common/configuration.yaml`.
    pub fn from_config(jwks_url: String, supabase_url: String) -> Arc<Self> {
        let verifier = Verifier::new(jwks_url, supabase_url);
        if verifier.is_none() {
            tracing::warn!(
                "SUPABASE_JWKS_URL and/or SUPABASE_URL resolved empty -- token verification is \
                 DISABLED and non-public GraphQL paths will return 503 (fail closed). An unset \
                 issuer never means 'skip the issuer check' (#519)."
            );
        }
        let external_tokens: Vec<String> = std::env::var("EXTERNAL_API_TOKENS")
            .ok()
            .map(|s| s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect())
            .unwrap_or_default();
        Arc::new(Self {
            verifier,
            external_tokens,
            http: jwks_client(),
            cache: RwLock::new(None),
            refresh_lock: tokio::sync::Mutex::new(()),
            last_failure: RwLock::new(None),
        })
    }

    /// Authorize a request for `path_role`. `/public` is always allowed and NEVER fails — it reads
    /// whatever credential is present and degrades to anonymous ([`Self::public_principal`]); every
    /// other path requires a valid bearer token whose `captain_food.role` equals `path_role`.
    pub async fn authorize(&self, path_role: RequestRole, headers: &HeaderMap) -> Result<Principal, AuthError> {
        if path_role == RequestRole::Public {
            return Ok(self.public_principal(headers).await);
        }
        // EXTERNAL machine callers (Stripe/HubRise/Avelo37 ACLs) present a pre-shared service token via
        // the `X-External-Api-Key` header instead of a user JWT. If a key header is present it is
        // authoritative (valid → allow, invalid → reject); if absent we fall through to JWT verification,
        // so a Supabase user carrying a Captain Food EXTERNAL role still works.
        if path_role == RequestRole::External {
            if let Some(key) = headers.get("x-external-api-key").and_then(|v| v.to_str().ok()) {
                return if self.external_key_valid(key) {
                    Ok(Principal::external_service())
                } else {
                    Err(AuthError::Unauthorized)
                };
            }
        }
        let session_token = token(headers).ok_or(AuthError::Unauthorized)?;
        let claims = self.verify(session_token).await?;
        // A verified token is not yet OUR token (#519). Under one identity project per product group
        // the signature, the issuer and the audience are all identical across siblings, so the grant
        // — the presence of a `captain_food` object carrying a role we recognise — is the separator.
        // Absent ⇒ 403: the credential is genuine, it simply is not a credential for this product,
        // and re-authenticating would not change that (which is what a 401 would invite).
        let Some(grant) = claims.app_metadata.grant() else {
            return Err(AuthError::Forbidden);
        };
        if role_permitted(path_role, grant.role) {
            Ok(Principal::role_path(path_role, claims.sub, grant.claims))
        } else {
            Err(AuthError::Forbidden)
        }
    }

    /// The OPEN path's identity resolution (#469): attempt verification, **degrade to anonymous on
    /// every failure**. Returns a `Principal`, never a `Result` — `/public` cannot 401, 403 or 503,
    /// whatever arrives, because a stale cookie is the COMMON case and a JWKS outage must not take
    /// anonymous browsing down with it. Friday 19:00 with zero orders from an auth dependency the
    /// storefront never had is the failure this shape exists to prevent.
    ///
    /// Three degradations, all counted (`public_credential_degraded_total{reason}`) so the
    /// degradation is visible rather than silent:
    /// - **the credential does not verify** (absent-kid, expired, tampered, JWKS unreachable) —
    ///   `invalid_token` / `verifier_unavailable`;
    /// - **the credential verifies but is not a CUSTOMER** — `role_not_customer`. An ADMIN /
    ///   RESTAURANT / RESTAURANT_ACCOUNT / RIDER token on the open path is anonymous, NOT elevated:
    ///   "role = path" (ADR-0047) is the whole reason staff must present their token to their own
    ///   path, and elevating here would turn a dead claim leg into privilege escalation on the one
    ///   path anyone can reach. Staff lose nothing — their surfaces talk to `/restaurant`,
    ///   `/rider`, `/admin`, which are unchanged.
    /// - **the CUSTOMER token carries no `captain_food.customer_id`** — `claim_absent` (reviewer S3).
    ///   This is the KNOWN LIMITATION recorded on [`Principal`]: a token minted BEFORE the claim
    ///   stamp, i.e. every signed-in customer for one token lifetime after rollout. Such a caller
    ///   resolves to `ReadScope::Public` whichever way it is spelled — no cart, no ownership match,
    ///   no leg — so it is a degrade, and it is counted as one HERE rather than falling through to
    ///   `read_scope`'s `read_authorization_bridge_unresolved_total`. That counter's contract says
    ///   it is *"a provisioning gap or staleness — never ordinary user denial"*, and a normal
    ///   rollout bumping it on EVERY storefront GraphQL request would read to an operator as an
    ///   incident: exactly the misreading `public_credential_degraded_total` exists to prevent.
    ///   `bridge_unresolved` keeps its meaning — an authenticated caller on a ROLE path who is
    ///   consequently DENIED something — which is why the counter is re-routed rather than the
    ///   contract text widened.
    ///
    /// A request with NO credential at all short-circuits before any I/O: anonymous browsing costs
    /// exactly what it cost before this change (no JWKS fetch, no verification).
    async fn public_principal(&self, headers: &HeaderMap) -> Principal {
        let Some(session_token) = token(headers) else {
            return Principal::anonymous();
        };
        let claims = match self.verify(session_token).await {
            Ok(claims) => claims,
            Err(e) => {
                telemetry::meters::read_authorization::public_credential_degraded(match e {
                    AuthError::Unavailable => "verifier_unavailable",
                    _ => "invalid_token",
                });
                return Principal::anonymous();
            }
        };
        // `role_not_customer` covers the whole population "the credential does not prove a Captain
        // Food CUSTOMER" — a staff token, AND (since #519) a token that proves no Captain Food role
        // at all: no `captain_food` object, or one whose role we do not recognise. From `/public`'s
        // point of view these are one outcome and one action (serve the anonymous view), and folding
        // them keeps the contract's `reason` set bounded as declared. Telling a sibling product's
        // token apart from our own staff's in telemetry is #517's job, not this counter's.
        let Some(grant) = claims.app_metadata.grant().filter(|g| g.role == RequestRole::Customer)
        else {
            telemetry::meters::read_authorization::public_credential_degraded("role_not_customer");
            return Principal::anonymous();
        };
        // No domain claim (or a malformed one) = nothing this path can act on: serve anonymous and
        // count it here, so the pre-claim-stamp window is a visible degrade rather than a
        // provisioning-gap alarm on every storefront request.
        let Some(customer_id) = claim_uuid(&grant.claims.customer_id) else {
            telemetry::meters::read_authorization::public_credential_degraded("claim_absent");
            return Principal::anonymous();
        };
        Principal::public_customer(claims.sub, customer_id)
    }

    /// Verify a JWT's signature (asymmetric, key + algorithm from the JWKS) and reserved claims.
    ///
    /// The [`Verifier`] is taken FIRST: with none configured there is nothing to verify against, so
    /// this returns `Unavailable` (`503` on a role path, a counted degrade on `/public`) rather than
    /// verifying a token with part of the contract switched off.
    async fn verify(&self, token: &str) -> Result<Claims, AuthError> {
        let verifier = self.verifier.as_ref().ok_or(AuthError::Unavailable)?;
        let header = decode_header(token).map_err(|_| AuthError::Unauthorized)?;
        let kid = header.kid.ok_or(AuthError::Unauthorized)?;
        let jwk = self.key_for(&kid).await?;
        let alg = asymmetric_alg(&jwk, header.alg).ok_or(AuthError::Unauthorized)?;
        let key = DecodingKey::from_jwk(&jwk).map_err(|_| AuthError::Unauthorized)?;

        decode::<Claims>(token, &key, &verifier.validation(alg))
            .map(|data| data.claims)
            .map_err(|_| AuthError::Unauthorized)
    }

    /// Find the JWK for `kid`, refreshing the cache if stale or if the key is unknown (rotation).
    ///
    /// **Serve-stale-on-refresh-failure**: if the set is past its TTL but the refresh fails (a transient
    /// JWKS outage), keep using the cached keys rather than locking everyone out — signing keys rotate
    /// rarely, so the TTL is a freshness hint, not a hard expiry. A genuinely unknown `kid` still forces a
    /// refetch and **fails closed** if it can't be resolved.
    ///
    /// **Bounded at peak** (#469 review round 2): both refetch paths go through the single-flight
    /// [`Self::refresh`], and the rotation path additionally refuses to refetch more often than
    /// [`JWKS_ROTATION_REFETCH_MIN_INTERVAL`]. The `kid` is attacker-supplied on the open path, so
    /// an unthrottled "unknown kid ⇒ fetch" would let an anonymous caller drive one outbound
    /// request per inbound one.
    async fn key_for(&self, kid: &str) -> Result<Jwk, AuthError> {
        if let Some(intent) = self.stale().await {
            if let Err(e) = self.refresh(intent).await {
                // Fail closed only when there is nothing cached to fall back to (cold cache).
                if self.cache.read().await.is_none() {
                    return Err(e);
                }
                // Otherwise carry on with the stale-but-present keys.
            }
        }
        if let Some(jwk) = self.lookup(kid).await {
            return Ok(jwk);
        }
        // Unknown kid (e.g. a just-rotated key): absorb the rotation with ONE refetch, but only if
        // the set we are holding is old enough to plausibly predate it. Keys fetched seconds ago do
        // not gain a member by asking again — that request would exist purely because someone sent
        // us a `kid` we never issued. This is a NEW decision, so it mints its own intent — reusing
        // the one from the top would treat any fetch since then as ours.
        let Some(intent) = self.rotation_refetch_due().await else {
            return Err(AuthError::Unauthorized);
        };
        self.refresh(intent).await?;
        self.lookup(kid).await.ok_or(AuthError::Unauthorized)
    }

    /// Is the cache stale (or absent)? `Some` means "a fetch is needed" and carries the witness
    /// `refresh` requires; `None` means the cache answers.
    async fn stale(&self) -> Option<FetchIntent> {
        FetchIntent::decide(&self.cache, |c| !matches!(c, Some(c) if c.fetched.elapsed() <= JWKS_TTL)).await
    }

    /// May an UNKNOWN kid trigger a refetch? Only if the cached set is older than the rotation
    /// interval (or there is none at all). `Some` carries the witness, as in [`Self::stale`].
    async fn rotation_refetch_due(&self) -> Option<FetchIntent> {
        FetchIntent::decide(&self.cache, |c| {
            !matches!(c, Some(c) if c.fetched.elapsed() < JWKS_ROTATION_REFETCH_MIN_INTERVAL)
        })
        .await
    }

    async fn lookup(&self, kid: &str) -> Option<Jwk> {
        self.cache.read().await.as_ref().and_then(|c| c.set.find(kid).cloned())
    }

    /// Fetch the key set — **single-flight, with a negative cache**.
    ///
    /// Callers that arrive while a fetch is running queue on `refresh_lock` and then take that
    /// fetch's result (`fetched > arrived`) instead of issuing their own: N concurrent requests at
    /// the TTL boundary cost ONE outbound fetch, not N. A fetch that FAILED silences the next
    /// [`JWKS_FAILURE_BACKOFF`] of attempts, so a JWKS outage costs one request the timeout and
    /// costs everyone else nothing — the difference between a degraded storefront and a 3-s-per-
    /// request storefront on a Friday evening.
    /// The `intent` is a WITNESS, not a parameter anyone may mint: production code can only get
    /// one from [`FetchIntent::decide`], whose body captures the instant and THEN runs the
    /// deciding cache read — so "the instant was taken after the read that decided to fetch"
    /// (#683) is unspellable rather than guarded by a comment.
    ///
    /// The instant used to be `Instant::now()` taken here, just above the lock -- i.e. AFTER
    /// `key_for`'s own `stale()` / `rotation_refetch_due()` read. A task that read stale on the
    /// cold cache and was then descheduled while the first fetch completed re-entered with its
    /// instant LATER than `c.fetched`, failed the test below, and issued a SECOND fetch. Fifty
    /// callers on four worker threads reach that window on a loaded runner
    /// (`concurrent_cold_requests_cost_exactly_one_jwks_fetch` failed twice in CI with `left: 2`)
    /// and never on an idle one, which is why it read as a flake (#683). The first repair passed a
    /// bare `Instant` from the caller — correct, but only by every call site reading a comment;
    /// the review of #684 asked for the witness (CLAUDE.md compiler-first, level 4 is the floor).
    ///
    /// TAKING THE CALLER'S INTENT, rather than testing "is the cache fresh now", is
    /// deliberate: a blanket `c.fetched.elapsed() <= JWKS_TTL` here would ALSO short-circuit the
    /// ROTATION refetch, because `JWKS_ROTATION_REFETCH_MIN_INTERVAL` is 5s against a 3600s TTL --
    /// an unknown `kid` on a cache 10 seconds old would pass `rotation_refetch_due()` and then be
    /// refused a fetch, so a just-rotated key would 401 every caller for the rest of the hour. The
    /// two callers ask different questions; only the arrival instant answers both.
    async fn refresh(&self, intent: FetchIntent) -> Result<(), AuthError> {
        let arrived = intent.formed_at();
        let url = self.verifier.as_ref().map(|v| v.jwks_url.as_str()).ok_or(AuthError::Unavailable)?;
        let _flight = self.refresh_lock.lock().await;
        // Someone else's fetch completed after we FORMED THE INTENT to fetch: it IS our fetch.
        if matches!(&*self.cache.read().await, Some(c) if c.fetched > arrived) {
            return Ok(());
        }
        // A failure is still standing: fail immediately rather than pay the timeout again.
        if matches!(*self.last_failure.read().await, Some(at) if at.elapsed() < JWKS_FAILURE_BACKOFF)
        {
            return Err(AuthError::Unavailable);
        }
        let fetched = async {
            let response = self.http.get(url).send().await.map_err(|_| AuthError::Unavailable)?;
            response.json::<JwkSet>().await.map_err(|_| AuthError::Unavailable)
        }
        .await;
        match fetched {
            Ok(set) => {
                *self.cache.write().await = Some(CachedJwks { set, fetched: Instant::now() });
                *self.last_failure.write().await = None;
                Ok(())
            }
            Err(e) => {
                *self.last_failure.write().await = Some(Instant::now());
                Err(e)
            }
        }
    }
}

impl AuthContext {
    /// True when `presented` matches a configured EXTERNAL service token, compared in constant time.
    fn external_key_valid(&self, presented: &str) -> bool {
        self.external_tokens.iter().any(|t| ct_eq(t.as_bytes(), presented.as_bytes()))
    }
}

/// The JWKS fetch client — **bounded**, deliberately (#469). Key refresh is a once-an-hour call on
/// a request's critical path, and since the open path started verifying credentials that path is
/// the STOREFRONT's. An unreachable-but-not-refusing JWKS host (a black-holed TCP connection) would
/// otherwise hang every cookie-carrying public request for as long as the socket stays open; the
/// timeout converts that into the honest `verifier_unavailable` degrade to anonymous, in seconds.
fn jwks_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(JWKS_FETCH_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Constant-time byte equality — no early return on the first differing byte, so a matching-prefix key
/// can't be discovered by timing. (Length is allowed to differ observably, as is standard for API keys.)
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Extract the `Authorization: Bearer <token>` value.
fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// The auth cookie name minted by `POST /auth/session` (#112, PROP-20260724-150500) — the httpOnly
/// carrier of the provider access JWT. MUST stay in sync with the value the auth routes set.
pub const AUTH_COOKIE: &str = "captain_auth";

/// The verified session token, from the `Authorization: Bearer` header OR the `captain_auth` cookie
/// (#112). The header wins when both are present (an explicit bearer is a deliberate override); the
/// cookie is the browser's carrier — same-origin `fetch`, the WS upgrade, and SSR all send it
/// automatically, which is exactly why one fallback here lights all three (the issue's core move).
fn token(headers: &HeaderMap) -> Option<&str> {
    bearer(headers).or_else(|| cookie_value(headers, AUTH_COOKIE))
}

/// Read one cookie value from the `Cookie` header (`a=1; b=2`). Borrowed slice — no allocation.
fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(axum::http::header::COOKIE).and_then(|v| v.to_str().ok()).and_then(|raw| {
        raw.split(';').map(str::trim).find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            (k.trim() == name).then(|| v.trim()).filter(|t| !t.is_empty())
        })
    })
}

/// Resolve the algorithm from the matched JWK (falling back to the header only for asymmetric families).
/// Restricting to asymmetric algorithms defeats `alg`-confusion (no HS* downgrade against a public key).
fn asymmetric_alg(jwk: &Jwk, header_alg: Algorithm) -> Option<Algorithm> {
    let from_jwk = jwk.common.key_algorithm.and_then(key_alg_to_alg);
    let alg = from_jwk.unwrap_or(header_alg);
    is_asymmetric(alg).then_some(alg)
}

fn key_alg_to_alg(k: KeyAlgorithm) -> Option<Algorithm> {
    Some(match k {
        KeyAlgorithm::RS256 => Algorithm::RS256,
        KeyAlgorithm::RS384 => Algorithm::RS384,
        KeyAlgorithm::RS512 => Algorithm::RS512,
        KeyAlgorithm::ES256 => Algorithm::ES256,
        KeyAlgorithm::ES384 => Algorithm::ES384,
        KeyAlgorithm::EdDSA => Algorithm::EdDSA,
        KeyAlgorithm::PS256 => Algorithm::PS256,
        KeyAlgorithm::PS384 => Algorithm::PS384,
        KeyAlgorithm::PS512 => Algorithm::PS512,
        _ => return None,
    })
}

fn is_asymmetric(alg: Algorithm) -> bool {
    !matches!(alg, Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512)
}

/// Map a `captain_food.role` claim to a role — **failing closed** (#519). Unknown, empty or
/// unrecognised is `None`: NO role, never a CUSTOMER baseline.
///
/// The old catch-all (`_ => CUSTOMER`, called "the least-privilege authenticated baseline") was
/// least-privilege only among roles we issue. It is not a baseline at all for a token from a
/// DIFFERENT product of the group, which carries no role of ours and would have landed on
/// `/customer/graphql` as an authenticated customer. `PUBLIC` is deliberately absent from the table:
/// the open path is reached without a credential, never granted by one.
fn parse_role(s: &str) -> Option<RequestRole> {
    Some(match s.trim().to_ascii_uppercase().as_str() {
        "ADMIN" => RequestRole::Admin,
        "CUSTOMER" => RequestRole::Customer,
        "RESTAURANT" => RequestRole::Restaurant,
        "RESTAURANT_ACCOUNT" => RequestRole::RestaurantAccount,
        "RIDER" => RequestRole::Rider,
        "EXTERNAL" => RequestRole::External,
        _ => return None,
    })
}

/// A caller granted `granted` may act on the `path_role` path. Strict equality: an ADMIN token must use
/// `/admin`, not `/customer`. `Public` is handled before this (open), so it never reaches here.
fn role_permitted(path_role: RequestRole, granted: RequestRole) -> bool {
    path_role == granted
}

#[cfg(test)]
mod tests {
    use infrastructure::integrations::supabase_auth::PRODUCT_CLAIM_KEY;

    use super::*;

    #[test]
    fn claim_maps_to_role_and_anything_else_maps_to_nothing() {
        assert_eq!(parse_role("ADMIN"), Some(RequestRole::Admin));
        assert_eq!(parse_role("admin"), Some(RequestRole::Admin));
        assert_eq!(parse_role("RESTAURANT_ACCOUNT"), Some(RequestRole::RestaurantAccount));
        assert_eq!(parse_role("EXTERNAL"), Some(RequestRole::External));
        // #519: fail CLOSED. The old catch-all was `_ => CUSTOMER`, which turned every unrecognised
        // string — including one from a product that has never heard of us — into a customer.
        assert_eq!(parse_role("nonsense"), None);
        assert_eq!(parse_role(""), None);
        assert_eq!(parse_role("  "), None);
        assert_eq!(parse_role("PUBLIC"), None, "the open path is reached, never granted");
    }

    #[test]
    fn role_gate_is_strict_equality() {
        assert!(role_permitted(RequestRole::Admin, RequestRole::Admin));
        assert!(role_permitted(RequestRole::Customer, RequestRole::Customer));
        // An ADMIN token cannot use the /customer path, and vice-versa.
        assert!(!role_permitted(RequestRole::Customer, RequestRole::Admin));
        assert!(!role_permitted(RequestRole::Admin, RequestRole::Customer));
        assert!(!role_permitted(RequestRole::Rider, RequestRole::Restaurant));
    }

    #[test]
    fn hs_algorithms_are_rejected_asymmetric_kept() {
        assert!(is_asymmetric(Algorithm::RS256));
        assert!(is_asymmetric(Algorithm::ES256));
        assert!(!is_asymmetric(Algorithm::HS256));
    }

    #[test]
    fn bearer_is_parsed_case_insensitively() {
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, "Bearer abc.def.ghi".parse().unwrap());
        assert_eq!(bearer(&h), Some("abc.def.ghi"));
        h.insert(AUTHORIZATION, "bearer  xyz ".parse().unwrap());
        assert_eq!(bearer(&h), Some("xyz"));
        h.insert(AUTHORIZATION, "Basic zzz".parse().unwrap());
        assert_eq!(bearer(&h), None);
    }

    #[test]
    fn token_falls_back_to_the_auth_cookie_and_the_header_wins() {
        // #112: the cookie carries the session when no bearer is present — the one seam that lights
        // HTTP, the WS upgrade and SSR (all send cookies automatically).
        let mut h = HeaderMap::new();
        h.insert(axum::http::header::COOKIE, "other=1; captain_auth=jwt.from.cookie; x=2".parse().unwrap());
        assert_eq!(token(&h), Some("jwt.from.cookie"));
        // An explicit bearer overrides the cookie (deliberate override).
        h.insert(AUTHORIZATION, "Bearer jwt.from.header".parse().unwrap());
        assert_eq!(token(&h), Some("jwt.from.header"));
        // Neither present → none.
        let empty = HeaderMap::new();
        assert_eq!(token(&empty), None);
        // A cookie header without our cookie → none.
        let mut h2 = HeaderMap::new();
        h2.insert(axum::http::header::COOKIE, "session=abc".parse().unwrap());
        assert_eq!(token(&h2), None);
    }

    /// A single-key JWKS (dummy RSA material — enough to parse + `find(kid)`, not to verify a signature).
    fn test_set() -> JwkSet {
        serde_json::from_str(
            r#"{"keys":[{"kty":"RSA","use":"sig","kid":"test-kid","alg":"RS256","n":"0vx7agoebGcQSuuPiLJXZptN","e":"AQAB"}]}"#,
        )
        .expect("parse test JWKS")
    }

    /// An `Instant` far enough in the past to be stale. LOUD on a host whose monotonic clock is
    /// younger than 2×TTL: the old `unwrap_or_else(Instant::now)` fallback silently INVERTED the
    /// fixture (a "stale" cache that is actually fresh), and its doc claimed the assertions stayed
    /// valid — false for any test that needs staleness to fire. A panic with a legible message
    /// beats a fixture that asserts the opposite of its name (review of #692).
    fn stale_instant() -> Instant {
        Instant::now()
            .checked_sub(JWKS_TTL * 2)
            .expect("host uptime exceeds twice the JWKS TTL; on a fresh-boot host this fixture cannot be constructed honestly")
    }

    /// A verifier whose JWKS fetch can only FAIL: loopback port 9 (discard) refuses instantly and
    /// resolves no DNS, so `refresh()` is a local no-op error. It replaces the pre-#519
    /// `jwks_url: None`, which expressed the same intent by way of a state the type no longer has —
    /// and which was also how every fixture ended up issuer-blind.
    fn unfetchable_verifier() -> Verifier {
        Verifier::new("http://127.0.0.1:9/jwks".into(), TEST_SUPABASE_URL.into())
            .expect("both halves present")
    }

    fn ctx_with_cache(set: JwkSet, fetched: Instant) -> AuthContext {
        AuthContext {
            // A refresh therefore fails — proving we never hit the network on a cache hit — while the
            // ISSUER is real, so these fixtures verify `iss` exactly as production does.
            verifier: Some(unfetchable_verifier()),
            external_tokens: Vec::new(),
            http: reqwest::Client::new(),
            cache: RwLock::new(Some(CachedJwks { set, fetched })),
            refresh_lock: tokio::sync::Mutex::new(()),
            last_failure: RwLock::new(None),
        }
    }

    fn ctx_with_external(tokens: &[&str]) -> AuthContext {
        AuthContext {
            verifier: None,
            external_tokens: tokens.iter().map(|t| t.to_string()).collect(),
            http: reqwest::Client::new(),
            cache: RwLock::new(None),
            refresh_lock: tokio::sync::Mutex::new(()),
            last_failure: RwLock::new(None),
        }
    }

    #[test]
    fn from_config_uses_its_arguments_not_env() {
        // Regression guard (prod-smoke L4 503 "auth unavailable", 2026-08-01): SUPABASE_JWKS_URL /
        // SUPABASE_URL are non-secret BAKED config (ADR-20260729-020000) — present in the resolved
        // `Config`, absent from the deployed service's env. The verifier must take them from that
        // resolved config, never from `std::env` directly, or every authenticated path fails closed.
        let ctx = AuthContext::from_config(
            "https://example.test/jwks.json".into(),
            "https://proj.supabase.co/".into(),
        );
        let v = ctx.verifier.as_ref().expect("both halves present -> a verifier");
        assert_eq!(v.jwks_url, "https://example.test/jwks.json");
        // issuer is derived from supabase_url, trailing slash trimmed.
        assert_eq!(v.issuer, "https://proj.supabase.co/auth/v1");

        // #519: the two halves are ONE contract. Either one missing ⇒ no verifier at all, so every
        // role path fails closed — an issuer-less verifier is not a state this type can be in.
        for (case, jwks, url) in [
            ("both empty", "", ""),
            ("no JWKS URL", "", "https://proj.supabase.co"),
            ("no SUPABASE_URL", "https://example.test/jwks.json", ""),
        ] {
            let ctx = AuthContext::from_config(jwks.into(), url.into());
            assert!(ctx.verifier.is_none(), "{case}: must yield no verifier (fail closed)");
        }
    }

    #[test]
    fn ct_eq_matches_only_identical_bytes() {
        assert!(ct_eq(b"s3cret-key", b"s3cret-key"));
        assert!(!ct_eq(b"s3cret-key", b"s3cret-keZ"));
        assert!(!ct_eq(b"s3cret-key", b"s3cret-ke")); // differing length
    }

    #[tokio::test]
    async fn external_service_key_authorizes_only_the_external_path() {
        let ctx = ctx_with_external(&["s3cret-key", "second-key"]);
        let mut ok = HeaderMap::new();
        ok.insert("x-external-api-key", "second-key".parse().unwrap());
        assert!(ctx.authorize(RequestRole::External, &ok).await.is_ok(), "valid key must authorize");

        let mut bad = HeaderMap::new();
        bad.insert("x-external-api-key", "wrong".parse().unwrap());
        assert!(ctx.authorize(RequestRole::External, &bad).await.is_err(), "wrong key must fail");

        // The external key is ignored on other paths (they still require a JWT, absent here → 401).
        assert!(ctx.authorize(RequestRole::Admin, &ok).await.is_err(), "external key must not open /admin");
        // /public stays open regardless.
        assert!(ctx.authorize(RequestRole::Public, &HeaderMap::new()).await.is_ok());
    }

    #[tokio::test]
    async fn serve_stale_returns_a_cached_key_when_refresh_fails() {
        // Stale cache + failing refresh (no JWKS URL): a known kid is still served from the cached set.
        let ctx = ctx_with_cache(test_set(), stale_instant());
        assert!(ctx.key_for("test-kid").await.is_ok(), "present key must survive a failed refresh");
    }

    #[tokio::test]
    async fn unknown_kid_still_fails_closed_when_refresh_fails() {
        // A kid we've never cached cannot be served from stale data and cannot be fetched → rejected.
        let ctx = ctx_with_cache(test_set(), stale_instant());
        assert!(ctx.key_for("rotated-unknown-kid").await.is_err(), "unknown key must fail closed");
    }

    /// A JWKS endpoint that COUNTS what it is asked for — the only way to assert fan-out rather
    /// than assume it. `failing` serves `500`s, i.e. the Supabase blip.
    async fn counting_jwks(failing: bool) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let app = axum::Router::new().route(
            "/jwks",
            axum::routing::get(move || {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    if failing {
                        (StatusCode::INTERNAL_SERVER_ERROR, "jwks down").into_response()
                    } else {
                        axum::Json(serde_json::json!({"keys":[{"kty":"EC","crv":"P-256","use":"sig",
                            "kid":"captain-test-es256","alg":"ES256",
                            "x":"baNA5O-X8ZdiMi8fPb8L41FpCQG_6eWyp5PLDXrQ1AQ",
                            "y":"ctUgalejEFEUlU1J4M3pAH0geXquoMtplHiKotq1Jws"}]}))
                        .into_response()
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}/jwks"), hits)
    }

    /// #469 review round 2, PEAK RISK: the storefront — not a handful of staff — is now the caller
    /// of this verifier, so the hourly TTL boundary lands on every concurrent cookie-carrying
    /// request at once. Fifty of them cost ONE outbound fetch, not fifty.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_cold_requests_cost_exactly_one_jwks_fetch() {
        use std::sync::atomic::Ordering;
        let (url, hits) = counting_jwks(false).await;
        let ctx = AuthContext::from_config(url, TEST_SUPABASE_URL.into());

        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..50 {
            let ctx = ctx.clone();
            tasks.spawn(async move { ctx.key_for("captain-test-es256").await.is_ok() });
        }
        while let Some(done) = tasks.join_next().await {
            assert!(done.expect("task joins"), "every caller gets the key");
        }
        assert_eq!(hits.load(Ordering::SeqCst), 1, "single-flight: one fetch serves them all");
    }

    /// #683, DETERMINISTIC: the concurrent test above reaches this only under scheduler pressure
    /// (it failed twice in CI with `left: 2` and never once locally in isolation), so a flake is
    /// what it looks like and a re-run is what it invites. This constructs the ordering directly.
    ///
    /// The single-flight test is `c.fetched > intent.formed_at`. If that instant is captured
    /// INSIDE `refresh` — after the caller's own `stale()` read — then a caller that decided to
    /// fetch on the cold cache and was descheduled while the first fetch landed resumes with an
    /// instant LATER than `fetched`, so the test says "not mine" and it fetches again. The first
    /// fix passed a bare `Instant` from the caller and this test redded its revert by name; since
    /// #691 the ordering is [`FetchIntent::decide`]'s BODY (capture, then read), so the revert is
    /// unspellable in production code — inside this module included, which is where every call
    /// site lives. What this test pins is the SEMANTICS of `refresh`'s comparison: a pre-fetch
    /// intent never pays a second fetch. The mint-vs-read ordering itself is closed by
    /// construction, not by coverage — #692's review judged it untestable deterministically
    /// without a seam (the cache read would have to block while another fetch completes), which
    /// is the argument for the constructor shape.
    #[tokio::test]
    async fn a_caller_whose_intent_predates_the_fetch_does_not_pay_a_second_one() {
        use std::sync::atomic::Ordering;
        let (url, hits) = counting_jwks(false).await;
        let ctx = AuthContext::from_config(url, TEST_SUPABASE_URL.into());

        // The intent forms here, on a cold cache — constructed directly (a liberty only this
        // module's tests have; production code can only get one from `stale()` /
        // `rotation_refetch_due()`, which mint it before their own cache read).
        let intent = FetchIntent::formed_now();

        // …and someone else's fetch lands before we get to run.
        ctx.key_for("captain-test-es256").await.expect("the first caller fetches");
        assert_eq!(hits.load(Ordering::SeqCst), 1, "precondition: exactly one fetch so far");

        // Now the descheduled caller resumes into `refresh`. Its intent predates the fetch that
        // already answered it, so it must take that answer rather than issue its own.
        ctx.refresh(intent).await.expect("refresh");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "#683: a caller whose intent PREDATES the completed fetch must not pay a second one"
        );
    }

    /// The other half of #683, and the reason the fix is an arrival instant rather than a blanket
    /// freshness test: `JWKS_ROTATION_REFETCH_MIN_INTERVAL` is 5s against a 3600s `JWKS_TTL`, so
    /// `if cache.fetched.elapsed() <= JWKS_TTL { return Ok(()) }` inside `refresh` would ALSO
    /// short-circuit the rotation refetch — a just-rotated `kid` would 401 every caller for the
    /// rest of the hour. That was the first shape of this fix, and this case is why it is not the
    /// shipped one.
    #[tokio::test]
    async fn a_rotation_refetch_still_happens_on_a_cache_inside_its_ttl() {
        use std::sync::atomic::Ordering;
        let (url, hits) = counting_jwks(false).await;
        let ctx = AuthContext::from_config(url, TEST_SUPABASE_URL.into());

        ctx.key_for("captain-test-es256").await.expect("warm the cache");
        assert_eq!(hits.load(Ordering::SeqCst), 1, "precondition: one fetch, cache well inside TTL");

        // An unknown kid on a cache that is FRESH by TTL but older than the rotation interval —
        // PLACED there, not waited into. This was `tokio::time::sleep(interval + 50ms)`: a real
        // 5.05 s wall-clock block on every run (the runtime cannot be `start_paused` here — real
        // loopback server, `std::time::Instant`), plus a fudge factor someone would eventually
        // have to defend. Aging the cache is instant, and the review of #684 flagged the irony of
        // this suite growing its first wall-clock dependency in the PR about a timing-shaped test.
        {
            let mut cache = ctx.cache.write().await;
            let c = cache.as_mut().expect("cache was just warmed");
            c.fetched = Instant::now()
                .checked_sub(JWKS_ROTATION_REFETCH_MIN_INTERVAL + Duration::from_secs(1))
                .expect("host uptime exceeds the rotation interval");
        }
        let _ = ctx.key_for("a-kid-we-never-issued").await;
        assert_eq!(
            hits.load(Ordering::SeqCst),
            2,
            "#683: the rotation refetch must still fire on a cache inside its TTL — a blanket freshness short-circuit would swallow it"
        );
    }

    /// The same shape when the JWKS is DOWN: the first caller pays the fetch, the rest degrade
    /// instantly off the negative cache. Without it, a Supabase blip taxes every storefront
    /// request `JWKS_FETCH_TIMEOUT` — 3 s each, at 19:00 on a Friday.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_failed_fetch_is_not_re_attempted_by_the_next_request() {
        use std::sync::atomic::Ordering;
        let (url, hits) = counting_jwks(true).await;
        let ctx = AuthContext::from_config(url, TEST_SUPABASE_URL.into());

        for _ in 0..5 {
            assert!(ctx.key_for("captain-test-es256").await.is_err(), "a down JWKS fails closed");
        }
        assert_eq!(hits.load(Ordering::SeqCst), 1, "one attempt stands for the backoff window");
    }

    /// An unknown `kid` is ATTACKER-SUPPLIED on the open path. One forged token must not buy one
    /// outbound JWKS fetch: after a set has just been fetched, an unknown kid is refused without
    /// asking again.
    #[tokio::test]
    async fn an_unknown_kid_cannot_drive_a_fetch_per_request() {
        use std::sync::atomic::Ordering;
        let (url, hits) = counting_jwks(false).await;
        let ctx = AuthContext::from_config(url, TEST_SUPABASE_URL.into());

        for _ in 0..10 {
            assert!(ctx.key_for("forged-kid").await.is_err(), "an unknown kid fails closed");
        }
        assert_eq!(hits.load(Ordering::SeqCst), 1, "the cold fetch only -- no per-request refetch");
        // …and the real key is still served from that one fetch.
        assert!(
            ctx.key_for("captain-test-es256").await.is_ok(),
            "rotation-throttling is not a lockout"
        );
    }

    /// TEST-ONLY ES256 keypair (generated for this test file, never a deployed key): the PEM signs,
    /// [`signing_set`] is its public point as a JWKS. Unlike [`test_set`]'s dummy material this
    /// key VERIFIES — it exists so [`cookie_delivered_jwt_yields_the_customer_principal_and_scope`]
    /// can exercise `authorize()` end to end (signature check included), the one link no pure test
    /// reaches (#437, beck's mandatory test: a claim→field transposition at the `Principal`
    /// construction passes every other test in this crate).
    const TEST_EC_PRIVATE_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgTCTNdGfegiVKVsm+
vZXPOa4xJAt5OT8zMSblfCEwtW2hRANCAARto0Dk75fxl2IyLx89vwvjUWkJAb/p
5bKnk8sNetDUBHLVIGpXoxBRFJVNSeDN6QB9IHl6rqDLaZR4iqLatScL
-----END PRIVATE KEY-----
";

    fn signing_set() -> JwkSet {
        serde_json::from_str(
            r#"{"keys":[{"kty":"EC","crv":"P-256","use":"sig","kid":"captain-test-es256","alg":"ES256","x":"baNA5O-X8ZdiMi8fPb8L41FpCQG_6eWyp5PLDXrQ1AQ","y":"ctUgalejEFEUlU1J4M3pAH0geXquoMtplHiKotq1Jws"}]}"#,
        )
        .expect("parse signing JWKS")
    }

    /// Sign a Supabase-shaped customer JWT: `sub` is the AUTH subject (deliberately a different
    /// uuid from the claim — an implementation deriving identity from `sub` cannot pass), the
    /// domain identity is `app_metadata.captain_food.customer_id` (#433/#437, nested by #519).
    fn signed_customer_jwt(sub: &str, customer_id: uuid::Uuid) -> String {
        signed_jwt(
            sub,
            captain_food_claims(
                "CUSTOMER",
                serde_json::json!({ "customer_id": customer_id.to_string() }),
            ),
            3600,
        )
    }

    /// OUR identity project, as the tests' `iss`. Before #519 the fixtures minted tokens with **no
    /// `iss` claim at all** and every context carried `issuer: None`, so the whole suite was
    /// issuer-blind BY CONSTRUCTION: not one assertion could have noticed that issuer validation
    /// was optional. `TEST_ISSUER` is written out rather than derived, and pinned equal to what
    /// [`Verifier::new`] derives from `TEST_SUPABASE_URL` in
    /// [`an_unset_issuer_refuses_every_role_path_instead_of_skipping_the_check`].
    const TEST_SUPABASE_URL: &str = "https://captain-under-test.supabase.co";
    const TEST_ISSUER: &str = "https://captain-under-test.supabase.co/auth/v1";
    /// A DIFFERENT project on the same provider — the sibling product sharing the group's identity
    /// project, or staging pointed at production's. Same signing key on purpose: the signature is
    /// not what separates products, so a test that changed the key would prove nothing.
    const OTHER_PROJECT_ISSUER: &str = "https://sibling-product.supabase.co/auth/v1";

    /// A Supabase-shaped JWT with an arbitrary `app_metadata` and lifetime, signed by the test key
    /// and issued by [`TEST_ISSUER`]. `ttl_secs` is signed: a NEGATIVE value produces an EXPIRED
    /// token (the stale-cookie case, which is the common one on the open path, not an exotic one).
    fn signed_jwt(sub: &str, app_metadata: serde_json::Value, ttl_secs: i64) -> String {
        signed_jwt_from(TEST_ISSUER, sub, app_metadata, ttl_secs)
    }

    /// The same token, minted by an arbitrary issuer — the only knob the cross-project tests need.
    fn signed_jwt_from(
        issuer: &str,
        sub: &str,
        app_metadata: serde_json::Value,
        ttl_secs: i64,
    ) -> String {
        signed_jwt_shaped(sub, app_metadata, ttl_secs, |claims| {
            claims.insert("iss".into(), serde_json::json!(issuer));
        })
    }

    /// The token with reserved claims REMOVED or RETYPED. `jsonwebtoken`'s `iss`/`aud` matchers are
    /// **present-only** — an absent or non-string claim takes their `_ => {}` arm and passes
    /// vacuously — so this is the only way to observe the difference between *matched* and
    /// *required*. It is the shape a custom access-token hook in a SHARED identity project can
    /// produce, which is exactly the project #519 is about.
    fn signed_jwt_reshaped(
        sub: &str,
        app_metadata: serde_json::Value,
        edit: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>),
    ) -> String {
        signed_jwt_shaped(sub, app_metadata, 3600, |claims| {
            claims.insert("iss".into(), serde_json::json!(TEST_ISSUER));
            edit(claims);
        })
    }

    fn signed_jwt_shaped(
        sub: &str,
        app_metadata: serde_json::Value,
        ttl_secs: i64,
        edit: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>),
    ) -> String {
        let mut header = jsonwebtoken::Header::new(Algorithm::ES256);
        header.kid = Some("captain-test-es256".into());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_secs() as i64;
        let mut claims = serde_json::Map::new();
        claims.insert("sub".into(), serde_json::json!(sub));
        claims.insert("aud".into(), serde_json::json!(SUPABASE_AUDIENCE));
        claims.insert("exp".into(), serde_json::json!(now + ttl_secs));
        claims.insert("app_metadata".into(), app_metadata);
        edit(&mut claims);
        let claims = serde_json::Value::Object(claims);
        let key = jsonwebtoken::EncodingKey::from_ec_pem(TEST_EC_PRIVATE_KEY_PEM.as_bytes())
            .expect("test EC key parses");
        jsonwebtoken::encode(&header, &claims, &key).expect("sign test JWT")
    }

    /// The `app_metadata` a Captain Food token carries after #519: ONE product-owned object, whose
    /// PRESENCE is what proves the token was minted for this product. Under a group-wide identity
    /// project `iss` and `aud` are identical across every sibling product, so this object is the
    /// only separator left.
    fn captain_food_claims(role: &str, claims: serde_json::Value) -> serde_json::Value {
        let mut obj = claims;
        obj["role"] = serde_json::json!(role);
        serde_json::json!({ "captain_food": obj })
    }

    /// The storefront's own request shape: a cookie-delivered credential, no Authorization header.
    fn cookie_headers(jwt: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(axum::http::header::COOKIE, format!("captain_auth={jwt}").parse().unwrap());
        h
    }

    /// #469 half 1, the fix itself at the auth seam: the OPEN path READS the credential the
    /// storefront actually sends (`web::router` pins the customer surfaces to `Role::Public`), so a
    /// signed-in customer arrives as `ReadScope::Customer` and `cart.current`'s claim leg can fire.
    /// Before this, `/public` returned `Principal::anonymous()` without reading any credential, and
    /// leg 1 was unreachable from a browser.
    #[tokio::test]
    async fn the_open_path_reads_the_customer_credential_it_used_to_ignore() {
        let ctx = ctx_with_cache(signing_set(), Instant::now());
        let sub = uuid::Uuid::from_u128(0xA0).to_string();
        let customer = uuid::Uuid::from_u128(0x469);

        let principal = ctx
            .authorize(RequestRole::Public, &cookie_headers(&signed_customer_jwt(&sub, customer)))
            .await
            .expect("the open path never fails");
        assert_eq!(
            principal.identity,
            Identity::Customer { sub: sub.clone(), customer_id: customer },
            "the verified claim IS the identity, and `sub` stays the auth subject beside it"
        );
        assert_eq!(
            read_scope(&principal),
            application::queries::ReadScope::Customer(domain::generated::scalars::CustomerId(
                customer
            )),
            "leg 1 of cart.current is reachable from the storefront's own request shape"
        );
    }

    /// #469 half 1, the escalation pin (mob: testing lens 4a + API lens 3, reached independently).
    /// Once `/public` reads credentials, an ADMIN / RESTAURANT / RESTAURANT_ACCOUNT / RIDER token
    /// presented there must degrade to ANONYMOUS — never yield `ReadScope::Admin`/`Restaurant`/
    /// `Rider`, and never 401. Elevating here would convert a dead claim leg into privilege
    /// escalation on the one path anyone can reach, and would silently widen every role-omitted
    /// read (an omitted `roles` key is open to every role path — specs/common/api.yaml).
    #[tokio::test]
    async fn a_staff_token_on_the_open_path_stays_anonymous() {
        let ctx = ctx_with_cache(signing_set(), Instant::now());
        let sub = uuid::Uuid::from_u128(0xA1).to_string();
        let tenant = uuid::Uuid::from_u128(0xBEEF);

        for role in ["ADMIN", "RESTAURANT", "RESTAURANT_ACCOUNT", "RIDER", "EXTERNAL"] {
            // The token carries EVERY tenant claim at once: if the open path copied claims
            // wholesale instead of constructing a customer-only principal, this is what would leak.
            let jwt = signed_jwt(
                &sub,
                captain_food_claims(
                    role,
                    serde_json::json!({
                        "restaurant_id": tenant.to_string(),
                        "restaurant_account_id": tenant.to_string(),
                        "rider_id": tenant.to_string(),
                        "customer_id": tenant.to_string(),
                    }),
                ),
                3600,
            );
            let principal = ctx
                .authorize(RequestRole::Public, &cookie_headers(&jwt))
                .await
                .unwrap_or_else(|_| panic!("{role}: the open path must never refuse"));
            assert_eq!(
                principal.identity,
                Identity::Anonymous,
                "{role}: not elevated, no identity and no tenant claim survives on the open path"
            );
            assert_eq!(principal.recorded_role(), RequestRole::Public, "{role}: not elevated");
            assert_eq!(
                read_scope(&principal),
                application::queries::ReadScope::Public,
                "{role}: a staff token on /public reads exactly what an anonymous browser reads"
            );
        }
    }

    /// #469, reviewer S3: a verified CUSTOMER token with NO `captain_food.customer_id` — the
    /// pre-claim-stamp window, which is EVERY signed-in customer for one token lifetime after
    /// rollout — degrades to anonymous and is counted as `claim_absent`, rather than becoming a
    /// `role: Customer, customer_id: None` principal.
    ///
    /// The assertion that carries the point is `role == Public`: `read_scope`'s
    /// `unresolved("CUSTOMER")` arm — which bumps `read_authorization_bridge_unresolved_total`, a
    /// counter whose contract says *"never ordinary user denial"* — is reachable ONLY through
    /// `role: Customer`. A principal that never claims that role cannot reach it, so a normal
    /// rollout cannot bump a provisioning-gap counter on every storefront GraphQL request. A
    /// MALFORMED claim is the same state by design (fail closed, indistinguishable from absent).
    #[tokio::test]
    async fn a_customer_token_without_its_claim_degrades_instead_of_alarming() {
        let ctx = ctx_with_cache(signing_set(), Instant::now());
        let sub = uuid::Uuid::from_u128(0xA3).to_string();

        for (case, app_metadata) in [
            ("claim absent", captain_food_claims("CUSTOMER", serde_json::json!({}))),
            (
                "claim malformed",
                captain_food_claims("CUSTOMER", serde_json::json!({ "customer_id": "not-a-uuid" })),
            ),
        ] {
            let jwt = signed_jwt(&sub, app_metadata, 3600);
            let principal = ctx
                .authorize(RequestRole::Public, &cookie_headers(&jwt))
                .await
                .unwrap_or_else(|_| panic!("{case}: the open path must never refuse"));
            assert_eq!(
                principal.identity,
                Identity::Anonymous,
                "{case}: an unusable claim is a DEGRADE, not an Unbound CUSTOMER principal"
            );
            assert_eq!(principal.recorded_role(), RequestRole::Public, "{case}");
            assert_eq!(
                read_scope(&principal),
                application::queries::ReadScope::Public,
                "{case}: the same scope either way -- so the only difference would have been the alarm"
            );
        }
    }

    /// #469 reviewer N1, **INVERTED by #519**. `parse_role`'s catch-all is reached by an UNKNOWN
    /// role string and by an ABSENT one — two branches `a_staff_token_on_the_open_path_stays_anonymous`
    /// cannot cover, because it enumerates the five explicit non-CUSTOMER roles.
    ///
    /// It used to read `_ => CUSTOMER` and this test pinned the resulting *"least-privilege
    /// authenticated baseline"*: such a caller got their own customer scope. That default is exactly
    /// the #519 defect — it is a baseline only among roles WE issue, and it silently adopted any
    /// token that verified. Same two branches, opposite expectation: **no role is no principal**,
    /// even though this token carries every domain claim there is.
    #[tokio::test]
    async fn an_unknown_or_absent_role_claim_grants_nothing_not_even_the_customer_baseline() {
        let ctx = ctx_with_cache(signing_set(), Instant::now());
        let sub = uuid::Uuid::from_u128(0xA4).to_string();
        let customer = uuid::Uuid::from_u128(0x469C);
        let tenant = uuid::Uuid::from_u128(0xBEEF);

        let with_claims = |role: Option<&str>| {
            let mut meta = serde_json::json!({
                "customer_id": customer.to_string(),
                "restaurant_id": tenant.to_string(),
                "restaurant_account_id": tenant.to_string(),
                "rider_id": tenant.to_string(),
            });
            if let Some(role) = role {
                meta["role"] = serde_json::json!(role);
            }
            serde_json::json!({ "captain_food": meta })
        };

        for (case, role) in
            [("empty", Some("")), ("unknown", Some("SUPERADMIN")), ("absent", None)]
        {
            let jwt = signed_jwt(&sub, with_claims(role), 3600);
            let principal = ctx
                .authorize(RequestRole::Public, &cookie_headers(&jwt))
                .await
                .unwrap_or_else(|_| panic!("{case}: the open path must never refuse"));
            assert_eq!(
                principal.identity,
                Identity::Anonymous,
                "{case}: an unrecognised role is NOT the customer baseline"
            );
            assert_eq!(
                read_scope(&principal),
                application::queries::ReadScope::Public,
                "{case}: no scope -- neither the caller's own nor a tenant one"
            );
            assert!(
                ctx.authorize(RequestRole::Customer, &cookie_headers(&jwt)).await.is_err(),
                "{case}: and the role path refuses outright"
            );
        }
    }

    /// #469 half 1, the availability pin (mob: testing lens 4b + API lens 3). A stale cookie is the
    /// COMMON case and a JWKS outage is a platform one; neither may take anonymous browsing down.
    /// Expired, tampered, unsigned-by-us and unverifiable-because-no-JWKS all serve the anonymous
    /// view — 200, never 401/503.
    #[tokio::test]
    async fn an_unverifiable_credential_on_the_open_path_degrades_never_refuses() {
        let ctx = ctx_with_cache(signing_set(), Instant::now());
        let sub = uuid::Uuid::from_u128(0xA2).to_string();
        let customer = uuid::Uuid::from_u128(0x469);

        // 1. EXPIRED — the stale cookie of a customer who left the tab open overnight.
        let expired = signed_jwt(
            &sub,
            captain_food_claims("CUSTOMER", serde_json::json!({ "customer_id": customer.to_string() })),
            -3600,
        );
        // 2. TAMPERED — a flipped payload byte, i.e. a forged claim.
        let valid = signed_customer_jwt(&sub, customer);
        let tampered = {
            let mut parts: Vec<String> = valid.split('.').map(str::to_string).collect();
            let p = &mut parts[1];
            let last = p.pop().expect("payload non-empty");
            p.push(if last == 'A' { 'B' } else { 'A' });
            parts.join(".")
        };
        for (case, jwt) in [("expired", expired), ("tampered", tampered), ("garbage", "not.a.jwt".into())] {
            let principal = ctx
                .authorize(RequestRole::Public, &cookie_headers(&jwt))
                .await
                .unwrap_or_else(|_| panic!("{case}: /public must serve 200 anonymous, never 401"));
            assert_eq!(principal.identity, Identity::Anonymous, "{case}");
        }

        // 3. The VERIFIER ITSELF is unavailable (no JWKS configured, empty cache — a cold instance
        //    during a Supabase outage): still anonymous, still 200. `/public` worked with no JWKS
        //    at all before this change and must keep working.
        let no_verifier = AuthContext {
            verifier: None,
            external_tokens: Vec::new(),
            http: jwks_client(),
            cache: RwLock::new(None),
            refresh_lock: tokio::sync::Mutex::new(()),
            last_failure: RwLock::new(None),
        };
        let principal = no_verifier
            .authorize(RequestRole::Public, &cookie_headers(&valid))
            .await
            .expect("a JWKS outage must not take anonymous browsing down");
        assert_eq!(principal.identity, Identity::Anonymous);

        // 4. NO credential at all: anonymous without touching the verifier (the cost of anonymous
        //    browsing is unchanged — no verifier here means any fetch attempt would fail, and
        //    the assertion above it would too).
        let anonymous = no_verifier
            .authorize(RequestRole::Public, &HeaderMap::new())
            .await
            .expect("no credential is the ordinary anonymous request");
        assert_eq!(anonymous.identity, Identity::Anonymous);
    }

    /// The end-to-end link every pure test stops short of (#437): a REAL signature verified
    /// against a seeded JWKS, the token delivered ONLY via the `captain_auth` cookie (the
    /// storefront's one credential — no Authorization header exists on the request), and the
    /// verified claim landing in `Principal.customer_id` → `ReadScope::Customer`. A field
    /// transposition in `authorize()`'s Principal construction (customer claim into another
    /// role's field)
    /// is caught HERE and nowhere else.
    #[tokio::test]
    async fn cookie_delivered_jwt_yields_the_customer_principal_and_scope() {
        let ctx = ctx_with_cache(signing_set(), Instant::now()); // fresh cache: no network, ever
        let sub = uuid::Uuid::from_u128(0xA0).to_string();
        let customer = uuid::Uuid::from_u128(0x437);
        let jwt = signed_customer_jwt(&sub, customer);

        let mut h = HeaderMap::new();
        h.insert(axum::http::header::COOKIE, format!("captain_auth={jwt}").parse().unwrap());
        assert!(h.get(AUTHORIZATION).is_none(), "cookie-only delivery: no bearer on this request");

        let principal =
            ctx.authorize(RequestRole::Customer, &h).await.expect("cookie-carried JWT authorizes");
        assert_eq!(
            principal.identity,
            Identity::Customer { sub: sub.clone(), customer_id: customer },
            "the verified claim IS the identity, `sub` stays the auth subject, and no cross-field \
             leakage is possible: the CUSTOMER identity has no rider/restaurant slot to leak into"
        );
        assert_eq!(
            read_scope(&principal),
            application::queries::ReadScope::Customer(domain::generated::scalars::CustomerId(customer)),
            "the scope every #437 tracking read is filtered by"
        );

        // The signature is REALLY checked: tamper with the last payload byte → Unauthorized.
        let mut parts: Vec<String> = jwt.split('.').map(str::to_string).collect();
        let flipped = {
            let p = &mut parts[1];
            let last = p.pop().expect("payload non-empty");
            p.push(if last == 'A' { 'B' } else { 'A' });
            parts.join(".")
        };
        let mut tampered = HeaderMap::new();
        tampered
            .insert(axum::http::header::COOKIE, format!("captain_auth={flipped}").parse().unwrap());
        assert!(
            ctx.authorize(RequestRole::Customer, &tampered).await.is_err(),
            "a tampered payload must be rejected by signature verification"
        );
    }

    // ---------------------------------------------------------------------------------------
    // #519 — WHAT A TOKEN MUST PROVE, once one identity project serves EVERY product of the group.
    //
    // The three separators a verifier normally leans on all stop separating:
    //   `aud`  is the Supabase constant "authenticated", which every user of every project carries;
    //   `iss`  becomes IDENTICAL across sibling products the moment they share a project;
    //   the signing KEY is the project's, so a sibling's token verifies against our JWKS.
    // What is left is the product's own claim object, and it only separates if its ABSENCE refuses.
    // ---------------------------------------------------------------------------------------

    /// **Issuer, positively.** Same signing key, a DIFFERENT project's `iss`: refused on a role
    /// path, anonymous on the open one. Seen RED first — the fixtures below used to build
    /// `issuer: None`, so this token was accepted as a fully bound CUSTOMER.
    #[tokio::test]
    async fn a_token_from_another_project_is_refused_even_though_our_key_signed_it() {
        let ctx = ctx_with_cache(signing_set(), Instant::now());
        let sub = uuid::Uuid::from_u128(0x519).to_string();
        let customer = uuid::Uuid::from_u128(0x519C);
        let foreign = signed_jwt_from(
            OTHER_PROJECT_ISSUER,
            &sub,
            captain_food_claims(
                "CUSTOMER",
                serde_json::json!({ "customer_id": customer.to_string() }),
            ),
            3600,
        );

        assert!(
            ctx.authorize(RequestRole::Customer, &cookie_headers(&foreign)).await.is_err(),
            "a token minted by another project must not authorize a role path -- audience proves \
             nothing (every Supabase user is `authenticated`) and the key is the project's"
        );
        assert_eq!(
            ctx.authorize(RequestRole::Public, &cookie_headers(&foreign))
                .await
                .expect("the open path never refuses")
                .identity,
            Identity::Anonymous,
            "and it buys no identity on the open path either"
        );
    }

    /// **Unset issuer REFUSES; it does not skip.** `SUPABASE_URL` empty used to mean "no issuer to
    /// compare against", i.e. no issuer check at all — the one configuration in which a staging or
    /// sibling token verifies in production. Now the verifier does not exist, and every role path
    /// fails closed with `503` while `/public` degrades to anonymous.
    #[tokio::test]
    async fn an_unset_issuer_refuses_every_role_path_instead_of_skipping_the_check() {
        let (url, _hits) = counting_jwks(false).await;
        let ctx = AuthContext::from_config(url, String::new()); // SUPABASE_URL unset
        let sub = uuid::Uuid::from_u128(0x51A).to_string();
        let meta = captain_food_claims(
            "CUSTOMER",
            serde_json::json!({ "customer_id": uuid::Uuid::from_u128(0x51AC).to_string() }),
        );

        for (case, issuer) in
            [("our own project", TEST_ISSUER), ("a sibling project", OTHER_PROJECT_ISSUER)]
        {
            let jwt = signed_jwt_from(issuer, &sub, meta.clone(), 3600);
            assert!(
                matches!(
                    ctx.authorize(RequestRole::Customer, &cookie_headers(&jwt)).await,
                    Err(AuthError::Unavailable)
                ),
                "{case}: an unconfigured issuer must FAIL CLOSED (503), never verify a token \
                 without checking who issued it"
            );
            assert_eq!(
                ctx.authorize(RequestRole::Public, &cookie_headers(&jwt))
                    .await
                    .expect("the open path never refuses")
                    .identity,
                Identity::Anonymous,
                "{case}: and the open path degrades rather than trusting an unverifiable claim"
            );
        }
    }

    /// **MATCHED is not REQUIRED, and only this test can tell them apart** (review round 1 on #519).
    ///
    /// `jsonwebtoken`'s `set_issuer`/`set_audience` assign a matcher and nothing else: they do NOT
    /// add the claim to `required_spec_claims` (which `Validation::new` leaves as `{"exp"}`), and
    /// `validate()`'s `iss` and `aud` arms both end in `_ => {}`. An **absent** claim — and a
    /// **non-string** one, which deserializes to `TryParse::FailedToParse` — therefore takes the
    /// fall-through and passes VACUOUSLY. Verified against the pinned `jsonwebtoken 10.3.0`
    /// (`validation.rs:112-115`, `:143-145`, `:258-272`, `:308-320`, `:325-349`); the crate says so
    /// itself: *"Validation only happens if `iss` claim is present in the token."*
    ///
    /// Not reachable by an outsider — the token must still be signed by a key in our JWKS. But the
    /// premise of #519 is a project whose claim shaping is **not ours alone**: an access-token hook
    /// or custom-claim arrangement in the shared group project is precisely the actor that can drop
    /// or retype `iss`. A guarantee that has never been seen red is not a guarantee.
    #[tokio::test]
    async fn a_token_that_omits_or_retypes_iss_or_aud_is_refused_not_passed_vacuously() {
        let ctx = ctx_with_cache(signing_set(), Instant::now());
        let sub = uuid::Uuid::from_u128(0x51E).to_string();
        let meta = || {
            captain_food_claims(
                "CUSTOMER",
                serde_json::json!({ "customer_id": uuid::Uuid::from_u128(0x51EC).to_string() }),
            )
        };

        type Edit = Box<dyn FnOnce(&mut serde_json::Map<String, serde_json::Value>)>;
        let cases: Vec<(&str, Edit)> = vec![
            ("no iss", Box::new(|c: &mut serde_json::Map<_, _>| {
                c.remove("iss");
            })),
            ("no aud", Box::new(|c: &mut serde_json::Map<_, _>| {
                c.remove("aud");
            })),
            ("neither", Box::new(|c: &mut serde_json::Map<_, _>| {
                c.remove("iss");
                c.remove("aud");
            })),
            // A non-string claim reaches the SAME silent arm by a different road: serde fails to
            // parse it, and a failed parse is not a mismatch.
            ("numeric iss", Box::new(|c: &mut serde_json::Map<_, _>| {
                c.insert("iss".into(), serde_json::json!(42));
            })),
            ("object aud", Box::new(|c: &mut serde_json::Map<_, _>| {
                c.insert("aud".into(), serde_json::json!({ "any": "thing" }));
            })),
        ];

        for (case, edit) in cases {
            let jwt = signed_jwt_reshaped(&sub, meta(), edit);
            assert!(
                ctx.authorize(RequestRole::Customer, &cookie_headers(&jwt)).await.is_err(),
                "{case}: a reserved claim we MATCH must also be REQUIRED -- otherwise omitting it \
                 skips the check entirely"
            );
            assert_eq!(
                ctx.authorize(RequestRole::Public, &cookie_headers(&jwt))
                    .await
                    .expect("the open path never refuses")
                    .identity,
                Identity::Anonymous,
                "{case}: and it buys no identity on the open path"
            );
        }
    }

    /// The requirement is DERIVED from the matchers, so the two cannot drift (see
    /// [`Verifier::validation`]). Pinned on the produced value, not on the source text.
    #[test]
    fn every_reserved_claim_the_verifier_matches_is_also_required() {
        let validation = unfetchable_verifier().validation(Algorithm::ES256);
        assert_eq!(validation.iss.as_ref().map(|s| s.len()), Some(1), "the issuer is matched");
        assert_eq!(validation.aud.as_ref().map(|s| s.len()), Some(1), "the audience is matched");
        let mut required: Vec<&str> =
            validation.required_spec_claims.iter().map(String::as_str).collect();
        required.sort_unstable();
        assert_eq!(
            required,
            ["aud", "exp", "iss"],
            "`exp` (the library default) plus every claim we match -- a matcher without its \
             requirement is a check an absent claim simply skips"
        );
    }

    /// **The positive product check.** A token that verifies completely — our key, our issuer, the
    /// Supabase audience — but carries NO `captain_food` claim object is not a principal of this
    /// product. Under a group-wide project this is the ordinary shape of a sibling product's user,
    /// and before #519 it landed on `/customer/graphql` as an authenticated CUSTOMER.
    #[tokio::test]
    async fn a_token_with_no_captain_food_claim_object_is_never_an_authenticated_principal() {
        let ctx = ctx_with_cache(signing_set(), Instant::now());
        let sub = uuid::Uuid::from_u128(0x51B).to_string();
        let id = uuid::Uuid::from_u128(0x51BC).to_string();

        for (case, app_metadata) in [
            ("no app_metadata at all", serde_json::json!({})),
            ("a sibling product's claims", serde_json::json!({ "other_product": { "role": "ADMIN" } })),
            ("provider bookkeeping only", serde_json::json!({ "provider": "phone", "providers": ["phone"] })),
            // The PRE-#519 shape: flat `captain_*` keys. Supabase merges `app_metadata` SHALLOWLY,
            // which is why nesting rather than renaming is the fix -- and why the flat keys survive
            // as inert siblings on an already-stamped auth user. They must grant nothing.
            (
                "the pre-#519 flat claims",
                serde_json::json!({ "captain_role": "CUSTOMER", "captain_customer_id": id }),
            ),
        ] {
            let jwt = signed_jwt(&sub, app_metadata, 3600);
            assert!(
                ctx.authorize(RequestRole::Customer, &cookie_headers(&jwt)).await.is_err(),
                "{case}: a token proving no Captain Food role must be refused, not defaulted"
            );
            assert_eq!(
                ctx.authorize(RequestRole::Public, &cookie_headers(&jwt))
                    .await
                    .expect("the open path never refuses")
                    .identity,
                Identity::Anonymous,
                "{case}: and it is anonymous on the open path"
            );
        }
    }

    /// **The writer and the reader agree, proved on the writer's actual output.** The claim stamp
    /// lives in `infrastructure` and the verifier here; nothing but this test connects them, and a
    /// key renamed on one side would otherwise be discovered by a production smoke timeout —
    /// customers logging in to a session that verifies as a stranger's.
    ///
    /// It also pins the NESTING, not just the names: the very same claims one level up are a
    /// stranger's metadata (asserted in `app_metadata_claims_deserialize_and_malformed_uuids_fail_closed`).
    #[test]
    fn the_verifier_reads_what_the_claim_stamp_writes() {
        let customer = uuid::Uuid::from_u128(0x437);
        let body = infrastructure::integrations::supabase_auth::stamp_put_body(
            &domain::generated::scalars::CustomerId(customer),
        );
        let claims: Claims = serde_json::from_value(serde_json::json!({
            "sub": "auth-subject",
            "app_metadata": body["app_metadata"],
        }))
        .expect("the stamped metadata is a Supabase-shaped claims payload");

        let grant = claims.app_metadata.grant().expect("the stamp yields a grant");
        assert_eq!(grant.role, RequestRole::Customer, "the stamp hardcodes CUSTOMER");
        assert_eq!(
            claim_uuid(&grant.claims.customer_id),
            Some(customer),
            "the stamped domain id is the one the verifier binds"
        );
        assert_eq!(
            body["app_metadata"].get(PRODUCT_CLAIM_KEY).and_then(|v| v.get("role")),
            Some(&serde_json::json!("CUSTOMER")),
            "and the wire key is the one constant both crates name"
        );
    }

    /// **Role parsing fails CLOSED.** An absent or unrecognised role grants nothing — it does not
    /// fall back to CUSTOMER. The token below carries a perfectly good `customer_id`, so the only
    /// thing standing between it and a customer session is the role check.
    #[tokio::test]
    async fn an_absent_or_unrecognised_role_grants_nothing_rather_than_customer() {
        let ctx = ctx_with_cache(signing_set(), Instant::now());
        let sub = uuid::Uuid::from_u128(0x51D).to_string();
        let customer = uuid::Uuid::from_u128(0x51DC);
        let claims = serde_json::json!({ "customer_id": customer.to_string() });

        for (case, role) in [("absent", None), ("empty", Some("")), ("unknown", Some("SUPERADMIN"))]
        {
            let mut obj = claims.clone();
            if let Some(role) = role {
                obj["role"] = serde_json::json!(role);
            }
            let jwt = signed_jwt(&sub, serde_json::json!({ "captain_food": obj }), 3600);
            assert!(
                ctx.authorize(RequestRole::Customer, &cookie_headers(&jwt)).await.is_err(),
                "{case} role: no role is NO role -- never the CUSTOMER baseline"
            );
            let public = ctx
                .authorize(RequestRole::Public, &cookie_headers(&jwt))
                .await
                .expect("the open path never refuses");
            assert_eq!(public.identity, Identity::Anonymous, "{case} role: no identity on /public");
            assert_eq!(
                read_scope(&public),
                application::queries::ReadScope::Public,
                "{case} role: and no customer scope, despite a usable customer_id beside it"
            );
        }
    }
}

/// A `captain_*` claim parsed as a domain uuid. Malformed values yield `None` — fail closed,
/// indistinguishable from an absent claim by design (an attacker-shaped string must never widen
/// into an identity; `read_scope` then denies and counts it as unresolved).
fn claim_uuid(v: &Option<String>) -> Option<uuid::Uuid> {
    v.as_deref().and_then(|s| uuid::Uuid::parse_str(s).ok())
}

// =====================================================================================
// IDENT-1 Phase A (ADR-20260818-004646, #641) — the request-seam translation from the verified
// auth SUBJECT (`authRef`) to the CUSTOMER's domain identity, resolved from Postgres INSTEAD OF
// the JWT `captain_food.customer_id` claim, gated by `RESOLVE_CUSTOMER_IDENTITY_FROM_POSTGRES`
// (specs/customer/configuration.yaml, DEFAULT off). Scope of the GATE: CUSTOMER only. The RIDER
// seam beside it (#639 part C step 2b, PROP-20260831-180622 row 2) is UNGATED — Postgres over the
// `Rider` read model part A landed, on every request — because no rider token has ever carried a
// domain binding, so there is no claim path an OFF state could preserve. RESTAURANT and
// RESTAURANT_ACCOUNT still read their claims (STAFF-AUTH step 6 owns them).
// =====================================================================================

/// The request-seam TRANSLATION from the verified auth subject to this product's CUSTOMER domain
/// identity (evans: named for the translation it performs — `authRef` is the vocabulary, never
/// `sub`, in identifiers/spans/metric names). Deciders/scope logic receive the [`CustomerIdentityResolution`]
/// RESULT; only implementations of this trait perform I/O.
#[async_trait::async_trait]
pub trait ResolveCustomerIdentity: Send + Sync {
    /// `auth_ref` is the verified Supabase `sub` — already authenticated by [`AuthContext::authorize`],
    /// never attacker-controlled at this point.
    async fn resolve(&self, auth_ref: &str) -> CustomerIdentityResolution;
}

/// A seam's typed THREE-WAY outcome, generic over the domain id it resolves to. A bare
/// `Option<Id>` would collapse "no mapping row" and "could not ask" into one signal — the ADR
/// requires them distinguishable, because they fail closed IDENTICALLY at the API boundary (both
/// become `ReadScope::Public`) but have OPPOSITE operator responses: a missing mapping is an
/// ordinary provisioning gap (OBSERVE), a failed lookup means the identity seam itself is
/// unavailable (PAGE).
///
/// One enum for both seams (#639 part C step 2b added the RIDER one) so the outcome vocabulary
/// cannot drift between roles: [`CustomerIdentityResolution`] and [`RiderIdentityResolution`] are
/// aliases, and a variant added to one is added to the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityResolution<Id> {
    /// The verified subject maps to this domain identity.
    Resolved(Id),
    /// The subject is verified but carries no `auth_ref` mapping row — ordinary (a
    /// not-yet-provisioned or genuinely unmapped subject), never an outage signal on its own.
    NoMapping,
    /// The lookup itself could not be answered (a repository/adapter failure) — distinct from
    /// [`IdentityResolution::NoMapping`] because THIS is the outage class.
    LookupFailed(LookupFailureReason),
}

/// The CUSTOMER seam's outcome (#641).
pub type CustomerIdentityResolution = IdentityResolution<domain::generated::scalars::CustomerId>;
/// The RIDER seam's outcome (#639 part C step 2b).
pub type RiderIdentityResolution = IdentityResolution<domain::generated::scalars::RiderId>;

/// The coarse, CLOSED set of reasons a lookup can fail — safe as a telemetry label (never the
/// underlying error string, which is unbounded cardinality on a labeled series).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupFailureReason {
    /// A repository/adapter failure (`DomainError::Repository`) — the ordinary shape for a DB
    /// outage or timeout.
    Repository,
    /// `DomainError::Invariant` — a legacy stringly-typed failure surfacing through this port.
    Invariant,
    /// `DomainError::Rejected` — an anticipated `errors.yaml` code surfacing through a read port
    /// that does not normally reject; kept distinct rather than silently folded into `Repository`.
    Rejected,
}

impl LookupFailureReason {
    /// The telemetry label — the contract's bounded `reason` attribute value.
    fn label(self) -> &'static str {
        match self {
            LookupFailureReason::Repository => "repository",
            LookupFailureReason::Invariant => "invariant",
            LookupFailureReason::Rejected => "rejected",
        }
    }

    fn from_domain_error(e: &domain::shared::errors::DomainError) -> Self {
        match e {
            domain::shared::errors::DomainError::Repository(_) => LookupFailureReason::Repository,
            domain::shared::errors::DomainError::Invariant(_) => LookupFailureReason::Invariant,
            domain::shared::errors::DomainError::Rejected { .. } => LookupFailureReason::Rejected,
        }
    }
}

/// The Postgres implementation: wraps the EXISTING `CustomerReadRepository::by_auth_ref` bridge
/// (already the `me` query's mechanism) rather than re-deriving the query (vernon/evans — reuse
/// the port, don't duplicate it).
pub struct PgCustomerIdentity {
    customers: Arc<dyn application::queries::CustomerReadRepository>,
}

impl PgCustomerIdentity {
    pub fn new(customers: Arc<dyn application::queries::CustomerReadRepository>) -> Self {
        Self { customers }
    }
}

#[async_trait::async_trait]
impl ResolveCustomerIdentity for PgCustomerIdentity {
    async fn resolve(&self, auth_ref: &str) -> CustomerIdentityResolution {
        match self
            .customers
            .by_auth_ref(domain::generated::scalars::ExternalReference(auth_ref.to_string()))
            .await
        {
            Ok(Some(row)) => CustomerIdentityResolution::Resolved(row.customer_id),
            Ok(None) => CustomerIdentityResolution::NoMapping,
            Err(e) => {
                CustomerIdentityResolution::LookupFailed(LookupFailureReason::from_domain_error(&e))
            }
        }
    }
}

/// Where the CUSTOMER's domain identity comes from — selected ONCE at startup/config-load from
/// `RESOLVE_CUSTOMER_IDENTITY_FROM_POSTGRES`, and NEVER a per-request fallback
/// (gate-then-stabilize; the ADR forbids a runtime try-Postgres-else-claim dual path): the mode is
/// fixed for the process's lifetime and cloned into every request's `GraphqlState`
/// (`crate::graphql::routes`). The gated-ON path reads no claim at all for this decision — see
/// [`resolve_identity_scope`], whose match arm destructures `Identity::Customer` as `{ sub, .. }`.
#[derive(Clone)]
pub enum CustomerIdentitySource {
    /// DEFAULT (config default `false`). The legacy JWT claim path — [`read_scope`], unchanged, no
    /// I/O — byte for byte the pre-Phase-A behaviour.
    Claim,
    /// The gated-ON path (config `true`): resolve through the seam instead of trusting the claim.
    Postgres(Arc<dyn ResolveCustomerIdentity>),
}

/// The request-seam TRANSLATION from the verified auth subject to this product's RIDER domain
/// identity (#639 part C step 2b — the rider sign-in door; ADR-20260818-004646: no business
/// identifier lives in the identity provider, so the mapping resolves in OUR Postgres, from the
/// `Rider` read model part A landed). Same shape as [`ResolveCustomerIdentity`]; only
/// implementations perform I/O.
#[async_trait::async_trait]
pub trait ResolveRiderIdentity: Send + Sync {
    /// `auth_subject` is the verified Supabase `sub` — already authenticated by
    /// [`AuthContext::authorize`], never attacker-controlled at this point.
    async fn resolve(&self, auth_subject: &str) -> RiderIdentityResolution;
}

/// The Postgres implementation: wraps the `RiderIdentityRepository` read port (one btree probe on
/// `rider.auth_ref UNIQUE`, `rider_id` and nothing else) — a PROJECTION probe, never a fold of the
/// `Rider-{id}` stream per request, which would be unbounded and would put the read path on
/// `domain_events` (ADR-20260830-234532 names that as the shape nobody should build).
pub struct PgRiderIdentity {
    riders: Arc<dyn application::queries::RiderIdentityRepository>,
}

impl PgRiderIdentity {
    pub fn new(riders: Arc<dyn application::queries::RiderIdentityRepository>) -> Self {
        Self { riders }
    }
}

#[async_trait::async_trait]
impl ResolveRiderIdentity for PgRiderIdentity {
    async fn resolve(&self, auth_subject: &str) -> RiderIdentityResolution {
        match self
            .riders
            .rider_id_by_auth_subject(domain::generated::scalars::AuthSubject(auth_subject.to_string()))
            .await
        {
            Ok(Some(rider_id)) => RiderIdentityResolution::Resolved(rider_id),
            Ok(None) => RiderIdentityResolution::NoMapping,
            Err(e) => RiderIdentityResolution::LookupFailed(LookupFailureReason::from_domain_error(&e)),
        }
    }
}

/// The rider seam of a process booted WITHOUT a database (the monolith's no-`DATABASE_URL` mode,
/// where customers stay on the claim path and no read model exists at all). It does not pretend:
/// every resolution is [`IdentityResolution::LookupFailed`] — "the seam could not be asked" — so a
/// rider fails closed to `Public` AND the PAGE-class counter fires. It is deliberately NOT a
/// `NoMapping` stand-in, which would report a missing database as an ordinary provisioning gap.
pub struct NoDatabaseRiderIdentity;

#[async_trait::async_trait]
impl ResolveRiderIdentity for NoDatabaseRiderIdentity {
    async fn resolve(&self, _auth_subject: &str) -> RiderIdentityResolution {
        RiderIdentityResolution::LookupFailed(LookupFailureReason::Repository)
    }
}

/// Where a RIDER's domain identity comes from: **Postgres, always** — there is deliberately no
/// `Claim` variant and no OFF state. `CustomerIdentitySource::Claim` is a real gate because OFF
/// reproduces working customer behaviour byte for byte; for RIDER no token has ever carried a
/// domain binding (the sole claim stamper hardcodes CUSTOMER), so an OFF state would preserve
/// nothing — "the feature does not exist" is dead code, not gate-then-stabilize
/// (PROP-20260831-180622 §10). One arm, final-vision-first.
///
/// A struct with a private field rather than a bare `Arc`: the only way to hold one is through
/// [`RiderIdentitySource::new`], so a composition root cannot leave the rider seam unset and have
/// the request path silently fall back to anything.
#[derive(Clone)]
pub struct RiderIdentitySource(Arc<dyn ResolveRiderIdentity>);

impl RiderIdentitySource {
    pub fn new(resolver: Arc<dyn ResolveRiderIdentity>) -> Self {
        Self(resolver)
    }
}

/// The identity seams a request resolves through, selected ONCE at startup/config-load and cloned
/// into every request's `GraphqlState` (`crate::graphql::routes`). One value rather than two
/// parameters so a transport cannot wire the customer seam and forget the rider one.
#[derive(Clone)]
pub struct IdentitySources {
    pub customer: CustomerIdentitySource,
    pub rider: RiderIdentitySource,
}

/// Resolve a verified [`Principal`] into the application's [`application::queries::ReadScope`] —
/// a PURE function of the token's verified claims (#433, ADR-20260809-050000 CARD-11: the
/// login-to-domain bridge lives in JWT claims for EVERY role; product-owner correction on #430).
///
/// No per-request lookup, no database, no async: `sub` is NEVER an identity — a customer token
/// whose `captain_*` claim is absent (or malformed) fails closed to Public, a rider ALWAYS does
/// here (its identity exists only through the seam, #639 part C step 2b), and the
/// `read_authorization_bridge_unresolved_total{role}` counter now means exactly one thing: an
/// authenticated caller whose token carries no domain binding (a provisioning gap or pre-refresh
/// staleness — never ordinary user denial, never a DB outage).
///
/// The #430 mechanisms this replaced: the per-request `customers.by_auth_ref` bridge and the
/// rider `sub`-parsed-as-uuid placeholder. `by_auth_ref` REMAINS the customer identity mechanism
/// at the write-side seams (the mailbox `resolve_actor`, the generated mutation edge bridges) —
/// a named follow-up on #432, envelope-shape territory, not silently claimed here.
pub fn read_scope(principal: &Principal) -> application::queries::ReadScope {
    use application::queries::ReadScope;
    use domain::generated::scalars::{CustomerId, RestaurantAccountId, RestaurantId};

    match &principal.identity {
        Identity::Admin { .. } => ReadScope::Admin,
        Identity::Restaurant { restaurant_id, .. } => ReadScope::Restaurant(RestaurantId(*restaurant_id)),
        Identity::RestaurantAccount { restaurant_account_id, .. } => {
            ReadScope::RestaurantAccount(RestaurantAccountId(*restaurant_account_id))
        }
        Identity::Customer { customer_id, .. } => ReadScope::Customer(CustomerId(*customer_id)),
        // A rider's scope is NEVER a function of the claims (#639 part C step 2b): it exists only
        // as the seam's outcome (`resolve_rider_scope` returns it beside the `Identity::Rider` it
        // minted, and never reaches this arm). Reached directly — a test-built principal — the
        // identity carries no id to derive it from, and the only honest answer is fail-closed.
        Identity::Rider { .. } => ReadScope::Public,
        Identity::Anonymous | Identity::External { .. } => ReadScope::Public,
        // The one arm that can be a DEFECT rather than a decision: an authenticated caller on a
        // role path with no domain binding. The claim/role pair cannot disagree here — the identity
        // carries both — so this counts exactly the population the contract names.
        Identity::Unbound { role, .. } => {
            telemetry::meters::read_authorization::bridge_unresolved(role_label(*role));
            ReadScope::Public
        }
    }
}

/// The `role` label of the bridge-unresolved counter — the scalars.yaml UserType text, matching the
/// generated `role_text` on the write side.
fn role_label(role: RequestRole) -> &'static str {
    match role {
        RequestRole::Public => "PUBLIC",
        RequestRole::Customer => "CUSTOMER",
        RequestRole::RestaurantAccount => "RESTAURANT_ACCOUNT",
        RequestRole::Restaurant => "RESTAURANT",
        RequestRole::Rider => "RIDER",
        RequestRole::Admin => "ADMIN",
        RequestRole::External => "EXTERNAL",
    }
}

/// [`read_scope`] for every identity/mode combination EXCEPT one: a verified `Identity::Customer`
/// under [`CustomerIdentitySource::Postgres`] resolves through the seam instead — the claim is
/// UNREAD, structurally, because the match arm below destructures `Identity::Customer` as
/// `{ sub, .. }` and never binds `customer_id` at all. Every other role, and the CUSTOMER role
/// under [`CustomerIdentitySource::Claim`] (the default), is exactly [`read_scope`]: pure, no I/O.
///
/// `NoMapping` and `LookupFailed` both resolve to `ReadScope::Public` — fail closed, identically at
/// this boundary — but are DISTINGUISHABLE in telemetry (`customer-identity` contract, #641):
/// `NoMapping` is OBSERVE (an ordinary provisioning gap), `LookupFailed` is PAGE (the seam itself
/// is unavailable).
///
/// Takes the principal BY VALUE and hands one back: for a RIDER the identity returned IS the
/// seam's outcome (see [`resolve_rider_scope`]), for every other role it is the one that came in.
async fn resolve_identity_scope(
    principal: Principal,
    correlation_id: crate::graphql::session::RequestCorrelationId,
    sources: &IdentitySources,
) -> (Principal, application::queries::ReadScope) {
    use application::queries::ReadScope;

    let (sub, resolver) = match (&principal.identity, &sources.customer) {
        (Identity::Customer { sub, .. }, CustomerIdentitySource::Postgres(resolver)) => {
            (sub.clone(), resolver.clone())
        }
        // RIDER (#639 part C step 2b): Postgres, ALWAYS, whatever the customer gate says, and
        // whatever shape the principal arrived in — the verifier yields `Unbound { role: Rider }`
        // (a token cannot prove a rider binding); a test may hand in an `Identity::Rider`. Either
        // way the seam is authoritative and REBUILDS the identity from its outcome, which is why
        // the incoming principal is dropped here. There is no claim to bind and no claim to fall
        // back to: `NoMapping` is `Public`, never "whoever the token says".
        (Identity::Unbound { sub, role: RequestRole::Rider } | Identity::Rider { sub }, _) => {
            let sub = sub.clone();
            return resolve_rider_scope(sub, correlation_id, &sources.rider).await;
        }
        // Every other combination — the remaining roles, an Unbound/Anonymous caller, or
        // CustomerIdentitySource::Claim (the default) — is the unchanged pure claims function,
        // and the principal goes back as it came.
        _ => {
            let scope = read_scope(&principal);
            return (principal, scope);
        }
    };

    let span = telemetry::spans::customer_identity_resolve(&correlation_id.0.to_string());
    let started = std::time::Instant::now();
    let outcome = resolver.resolve(&sub).instrument(span.clone()).await;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;

    let (scope, result, reason) = match &outcome {
        CustomerIdentityResolution::Resolved(customer_id) => {
            (ReadScope::Customer(*customer_id), "resolved", None)
        }
        CustomerIdentityResolution::NoMapping => {
            telemetry::meters::customer_identity::not_found();
            (ReadScope::Public, "not_found", None)
        }
        CustomerIdentityResolution::LookupFailed(reason) => {
            telemetry::meters::customer_identity::lookup_failed(reason.label());
            (ReadScope::Public, "lookup_failed", Some(reason.label()))
        }
    };
    telemetry::spans::record_customer_identity_resolve_result(&span, result, reason);
    telemetry::meters::customer_identity::duration(elapsed_ms, result);
    // Phase A resolves read scope exactly ONCE per request (both call sites, HTTP POST and WS
    // connection_init) — every actual lookup is a real Postgres round trip. `request_reuse` is
    // declared in the contract for a later resolver reusing this seam's result within the same
    // request; it is not reachable from this change alone, and this counter guarantees that shape
    // can never silently hide an outage once it exists.
    telemetry::meters::customer_identity::lookup_source("db");
    (principal, scope)
}

/// The RIDER half of [`resolve_identity_scope`] under the `rider-identity` contract: one
/// `rider.identity.resolve` span, the three-way outcome on its own counters (never the customer
/// seam's — a paging rule keyed on `customer_identity_lookup_failed_total` must not go quiet
/// because riders started failing on a differently-named seam). `NoMapping` and `LookupFailed`
/// both fail closed to `Public`; only telemetry tells them apart (OBSERVE vs PAGE).
///
/// **The one producer of [`Identity::Rider`] on the request path.** The principal handed back is
/// BUILT from the outcome, not from whatever the caller presented: a row makes an
/// `Identity::Rider` (acts RIDER, records RIDER, reads that rider), anything else is
/// `Identity::Unbound { role: Rider }` (acts PUBLIC, records PUBLIC, reads `Public`) — so the
/// write-half witness minted from this principal can only say RIDER when Postgres said so
/// (the #849 re-presentation).
async fn resolve_rider_scope(
    sub: String,
    correlation_id: crate::graphql::session::RequestCorrelationId,
    source: &RiderIdentitySource,
) -> (Principal, application::queries::ReadScope) {
    use application::queries::ReadScope;

    let span = telemetry::spans::rider_identity_resolve(&correlation_id.0.to_string());
    let started = std::time::Instant::now();
    let outcome = source.0.resolve(&sub).instrument(span.clone()).await;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;

    let unbound = || Identity::Unbound { sub: sub.clone(), role: RequestRole::Rider };
    let (identity, scope, result, reason) = match &outcome {
        RiderIdentityResolution::Resolved(rider_id) => (
            Identity::Rider { sub: sub.clone() },
            ReadScope::Rider(*rider_id),
            "resolved",
            None,
        ),
        RiderIdentityResolution::NoMapping => {
            telemetry::meters::rider_identity::not_found();
            (unbound(), ReadScope::Public, "not_found", None)
        }
        RiderIdentityResolution::LookupFailed(reason) => {
            telemetry::meters::rider_identity::lookup_failed(reason.label());
            (unbound(), ReadScope::Public, "lookup_failed", Some(reason.label()))
        }
    };
    telemetry::spans::record_rider_identity_resolve_result(&span, result, reason);
    telemetry::meters::rider_identity::duration(elapsed_ms, result);
    // ONE real Postgres round trip per request / WS connect, on the same seam the customer arm
    // uses — no second mechanism, no cache. `request_reuse` is declared for the same reason as on
    // the customer contract: so a later in-request reuse can never hide an outage.
    telemetry::meters::rider_identity::lookup_source("db");
    (Principal { identity }, scope)
}

/// [`resolve_identity_scope`] under the contract's `auth.read_scope` span (ONE per request),
/// stamped with the request's correlation id. This is the transport entry point; the function
/// above resolves the CUSTOMER-Postgres arm, delegating everything else to [`read_scope`], the pure
/// claims function.
///
/// The id is PASSED IN, never minted here (#451 Phase 2b): reads carry no command envelope, so the
/// server mints `request.correlation_id` once at the transport boundary
/// ([`crate::graphql::session::RequestCorrelationId`]) and every read-path span of that request —
/// `auth.read_scope` here, `cart.price` at the pricing seam — records the SAME value. Minting one
/// per span made a single request emit several unrelated ids, which is the attribute present and
/// the correlation absent.
///
/// `customer_identity` is resolved ONCE at startup/config-load and REUSED across every request of
/// the process (`CustomerIdentitySource`, gate-then-stabilize) — never a per-request fallback: the
/// Postgres arm, when selected, never falls back to the claim on `NoMapping` or `LookupFailed`,
/// both of which fail closed to `Public` instead.
///
/// **Consumes the principal and hands back the one the request runs as** (the #849
/// re-presentation): for a RIDER the identity returned is the seam's OUTCOME — `Identity::Rider`
/// only when a row answered, `Identity::Unbound` otherwise — and the pre-seam principal no longer
/// exists to mint anything from. Both halves of the request read this one value: the
/// `ActingRole` the guards check is minted from it AFTER this call
/// (`routes::authorize_and_resolve_scope`), and `recorded_role` stamps the envelope from it.
pub async fn resolve_read_scope(
    principal: Principal,
    correlation_id: crate::graphql::session::RequestCorrelationId,
    sources: &IdentitySources,
) -> (Principal, application::queries::ReadScope) {
    let span = telemetry::spans::auth_read_scope(&format!("{:?}", principal.recorded_role()));
    span.record("business.correlation_id", correlation_id.0.to_string().as_str());
    let (principal, scope) = resolve_identity_scope(principal, correlation_id, sources)
        .instrument(span.clone())
        .await;
    // `business.role` re-recorded from the principal the seam handed back: at span open a RIDER
    // reads as the verifier's answer (PUBLIC — a token proves no binding); the value that matters
    // is the seam's, the same role the mutation envelope will stamp.
    span.record("business.role", format!("{:?}", principal.recorded_role()).as_str());
    // Asked of the identity AND the scope it actually resolved to (`Principal::bridge_resolved`),
    // because the inline form this replaced read the role — and went silently "always true" the
    // moment an Unbound caller stopped reporting its declared role (#639 part B), turning the one
    // population this attribute exists to surface into a healthy reading. Two populations are
    // `false` now and both are meant to be: an Unbound caller, and a bound one whose Postgres
    // lookup said no or could not answer (#641).
    telemetry::spans::record_bridge_resolved(&span, principal.bridge_resolved(&scope));
    (principal, scope)
}

#[cfg(test)]
mod read_scope_tests {
    use application::queries::ReadScope;
    use domain::generated::scalars::{CustomerId, RestaurantAccountId, RestaurantId, RiderId};

    use super::*;

    /// A role-path principal built the way `authorize()` builds one — the verified `sub` plus this
    /// product's claim object. Nothing here reaches inside the type: since the identity is a single
    /// private value, a test CANNOT hand-assemble a role/claim pair the constructor would refuse,
    /// which is the guarantee the previous field-bag shape could not give.
    fn principal(role: RequestRole, sub: &str, claim: Option<uuid::Uuid>) -> Principal {
        let claim = claim.map(|c| c.to_string());
        let claims = match role {
            RequestRole::Customer => ProductClaims { customer_id: claim, ..Default::default() },
            RequestRole::Restaurant => ProductClaims { restaurant_id: claim, ..Default::default() },
            RequestRole::RestaurantAccount => {
                ProductClaims { restaurant_account_id: claim, ..Default::default() }
            }
            _ => ProductClaims::default(),
        };
        Principal::role_path(role, sub.to_string(), &claims)
    }

    /// The pure claims function, per role. The load-bearing data shape (beck): `sub` and the claim
    /// are DIFFERENT uuids — an implementation that still derives identity from `sub` fails these
    /// assertions instead of passing by coincidence. Seen RED by re-planting #430's fallbacks
    /// (customer via `user_id`, rider via `sub`-parse): the absent-claim arms below went red with
    /// "sub is never an identity", green restored on removal.
    #[test]
    fn read_scope_is_a_pure_claims_function() {
        let sub = uuid::Uuid::from_u128(1).to_string();

        // Claim present -> the DOMAIN id, verbatim — never the subject.
        let customer_claim = uuid::Uuid::from_u128(2);
        let p = principal(RequestRole::Customer, &sub, Some(customer_claim));
        assert_eq!(read_scope(&p), ReadScope::Customer(CustomerId(customer_claim)));
        assert_eq!(p.user_id(), Some(sub.as_str()), "the subject rides along, unused as identity");

        // A RIDER is the one role whose scope is NEVER a claims function (#639 part C step 2b):
        // `ProductClaims` has no rider field, so the binding argument has nowhere to land, and
        // the pure function fails closed. The seam (`resolve_read_scope`) is the only door — the
        // §10 pair below is where a rider actually resolves.
        let p = principal(RequestRole::Rider, &sub, Some(uuid::Uuid::from_u128(4)));
        assert_eq!(read_scope(&p), ReadScope::Public, "a rider never resolves without Postgres");
        assert_eq!(
            p.recorded_role(),
            RequestRole::Public,
            "and until the seam binds it a rider token is Unbound: it records PUBLIC, not the role \
             it asserts — RIDER here was a false author (the #849 re-presentation)"
        );
        assert_eq!(
            p.acting_role(RequestRole::Rider).get(),
            RequestRole::Public,
            "and acts as PUBLIC on the write half, for the same reason"
        );

        let rid = uuid::Uuid::from_u128(11);
        let p = principal(RequestRole::Restaurant, "s", Some(rid));
        assert_eq!(read_scope(&p), ReadScope::Restaurant(RestaurantId(rid)));
        let p = principal(RequestRole::RestaurantAccount, "s", Some(rid));
        assert_eq!(read_scope(&p), ReadScope::RestaurantAccount(RestaurantAccountId(rid)));

        // Claim ABSENT -> Public, even with a perfectly parseable sub: sub is never an identity.
        // The declared role survives INSIDE the identity, which is what keeps the denial
        // attributable in `read_authorization_bridge_unresolved_total{role}` — `read_scope`
        // destructures `Identity::Unbound { role, .. }` to label the counter. What it must NOT
        // survive is anything downstream believing it: this assertion used to read
        // `unbound.role() == Customer, "the unbound caller keeps its role"` and was GREEN, which
        // is the #639 part B defect stated as a passing test.
        let unbound = principal(RequestRole::Customer, &sub, None);
        assert_eq!(read_scope(&unbound), ReadScope::Public, "sub is never an identity (customer)");
        assert_eq!(
            unbound.recorded_role(),
            RequestRole::Public,
            "an unbound caller records as PUBLIC — stamping CUSTOMER into domain_events.user_type \
             would be a false author in an immutable log"
        );
        assert!(
            !unbound.bridge_resolved(&read_scope(&unbound)),
            "and it is the population `bridge_resolved: false` names"
        );
        // The other end, which an identity-only predicate got wrong (#641): a BOUND caller whose
        // Postgres lookup said no, or could not answer, also degrades to Public and must also read
        // as unresolved — otherwise the seam's own outage reports healthy.
        let bound = principal(RequestRole::Customer, &sub, Some(uuid::Uuid::from_u128(2)));
        assert!(
            bound.bridge_resolved(&read_scope(&bound)),
            "a bound caller that resolved is resolved"
        );
        assert!(
            !bound.bridge_resolved(&ReadScope::Public),
            "a bound caller that resolved to Public did NOT resolve — the #641 NoMapping / \
             LookupFailed population"
        );
        assert_eq!(
            read_scope(&principal(RequestRole::Rider, &sub, None)),
            ReadScope::Public,
            "sub is never an identity (rider — #430's placeholder must stay dead)"
        );
        assert_eq!(read_scope(&principal(RequestRole::Restaurant, "s", None)), ReadScope::Public);
        assert_eq!(
            read_scope(&principal(RequestRole::RestaurantAccount, "s", None)),
            ReadScope::Public
        );

        // Role decisions, no claims involved.
        assert_eq!(read_scope(&principal(RequestRole::Admin, "s", None)), ReadScope::Admin);
        assert_eq!(read_scope(&Principal::anonymous()), ReadScope::Public);
        assert_eq!(read_scope(&principal(RequestRole::External, "s", None)), ReadScope::Public);
        assert_eq!(read_scope(&Principal::external_service()), ReadScope::Public);
    }

    /// The claim that does NOT match the path role is dropped at construction, not merely ignored
    /// downstream (#469 review round 2). A `/restaurant` token carrying `customer_id` used to keep
    /// it on the principal; now the RESTAURANT identity has nowhere to put it.
    #[test]
    fn a_role_path_principal_keeps_only_the_claim_of_its_own_role() {
        let every_claim = ProductClaims {
            role: Some("RESTAURANT".into()),
            restaurant_id: Some(uuid::Uuid::from_u128(11).to_string()),
            restaurant_account_id: Some(uuid::Uuid::from_u128(12).to_string()),
            customer_id: Some(uuid::Uuid::from_u128(13).to_string()),
        };
        let p = Principal::role_path(RequestRole::Restaurant, "sub".into(), &every_claim);
        assert_eq!(
            p.identity,
            Identity::Restaurant { sub: "sub".into(), restaurant_id: uuid::Uuid::from_u128(11) }
        );
        assert_eq!(read_scope(&p), ReadScope::Restaurant(RestaurantId(uuid::Uuid::from_u128(11))));
    }

    /// The serde seam the pure test cannot reach: a misspelled claim field name would silently
    /// deserialize to `None` -> Public everywhere, and the first detector would be a production
    /// smoke timeout. All four keys pinned, INSIDE the product object; a malformed uuid claim fails
    /// closed. The nesting is pinned too (#519): the same keys at the TOP level of `app_metadata`
    /// are a stranger's metadata, and must deserialize to no grant at all.
    #[test]
    fn app_metadata_claims_deserialize_and_malformed_uuids_fail_closed() {
        let meta: AppMetadata = serde_json::from_str(
            r#"{
                "provider": "phone",
                "captain_food": {
                    "role": "CUSTOMER",
                    "restaurant_id": "00000000-0000-0000-0000-000000000001",
                    "restaurant_account_id": "00000000-0000-0000-0000-000000000002",
                    "customer_id": "00000000-0000-0000-0000-000000000003",
                    "rider_id": "00000000-0000-0000-0000-000000000004"
                }
            }"#,
        )
        .expect("app_metadata blob");
        let grant = meta.grant().expect("a CUSTOMER role parses into a grant");
        assert_eq!(grant.role, RequestRole::Customer);
        assert_eq!(claim_uuid(&grant.claims.restaurant_id), Some(uuid::Uuid::from_u128(1)));
        assert_eq!(claim_uuid(&grant.claims.restaurant_account_id), Some(uuid::Uuid::from_u128(2)));
        assert_eq!(claim_uuid(&grant.claims.customer_id), Some(uuid::Uuid::from_u128(3)));
        // `rider_id` stays IN the fixture and is asserted NOWHERE: since #639 part C step 2b the
        // product parses no such claim (a rider binds through Postgres at the seam), so the key
        // is a stranger's — inert, not refused, exactly like the flat pre-#519 keys below.

        // Garbage never widens into an identity — indistinguishable from absent, by design.
        assert_eq!(claim_uuid(&Some("not-a-uuid".into())), None);
        assert_eq!(claim_uuid(&None), None);

        // The pre-#519 FLAT shape, and a sibling product's object: neither is a grant. Supabase
        // merges `app_metadata` shallowly, so the flat keys genuinely survive next to ours on an
        // already-stamped auth user — this is what keeps them inert rather than authoritative.
        for stranger in [
            r#"{"captain_role":"ADMIN","captain_customer_id":"00000000-0000-0000-0000-000000000003"}"#,
            r#"{"other_product":{"role":"ADMIN"}}"#,
            r#"{}"#,
        ] {
            let meta: AppMetadata = serde_json::from_str(stranger).expect("app_metadata blob");
            assert!(meta.grant().is_none(), "no captain_food object is no grant: {stranger}");
        }
    }

    // #430's `an_empty_resolver_degrades_to_public` is DELETED deliberately: its premise (a
    // resolver whose DB dependency may be missing) dissolved when resolution became a pure claims
    // function — there is no dependency left to be missing.

    // ---- The §10 PAIR (PROP-20260831-180622, #639 part C step 2b: the rider sign-in door) ----
    //
    // `beck`: the pair is the whole point. "Try Postgres, else fall back to the claim" — the
    // implementation a reasonable person writes — passes (a) and FAILS (b), and (b) is the slice:
    // a rider the projector has not caught up on must be nobody, not whoever the token says.

    /// A scripted `Rider` table: `auth_subject -> rider_id` rows, `NoMapping` for everything else.
    struct ScriptedRiderTable(std::collections::HashMap<String, uuid::Uuid>);

    #[async_trait::async_trait]
    impl ResolveRiderIdentity for ScriptedRiderTable {
        async fn resolve(&self, auth_subject: &str) -> RiderIdentityResolution {
            match self.0.get(auth_subject) {
                Some(id) => RiderIdentityResolution::Resolved(RiderId(*id)),
                None => RiderIdentityResolution::NoMapping,
            }
        }
    }

    fn sources_with_rider_table(rows: &[(&str, uuid::Uuid)]) -> IdentitySources {
        IdentitySources {
            customer: CustomerIdentitySource::Claim,
            rider: RiderIdentitySource::new(Arc::new(ScriptedRiderTable(
                rows.iter().map(|(sub, id)| (sub.to_string(), *id)).collect(),
            ))),
        }
    }

    /// A RIDER principal built through the REAL claims path — a `captain_food` object carrying
    /// `role: RIDER` AND a `rider_id` — so the pair proves the seam ignores a claim that is
    /// actually present in the token, not one the fixture never planted.
    fn rider_token_principal(sub: &str, claim_rider_id: uuid::Uuid) -> Principal {
        let meta: AppMetadata = serde_json::from_value(serde_json::json!({
            "captain_food": { "role": "RIDER", "rider_id": claim_rider_id.to_string() }
        }))
        .expect("app_metadata blob");
        let grant = meta.grant().expect("a RIDER role parses into a grant");
        Principal::role_path(RequestRole::Rider, sub.to_string(), &grant.claims)
    }

    /// (a) a rider WITH a row resolves to that row's riderId, even when the JWT claim says
    /// something else — Postgres wins over the claim.
    #[tokio::test]
    async fn a_rider_with_a_row_resolves_to_that_row_never_to_the_claim() {
        let sub = "auth-supabase-rider-S";
        let rider_a = uuid::Uuid::from_u128(0xA);
        let rider_b = uuid::Uuid::from_u128(0xB);
        let principal = rider_token_principal(sub, rider_b);
        assert_eq!(
            principal.acting_role(RequestRole::Rider).get(),
            RequestRole::Public,
            "before the seam answers, a rider token can act as nobody"
        );

        let (principal, scope) = resolve_read_scope(
            principal,
            crate::graphql::session::RequestCorrelationId::mint(),
            &sources_with_rider_table(&[(sub, rider_a)]),
        )
        .await;

        assert_eq!(scope, ReadScope::Rider(RiderId(rider_a)), "Postgres wins over the claim");
        assert_ne!(
            scope,
            ReadScope::Rider(RiderId(rider_b)),
            "the claim's rider id is never the scope"
        );
        assert!(principal.bridge_resolved(&scope), "a resolved rider reads as resolved");
        // The write half reads the SAME outcome (#849 re-presentation): the principal handed back
        // is the seam's, so the witness and the envelope say RIDER because Postgres did.
        assert_eq!(principal.acting_role(RequestRole::Rider).get(), RequestRole::Rider);
        assert_eq!(principal.recorded_role(), RequestRole::Rider);
        assert_eq!(principal.user_id(), Some(sub), "the subject rides along");
    }

    /// (b) a rider with NO row resolves to `ReadScope::Public` — specifically NOT the claim's rider
    /// id. This is the slice itself: without it the claim stays authoritative for everyone the
    /// projector has not caught up on.
    #[tokio::test]
    async fn a_rider_with_no_row_resolves_to_public_never_to_the_claim() {
        let sub = "auth-supabase-rider-S";
        let rider_b = uuid::Uuid::from_u128(0xB);
        let principal = rider_token_principal(sub, rider_b);

        let (principal, scope) = resolve_read_scope(
            principal,
            crate::graphql::session::RequestCorrelationId::mint(),
            &sources_with_rider_table(&[]),
        )
        .await;

        assert_eq!(scope, ReadScope::Public, "no row -> fail closed");
        assert_ne!(
            scope,
            ReadScope::Rider(RiderId(rider_b)),
            "and specifically NOT the claim's rider: try-Postgres-else-claim passes (a) and fails here"
        );
        assert!(
            !principal.bridge_resolved(&scope),
            "an unmapped rider is the population `bridge_resolved: false` names"
        );
        // And nobody on the write half either (#849 re-presentation): the witness minted from this
        // principal is PUBLIC and the envelope records PUBLIC — as first pushed, both said RIDER.
        assert_eq!(
            principal.acting_role(RequestRole::Rider).get(),
            RequestRole::Public,
            "no row -> cannot act as a rider"
        );
        assert_eq!(
            principal.recorded_role(),
            RequestRole::Public,
            "no row -> no RIDER author in domain_events.user_type"
        );
    }

    /// The seam is authoritative whatever shape a rider principal arrives in: a test-built BOUND
    /// rider (`Principal::role_binding(Rider, _, Some(_))`) handed to a seam with no row comes back
    /// Unbound — there is no way to keep a RIDER witness the table does not back.
    #[tokio::test]
    async fn a_test_built_bound_rider_is_demoted_by_a_seam_with_no_row() {
        let principal =
            Principal::role_binding(RequestRole::Rider, "s".into(), Some(uuid::Uuid::from_u128(1)));
        assert_eq!(principal.acting_role(RequestRole::Rider).get(), RequestRole::Rider);
        let (principal, scope) = resolve_read_scope(
            principal,
            crate::graphql::session::RequestCorrelationId::mint(),
            &sources_with_rider_table(&[]),
        )
        .await;
        assert_eq!(scope, ReadScope::Public);
        assert_eq!(principal.acting_role(RequestRole::Rider).get(), RequestRole::Public);
        assert_eq!(principal.recorded_role(), RequestRole::Public);
    }
}
