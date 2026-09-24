# Working on Arbiter (for coding agents)

## Invariants: do not break these
- **Tasks are mutable planning records, unlike thread events.** The `tasks` table is updated in place. Task status follows thread status in `service.rs::sync_tasks`, and every event write must go through `AppState::append` so that sync runs.
- **The event log is append-only.** Never UPDATE or DELETE rows in `events`. Read models live in projection tables and must be rebuildable with `Store::rebuild_projections`.
- **`EventKind` and `AgentEvent` variants are additive.** Never rename or remove one, because old logs must still deserialize. Add new fields as `Option`/`#[serde(default)]`.
- **The daemon owns all state and processes.** UIs (desktop, CLI) talk only to the HTTP/WS API. No business logic goes in `apps/desktop/src-tauri`.
- **Cross-platform.** Anything OS-specific goes behind a trait or `cfg` in `arbiter-supervisor`. Use the `git` CLI (not libgit2). Keep worktree paths short (Windows MAX_PATH).
- **Security.** Bind only to 127.0.0.1. Every route except `/v1/health` requires the bearer token. CORS is an explicit allow-list.
- **Token discipline.** Anything sent back to an agent (heal feedback, memory, context) must be bounded and deduplicated. Never forward raw logs.

## Commands
- Run the tests: `cargo test --workspace`
- Run clippy: `cargo clippy --workspace --all-targets -- -D warnings`
- Check the UI: `pnpm --dir apps/desktop build` (runs tsc + vite)
- Run a dev daemon in isolation: `cargo run -p arbiterd -- --home <tmpdir> --port 7439`. Then open `http://localhost:1420/?port=7439&token=<token from daemon.json>` while `pnpm --dir apps/desktop dev` runs.

## Plan
The product plan and phases are in `docs/PLAN.md`. Read it before starting a feature, and keep it updated when decisions change.

## Adapters
- Each harness is a `Codec`: pure protocol translation with no I/O, tested by replaying the transcripts in `crates/arbiter-adapters/tests/fixtures/`. Re-record fixtures when a harness changes its protocol, and sanitize them (no local paths, tool lists or signatures).
- Claude: `total_cost_usd` is cumulative per process, so emit deltas. `usage` in `result` is per turn.
- Codex: `thread/tokenUsage/updated` totals are cumulative, so emit deltas. Codex reports no prices.
- Daemon tests use fake launchers (`Config.launcher`) and must never start a real harness.

## Style
- Rust 2024 edition, `thiserror` in libraries, `anyhow` in binaries.
- Keep crates focused. New subsystems (heal, decider, index, memory, mcp, adapters) get their own `crates/arbiter-*`.
- Tests sit next to the code. Adapter tests replay recorded transcripts and never spend live tokens.
