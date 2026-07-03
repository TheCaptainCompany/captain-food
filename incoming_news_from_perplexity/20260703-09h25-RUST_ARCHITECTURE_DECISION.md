# Architecture Decision: Full-Stack Rust

**Date:** 2026-07-03  
**Status:** Adopted  
**Author:** CTPO, Captain.Food

---

## ⚠️ CRITICAL INSTRUCTION FOR CLAUDE CODE

**This codebase is written entirely in Rust. Do not generate TypeScript, JavaScript, or any other language unless explicitly instructed for a specific integration boundary.**

Specific rules:
- Frontend → **Leptos** (Rust, compiled to WASM). Not React. Not Next.js. Not TypeScript.
- Backend → **Axum + Tokio + SQLx** (Rust). Not Node.js. Not Express. Not tRPC.
- Shared business logic → **Crux Core** (Rust). Not TypeScript services.
- Desktop shell → **Tauri 2.0** with Leptos inside. Not Electron. Not a web wrapper.
- Mobile UI shells → **SwiftUI** (iOS) and **Jetpack Compose** (Android) calling Rust core via UniFFI bindings. The Rust core itself is never Swift or Kotlin.
- Configuration, scripts, CI → **TOML / YAML / shell**. Not package.json. Not tsconfig.

**If you are about to write `const`, `interface`, `import from`, `export default`, `npm install`, or `package.json` — stop. You are about to generate TypeScript/JavaScript. That is wrong for this project.**

The only exception: UniFFI-generated Swift and Kotlin binding files are auto-generated — do not write or edit them manually.

---

## Context

This project started with a TypeScript/Next.js assumption. After architectural exploration, the decision has been made to go **full-stack Rust** across all platforms. This document explains the rationale and the concrete implications for every layer of the system.

---

## The Core Principle

> Rust's compiler is the best code reviewer we have. In an agentic development model (Claude Code generating code from specs), the borrow checker acts as a zero-cost quality gate that catches structural errors before runtime. The stricter the type system, the better the AI agent performs — constraints eliminate ambiguity.

This is not a technology preference. It is an architectural decision based on:

1. **Spec-driven code generation** — Claude Code generates Rust from `captain_food_ui_spec.yaml` and `domain_spec.yaml`. The compiler validates structural correctness automatically.
2. **Single shared core** — Business logic written once in Rust, compiled to all target platforms (web, iOS, Android, desktop).
3. **Agentic engineering model** — Not vibe-coding. The spec is the contract, the compiler is the enforcer, Claude Code is the implementer. Humans define decisions, agents implement structure.

---

## Platform Decision Table

| Platform | UI Layer | Business Logic | Bridge | Framework |
|---|---|---|---|---|
| **Web (customer app)** | Leptos (Rust/WASM) | Rust Core via Crux | Direct (WASM) | Leptos + Axum |
| **iOS (future)** | SwiftUI | Rust Core via Crux | UniFFI → Swift bindings | Crux + UniFFI |
| **Android (future)** | Jetpack Compose | Rust Core via Crux | UniFFI → Kotlin bindings | Crux + UniFFI |
| **Windows (restaurant manager)** | Leptos in Tauri shell | Rust Core via Crux | Direct (same Rust process) | Tauri 2.0 |
| **Backend BFF** | — | Rust Core | Direct | Axum + Tokio + SQLx |

---

## Language Per File Type

| File / concern | Language | Crate / tool |
|---|---|---|
| UI components | Rust | `leptos` |
| Routing (web) | Rust | `leptos_router` |
| Server functions | Rust | `leptos` server actions |
| HTTP server | Rust | `axum` |
| Async runtime | Rust | `tokio` |
| Database queries | Rust | `sqlx` (compile-time checked) |
| Business logic core | Rust | `crux` |
| State management | Rust | Crux model |
| Domain events | Rust | Crux events |
| Validation schemas | Rust | `validator` or `garde` crates |
| Serialization | Rust | `serde` + `serde_json` |
| GraphQL | Rust | `async-graphql` |
| Auth (JWT/session) | Rust | `jsonwebtoken` + Supabase REST |
| i18n | Rust | `leptos_i18n` |
| Tests | Rust | built-in `#[test]` + `rstest` |
| CI scripts | Shell / YAML | GitHub Actions |
| DB migrations | SQL | `sqlx migrate` |
| Config | TOML | `Cargo.toml`, `config` crate |
| iOS UI shell | Swift | SwiftUI (hand-written, thin) |
| Android UI shell | Kotlin | Jetpack Compose (hand-written, thin) |
| Mobile bindings | Auto-generated | UniFFI (never edit manually) |

---

## Project Structure

```
captain-food/
├── Cargo.toml                  # workspace root
├── crates/
│   ├── core/                   # Crux shared business logic (pure Rust, no side effects)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── model.rs        # App state
│   │   │   ├── events.rs       # All user/system events
│   │   │   └── capabilities.rs # Declared side effects (HTTP, storage...)
│   ├── shared_types/           # Serde types shared across crates + UniFFI
│   ├── web/                    # Leptos frontend (compiles to WASM)
│   │   └── src/
│   │       ├── main.rs
│   │       ├── registry.rs     # GENERATED — SDUI component registry
│   │       ├── renderer.rs     # Recursive SDUI renderer
│   │       └── components/     # One .rs file per SDUI component type
│   ├── server/                 # Axum backend (BFF + API)
│   │   └── src/
│   │       ├── main.rs
│   │       ├── screens.rs      # Screen hydration handlers
│   │       ├── resolvers.rs    # SDUI data resolver allowlist
│   │       └── graphql/        # async-graphql schema
│   └── desktop/                # Tauri shell (restaurant manager)
│       └── src/
│           └── main.rs
├── ios/                        # SwiftUI shell (thin — calls Rust core via UniFFI)
├── android/                    # Compose shell (thin — calls Rust core via UniFFI)
├── supabase/
│   └── migrations/             # GENERATED SQL from domain_spec.yaml
├── specs/
│   ├── captain_food_ui_spec.yaml     # UI source of truth
│   └── domain_spec.yaml             # Domain source of truth
└── scripts/
    └── generate.rs             # Codegen script (spec → Rust artifacts)
```

---

## Why Leptos for Web (not React/Next.js)

- Fine-grained reactivity — no Virtual DOM diffing, direct DOM updates
- SSR + hydration built-in, equivalent to Next.js App Router RSC model
- Same language as the backend — no context switching, shared types natively
- Integrates natively with Axum for server functions
- Claude Code generates Leptos components from the SDUI spec — same workflow, same spec contract

**Known tradeoff:** No shadcn/ui equivalent. All UI components are generated from scratch by Claude Code based on `captain_food_ui_spec.yaml`. The spec fully defines component structure, props, and behavior — Claude Code has everything it needs.

---

## Why Crux for Cross-Platform Core

Crux implements the **Ports and Adaptors pattern** (Hexagonal Architecture) in Rust:

- The **Core** contains pure business logic — state machines, commands, events, queries — zero side effects
- The **Shell** on each platform handles rendering and side effects (HTTP, storage, notifications)
- The Core is **100% testable without a device** — unit tests run anywhere without mocks

The Rust Core IS the implementation of `domain_spec.yaml`. Every aggregate, command, event, and query declared in the spec maps 1:1 to a Crux Core module.

---

## What "Spec-Driven + Rust" Means for Claude Code

The combination of a strict spec and a strict compiler creates a **two-layer constraint system**:

1. **Spec layer** — Claude Code cannot generate components, types, resolvers, or actions not declared in the spec. CI diff check enforces this.
2. **Compiler layer** — The borrow checker, exhaustive pattern matching, and type system catch what the spec doesn't cover.

**Every Claude Code session must start with the relevant spec file as context:**
- Frontend work → `specs/captain_food_ui_spec.yaml` + `SDUI_ARCHITECTURE.md`
- Backend/core work → `specs/domain_spec.yaml`
- Architectural decisions → this file

Never ask Claude Code to generate code without providing the relevant spec. **The spec is the prompt.**

---

## What This Architecture Is NOT

- **Not vibe coding.** Karpathy declared vibe coding obsolete in February 2026. This is agentic engineering: spec → agent → compiler validation → human review of business logic only.
- **Not a rewrite risk.** The spec is renderer-agnostic. Swapping Leptos for another renderer only requires a new component registry — the spec, backend, business logic, and SDUI JSON in Supabase don't change.
- **Not premature optimization.** Rust is chosen because the compiler is the most valuable tool in an AI-assisted workflow where human review time is scarce.

---

## Evolution from Initial Assumptions

| Layer | Previous assumption | Current decision |
|---|---|---|
| Web frontend | Next.js + React + TypeScript | Leptos (Rust/WASM) |
| Backend BFF | Next.js RSC / Node.js | Axum + Tokio (Rust) |
| Business logic | TypeScript services | Crux Core (Rust), shared across platforms |
| iOS | React Native or Flutter | SwiftUI shell + Rust Core via UniFFI |
| Android | React Native or Flutter | Jetpack Compose shell + Rust Core via UniFFI |
| Desktop (restaurant) | Electron or web | Tauri 2.0 + Leptos + Rust Core |
| Validation | Zod (TypeScript) | `validator` / `garde` crates (Rust) |
| GraphQL | TypeScript resolvers | `async-graphql` (Rust) |
| Spec role | Generation artifact (one-time) | Permanent source of truth, governs all layers |
