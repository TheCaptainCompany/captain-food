//! Shared domain vocabulary: value objects, typed identifiers, domain errors (ADR-0035).

pub mod value_objects {
    //! Value objects are GENERATED from `specs/entities.yaml` (ADR-0034 #3) and re-exported here so the
    //! stable `domain::shared::value_objects::…` path keeps resolving across the layers. Money is integer
    //! minor units + ISO currency (CLAUDE.md convention); the HubRise `"9.80 EUR"` string form is
    //! converted only at the ACL.
    pub use crate::generated::entities::Money;
}

pub mod identifiers {
    //! Strongly-typed aggregate ids — one dedicated type per aggregate, client-generated (ADR-0034) so
    //! creates are idempotent. The types are GENERATED from `scalars.yaml` (ADR-0034 #3) and re-exported
    //! here so the stable `domain::shared::identifiers::…` path keeps resolving across the layers.
    pub use crate::generated::scalars::RestaurantId;
}

pub mod errors {
    use thiserror::Error;

    /// Domain-level failure (an invariant a command handler may reject). Anticipated business errors are
    /// modelled in `specs/errors.yaml`; this is the crate-local umbrella type.
    #[derive(Debug, Error, PartialEq, Eq)]
    pub enum DomainError {
        #[error("invariant violated: {0}")]
        Invariant(String),
    }
}
