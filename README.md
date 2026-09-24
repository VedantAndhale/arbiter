<div align="center">

# Arbiter

**Describe what you want built. Arbiter plans it, runs coding agents on it in parallel, checks their work and repairs it, using as few tokens as possible.**

[![CI](https://github.com/VedantAndhale/arbiter/actions/workflows/ci.yml/badge.svg)](https://github.com/VedantAndhale/arbiter/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/VedantAndhale/arbiter?include_prereleases&sort=semver)](https://github.com/VedantAndhale/arbiter/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A desktop app and local service that orchestrate **Claude Code** and **Codex** with your own subscriptions. Windows first; macOS and Linux build from the same code.

</div>

---

## Why Arbiter

Coding agents are powerful, but unsupervised they collide, stall, burn tokens and leave tests red. Arbiter sits above them and takes care of the rest:

| | |
|---|---|
| **Plans, then runs in parallel** | Large requests become a plan you approve. Each step runs in its own git worktree, and the steps merge together only after their checks pass. |
| **Self-healing** | Crashes, stalls and rate limits are resumed or rerouted. A failing test, build or lint goes back to the agent as a short, bounded excerpt until it passes. |
| **Token discipline** | Deterministic fixes come before any model call. A local model handles triage, questions, reviews, web research and commit messages. Frontier agents get only what they need. |
| **Several projects, one feature** | A task can span repositories, such as a data pipeline and the app that uses it. Each step works in its own project and hands its interface to the next. |
| **Clean publishing** | Your work lands as one commit with a clear message on a normal branch, or as a draft PR. Arbiter's own branches never leave your computer. |
| **Your memory, kept separate** | Notes, your guidance for agents and a full backup live in their own private Git repository. Nothing personal is written into projects you share. |
| **Bring your own subscriptions** | You sign in to Claude Code or Codex yourself. Arbiter never proxies your tokens, and paid usage is off unless you turn it on. |

## Is it free?

Yes, completely. Arbiter adds no fees and sells nothing: you bring your own Claude Code or Codex subscription, and Arbiter helps you get more out of it. It's MIT-licensed, so if it ever stops serving you, you have everything you need to fork it and build the version you want.

> **Status: early.** Arbiter is young and moving fast. Expect rough edges, and please [open an issue](https://github.com/VedantAndhale/arbiter/issues) when you hit one.

## Who it's for

- **Developers** who want parallel agents, real diffs and review, a terminal and an editor, without babysitting runs.
- **New developers** who want the steps explained in plain words and a working app they can see.
- **Founders building their first product**, who describe the outcome and let Arbiter ask only the questions that matter.

## Install

Download the latest installer from **[Releases](https://github.com/VedantAndhale/arbiter/releases)** and run it. It installs for your user, with no admin rights needed. The app updates itself: new versions download in the background and install when you close Arbiter, or right away from the banner at the top.

> Early builds are not code-signed yet, so Windows SmartScreen may warn. Choose **More info → Run anyway**.

On first start, **Settings → Set up automatically** checks your tools. Claude Code, Codex, Git and Node are detected, can be installed with your approval, and open their sign-in when needed.

## How it works

```
Desktop app ─┐
arb CLI     ─┼─ HTTP + WebSocket on 127.0.0.1 (bearer token) ─► arbiterd (Rust)
             │                                                   ├─ append-only event log (SQLite WAL)
             │                                                   ├─ supervisor: worktrees, process trees, terminals
             │                                                   ├─ adapters: Claude Code, Codex, local models
             │                                                   └─ heal engine · planner · memory · browser checks
```

The background service owns all state and processes, and every UI is a client of it. Agents keep working when the window closes. Everything listens only on `127.0.0.1`, and every route except health needs a token.

| Crate | Purpose |
|---|---|
| `arbiter-core` | IDs, the event schema, plans, daemon discovery |
| `arbiter-store` | Append-only event log, projections, fork, backup and restore |
| `arbiter-supervisor` | Git worktrees, process-tree control, terminals, prerequisites |
| `arbiter-adapters` | Claude Code (stream-json) and Codex (app-server JSON-RPC) as normalized events |
| `arbiter-heal` | Check detection, runners, failure parsers, minimal excerpts |
| `arbiter-plan` | Plan validation and replay |
| `arbiter-intake` | Local models: classification, questions, intent specs |
| `arbiter-local` | The local coding agent's bounded tool set |
| `arbiter-browser` | Headless Chromium for previews, checks and rendered research |
| `arbiter-vault` | Guarded notes and bounded memory retrieval |
| `arbiter-mcp` | Arbiter's tools for agents (research, handoffs, browser) |
| `arbiterd` | The background service and its HTTP/WS API |
| `arbiter-cli` | The `arb` command-line client |
| `apps/desktop` | Tauri 2, React, TanStack Query, Tailwind |

## Privacy and safety

- Code and prompts go only to the coding tools you signed in to, and to nothing else.
- Web research runs on your computer, and only public questions reach the search engine. Summaries from sites you are signed into are checked locally, and you are asked before an agent sees anything that looks private.
- Your memory backup redacts anything that looks like a secret. It goes only to the remote you choose.
- Arbiter never pushes its own branches, never force-pushes, and asks before every push or pull request.

## Develop

Requirements: Rust (stable), Node 22+, pnpm, Git, CMake, a C++ compiler and libclang. See [local runtime setup](docs/PHASE-C.md#development).

```bash
cargo test --workspace
```

```bash
cargo build -p arbiterd
```

```bash
pnpm --dir apps/desktop tauri dev
```

- **Testing the whole app by hand:** see [docs/TESTING.md](docs/TESTING.md).
- **Guidance for coding agents** working on this repository: see [AGENTS.md](AGENTS.md).
- **The product plan and decisions:** see [docs/PLAN.md](docs/PLAN.md).

### Releases

Every push to `main` runs the tests on Windows, macOS and Linux, and builds a Windows installer you can download from the run. Publishing a GitHub Release tagged `vX.Y.Z` builds the signed update and attaches it. Running apps pick it up automatically.

## Contributing

Issues and pull requests are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) first; security reports go through [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE) © 2026 Vedant Andhale. Claude Code and Codex are trademarks of their respective owners; Arbiter is an independent project and is not affiliated with them.
