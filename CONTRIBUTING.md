# Contributing to Arbiter

Thanks for helping. A few rules keep the project reliable.

## Before you start

- For anything larger than a small fix, open an issue first so we can agree on the approach.
- Read [AGENTS.md](AGENTS.md). Its invariants apply to people too. The main ones:
  - the event log is append-only;
  - event variants are additive only;
  - the daemon owns all state;
  - the service binds only to 127.0.0.1;
  - anything sent to an agent is bounded.

## Making a change

1. Fork the repository and create a branch from `main`.
2. Keep the change focused. Put tests next to the code they cover. Adapter tests replay recorded transcripts and never spend live tokens, and daemon tests use fake launchers.
3. Run the checks:

   ```bash
   cargo fmt --all
   ```

   ```bash
   cargo clippy --workspace --all-targets -- -D warnings
   ```

   ```bash
   cargo test --workspace
   ```

   ```bash
   pnpm --dir apps/desktop build
   ```

4. Open a pull request that explains what changed and why, and how you tested it.

## Style

- Rust 2024 edition, `thiserror` in libraries and `anyhow` in binaries.
- User-facing text is plain and specific. Say what happened and what to do next.
- Never commit secrets, local paths or personal data, including in test fixtures.
