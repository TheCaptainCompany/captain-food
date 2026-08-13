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

.PHONY: typecheck validate-schema test-behaviour test-observability c4-validate validate warning-baseline generate check-drift review gate night-loop budget-check budgeted-loop docs c4-export c4-render help rust rust-build rust-test test-crates smoke-prod

help:
	@echo "targets: validate generate typecheck test-crates review gate night-loop budgeted-loop budget-check docs"
	@echo "         warning-baseline = refresh the warning ratchet (tools/codegen-rs/warning-baseline.json)"
	@echo "         test-crates = the WORKSPACE test gate (#474): cargo test --workspace with the DB"
	@echo "         suites REQUIRED. 'make rust' is the spec gate and runs NO crates/** test."
	@echo "         c4-render (Structurizr Lite + docs/ADRs) | c4-export (validate/export DSL)"
	@echo "         (validate-schema test-behaviour test-observability c4-validate -> all fold into 'validate')"
	@echo "         budgeted-loop runs the night loop under a 30-min/week budget (.claude/loop-budget.json)"
	@echo "         codegen = tools/codegen-rs (Rust, ADR-0034); needs cargo. 'rust' = build+test alias."

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
rust: rust-build rust-test validate check-drift
	@echo "rust: build + test + validate + generate(+diff) OK"
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
DB_TEST_RECEIPT = target/db-test-skips.log
test-crates:
	@rm -f $(DB_TEST_RECEIPT)
	@mkdir -p target
	DB_TEST_SKIP_RECEIPT=$(abspath $(DB_TEST_RECEIPT)) $(CARGO) test --workspace --no-fail-fast; \
	  status=$$?; \
	  if [ -s $(DB_TEST_RECEIPT) ]; then \
	    echo "test-crates: DB-GATED SUITES SKIPPED -- this run exercised NO database behaviour."; \
	    echo "test-crates: skipped $$(cut -f1 $(DB_TEST_RECEIPT) | sort -u | wc -l) suite(s): $$(cut -f1 $(DB_TEST_RECEIPT) | sort -u | tr '\n' ' ')"; \
	    echo "test-crates: re-run with DATABASE_URL set to exercise them (see docs/claude/sessions.md)."; \
	  fi; \
	  exit $$status

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

# Night loop: validate the frozen DSL, regenerate, re-validate. NEVER edits specs/**.
night-loop: validate generate
	@echo "night-loop: complete."

# Self-imposed WEEKLY time budget (Claude Code has no native cap). State: .claude/loop-budget.json
# (resets each ISO week). `budget-check` exits 2 when the week's budget is spent -- unless the config
# sets "capIsAStopSign": false, in which case over-cap is reported on stderr but exits 0
# (ADR-20260813-132540).
budget-check:
	bash .claude/hooks/loop-budget.sh check

# Budget-aware night loop: skip cleanly when the guard refuses, else run and record elapsed.
# A non-zero `start` is NOT always "budget exhausted": exit 2 = over cap (only while the cap is a
# stop sign), exit 3 = timer integrity (a run timer is already open). The guard's own stderr above
# this message says which.
budgeted-loop:
	@if bash .claude/hooks/loop-budget.sh start; then \
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
