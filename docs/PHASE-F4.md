# F4 checkpoint: review, landing and power tools (2026-09-24)

In progress. The user approved F2–F4. All features below are implemented and fixture-tested. Real GitHub publishing and real-provider comparisons have not been exercised.

## Implemented

- **Review comments on diffs** (`DiffView.tsx`, `landing.rs::review`).
  - Line numbers come from the hunk headers. Each new-file line has a comment control.
  - Draft comments are kept in session storage per thread and sent as one batch of at most 8, each up to 500 characters.
  - **Finished plan:** one review-fix node that must be approved (existing `request_review_fixes`).
  - **Single agent:** one structured message: "Review comments on your changes … `- path:line: text`".
  - Route: `POST /v1/threads/{id}/review`. Sending is refused while an agent is working, but an idle warm process does not block it.
- **Landing** (the "Commit and publish" panel).
  - **Draft text.** `GET …/landing` now returns `untracked` and a `draft` (commit message, PR title and body). The draft is built deterministically from:
    - the plan or thread title, without the task key;
    - handoff summaries, or the latest agent message;
    - the diff stat;
    - the latest checks.

    No model is called, and the user edits the text before committing.
  - **Commit.** It takes tracked changes plus only the new files the user ticks (`include`). Credential-like paths are refused, and unticked new files stay out; the notice says how many.
  - **Publish.** It keeps the existing gates: explicit approval, network on, a matching fingerprint and a clean tree. It pushes and opens a draft PR with `gh`.
- **Safety fix.** Viewing a worktree diff ran `git add --intent-to-add --all` and left the marks in place. A later `commit -a` could then include files nobody chose, credential files among them. The diff now removes the marks afterwards, and landing treats intent-to-add entries as new files needing explicit selection. Regression test: `landing_review.rs`.
- **`agent_working`.** Review, landing and comparisons use it: busy frontier turns, local runs and preparations count as working; a warm idle process does not.
- **Command palette and shortcuts.**
  - Ctrl+K opens a palette of actions and a task search, with arrow keys, Enter and Esc.
  - The shortcuts dialog captures a new combination for any action. A shortcut must include Ctrl/⌘ or Alt, and a conflicting shortcut moves to the new action.
  - Restore defaults is available. Bindings are stored per viewer in localStorage, with defaults if storage is unavailable.
- **Notifications.**
  - Web Notifications are opt-in from the palette and appear only while the window is in the background. They fire for needs-approval, review and failed. Clicking one opens the task.
  - The window title shows a "(n) Arbiter" waiting count, which works without notification permission.
  - The native Tauri notification plugin is not added yet: the build is offline and would need a new crate.
- **Side questions** (`side.rs`, the "Ask aside" button).
  - A local model answers from a bounded summary: request, latest agent message, diff stat, checks and the last 3 side questions, capped at 8,000 characters.
  - They are recorded as an additive `side_chat` event and never forwarded to the agent. No full event list is serialized into any agent prompt.
  - With no local model it says so; there is no frontier fallback.
- **Opt-in best-of-N** (`compare.rs`, "Compare agents on this task" under Advanced in the composer).
  - **Limits.** 2–3 explicit candidates: claude, codex, or local with an installed model, at most one local. It requires `approved: true` and a consent checkbox.
  - **Pre-checks.** Cloud candidates must fit within the parallel cloud-run limit. Every candidate is admission-checked before anything is created.
  - **Running.** Candidates run in separate worktrees. The comparison view shows status, checks, a shortstat and tokens per candidate.
  - **Keep one.** The others are stopped and settled with a notice; their worktrees are never deleted.
  - Additive events: `comparison_joined` and `comparison_decided`.

## Verification

- **Tests:**
  - `landing_review.rs`: comment bounds, comment delivery, a diff not leaving index marks, the draft, credential refusal, a stale fingerprint, and a selective commit whose contents are checked.
  - `side_chat.rs`: bounds, bounded context, recorded but not forwarded, and follow-ups that see earlier questions.
  - `compare.rs`: refusal without approval, refusal over the parallel limit with nothing created, two candidates with different results, keep and settle, and a second keep refused.
- **Workspace:** 127 passed, 0 failed, 1 ignored. Clippy is clean with `-D warnings`, and the UI build passes.
- **Browser:**
  - A diff line comment was drafted.
  - The landing panel listed the new file unticked; a UI commit then contained exactly the modified file and the ticked new file.
  - The publish form appears once the tree is clean. It was not submitted: that is an outward action, and there is no remote.
  - The palette opened a task and a rebind moved a conflicting shortcut; the defaults were then restored.
  - Ask aside shows the no-model message.
  - The comparison setup renders with consent gating. It was not submitted.
- **Not live-tested:**
  - Notifications need an OS permission grant from the user.
  - Real `gh pr create`.
  - Real multi-provider comparisons, which spend allowance.

## Open

- A native notification plugin for the packaged app.
- Splitting review comments into several fix nodes.
- Commit and PR for plan roots from PlanView. The backend supports it; the panel is currently on worktree threads.
- A local-model-polished PR description, as an option.
- Comparison learning: recording the kept candidate as an outcome for routing.
