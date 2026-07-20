# Captain.Food — developer & loop entrypoints.
# Most "gates" are folded into the single codegen validator (`validate`): schema, behaviour-test
# coverage, observability contracts, and C4 consistency are all checked there. They are exposed as
# separate targets so the loop reads like the playbook, but they currently delegate to `validate`.
#
# The codegen is the Rust tool (tools/codegen-rs, ADR-0034); it needs a local Rust toolchain (`cargo`).

CODEGEN_RS = tools/codegen-rs

.PHONY: typecheck validate-schema test-behaviour test-observability c4-validate validate generate check-drift review gate night-loop budget-check budgeted-loop docs c4-export c4-render help rust rust-build rust-test smoke-prod

help:
	@echo "targets: validate generate typecheck review gate night-loop budgeted-loop budget-check docs"
	@echo "         c4-render (Structurizr Lite + docs/ADRs) · c4-export (validate/export DSL)"
	@echo "         (validate-schema test-behaviour test-observability c4-validate -> all fold into 'validate')"
	@echo "         budgeted-loop runs the night loop under a 30-min/week budget (.claude/loop-budget.json)"
	@echo "         codegen = tools/codegen-rs (Rust, ADR-0034); needs cargo. 'rust' = build+test alias."

# Production E2E smoke (Stripe TEST mode) against the live deployment — tools/smoke/README.md.
# Needs: STRIPE_SECRET_KEY (sk_test), RENDER_API_KEY (or SUPABASE_URL+SUPABASE_SECRET_KEY).
smoke-prod:
	bash tools/smoke/prod-smoke.sh

# `typecheck` = the Rust compiler is the type gate (build must succeed).
typecheck:
	cd $(CODEGEN_RS) && cargo build

# The codegen validator is the single source of truth for these gates (validate.ts §1–§11 in Rust).
validate-schema:
	cargo run --manifest-path $(CODEGEN_RS)/Cargo.toml -- --check --specs specs

test-behaviour: validate-schema      ## behaviour-test coverage is enforced inside `validate`
test-observability: validate-schema  ## observability contracts are validated inside `validate`
c4-validate: validate-schema         ## C4 consistency is validated inside `validate`

validate: typecheck validate-schema

# Generate every artifact from the specs (writes into specs/generated/** + the database.md §2 region).
generate:
	cargo run --manifest-path $(CODEGEN_RS)/Cargo.toml -- --specs specs

# Regenerate, then fail if the result drifts from what's committed (the CI drift gate, runnable locally).
# Whole-tree diff (matches CI): generated output spans specs/generated + specs/database.md AND the
# generated Rust under crates/**/generated. Run on a clean tree — it's the gate, not a mid-edit helper.
check-drift: generate
	@git diff --quiet --ignore-cr-at-eol || { echo "check-drift: generated artifacts drifted — run 'make generate' and commit the regenerated files."; git --no-pager diff --ignore-cr-at-eol --stat; exit 1; }

# --- Rust codegen build/test aliases (ADR-0034). ---
rust-build:
	cd $(CODEGEN_RS) && cargo build
rust-test:
	cd $(CODEGEN_RS) && cargo test
rust: rust-build rust-test validate check-drift
	@echo "rust: build + test + validate + generate(+diff) OK"

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
# (default 30 min/week; resets each ISO week). `budget-check` exits 2 when the week's budget is spent.
budget-check:
	bash .claude/hooks/loop-budget.sh check

# Budget-aware night loop: skip cleanly when the weekly budget is spent, else run and record elapsed.
budgeted-loop:
	@if bash .claude/hooks/loop-budget.sh start; then \
		$(MAKE) night-loop; rc=$$?; \
		bash .claude/hooks/loop-budget.sh stop; \
		exit $$rc; \
	else \
		echo "budgeted-loop: skipped — weekly budget exhausted (resets Monday)."; \
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
		echo "c4-export: no structurizr-cli or Docker — skipped. DSL is at $(DSL)"; \
	fi

# Open the model in Structurizr Lite (SystemContext / Containers / ApiComponents views) with the ADRs and
# docs embedded. Stages a docs-enriched workspace under .structurizr/ so the portable $(DSL) stays clean.
c4-render: generate
	@command -v docker >/dev/null 2>&1 || { echo "c4-render: Docker not found — skipped. DSL is at $(DSL)"; exit 0; }
	@rm -rf .structurizr && mkdir -p .structurizr && cp $(DSL) .structurizr/workspace.dsl && cp -r docs .structurizr/docs
	@node -e "const fs=require('fs'),f='.structurizr/workspace.dsl';let s=fs.readFileSync(f,'utf8');const i=s.lastIndexOf('}');fs.writeFileSync(f,s.slice(0,i)+'  !docs docs\n  !adrs docs/adr\n'+s.slice(i));"
	@echo "Structurizr Lite → http://localhost:8080  (Ctrl+C to stop)"
	MSYS_NO_PATHCONV=1 docker run --rm -p 8080:8080 -v "$$(pwd -W 2>/dev/null || pwd)/.structurizr:/usr/local/structurizr" structurizr/lite
