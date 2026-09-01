# Captain.Food — developer & loop entrypoints.
# Most "gates" are folded into the single codegen validator (`validate`): schema, behaviour-test
# coverage, observability contracts, and C4 consistency are all checked there. They are exposed as
# separate targets so the loop reads like the playbook, but they currently delegate to `validate`.
#
# The codegen is the Rust tool (tools/codegen-rs, ADR-0034); it needs a local Rust toolchain (`cargo`).

# PORTABILITY: keep RECIPE lines pure ASCII (use `--`, `->`, `|` rather than em dashes/arrows).
# Native Windows GNU Make hands recipes to Cygwin's `sh` with broken quoting as soon as the line
# contains a byte > 127: `sh` then receives the whole recipe as ONE word and reports
# "$'...': command not found". Comments and $(shell ...) are fine; only recipe text matters.

CODEGEN_RS = tools/codegen-rs

# How to invoke cargo. Plain `cargo` everywhere (Linux, macOS, CI, Git-Bash/MSYS) — EXCEPT under
# Cygwin, where the rustup `cargo` proxy mis-detects its own argv[0] and runs as `rustup`, so every
# `cargo build` fails with "invalid value 'build' for '[+toolchain]'". Routing through `rustup run`
# sidesteps the proxy. `uname` is absent under a cmd.exe shell, which harmlessly picks plain cargo.
# Override explicitly if needed: `make validate CARGO=/path/to/cargo`.
UNAME_S := $(shell uname -s 2>/dev/null)
ifneq (,$(findstring CYGWIN,$(UNAME_S)))
  # Keep in step with rust-toolchain.toml (ADR-0034); `stable` if it can't be read.
  RUST_CHANNEL ?= $(or $(shell sed -n 's/^[ \t]*channel[ \t]*=[ \t]*"\([^"]*\)".*/\1/p' rust-toolchain.toml 2>/dev/null),stable)
  CARGO ?= rustup run $(RUST_CHANNEL) cargo
else
  CARGO ?= cargo
endif

.PHONY: hooks-test stub-tests link-check typecheck validate-schema test-behaviour test-observability c4-validate validate warning-baseline generate check-drift review gate night-loop budget-check budgeted-loop docs c4-export c4-render help rust rust-build rust-test test-crates test-quiet rust-quiet smoke-prod

help:
	@echo "targets: validate generate typecheck test-crates review gate night-loop budgeted-loop budget-check docs"
	@echo "         warning-baseline = refresh the warning ratchet (tools/codegen-rs/warning-baseline.json)"
	@echo "         test-crates = the WORKSPACE test gate (#474): cargo test --workspace with the DB"
	@echo "         suites REQUIRED. 'make rust' is the spec gate and runs NO crates/** test."
	@echo "         c4-render (Structurizr Lite + docs/ADRs) | c4-export (validate/export DSL)"
	@echo "         (validate-schema test-behaviour test-observability c4-validate -> all fold into 'validate')"
	@echo "         test-quiet / rust-quiet = the same gates, output filtered to VERDICTS only"
	@echo "         (progress dropped, verdicts never; full log in target/quiet-gate.log)"
	@echo "         budgeted-loop runs the night loop under a 30-min/week budget (.claude/loop-budget.json)"
	@echo "         codegen = tools/codegen-rs (Rust, ADR-0034); needs cargo. 'rust' = build+test alias."
	@echo "         link-check = every relative link in tracked markdown must resolve (#837);"
	@echo "         runs its own selftest first. External URLs are deliberately not checked."
	@echo "         hooks-test / stub-tests = the two gate-script suites, run with their"
	@echo "         self-verification opt-out so an edit-and-re-run loop works. CI runs both"
	@echo "         WITHOUT the opt-out, so a local green here excludes the gate-set comparison."

# Production E2E smoke (Stripe TEST mode) against the live deployment — tools/smoke/README.md.
# Needs: STRIPE_SECRET_KEY (sk_test) and SUPABASE_SECRET_KEY.
smoke-prod:
	bash tools/smoke/prod-smoke.sh

# `typecheck` = the Rust compiler is the type gate (build must succeed).
typecheck:
	$(CARGO) build --manifest-path $(CODEGEN_RS)/Cargo.toml

# The codegen validator is the single source of truth for these gates (validate.ts §1–§11 in Rust).
validate-schema:
	$(CARGO) run --manifest-path $(CODEGEN_RS)/Cargo.toml -- --check --specs specs

test-behaviour: validate-schema      ## behaviour-test coverage is enforced inside `validate`
test-observability: validate-schema  ## observability contracts are validated inside `validate`
c4-validate: validate-schema         ## C4 consistency is validated inside `validate`

validate: typecheck validate-schema

# The warning RATCHET (validator section 17). `validate` fails when the live per-rule warning
# histogram differs from tools/codegen-rs/warning-baseline.json in EITHER direction; this target is
# the only writer. Run it when a change legitimately moves the warning surface and commit the
# refreshed artifact in the SAME commit -- the diff is the record of what the change did.
warning-baseline:
	$(CARGO) run --manifest-path $(CODEGEN_RS)/Cargo.toml -- --write-warning-baseline --specs specs

# Generate every artifact from the specs (writes into specs/generated/** + the database.md §2 region).
generate:
	$(CARGO) run --manifest-path $(CODEGEN_RS)/Cargo.toml -- --specs specs

# Regenerate, then fail if the result drifts from what's committed (the CI drift gate, runnable locally).
# Whole-tree diff (matches CI): generated output spans specs/generated + specs/database.md AND the
# generated Rust under crates/**/generated. Run on a clean tree — it's the gate, not a mid-edit helper.
check-drift: generate
	@git diff --quiet --ignore-cr-at-eol || { echo "check-drift: generated artifacts drifted -- run 'make generate' and commit the regenerated files."; git --no-pager diff --ignore-cr-at-eol --stat; exit 1; }

# --- Rust codegen build/test aliases (ADR-0034). ---
rust-build:
	$(CARGO) build --manifest-path $(CODEGEN_RS)/Cargo.toml
rust-test:
	$(CARGO) test --manifest-path $(CODEGEN_RS)/Cargo.toml
# `link-check` IS A PREREQUISITE OF `rust`, and that is the founder's directive's local half
# actually landing. Without it the target existed and nothing invoked it -- not `rust`, `gate`,
# `review`, `night-loop` or the stop hook -- while CLAUDE.md tells anyone doing docs work to run
# `make rust` before pushing. And docs-only changes push STRAIGHT TO `main` with no PR, so the
# first consequence of a broken citation would have been a red `main`, not a red PR: the one lane
# where the gate is most needed was the one lane nothing local covered (#837 review round 1).
rust: rust-build rust-test validate check-drift link-check
	@echo "rust: build + test + validate + generate(+diff) + link-check OK"
	@echo "rust: NOTE -- this gate does NOT run crates/** tests. For a code change run 'make test-crates'."

# --- The workspace test gate (#474). ---
#
# `make rust` is the SPEC gate: it builds and tests tools/codegen-rs ONLY. It proves nothing about
# crates/**. Measured on the #474 branch, against a deliberately planted migration defect that
# permanently bricks the Cart projection and kills placeOrder: `make rust` exited 0, and
# `cargo test --workspace` with no DATABASE_URL reported 990 passed / 0 failed. The same command
# with a real Postgres failed. This target is the honest half.
#
# DB_TESTS_REQUIRED is REQUIRED-by-default since #474 (crates/db_test_gate): with no DATABASE_URL
# the run FAILS unless the caller opts out with DB_TESTS_REQUIRED=0, and an opt-out leaves a
# receipt this target reads back so the summary survives libtest's output capture (a passing test's
# stderr is swallowed -- the old SKIP lines never appeared in any log).
#
# --no-fail-fast on purpose: without it cargo stops launching further test binaries after the first
# failure, so the pass TOTAL silently shrinks and two runs are not comparable (990 vs 744 was
# measured exactly this way).
#
# The recipe is SILENCED (`@`) and echoes its own command instead, because make printing the recipe
# put the literal text `test-crates: DB-GATED SUITES SKIPPED` in the output of every run -- including
# runs where nothing was skipped (#597). A reader grepping their own gate evidence for the skip
# warning matched the recipe line and concluded the DB suites had not run; QUIET_KEEP's `DB-GATED`
# alternative matched it too, so the quiet wrapper reprinted it as a verdict. With the echo gone,
# `^test-crates:` is unambiguous: those lines exist only when the run actually emitted them.
#
# THE RECEIPT IS ONE-SIDED, AND `tools/db-preflight.sh` IS THE OTHER SIDE (#830). The receipt below
# can only speak when suites SKIP -- so on a run where `DATABASE_URL` is set but the server is
# stopped, it stays EMPTY while every DB-gated suite fails with a connection error minutes into the
# build. Grepping a run for `DB-GATED SUITES SKIPPED` therefore reports the same thing on a live
# database and on a dead one. That is CLAUDE.md's named defect class: a monitoring path that can
# only fire when a signal ARRIVES. The pre-flight is the dead-man's-switch -- it fails BEFORE the
# build when the database is unreachable, and prints a POSITIVE line when it is not, which is what
# makes an empty receipt mean anything. The complete evidence claim is all three together:
#   DB PRE-FLIGHT OK  +  empty skip receipt  +  exit 0  =>  the DB-gated suites RAN, live.
# It runs FIRST and its status is NOT swallowed: a failure here must stop the target.
DB_TEST_RECEIPT = target/db-test-skips.log
test-crates:
	@rm -f $(DB_TEST_RECEIPT)
	@mkdir -p target
	@bash tools/db-preflight.sh
	@echo "test-crates: running cargo test --workspace --no-fail-fast"
	@DB_TEST_SKIP_RECEIPT=$(abspath $(DB_TEST_RECEIPT)) $(CARGO) test --workspace --no-fail-fast; \
	  status=$$?; \
	  if [ -s $(DB_TEST_RECEIPT) ]; then \
	    echo "test-crates: DB-GATED SUITES SKIPPED -- this run exercised NO database behaviour."; \
	    echo "test-crates: skipped $$(cut -f1 $(DB_TEST_RECEIPT) | sort -u | wc -l) suite(s): $$(cut -f1 $(DB_TEST_RECEIPT) | sort -u | tr '\n' ' ')"; \
	    echo "test-crates: re-run with DATABASE_URL set to exercise them (see docs/claude/sessions/gates.md)."; \
	  fi; \
	  exit $$status

# --- Quiet gate wrappers (ADR-20260816-020752, farley): filtered output for token-bound sessions. ---
#
# THE RULE, and it is the whole design: FILTERING MAY DROP PROGRESS, NEVER VERDICTS. A verdict is
# any line that could turn green into red -- the DB-skip receipt (#230, "a skip that reports ok is
# not evidence"), EVERY `^test-crates:` line the workspace gate emits about itself (#830: the
# DB pre-flight's POSITIVE line is a verdict, because it is the only thing that makes an empty skip
# receipt mean anything, and its UNAVAILABLE line announces a DECLARED degraded mode that this
# filter would otherwise make SILENT), the first panic, every `test result:` summary, the
# validator's error lines, and the warning-baseline diff. So the filter is grep-FIRST (every verdict line, wherever in the run it
# occurred) and tail-SECOND (the last lines, for context). A tail-only filter would lose an early
# panic -- exactly the case that matters. Nothing is discarded: the full output is always in
# $(QUIET_LOG), and the wrapper prints where.
#
# These are Makefile TARGETS, not a hook, on purpose: a hook is invisible to CI and cannot be diffed.
#
# The gate is NOT piped, and that is deliberate. Its exit status is captured directly and re-raised,
# which is stronger than `set -o pipefail` AND portable: make runs recipes under /bin/sh, which is
# dash on Debian/Ubuntu, and dash answers `set -o pipefail` with "Illegal option" -- the recipe would
# fail before ever running the gate.
#
# Override QUIET_TEST_CMD / QUIET_RUST_CMD to point a wrapper at any command. That is also how the
# wrapper's own RED is proven: a command that exits non-zero must make the target exit non-zero.
QUIET_LOG ?= target/quiet-gate.log
QUIET_TAIL ?= 50
QUIET_TEST_CMD ?= $(MAKE) --no-print-directory test-crates
QUIET_RUST_CMD ?= $(MAKE) --no-print-directory rust
# Verdict lines, as one ERE. Anchored forms first, then unanchored ones for output that indents its
# verdicts (the validator prints "  [error] rule  location"; cargo prints "error[E0433]"). Keep this
# pattern PURE ASCII: it is expanded INTO a recipe line, so a byte > 127 here breaks Cygwin make at
# runtime even though the recipe text itself reads as ASCII to the guard test.
QUIET_KEEP ?= ^test-crates:|PRE-FLIGHT|^(error|warning|panic|thread .* panicked|SKIP|skipped|test result:|FAILED|failures:)|\[error\]|\[warn |error\[E|error:|warning:|panicked at|error\(s\)|FAILED|failures:|SKIPPED|DB-GATED|drifted|baseline|test result:

# $(1) = label, $(2) = the command to run.
define run-quiet
	@mkdir -p $(dir $(QUIET_LOG))
	@$(2) > $(QUIET_LOG) 2>&1; status=$$?; \
	  echo "---- $(1): verdict lines (grep-first; progress dropped, verdicts never) ----"; \
	  grep -E -- '$(QUIET_KEEP)' $(QUIET_LOG) || echo "(no verdict line matched)"; \
	  echo "---- $(1): last $(QUIET_TAIL) lines ----"; \
	  tail -n $(QUIET_TAIL) $(QUIET_LOG); \
	  echo "---- $(1): exit=$$status -- full unfiltered output: $(QUIET_LOG) ----"; \
	  exit $$status
endef

test-quiet:
	$(call run-quiet,test-quiet,$(QUIET_TEST_CMD))

rust-quiet:
	$(call run-quiet,rust-quiet,$(QUIET_RUST_CMD))

# Compile-check the wasm32 hydrate build of crates/web (split 4/4 of #21). The real bundle
# (wasm-bindgen output) is produced in the Docker image build; this is the fast CI/local gate that
# the hydrate target still compiles. Needs: rustup target add wasm32-unknown-unknown
wasm:
	$(CARGO) build -p web --target wasm32-unknown-unknown --no-default-features --features hydrate
	@echo "wasm: hydrate target compiles OK"

# Independent review: regenerate, then confirm the generated artifacts are in step with the DSL.
review: validate generate
	@git status --porcelain || true
	@echo "review: if 'git status' shows generated diffs, the DSL and generated artifacts are out of step."

# The same gate the Stop hook runs.
gate:
	bash .claude/hooks/stop-gate.sh

# Guard tests for the register-check gate (ADR-20260821-010543): hook verdicts on fixture
# payloads, the settings.json wiring, and the agent files' citation blocks. Also run by the
# Stop hook on every turn; this target is the direct entrypoint.
# REGISTER_CHECK_ALLOW_DIRTY: the selftest compares all four gate scripts against their committed
# blobs and refuses to report otherwise. This is the EDIT-AND-RE-RUN entrypoint, so it opts out --
# visibly, like stop-gate.sh. CI invokes the script directly and gets the comparison. Review #9 of
# PR #679 found this caller unlisted, which made `make hooks-test` exit 1 on any uncommitted hook
# edit: a silent trap on the one loop the target exists for.
hooks-test:
	env REGISTER_CHECK_ALLOW_DIRTY=1 bash .claude/hooks/register-check-selftest.sh

# The SAME entrypoint for the other gate script. `make hooks-test` was given the opt-out so an
# edit-and-re-run loop stays possible; the decision-lookup stub suite -- the one PR #679 is about
# -- was left with no interactive caller at all, so a maintainer editing the wrapper got
# `FATAL: ... differs from the committed blob` and zero cases run (review #13).
stub-tests:
	env DECISION_LOOKUP_ALLOW_DIRTY=1 bash .claude/skills/decision-lookup/scripts/stub-tests.sh

# Relative links in every tracked markdown file must resolve (#837). Founder directive,
# 2026-09-01: "put in place this url checker that must be executed locally and enforced in the CI
# too" -- BOTH halves. This is the local half; the CI half is two steps in ci.yml's `gate-scripts`
# job, pinned by codegen tests so neither can be disarmed.
#
# `docs/**` IS the operating model here: CLAUDE.md is an index whose authority is the topic file
# it links to, every ADR cites its neighbours, and the register-check discipline turns on a reader
# being able to FOLLOW a citation. A broken link is a citation that silently resolves to nothing.
#
# Runs the SELFTEST FIRST, on purpose. The checker's verdict is only worth reading if its three
# vacuity guards still work -- a scanner that matches nothing passes, and this repo has shipped
# that exact green twice. External URLs are NOT checked; `tools/link-check.py`'s docstring argues
# why a third party's rate limiter must not be able to red a merge.
link-check:
	bash tools/link-check-selftest.sh
	python3 tools/link-check.py

# Night loop: validate the frozen DSL, regenerate, re-validate. NEVER edits specs/**.
night-loop: validate generate
	@echo "night-loop: complete."

# Self-imposed WEEKLY time budget (Claude Code has no native cap). Cap: .claude/loop-budget.json
# (config; nothing writes it). Usage: the append-only ledger .claude/loop-budget/<ISO-week>/*.json,
# which resets each ISO week because the next week is a new directory. `budget-check` exits 2 when the week's budget is spent -- unless the config
# sets "capIsAStopSign": false, in which case over-cap is reported on stderr but exits 0
# (ADR-20260813-132540).
budget-check:
	bash .claude/hooks/loop-budget.sh check

# Budget-aware night loop: skip cleanly when the guard refuses, else run and record elapsed.
# `make -n budgeted-loop` IS NOT A DRY RUN: GNU make executes any recipe line containing $(MAKE)
# even under -n, and this whole if/then is ONE backslash-continued line, so a "dry run" really opens
# a timer, runs night-loop under -n, and BILLS a segment to the weekly cap (measured on #821: two
# 0-second receipts). Use `make budget-check` to look without billing.
# A non-zero `start` is NOT always "budget exhausted": exit 2 = over cap (only while the cap is a
# stop sign), exit 3 = timer integrity (a run timer is already open). The guard's own stderr above
# this message says which.
budgeted-loop:
	@LOOP_BUDGET_RUN_ID="budgeted-loop-$$$$-$$(date -u +%Y%m%dT%H%M%SZ)"; export LOOP_BUDGET_RUN_ID; \
	if bash .claude/hooks/loop-budget.sh start; then \
		$(MAKE) night-loop; rc=$$?; \
		bash .claude/hooks/loop-budget.sh stop; \
		exit $$rc; \
	else \
		echo "budgeted-loop: skipped -- loop-budget.sh start refused; its stderr above says why (over cap, or a timer is already open)."; \
	fi

docs: generate
	@echo "open specs/generated/documentation.generated.html"

# Canonical generated artifacts live in specs/generated/ (committed). $(SCRATCH) is ephemeral scratch.
DSL = specs/generated/c4.generated.dsl
SCRATCH = tools/codegen-rs/out

# Parse-VALIDATE + export the generated Structurizr DSL with the real Structurizr toolchain (catches any
# emitter syntax drift our brace check can't). Uses structurizr-cli if installed, else the Docker image.
# The .mmd exports go to the scratch $(SCRATCH) (never into specs/generated, which must stay clean).
# Gracefully skips when neither is available — the portable DSL still lives at $(DSL).
c4-export: generate
	@mkdir -p $(SCRATCH) && cp $(DSL) $(SCRATCH)/c4.generated.dsl
	@if command -v structurizr-cli >/dev/null 2>&1; then \
		structurizr-cli export -workspace $(SCRATCH)/c4.generated.dsl -format mermaid -output $(SCRATCH); \
	elif command -v docker >/dev/null 2>&1; then \
		MSYS_NO_PATHCONV=1 docker run --rm -v "$$(pwd -W 2>/dev/null || pwd)/$(SCRATCH):/work" structurizr/structurizr export -workspace /work/c4.generated.dsl -format mermaid -output /work; \
	else \
		echo "c4-export: no structurizr-cli or Docker -- skipped. DSL is at $(DSL)"; \
	fi

# Open the model in Structurizr Lite (SystemContext / Containers / ApiComponents views) with the ADRs and
# docs embedded. Stages a docs-enriched workspace under .structurizr/ so the portable $(DSL) stays clean.
c4-render: generate
	@command -v docker >/dev/null 2>&1 || { echo "c4-render: Docker not found -- skipped. DSL is at $(DSL)"; exit 0; }
	@rm -rf .structurizr && mkdir -p .structurizr && cp $(DSL) .structurizr/workspace.dsl && cp -r docs .structurizr/docs
	@node -e "const fs=require('fs'),f='.structurizr/workspace.dsl';let s=fs.readFileSync(f,'utf8');const i=s.lastIndexOf('}');fs.writeFileSync(f,s.slice(0,i)+'  !docs docs\n  !adrs docs/adr\n'+s.slice(i));"
	@echo "Structurizr Lite -> http://localhost:8080  (Ctrl+C to stop)"
	MSYS_NO_PATHCONV=1 docker run --rm -p 8080:8080 -v "$$(pwd -W 2>/dev/null || pwd)/.structurizr:/usr/local/structurizr" structurizr/lite
