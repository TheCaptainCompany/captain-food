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
    #[derive(Debug, Error, PartialEq)]
    pub enum DomainError {
        /// An anticipated `errors.yaml` rejection: the stable PascalCase CODE (= the errors.yaml key =
        /// the wire `extensions.code`, GraphQL error contract P-10) plus the error's typed context as
        /// JSON — field names are the errors.yaml `context` keys (camelCase). The context feeds the
        /// `{placeholder}` interpolation of the catalogued `en`/`fr` message templates
        /// (`crate::generated::errors`) and is surfaced under the GraphQL error extensions.
        #[error("{code}: {context}")]
        Rejected { code: String, context: serde_json::Value },
        /// Legacy stringly-typed invariant, kept for NON-catalogued failures: the event store's
        /// optimistic-concurrency version conflict and interim adapters still carrying the old
        /// `"<Code>: <detail>"` shape (e.g. the fail-closed payment stand-in). Command handlers reject
        /// with [`DomainError::Rejected`] instead.
        #[error("invariant violated: {0}")]
        Invariant(String),
        /// A dependency (repository/adapter) failed — e.g. a read-model query or the event store. Carried
        /// here so read ports can return `Result<_, DomainError>` without leaking the adapter's error type.
        #[error("repository error: {0}")]
        Repository(String),
    }

    impl DomainError {
        /// Build the canonical `errors.yaml` rejection: `code` is the PascalCase key (checked against
        /// the generated catalog in debug builds), `context` the error's typed fields as a JSON object
        /// (camelCase keys, as declared by the error's `context` in errors.yaml).
        pub fn rejected(code: impl Into<String>, context: serde_json::Value) -> Self {
            let code = code.into();
            debug_assert!(
                crate::generated::errors::find(&code).is_some(),
                "'{code}' is not an errors.yaml error code"
            );
            DomainError::Rejected { code, context }
        }

        /// The `errors.yaml` code this error carries, if it is an anticipated rejection.
        pub fn code(&self) -> Option<&str> {
            match self {
                DomainError::Rejected { code, .. } => Some(code),
                _ => None,
            }
        }
    }
}

pub mod text {
    //! Pure text helpers shared across the layers. `slugify` lives here rather than in an ACL because
    //! it encodes a DOMAIN rule -- the `Slug` scalar's pattern -- and more than one boundary needs it
    //! (the SIRENE mapping and the HubRise catalog import).

    /// Lowercase-dash slug matching `^[a-z0-9]+(?:-[a-z0-9]+)*$`, with French accents folded to ASCII.
    /// Non-alphanumeric runs collapse to a single dash; leading/trailing dashes are trimmed.
    pub fn slugify(name: &str) -> String {
        let mut out = String::with_capacity(name.len());
        for c in name.chars() {
            let folded: &str = match c {
                'à' | 'â' | 'ä' | 'á' | 'ã' | 'À' | 'Â' | 'Ä' | 'Á' | 'Ã' => "a",
                'ç' | 'Ç' => "c",
                'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => "e",
                'î' | 'ï' | 'í' | 'Î' | 'Ï' | 'Í' => "i",
                'ô' | 'ö' | 'ó' | 'õ' | 'Ô' | 'Ö' | 'Ó' | 'Õ' => "o",
                'ù' | 'û' | 'ü' | 'ú' | 'Ù' | 'Û' | 'Ü' | 'Ú' => "u",
                'ÿ' | 'Ÿ' | 'ý' => "y",
                'ñ' | 'Ñ' => "n",
                'œ' | 'Œ' => "oe",
                'æ' | 'Æ' => "ae",
                _ => {
                    if c.is_ascii_alphanumeric() {
                        out.push(c.to_ascii_lowercase());
                    } else if !out.ends_with('-') && !out.is_empty() {
                        out.push('-');
                    }
                    continue;
                }
            };
            out.push_str(folded);
        }
        out.trim_matches('-').to_string()
    }
}
