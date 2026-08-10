//! §16 — Writer/schema agreement (#474 test 2, the [#451] shape caught without a database).
//!
//! The defect: `Cart.total_amount_cents` and `Cart.currency` were `NOT NULL` with **no
//! `DEFAULT`**, and `cart_store::upsert` never listed them. Every folded Cart event then failed
//! `23502`, the projector log-and-skipped it, and the read model stayed permanently empty while
//! every gate reported green. `cargo check` passed. Six crate suites run by hand passed. Three
//! `make rust` rounds passed. It went red only in CI's Postgres job, hours later.
//!
//! The rule is small enough to be exact and needs no database: **a column that is `NOT NULL` with
//! no `DEFAULT` must appear in the insert list of every writer that inserts into its table.** If it
//! does not, the very first insert fails, always — this is not a race, a data condition or a
//! load-dependent bug, which is precisely why it is worth catching statically. Postgres would have
//! told us at the first write; now `make validate` tells us in under a second.
//!
//! Scope: the read-model writers in `crates/infrastructure/src/persistence/*_store.rs` and the
//! tables `migrations/*.sql` creates. Both sides are repo-controlled text, so the parse below is
//! deliberately narrow — and where it does NOT understand a statement touching a table it guards,
//! it says so as an ERROR (`schema-writer-unparsed-ddl`) rather than passing. A checker that
//! silently stops checking is the failure mode this whole issue is about.

use std::collections::BTreeMap;
use std::path::Path;

use crate::model::{err, Issue};

/// One column's state after replaying the migration chain in order.
#[derive(Clone, Debug)]
struct Column {
    not_null: bool,
    has_default: bool,
    /// Migration file that last set this column's shape — the fix site named in the message.
    origin: String,
}

type Table = BTreeMap<String, Column>;

/// A writer's insert: which table, which columns, and where to go and fix it.
#[derive(Debug)]
struct Writer {
    table: String,
    columns: Vec<String>,
    file: String,
}

// ─── SQL text handling ──────────────────────────────────────────────────────────────────────

/// Drop `--` line comments and `/* */` blocks. Comment text carrying the words `NOT NULL` (this
/// repo's migrations are heavily commented, and one of them literally discusses this defect) would
/// otherwise be read as DDL.
fn strip_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    let mut in_string: Option<char> = None;
    while let Some(c) = chars.next() {
        if let Some(q) = in_string {
            out.push(c);
            if c == q {
                in_string = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                in_string = Some(c);
                out.push(c);
            }
            '-' if chars.peek() == Some(&'-') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for c in chars.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

/// Split on `;` at paren depth 0 and outside strings / `$$ … $$` bodies (the retention-sweep and
/// event-append functions are `$$`-quoted and full of semicolons).
fn split_statements(sql: &str) -> Vec<String> {
    let bytes: Vec<char> = sql.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut in_string: Option<char> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = in_string {
            cur.push(c);
            if c == q {
                in_string = None;
            }
            i += 1;
            continue;
        }
        if c == '$' && bytes.get(i + 1) == Some(&'$') {
            // Copy the whole dollar-quoted body verbatim.
            cur.push('$');
            cur.push('$');
            i += 2;
            while i < bytes.len() && !(bytes[i] == '$' && bytes.get(i + 1) == Some(&'$')) {
                cur.push(bytes[i]);
                i += 1;
            }
            if i < bytes.len() {
                cur.push('$');
                cur.push('$');
                i += 2;
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                in_string = Some(c);
                cur.push(c);
            }
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            ';' if depth == 0 => {
                if !cur.trim().is_empty() {
                    out.push(cur.clone());
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
        i += 1;
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Split on commas at paren depth 0 (column definitions, `ALTER TABLE` action lists).
fn split_top_commas(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut in_string: Option<char> = None;
    for c in s.chars() {
        if let Some(q) = in_string {
            cur.push(c);
            if c == q {
                in_string = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                in_string = Some(c);
                cur.push(c);
            }
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                out.push(cur.clone());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Collapse all whitespace runs to single spaces, so patterns can be written flatly.
fn flatten(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn unquote(ident: &str) -> String {
    ident.trim().trim_matches('"').to_ascii_lowercase()
}

/// Everything after the first word of a column definition — the type and its constraints.
fn column_has_default(def_lower: &str, type_and_rest: &str) -> bool {
    def_lower.contains(" default ")
        || def_lower.contains(" generated ")
        || type_and_rest.starts_with("serial")
        || type_and_rest.starts_with("bigserial")
        || type_and_rest.starts_with("smallserial")
}

// ─── Migration replay ───────────────────────────────────────────────────────────────────────

/// Replay the chain and return the final shape of every table, plus any DDL we could not read.
fn replay(migrations: &[(String, String)]) -> (BTreeMap<String, Table>, Vec<(String, String, String)>) {
    let mut tables: BTreeMap<String, Table> = BTreeMap::new();
    let mut unparsed: Vec<(String, String, String)> = Vec::new();

    for (file, text) in migrations {
        for stmt in split_statements(&strip_comments(text)) {
            let flat = flatten(&stmt);
            let lower = flat.to_ascii_lowercase();

            if let Some(rest) = strip_prefix_words(&lower, &["create", "table"]) {
                let rest = strip_optional(rest, "if not exists ");
                let Some(paren) = flat[flat.len() - rest.len()..].find('(') else { continue };
                let head = &flat[flat.len() - rest.len()..][..paren];
                let name = unquote(head);
                let body_start = flat.len() - rest.len() + paren + 1;
                let Some(body_end) = flat.rfind(')') else { continue };
                if body_end <= body_start {
                    continue;
                }
                let mut cols = Table::new();
                for part in split_top_commas(&flat[body_start..body_end]) {
                    let p = part.trim();
                    if p.is_empty() {
                        continue;
                    }
                    let pl = p.to_ascii_lowercase();
                    let mut words = pl.split(' ');
                    let first = words.next().unwrap_or("");
                    // Table-level constraints are not columns.
                    if matches!(
                        first,
                        "primary" | "foreign" | "unique" | "check" | "constraint" | "exclude" | "like"
                    ) {
                        continue;
                    }
                    let type_and_rest = pl[first.len()..].trim_start().to_string();
                    cols.insert(
                        unquote(first),
                        Column {
                            not_null: pl.contains(" not null"),
                            has_default: column_has_default(&pl, &type_and_rest),
                            origin: file.clone(),
                        },
                    );
                }
                tables.insert(name, cols);
                continue;
            }

            if let Some(rest) = strip_prefix_words(&lower, &["drop", "table"]) {
                let rest = strip_optional(rest, "if exists ");
                let name = unquote(rest.split([' ', ';']).next().unwrap_or(""));
                tables.remove(&name);
                continue;
            }

            let Some(rest) = strip_prefix_words(&lower, &["alter", "table"]) else { continue };
            let rest = strip_optional(strip_optional(rest, "if exists "), "only ");
            let Some(sp) = rest.find(' ') else { continue };
            let table = unquote(&rest[..sp]);
            // A table we never saw created (an extension's, or one created before this chain).
            if !tables.contains_key(&table) {
                continue;
            }
            for action in split_top_commas(&rest[sp + 1..]) {
                let a = action.trim().to_string();
                if apply_alter(&mut tables, &table, &a, file) {
                    continue;
                }
                unparsed.push((file.clone(), table.clone(), a.chars().take(120).collect()));
            }
        }
    }
    (tables, unparsed)
}

/// Apply ONE `ALTER TABLE` action. Returns false when the action is not understood, which the
/// caller turns into an error rather than a silent pass.
fn apply_alter(tables: &mut BTreeMap<String, Table>, table: &str, action: &str, file: &str) -> bool {
    let cols = tables.get_mut(table).expect("caller checked");

    if let Some(rest) = strip_prefix_words(action, &["add", "column"]) {
        let rest = strip_optional(rest, "if not exists ");
        let Some(sp) = rest.find(' ') else { return false };
        let name = unquote(&rest[..sp]);
        let type_and_rest = rest[sp + 1..].trim_start().to_string();
        cols.insert(
            name,
            Column {
                not_null: rest.contains(" not null"),
                has_default: column_has_default(&format!(" {rest} "), &type_and_rest),
                origin: file.to_string(),
            },
        );
        return true;
    }
    if let Some(rest) = strip_prefix_words(action, &["drop", "column"]) {
        let rest = strip_optional(rest, "if exists ");
        let name = unquote(rest.split([' ', ';']).next().unwrap_or(""));
        cols.remove(&name);
        return true;
    }
    if let Some(rest) = strip_prefix_words(action, &["rename", "column"]) {
        // `rename column <old> to <new>` — carrying the shape over, since a rename that lost
        // `not_null` would make this checker silently stop guarding the column.
        let parts: Vec<&str> = rest.split(' ').collect();
        if parts.len() >= 3 && parts[1] == "to" {
            let (old, new) = (unquote(parts[0]), unquote(parts[2]));
            if let Some(meta) = cols.remove(&old) {
                cols.insert(new, meta);
            }
            return true;
        }
        return false;
    }
    // `alter [column] <name> <set|drop> <default …|not null>` — and the `type` form, which changes
    // neither nullability nor default.
    let rest = strip_prefix_words(action, &["alter", "column"])
        .or_else(|| strip_prefix_words(action, &["alter"]))
        .unwrap_or("");
    if !rest.is_empty() {
        let Some(sp) = rest.find(' ') else { return false };
        let name = unquote(&rest[..sp]);
        let tail = rest[sp + 1..].trim_start();
        if tail.starts_with("type ") {
            return true;
        }
        let entry = cols.entry(name).or_insert(Column {
            not_null: false,
            has_default: false,
            origin: file.to_string(),
        });
        if let Some(what) = tail.strip_prefix("set ") {
            if what.starts_with("default") {
                entry.has_default = true;
                entry.origin = file.to_string();
                return true;
            }
            if what.starts_with("not null") {
                entry.not_null = true;
                entry.origin = file.to_string();
                return true;
            }
        }
        if let Some(what) = tail.strip_prefix("drop ") {
            if what.starts_with("default") {
                entry.has_default = false;
                entry.origin = file.to_string();
                return true;
            }
            if what.starts_with("not null") {
                entry.not_null = false;
                entry.origin = file.to_string();
                return true;
            }
        }
        return false;
    }
    // Constraints, ownership, RLS and friends do not change a column's insertability.
    let head: Vec<&str> = action.split(' ').take(2).collect();
    matches!(
        head.as_slice(),
        ["add", "constraint"]
            | ["add", "primary"]
            | ["add", "foreign"]
            | ["add", "unique"]
            | ["add", "check"]
            | ["drop", "constraint"]
            | ["validate", "constraint"]
            | ["enable", _]
            | ["disable", _]
            | ["owner", _]
            | ["set", _]
            | ["cluster", _]
            | ["replica", _]
    )
}

fn strip_prefix_words<'a>(s: &'a str, words: &[&str]) -> Option<&'a str> {
    let mut rest = s.trim_start();
    for w in words {
        rest = rest.strip_prefix(w)?;
        if !rest.starts_with(' ') && !rest.is_empty() {
            return None;
        }
        rest = rest.trim_start();
    }
    Some(rest)
}

fn strip_optional<'a>(s: &'a str, prefix: &str) -> &'a str {
    s.strip_prefix(prefix).unwrap_or(s)
}

// ─── Writer extraction ──────────────────────────────────────────────────────────────────────

/// Rust string literals in this repo wrap SQL with `\` line continuations; rejoin them so the
/// insert's column list is one flat run of text.
fn dechunk_rust_string(src: &str) -> String {
    let rejoined = src
        .split("\\\n")
        .map(|part| part.trim_start_matches([' ', '\t']))
        .collect::<Vec<_>>()
        .join(" ");
    rejoined
}

/// Pull `const COLUMNS: &str = "…"` and every `INSERT INTO <table> (<columns>)` out of one store.
fn writers_in(file: &str, src: &str) -> Vec<Writer> {
    let text = dechunk_rust_string(src);
    let columns_const: Option<Vec<String>> = text.find("const COLUMNS").and_then(|at| {
        let after = &text[at..];
        let eq = after.find('=')?;
        let open = after[eq..].find('"')? + eq + 1;
        let close = after[open..].find('"')? + open;
        Some(
            after[open..close]
                .split(',')
                .map(|c| c.trim().to_ascii_lowercase())
                .filter(|c| !c.is_empty())
                .collect(),
        )
    });

    let mut out = Vec::new();
    let mut search = text.as_str();
    while let Some(at) = search.find("INSERT INTO ") {
        let after = &search[at + "INSERT INTO ".len()..];
        search = after;
        let Some(open) = after.find('(') else { continue };
        let table = after[..open].trim().trim_end_matches(char::is_whitespace);
        if table.is_empty() || table.split_whitespace().count() != 1 {
            continue;
        }
        let Some(close) = after[open..].find(')') else { continue };
        let body = &after[open + 1..open + close];
        let columns: Vec<String> = if body.contains("{COLUMNS}") {
            match &columns_const {
                Some(c) => c.clone(),
                // An `{COLUMNS}` insert with no const to resolve: refuse to guess.
                None => continue,
            }
        } else {
            body.split(',').map(|c| c.trim().to_ascii_lowercase()).filter(|c| !c.is_empty()).collect()
        };
        // Guard against matching something that is not a column list (a `SELECT`, a placeholder).
        if columns.is_empty() || columns.iter().any(|c| c.contains(' ') || c.contains('$')) {
            continue;
        }
        out.push(Writer {
            table: unquote(table),
            columns,
            file: file.to_string(),
        });
    }
    out
}

// ─── The rule ───────────────────────────────────────────────────────────────────────────────

/// Pure core (`(filename, contents)` pairs in, issues out) so it is unit-testable from fixture
/// strings, exactly like §13's proposal hygiene.
pub(crate) fn validate_writer_schema_agreement(
    migrations: &[(String, String)],
    writers: &[(String, String)],
) -> Vec<Issue> {
    let mut issues = Vec::new();
    let (tables, unparsed) = replay(migrations);
    let parsed_writers: Vec<Writer> =
        writers.iter().flat_map(|(f, src)| writers_in(f, src)).collect();

    for w in &parsed_writers {
        let Some(table) = tables.get(&w.table) else {
            issues.push(err(
                "schema-writer-unknown-table",
                w.file.clone(),
                format!(
                    "inserts into `{}`, which no migration in migrations/ creates -- either the \
                     table name is wrong or the migration is missing; every insert into it fails",
                    w.table
                ),
            ));
            continue;
        };
        for (name, col) in table {
            if col.not_null && !col.has_default && !w.columns.iter().any(|c| c == name) {
                issues.push(err(
                    "schema-writer-missing-column",
                    format!("{} -> {}.{}", w.file, w.table, name),
                    format!(
                        "`{}.{}` is NOT NULL with no DEFAULT ({}), but the writer's insert list \
                         omits it -- EVERY insert fails with 23502 and, on a projection, the read \
                         model stays empty while the projector reports a clean drain (#451). Fix: \
                         add the column to the writer, or give it a DEFAULT in a migration.",
                        w.table, name, col.origin
                    ),
                ));
            }
        }
    }

    // A guarded table whose DDL we could not read means the check silently stopped guarding it.
    let guarded: std::collections::BTreeSet<&String> = parsed_writers.iter().map(|w| &w.table).collect();
    for (file, table, action) in &unparsed {
        if guarded.contains(table) {
            issues.push(err(
                "schema-writer-unparsed-ddl",
                format!("migrations/{file}"),
                format!(
                    "`ALTER TABLE {table} {action}` is not understood by the writer/schema check, \
                     so `{table}` would stop being guarded. Teach validate/schema_writers.rs this \
                     action rather than leaving the gate half-blind."
                ),
            ));
        }
    }
    issues
}

/// Read `migrations/*.sql` in apply order (filename order, as sqlx-cli applies them).
pub(crate) fn load_migration_files(repo_root: &Path) -> Vec<(String, String)> {
    read_sorted(&repo_root.join("migrations"), ".sql")
}

/// Read the read-model writers, `crates/infrastructure/src/persistence/*_store.rs`.
pub(crate) fn load_writer_files(repo_root: &Path) -> Vec<(String, String)> {
    read_sorted(&repo_root.join("crates/infrastructure/src/persistence"), "_store.rs")
}

fn read_sorted(dir: &Path, suffix: &str) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<(String, String)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_str()?.to_string();
            if !name.ends_with(suffix) {
                return None;
            }
            Some((name, std::fs::read_to_string(&p).ok()?))
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mig(sql: &str) -> Vec<(String, String)> {
        vec![("0001_test.sql".to_string(), sql.to_string())]
    }
    fn writer(rs: &str) -> Vec<(String, String)> {
        vec![("cart_store.rs".to_string(), rs.to_string())]
    }
    /// `Issue` is not `Debug`, so assertions render it themselves.
    fn render(issues: &[Issue]) -> String {
        issues
            .iter()
            .map(|i| format!("  {} {}: {}", i.rule, i.location, i.message))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The #451 defect itself, reduced: NOT NULL, no DEFAULT, absent from the insert list.
    #[test]
    fn a_not_null_column_without_a_default_that_the_writer_omits_is_an_error() {
        let issues = validate_writer_schema_agreement(
            &mig("CREATE TABLE Cart (cart_id UUID PRIMARY KEY, total_amount_cents BIGINT NOT NULL);"),
            &writer(r#" "INSERT INTO cart (cart_id) VALUES ($1)" "#),
        );
        assert_eq!(issues.len(), 1, "{}", render(&issues));
        assert_eq!(issues[0].rule, "schema-writer-missing-column");
        assert!(issues[0].message.contains("23502"), "{}", issues[0].message);
    }

    /// The fix that actually shipped for #451 was a DEFAULT, so a DEFAULT must clear the finding.
    #[test]
    fn a_default_added_by_a_later_migration_clears_it() {
        let issues = validate_writer_schema_agreement(
            &mig("CREATE TABLE Cart (cart_id UUID PRIMARY KEY, total_amount_cents BIGINT NOT NULL); \
                  ALTER TABLE Cart ALTER COLUMN total_amount_cents SET DEFAULT 0;"),
            &writer(r#" "INSERT INTO cart (cart_id) VALUES ($1)" "#),
        );
        assert!(issues.is_empty(), "{}", render(&issues));
    }

    /// ...and dropping it again brings the finding back, so the replay is ordered, not a scan.
    #[test]
    fn dropping_the_default_again_brings_the_finding_back() {
        let issues = validate_writer_schema_agreement(
            &mig("CREATE TABLE Cart (cart_id UUID PRIMARY KEY, total_amount_cents BIGINT NOT NULL); \
                  ALTER TABLE Cart ALTER COLUMN total_amount_cents SET DEFAULT 0; \
                  ALTER TABLE Cart ALTER COLUMN total_amount_cents DROP DEFAULT;"),
            &writer(r#" "INSERT INTO cart (cart_id) VALUES ($1)" "#),
        );
        assert_eq!(issues.len(), 1, "{}", render(&issues));
    }

    #[test]
    fn a_nullable_column_is_fine_and_so_is_one_the_writer_lists() {
        let issues = validate_writer_schema_agreement(
            &mig("CREATE TABLE Cart (cart_id UUID PRIMARY KEY, note TEXT, currency TEXT NOT NULL);"),
            &writer(r#" "INSERT INTO cart (cart_id, currency) VALUES ($1,$2)" "#),
        );
        assert!(issues.is_empty(), "{}", render(&issues));
    }

    /// The `{COLUMNS}` indirection every generated store uses, across a Rust line continuation.
    #[test]
    fn the_columns_const_is_resolved_across_rust_line_continuations() {
        let src = "pub(crate) const COLUMNS: &str = \"cart_id, \\\n     currency\";\n\
                   let sql = format!(\"INSERT INTO cart ({COLUMNS}) VALUES ($1,$2)\");";
        let issues = validate_writer_schema_agreement(
            &mig("CREATE TABLE Cart (cart_id UUID PRIMARY KEY, currency TEXT NOT NULL);"),
            &writer(src),
        );
        assert!(issues.is_empty(), "{}", render(&issues));
    }

    /// Comments in these migrations discuss NOT NULL columns at length; they are not DDL.
    #[test]
    fn commentary_about_not_null_is_not_ddl() {
        let issues = validate_writer_schema_agreement(
            &mig("-- currency is NOT NULL with no DEFAULT and that is the bug\n\
                  CREATE TABLE Cart (cart_id UUID PRIMARY KEY);"),
            &writer(r#" "INSERT INTO cart (cart_id) VALUES ($1)" "#),
        );
        assert!(issues.is_empty(), "{}", render(&issues));
    }

    #[test]
    fn a_dropped_column_stops_being_required() {
        let issues = validate_writer_schema_agreement(
            &mig("CREATE TABLE Cart (cart_id UUID PRIMARY KEY, gone TEXT NOT NULL); \
                  ALTER TABLE Cart DROP COLUMN gone;"),
            &writer(r#" "INSERT INTO cart (cart_id) VALUES ($1)" "#),
        );
        assert!(issues.is_empty(), "{}", render(&issues));
    }

    /// A rename must carry the shape, or the checker silently stops guarding the column.
    #[test]
    fn a_renamed_column_is_still_guarded_under_its_new_name() {
        let issues = validate_writer_schema_agreement(
            &mig("CREATE TABLE Cart (cart_id UUID PRIMARY KEY, old_name TEXT NOT NULL); \
                  ALTER TABLE Cart RENAME COLUMN old_name TO new_name;"),
            &writer(r#" "INSERT INTO cart (cart_id) VALUES ($1)" "#),
        );
        assert_eq!(issues.len(), 1, "{}", render(&issues));
        assert!(issues[0].location.ends_with("cart.new_name"), "{}", issues[0].location);
    }

    /// SERIAL and GENERATED columns supply their own value.
    #[test]
    fn serial_and_generated_columns_are_not_required_of_the_writer() {
        let issues = validate_writer_schema_agreement(
            &mig("CREATE TABLE Cart (position BIGSERIAL PRIMARY KEY, cart_id UUID NOT NULL, \
                  n INT NOT NULL GENERATED ALWAYS AS IDENTITY);"),
            &writer(r#" "INSERT INTO cart (cart_id) VALUES ($1)" "#),
        );
        assert!(issues.is_empty(), "{}", render(&issues));
    }

    /// The half-blind case: DDL the checker cannot read, on a table it guards, is loud.
    #[test]
    fn unreadable_ddl_on_a_guarded_table_is_an_error_not_a_silent_pass() {
        let issues = validate_writer_schema_agreement(
            &mig("CREATE TABLE Cart (cart_id UUID PRIMARY KEY); \
                  ALTER TABLE Cart INHERIT some_other_table;"),
            &writer(r#" "INSERT INTO cart (cart_id) VALUES ($1)" "#),
        );
        assert_eq!(issues.len(), 1, "{}", render(&issues));
        assert_eq!(issues[0].rule, "schema-writer-unparsed-ddl");
    }

    #[test]
    fn a_writer_pointing_at_a_table_no_migration_creates_is_an_error() {
        let issues = validate_writer_schema_agreement(
            &mig("CREATE TABLE Cart (cart_id UUID PRIMARY KEY);"),
            &writer(r#" "INSERT INTO carts (cart_id) VALUES ($1)" "#),
        );
        assert_eq!(issues.len(), 1, "{}", render(&issues));
        assert_eq!(issues[0].rule, "schema-writer-unknown-table");
    }

    /// THE regression bar: run the rule over the REAL repo. It must be silent -- and it must
    /// actually have looked at something, or a path typo would make this vacuously green.
    #[test]
    fn the_real_migrations_and_writers_agree() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let migrations = load_migration_files(&root);
        let writers = load_writer_files(&root);
        assert!(migrations.len() > 40, "expected the real migration chain, got {}", migrations.len());
        assert!(writers.len() > 8, "expected the real store writers, got {}", writers.len());
        let issues = validate_writer_schema_agreement(&migrations, &writers);
        assert!(
            issues.is_empty(),
            "writer/schema disagreement:\n{}",
            issues.iter().map(|i| format!("  {} {}: {}", i.rule, i.location, i.message)).collect::<Vec<_>>().join("\n")
        );
    }
}
