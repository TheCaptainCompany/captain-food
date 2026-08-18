//! The read-surface scope binding mode (#618, DECISIONS §39 IDOR-1).
//!
//! `READ_SCOPE_BINDING_MODE` decides, per request, whether a tenant-scoped READ is bound to the
//! caller's own `ReadScope`. It governs the binding COMPARISON only, never the absence of an
//! identity: an unbound caller is denied under [`Observe`](ReadScopeBindingMode::Observe) and
//! [`Enforce`](ReadScopeBindingMode::Enforce) alike, because that is not a binding — it is the
//! absence of one, and the system already answers it (a missing claim fails closed inside
//! `read_scope`). Only [`Off`](ReadScopeBindingMode::Off) restores it.
//!
//! The four-row behaviour table is at the key's declaration site in
//! `specs/common/configuration.yaml`, not here and not in a dispatch card: a mode enum whose
//! meaning lives only in a card is a convention rather than a published term.
//!
//! **The value is per-REQUEST data, deliberately, and not a process global.** A `OnceLock` read
//! from the composition root would be cheaper and would make the two-modes-differ probe
//! unwritable — one test binary cannot install two process globals — which is the same reason the
//! metric probe lives in a binary of its own.
//!
//! **What "read per request" actually costs to flip.** The value's SOURCE is the typed `Config`,
//! resolved at startup, so flipping it is an env override plus a pod restart: no rebuild, no
//! image, no CI, no migration. It is not a live toggle — no runtime settings source exists in this
//! system.

/// Whether, and how hard, a tenant-scoped read is bound to the caller's `ReadScope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadScopeBindingMode {
    /// The incident rollback rung: today's pre-#618 behaviour, exactly. Every RESTAURANT
    /// credential reads whatever it asks for, including the whole platform's queue when it asks
    /// for nothing.
    ///
    /// ⚠️ This rung has a SUNSET. While no restaurant credential exists it is a harmless rollback.
    /// The moment a real restaurateur holds one it stops being a rollback and becomes a
    /// cross-member disclosure switch an operator can throw with no second signature.
    Off,
    /// The shipped default. The unbound caller is denied and a BOUND caller supplying another
    /// restaurant's id is still SERVED that restaurant's rows — the mismatch is only counted.
    ///
    /// Free today, and that is what makes the eventual flip a reading rather than a guess:
    /// `Observe` and `Enforce` are observationally identical until a restaurant claim exists, and
    /// nothing mints one (the only claim writer in the tree stamps `{role, customer_id}` and has
    /// no restaurant sibling).
    Observe,
    /// The binding applies in full: a filter may narrow within the caller's scope and never move
    /// it, so a conflicting filter intersects to EMPTY.
    Enforce,
}

impl ReadScopeBindingMode {
    /// The mode in force for this request. **Absent from the context ⇒ [`Enforce`]**: fail closed,
    /// the same posture as `ctx.data_opt::<ReadScope>().unwrap_or(Public)` on every scoped
    /// resolver. An `Option` defaulted inside a resolver would make "forgot to wire it"
    /// indistinguishable from "chose the permissive value", which is the whole defect class.
    ///
    /// [`Enforce`]: ReadScopeBindingMode::Enforce
    pub fn from_context(ctx: &async_graphql::Context<'_>) -> Self {
        ctx.data_opt::<Self>().copied().unwrap_or(Self::Enforce)
    }

    /// Resolve the declared `READ_SCOPE_BINDING_MODE` value.
    ///
    /// An UNRECOGNISED value resolves to [`Enforce`], not to the declared default: the config
    /// emitter validates patterns, and an enum key declares none (a scalar with no `pattern` is a
    /// validation that silently does nothing — `config-scalar-no-pattern`), so a typo reaches
    /// here unfiltered. Falling back to the permissive end of an authorization switch on a typo is
    /// how a mode becomes a hole. It is reported, because a silent correction is worse than either.
    ///
    /// [`Enforce`]: ReadScopeBindingMode::Enforce
    pub fn from_declared(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" => Self::Off,
            "observe" => Self::Observe,
            "enforce" => Self::Enforce,
            other => {
                tracing::warn!(
                    value = other,
                    "READ_SCOPE_BINDING_MODE: unrecognised value -- using enforce (fail closed)"
                );
                Self::Enforce
            }
        }
    }

    /// Whether the binding is applied rather than merely observed.
    pub fn is_enforcing(self) -> bool {
        matches!(self, Self::Enforce)
    }

    /// The declared token, as it appears in `specs/common/configuration.yaml` and on the
    /// `read_authorization_scope_mismatch_total{mode}` series.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Observe => "observe",
            Self::Enforce => "enforce",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three declared tokens round-trip, and anything else fails CLOSED rather than falling
    /// back to the declared default. A typo on an authorization switch must never widen it.
    #[test]
    fn an_unrecognised_value_fails_closed_rather_than_defaulting() {
        assert_eq!(ReadScopeBindingMode::from_declared("off"), ReadScopeBindingMode::Off);
        assert_eq!(ReadScopeBindingMode::from_declared("observe"), ReadScopeBindingMode::Observe);
        assert_eq!(ReadScopeBindingMode::from_declared("ENFORCE"), ReadScopeBindingMode::Enforce);
        assert_eq!(
            (
                ReadScopeBindingMode::from_declared("of"),
                ReadScopeBindingMode::from_declared(""),
                ReadScopeBindingMode::from_declared("observe"),
            ),
            (
                ReadScopeBindingMode::Enforce,
                ReadScopeBindingMode::Enforce,
                ReadScopeBindingMode::Observe,
            ),
            "a typo must not resolve to the permissive end of an authorization switch"
        );
    }

    /// Every token that reaches the metric series is the one the spec declares.
    #[test]
    fn the_metric_label_is_the_declared_token() {
        assert_eq!(
            [
                ReadScopeBindingMode::Off.as_str(),
                ReadScopeBindingMode::Observe.as_str(),
                ReadScopeBindingMode::Enforce.as_str()
            ],
            ["off", "observe", "enforce"]
        );
    }
}
