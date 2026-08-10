//! The database-test gate: **DB-gated suites are REQUIRED by default** (#474).
//!
//! # Why this crate exists
//!
//! Before #474 the gate was the opposite way round, and it was written out by hand at **seventeen
//! call sites across five crates**: no `DATABASE_URL` meant *skip silently*, and setting
//! `DB_TESTS_REQUIRED` was the opt-IN that made the skip loud (#230). Two things were measured on
//! the #474 branch, against a deliberately planted migration defect that bricks the Cart
//! projection:
//!
//! 1. `cargo test --workspace` with no `DATABASE_URL` reported **990 passed / 0 failed** and exit
//!    0. The same command with a real Postgres and `DB_TESTS_REQUIRED=1` failed
//!    (`cart_projection::cart_events_fold_into_the_read_model`, `RowNotFound`). The headline total
//!    is byte-identical whether the database behaviour ran or not.
//! 2. The old `eprintln!("SKIP …")` lines were **invisible**: `grep -c SKIP` over that whole
//!    990-test log returns **0**, because libtest captures a passing test's stdout AND stderr and
//!    only replays it for failures. The skip was not merely quiet, it was unobservable.
//!
//! So the polarity is inverted here and the decision lives in ONE place. A test crate cannot get
//! the polarity wrong because it cannot spell the decision at all — it calls [`database_url`] or
//! it has no database (compiler-first, ADR-20260803-234035).
//!
//! # The contract
//!
//! | `DATABASE_URL` | `DB_TESTS_REQUIRED` | outcome |
//! |---|---|---|
//! | set, non-empty | anything | the suite runs against it |
//! | unset/empty | unset, empty, or anything not an explicit opt-out | **panic** — loud failure |
//! | unset/empty | `0` / `false` / `no` / `off` (any case) | skip, **with a receipt** |
//!
//! Fail-closed on purpose: only the four literal opt-out spellings skip. `DB_TESTS_REQUIRED=1`
//! keeps meaning "required", so every existing CI invocation and every documented developer
//! command keeps working unchanged.
//!
//! # The receipt
//!
//! Because libtest swallows the per-suite line, a skip also **appends the suite name to a receipt
//! file**, which `make test-crates` truncates before the run and reads after it to print the
//! summary line naming exactly which suites did not exercise the database. Losing the receipt can
//! only cost visibility, never correctness — the loud path is the panic, which no capture can hide.

use std::io::Write;

/// What the gate decides, given the two environment values. Pure: no environment access, no I/O —
/// the house pattern from `clippy.toml` (a pure core plus an env-reading shell), so the table above
/// is unit-testable without `env::set_var`, which is banned workspace-wide (#388).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// A database is configured: run the suite against this URL.
    Run(String),
    /// No database, and the caller explicitly opted out: skip, and leave a receipt.
    SkipByOptOut {
        /// The literal opt-out value, quoted back in the receipt so it is obvious who asked.
        opt_out: String,
    },
    /// No database and no explicit opt-out: the run must FAIL. This is the inversion.
    Required,
}

/// The four literal opt-out spellings, compared case-insensitively. Deliberately a closed list:
/// any other value — including the empty string, a typo, or `DB_TESTS_REQUIRED=maybe` — means
/// REQUIRED, so a mistyped opt-out fails the run instead of silently granting itself one.
const OPT_OUT: [&str; 4] = ["0", "false", "no", "off"];

/// The gate's whole decision, as a pure function of the two variables.
///
/// `database_url` and `db_tests_required` are the raw `Option<&str>` reads. An empty string is
/// treated as unset for both: `DATABASE_URL=` in a CI matrix is a missing value, not a connection
/// string, and it would otherwise fail deep inside `PgPool::connect` with a parse error rather
/// than here with an explanation.
pub fn decide(database_url: Option<&str>, db_tests_required: Option<&str>) -> Verdict {
    let url = database_url.map(str::trim).filter(|u| !u.is_empty());
    if let Some(url) = url {
        return Verdict::Run(url.to_string());
    }
    let flag = db_tests_required.map(str::trim).unwrap_or("");
    if OPT_OUT.iter().any(|o| flag.eq_ignore_ascii_case(o)) {
        return Verdict::SkipByOptOut { opt_out: flag.to_string() };
    }
    Verdict::Required
}

/// The env-reading shell every DB-gated suite calls.
///
/// Returns `Some(url)` to run, `None` to skip. **Panics** when a database is required and none is
/// configured — that panic is the point of #474: a run that cannot exercise the database must not
/// be able to report success.
///
/// `suite` names the caller for the receipt and for the panic message; use the test-module or
/// suite name, not the file path.
#[must_use]
pub fn database_url(suite: &str) -> Option<String> {
    let url = std::env::var("DATABASE_URL").ok();
    let required = std::env::var("DB_TESTS_REQUIRED").ok();
    match decide(url.as_deref(), required.as_deref()) {
        Verdict::Run(url) => Some(url),
        Verdict::SkipByOptOut { opt_out } => {
            let line = format!(
                "SKIP[db] {suite}: no DATABASE_URL, skipped by DB_TESTS_REQUIRED={opt_out} \
                 -- this suite exercised NO database behaviour."
            );
            // Visible under `--nocapture`; libtest swallows it otherwise, which is exactly why the
            // receipt below exists rather than this line being the whole mechanism.
            eprintln!("{line}");
            append_receipt(suite, &opt_out);
            None
        }
        Verdict::Required => panic!(
            "{suite}: DATABASE_URL is not set, so this database-gated suite cannot run -- and \
             since #474 a missing database FAILS instead of skipping silently.\n\
             \n\
             Run it for real:   DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres make test-crates\n\
             (docs/claude/sessions.md carries the ~40s initdb recipe for this container.)\n\
             \n\
             If you genuinely have no database, opt out EXPLICITLY and accept the receipt:\n\
             Opt out:           DB_TESTS_REQUIRED=0 make test-crates\n\
             \n\
             Why this is not merely annoying: with no database, `cargo test --workspace` reported\n\
             990 passed / 0 failed over a migration defect that permanently bricks the Cart\n\
             projection and kills placeOrder. The total looks identical either way."
        ),
    }
}

/// Append one skipped suite to the receipt file, so the summary survives libtest's output capture.
///
/// Best-effort by design: every failure to write is swallowed. The receipt is a *visibility* aid
/// for an opt-out the caller already asked for explicitly; the enforcement path is the panic
/// above, which cannot be suppressed. Making a test suite fail because a log file was unwritable
/// would trade a real gate for a flaky one.
fn append_receipt(suite: &str, opt_out: &str) {
    let Some(path) = receipt_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // ONE `write_all` of a pre-built line, not `writeln!`. O_APPEND makes a single small write
    // atomic across processes, but `writeln!` does NOT make a single write: `fmt::Write` issues a
    // separate syscall per format fragment, so parallel test binaries interleave mid-line. Measured
    // on the first #474 run of `DB_TESTS_REQUIRED=0 make test-crates`: the receipt contained a bare
    // `0` and a fused `mailbox_retentionmailbox_delivery` among 42 suites.
    let line = format!("{suite}\t{opt_out}\n");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Where the receipt lives, in precedence order:
///
/// 1. `DB_TEST_SKIP_RECEIPT` — an absolute path; what `make test-crates` sets.
/// 2. `$CARGO_TARGET_DIR/db-test-skips.log`.
/// 3. `<workspace root>/target/db-test-skips.log`, the workspace root being the first ancestor of
///    `CARGO_MANIFEST_DIR` that holds a `Cargo.lock` (cargo sets `CARGO_MANIFEST_DIR` for test
///    binaries, so this works for a bare `cargo test` too).
fn receipt_path() -> Option<std::path::PathBuf> {
    if let Ok(explicit) = std::env::var("DB_TEST_SKIP_RECEIPT") {
        if !explicit.trim().is_empty() {
            return Some(std::path::PathBuf::from(explicit));
        }
    }
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
        if !target.trim().is_empty() {
            return Some(std::path::Path::new(&target).join("db-test-skips.log"));
        }
    }
    let manifest = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let mut dir: Option<&std::path::Path> = Some(std::path::Path::new(&manifest));
    while let Some(d) = dir {
        if d.join("Cargo.lock").is_file() {
            return Some(d.join("target").join("db-test-skips.log"));
        }
        dir = d.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configured_database_always_runs() {
        assert_eq!(
            decide(Some("postgres://x"), None),
            Verdict::Run("postgres://x".into())
        );
        // Even an explicit opt-out does not stop a suite that HAS a database: the opt-out says
        // "I have no Postgres", not "do less work".
        assert_eq!(
            decide(Some("postgres://x"), Some("0")),
            Verdict::Run("postgres://x".into())
        );
    }

    /// The inversion itself. Before #474 this row was a silent skip; it is the whole issue.
    #[test]
    fn no_database_and_no_opt_out_is_required() {
        assert_eq!(decide(None, None), Verdict::Required);
        assert_eq!(decide(None, Some("1")), Verdict::Required);
        assert_eq!(decide(None, Some("true")), Verdict::Required);
    }

    #[test]
    fn only_the_four_literal_spellings_opt_out_and_case_does_not_matter() {
        for v in ["0", "false", "no", "off", "FALSE", "Off", "No"] {
            assert_eq!(
                decide(None, Some(v)),
                Verdict::SkipByOptOut { opt_out: v.to_string() },
                "{v} should be an opt-out"
            );
        }
    }

    /// Fail-closed: a typo or a half-set variable must NOT grant itself a skip, because a skip
    /// nobody asked for is exactly the false signal #474 exists to remove.
    #[test]
    fn a_mistyped_or_empty_opt_out_is_still_required() {
        for v in ["", "  ", "nope", "flase", "disabled", "2", "-1"] {
            assert_eq!(decide(None, Some(v)), Verdict::Required, "{v:?} must not opt out");
        }
    }

    /// `DATABASE_URL=` in a CI matrix is a missing value, not a connection string. Treating it as
    /// set would push the failure into `PgPool::connect` as an opaque URL parse error.
    #[test]
    fn an_empty_database_url_counts_as_unset() {
        assert_eq!(decide(Some(""), None), Verdict::Required);
        assert_eq!(decide(Some("   "), None), Verdict::Required);
        assert_eq!(
            decide(Some(""), Some("0")),
            Verdict::SkipByOptOut { opt_out: "0".into() }
        );
    }

    /// Surrounding whitespace is a shell/YAML artifact, not an intention.
    #[test]
    fn values_are_trimmed() {
        assert_eq!(
            decide(Some(" postgres://x "), None),
            Verdict::Run("postgres://x".into())
        );
        assert_eq!(
            decide(None, Some(" off ")),
            Verdict::SkipByOptOut { opt_out: "off".into() }
        );
    }
}
