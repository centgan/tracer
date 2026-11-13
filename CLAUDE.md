# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

- `npm run tauri dev` — run the full desktop app (spawns `vite dev` on port 1420, then the Tauri Rust shell). This is the primary dev command.
- `npm run dev` — run only the SvelteKit frontend in the browser (no Tauri APIs available; `invoke` calls will fail).
- `npm run build` — build the static frontend into `../build` (consumed by Tauri as `frontendDist`).
- `npm run tauri build` — produce a distributable desktop bundle.
- `npm run check` — type-check Svelte + TypeScript (`svelte-kit sync` then `svelte-check`). Use `npm run check:watch` for continuous checking.
- Rust: `cargo build` / `cargo check` / `cargo test` from within `src-tauri/`.

There is no test runner or linter configured yet beyond `svelte-check` and the Rust toolchain.

## Architecture

Tauri 2 desktop app: a Rust backend hosting a SvelteKit (Svelte 5) frontend rendered in a webview.

- **Frontend** (`src/`): SvelteKit in SPA mode. `svelte.config.js` uses `adapter-static` with an `index.html` fallback, and `src/routes/+layout.ts` sets `ssr = false` — required because Tauri has no Node server. Uses Svelte 5 runes (`$state`, etc.). The frontend calls into Rust via `invoke("command_name", args)` from `@tauri-apps/api/core`; every such command must be registered in `generate_handler!` in `src-tauri/src/lib.rs`.
- **Backend** (`src-tauri/src/`): `main.rs` is a thin entry point delegating to `tracer_lib::run()` in `lib.rs`. `lib.rs` builds the Tauri app, registers plugins, and wires up command handlers. The crate is named `tracer_lib` (see `Cargo.toml` `[lib]`) to avoid a Windows name clash with the binary.

The frontend↔backend contract is the set of `#[tauri::command]` functions listed in `generate_handler!`. Adding a feature that touches Rust means: write the command, add it to `generate_handler!`, then `invoke` it from Svelte.

### Intended domain (in progress)

The app is a file version tracker ("tracer"). `src-tauri/src/db.rs` defines the SQLite schema via `init_db`:
- `files` — tracked file paths with a `status`.
- `revisions` — per-file snapshots referencing a `delta_path` on disk, with `timestamp` and `byte_size`.
- `milestones` — user-labeled revisions (one per revision).

The Cargo dependencies signal the planned pipeline: `notify` (watch files for changes) → `zip`/`flate2` (compress/store deltas) → `rusqlite` (index revisions/milestones). **Note:** `db.rs` is not yet imported or called from `lib.rs`, and the only registered command is the scaffold `greet`. Wiring the database and file-watching into the Tauri app is the current build-out work.

## Worktree workflow

When a new task is assigned and work happens in a separate git worktree, do **not** merge the worktree branch back into `main`. Instead:

1. Verify there are no merge conflicts between the worktree and `main`.
2. Copy the changed files over into the main working tree directly.
3. Delete the worktree once the files are copied.

## Conventions

- Tauri capabilities/permissions live in `src-tauri/capabilities/`; new plugin or command permissions the frontend needs must be granted there.
- `src-tauri/gen/` and `src-tauri/target/` are generated/build artifacts — do not edit.
