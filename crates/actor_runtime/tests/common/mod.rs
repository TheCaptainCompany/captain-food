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
//! The contract is identical to `db_test_gate`, INCLUDING the receipt: a database is REQUIRED by
//! default; only the four literal spellings `0` / `false` / `no` / `off` opt out; anything else
//! fails loudly; and an opt-out appends the suite name to the same receipt file, in the same
//! format, so `make test-crates` names it in the summary.
//!
//! The receipt half is not cosmetic. Without it this crate's five DB-gated binaries
//! (`concurrency`, `discipline`, `mailbox`, `poison`, `rebalance`) skipped INVISIBLY under an
//! opt-out: libtest swallows a passing test's stderr, so the `eprintln!` below never reached any
//! log, and `make test-crates` printed "skipped N suite(s): …" over a run where MORE suites had
//! skipped than the line named. A reader who looked for `rebalance` in that list and did not find
//! it could only conclude it had run — which is precisely the false signal #474 exists to remove,
//! reintroduced inside the fix for it.

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
    // Trimmed like the shared gate's `Verdict::SkipByOptOut`, so the two receipts are comparable.
    let opt_out = flag.as_deref().map(str::trim).unwrap_or("");
    // Visible under `--nocapture` only; the receipt below is what actually survives libtest.
    eprintln!(
        "SKIP[db] {suite}: no DATABASE_URL, skipped by DB_TESTS_REQUIRED={opt_out} \
         -- this suite exercised NO database behaviour."
    );
    append_receipt(suite, opt_out);
    None
}

/// Append one skipped suite to the receipt `make test-crates` reads back — the local half of
/// `db_test_gate::append_receipt`, byte-identical in format (`<suite>\t<opt-out>\n`).
///
/// Best-effort by design, exactly like the shared crate: every write failure is swallowed. The
/// enforcement path is the `assert!` above, which no capture can hide; the receipt is visibility,
/// and failing a suite because a log file was unwritable would trade a real gate for a flaky one.
///
/// ONE `write_all` of a pre-built line, NOT `writeln!`. O_APPEND makes a single small write atomic
/// across processes; `writeln!` does not, because `fmt::Write` issues a separate syscall per format
/// fragment, so parallel test binaries interleave mid-line. Measured on the first #474 receipt run:
/// a bare `0` and a fused `mailbox_retentionmailbox_delivery` among 42 suites. These five binaries
/// run in parallel with the rest of the workspace, so the hazard is live here too.
fn append_receipt(suite: &str, opt_out: &str) {
    use std::io::Write as _;
    let Some(path) = receipt_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let line = format!("{suite}\t{opt_out}\n");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Where the receipt lives, in the SAME precedence order as `db_test_gate::receipt_path`:
/// 1. `DB_TEST_SKIP_RECEIPT` — an absolute path; what `make test-crates` sets.
/// 2. `$CARGO_TARGET_DIR/db-test-skips.log`.
/// 3. `<workspace root>/target/db-test-skips.log`, the workspace root being the first ancestor of
///    `CARGO_MANIFEST_DIR` holding a `Cargo.lock` (cargo sets it for test binaries, so a bare
///    `cargo test -p actor_runtime` lands in the same file as a full `make test-crates`).
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
