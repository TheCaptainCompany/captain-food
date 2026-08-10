//! `actor_runtime`'s LOCAL copy of the #474 database-test gate — the one place in the repo that
//! deliberately does not call `db-test-gate`.
//!
//! Why the copy exists: `tests/dependency_rule.rs` asserts this crate's manifest carries **no path
//! dependency into the workspace at all** (ADR-20260730-234918 — extraction-readiness: lifting
//! `actor_runtime` into another product is a `git mv`, not a rewrite). A `[dev-dependencies]`
//! entry for `db-test-gate` would break that gate, and weakening an executable rule to make a
//! refactor tidy is not on the table. So the decision is restated here, in the crate's own test
//! tree, where extraction carries it along for free.
//!
//! Why the copy cannot drift: the codegen guard
//! `only_the_db_test_gate_spells_the_database_skip_polarity` (`tools/codegen-rs/src/tests.rs`)
//! allowlists exactly two files that may read `DB_TESTS_REQUIRED` — `crates/db_test_gate/src/lib.rs`
//! and this one — so a third copy cannot appear silently, and the table below is asserted by the
//! same cases the shared crate asserts.
//!
//! The contract is identical to `db_test_gate`: a database is REQUIRED by default; only the four
//! literal spellings `0` / `false` / `no` / `off` opt out; anything else fails loudly.

/// The four literal opt-out spellings, compared case-insensitively (see the module docs).
const OPT_OUT: [&str; 4] = ["0", "false", "no", "off"];

/// Pure decision, so the table is testable without `env::set_var` (banned workspace-wide, #388).
/// `true` = the suite may skip; `false` = a database is required.
pub(crate) fn may_skip(db_tests_required: Option<&str>) -> bool {
    let flag = db_tests_required.map(str::trim).unwrap_or("");
    OPT_OUT.iter().any(|o| flag.eq_ignore_ascii_case(o))
}

/// `Some(url)` to run, `None` to skip. Panics when a database is required and none is configured.
pub(crate) fn database_url(suite: &str) -> Option<String> {
    let url = std::env::var("DATABASE_URL").ok();
    let url = url.as_deref().map(str::trim).filter(|u| !u.is_empty());
    if let Some(url) = url {
        return Some(url.to_string());
    }
    let flag = std::env::var("DB_TESTS_REQUIRED").ok();
    assert!(
        may_skip(flag.as_deref()),
        "{suite}: DATABASE_URL is not set, so this database-gated suite cannot run -- and since \
         #474 a missing database FAILS instead of skipping silently.\n\
         Run it for real:  DATABASE_URL=postgres://... make test-crates\n\
         Opt out loudly:   DB_TESTS_REQUIRED=0 make test-crates"
    );
    eprintln!(
        "SKIP[db] {suite}: no DATABASE_URL, skipped by DB_TESTS_REQUIRED={} \
         -- this suite exercised NO database behaviour.",
        flag.unwrap_or_default()
    );
    None
}

#[cfg(test)]
mod tests {
    /// The same table `db_test_gate` asserts, restated here so the copy cannot drift in meaning
    /// even though the guard only proves it has not drifted in location.
    #[test]
    fn the_polarity_matches_the_shared_gate() {
        for opt_out in ["0", "false", "no", "off", "FALSE", "Off"] {
            assert!(super::may_skip(Some(opt_out)), "{opt_out} should opt out");
        }
        for required in ["", "  ", "1", "true", "nope", "flase", "2"] {
            assert!(!super::may_skip(Some(required)), "{required:?} must not opt out");
        }
        assert!(!super::may_skip(None), "unset means REQUIRED -- that is the #474 inversion");
    }
}
