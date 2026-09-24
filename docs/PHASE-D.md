# Phase D: approved plans and coordinated execution

Implemented on 2026-09-23. This is an alpha workflow milestone, not a claim of feature parity with a mature desktop product.

## User flow

1. Create a task with **Plan and coordinate agents** enabled (the desktop default).
2. Answer intake cards if needed. A small request gets an editable local template; complex requests escalate to a read-only harness planner.
3. Review the graph, goals, writable scope, dependencies, acceptance commands, model assignments and budgets. Edit the draft before approval.
4. **Approve and run** approves that exact revision. Steps run in separate worktrees; dependencies wait for integration. Parent/child tasks appear in the sidebar and kanban.
5. Follow the agent map, step list or timestamped timeline. Each step exposes its stored brief, handoff, agent task and merge-repair task when present.
6. Scope drift, exhausted budgets and unresolved failures pause the plan. Scope/budget changes require an explicit action; resuming is separate.
7. Review **Combined changes** on the isolated plan branch. This phase does not land that branch or create a PR.

## Durability and checks

- `arbiter-plan` validates plans and replays additive `Plan` events. The daemon owns all scheduling and Git operations.
- Node task IDs, worktrees, bases, briefs, snapshots and integration commits are persisted. Git ancestry makes completed integrations idempotent.
- Independent steps overlap. The concurrency cap is the minimum of the requested limit, half the available logical CPUs, and an initial three slots that can grow after successful integrations (absolute maximum six).
- Provider reset times hold a step until retry is available. The existing run supervisor handles process recovery and detected project checks.
- Explicit acceptance commands run before node integration and again on the combined branch. Failed node acceptance gets at most two additional turns; repeated feedback stops earlier.
- Diff-based scope checks include untracked files and run before integration. A scoped merge-repair agent resolves conflicts; its changed files are checked against the captured conflict baseline.
- Pause persists immediately, stops agents and cancels a running acceptance check. A paused plan remains paused after restart.
- Budgets use cumulative reported usage, including merge-repair cost in the plan total. Dollar caps cannot account for costs a provider does not report; use token caps for those steps.
- Agent briefs are bounded to 6,000 UTF-8 bytes. Required scope/checks are preserved; optional dependency context is shortened or omitted when space is exhausted. This is an approximate token budget, not a tokenizer-exact 1,500-token guarantee. Full handoffs remain in the log/UI.

## API and CLI

All endpoints retain loopback binding and bearer authentication:

| Action | Endpoint |
|---|---|
| Read/edit draft | `GET/PATCH /v1/threads/{id}/plan` |
| Read-only planner | `POST /v1/threads/{id}/plan/draft` |
| Approve revision | `POST /v1/threads/{id}/plan/approve` |
| Pause/resume | `POST /v1/threads/{id}/plan/control` |
| Extend scope | `POST /v1/threads/{id}/plan/scope` |
| Change paused-plan budgets | `POST /v1/threads/{id}/plan/budget` |
| Combined diff | `GET /v1/threads/{id}/plan/diff` |
| Agent handoff/escalation | `POST /v1/threads/{id}/handoff`, `/scope-request` |

Use `arb new "request" --workflow` and `arb plan show|edit|refine|approve|pause|resume|scope|budget|diff`. Approval requires `--revision`. `--json` provides structured responses. MCP adds `report_handoff` and `request_scope`; the final JSON response is a fallback when MCP is unavailable.

## Verification evidence

- Gates: 94 workspace tests passed, workspace clippy passed with warnings denied, desktop TypeScript/Vite build passed, and the strict UI audit reported zero findings.
- The daemon integration suite uses real Git and fake launchers: three nodes with dependency ordering and parallel overlap; scope rejection and explicit extension; custom acceptance healing and repetition stop; long-check cancellation; budget approval; conflicting changes repaired once while preserving both steps.
- Projection rebuild coverage preserves plan parent/child links.
- A separate process was forcibly killed with two independent steps active. After restart, the same two child IDs and plan worktree were reused, all three nodes integrated, and only four task threads existed (one root, three children).
- Browser verification covered invalid-edit recovery, saving a revision, approval, agent map, handoffs, combined diff, timeline and a 900-pixel desktop window.
- Preview fixture: `cargo run -p arbiterd --example phase_d_preview -- <home> <git-repo> 7441 8000`. It uses fake agents, creates a sample plan only in an empty fixture home, and retains it on restart. Generated code is deliberately a small test fixture.
- Screenshots and before/after recovery state are under `target/phase-d/`. No paid harness execution was used for this checkpoint.

## Remaining product work

- User-requested follow-up: screenshot review controls and small-window usability fixes were approved and implemented as the first Phase E milestone. See `PHASE-E.md` for evidence and remaining release gates.

- Phase C's neural first-card latency target is still unmet. Local plan drafts currently use a deterministic template, not a latency-qualified local planning model.
- Concurrency is conservative CPU/success based; RAM pressure sensing and learned provider/model failover are not implemented.
- The scheduler serializes coordination and Git/check operations across plans. Agents within a plan run concurrently; heavy checks can delay coordination of another plan.
- The graph is a keyboard-operable scrollable dependency map, not a zoomable canvas editor. The step list is the alternative navigation view.
- Vault-backed context, measured strengths and learned routing are implemented in Phase E; see its checkpoint for limits. Rich PR review/landing, inline review comments, command palette, notifications and side chat belong to Phase F.
- Installer/onboarding hardening, a complete accessibility audit, broad operating-system testing, realistic paid-provider trials and a measured token-regression suite remain release work.

Passing this milestone demonstrates the coordinated workflow and crash recovery. It does not make the overall product production-ready.
