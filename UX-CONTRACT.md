# Arbiter UI contract

## Sources

| Concern | Source |
|---|---|
| Product flow and phase scope | docs/PLAN.md |
| State ownership, append-only events, authentication | AGENTS.md; crates/arbiterd/src/api.rs |
| Thread lifecycle and attachment delivery | crates/arbiterd/src/service.rs |
| Visual identity and runtime tokens | DESIGN.md; apps/desktop/src/index.css |

The daemon owns domain state. The desktop communicates only through authenticated API requests. The preview receives rendered pixels from a daemon-owned browser page; inspected app JavaScript has no access to the desktop DOM or bearer token.

## Canonical UI Map

| Capability | Canonical owner | Source of truth | Allowed variants | Verification |
|---|---|---|---|---|
| Form | components/ui.tsx Input; Draft and ThreadView composers | API validation and this contract | task / follow-up | build + browser flow |
| Scrollbar | src/index.css global baseline | DESIGN.md | forced colors | browser computed style |
| Select/Listbox | existing native selects and RunControls picker | existing app conventions | platform popup / model picker | keyboard browser flow |
| Dialog | components/ui.tsx Dialog | this contract | default / viewer / navigation | focus, Escape, cancel |
| Upload | components/Attachments.tsx | attach.rs; this contract | draft / follow-up | API integration + browser flow |
| Feedback | owning panel's inline status/error region | this contract | pending / success / error | failure recovery |
| Setup | components/Setup.tsx; shared Input/Button/Dialog | arbiter-setup preferences + daemon status API | first run / reopen | API revision/policy tests + browser walkthrough |

## Flow ledger

| Action | Result | Failure behavior |
|---|---|---|
| Start a task | Open the created thread; attachments are copied into its worktree | Keep draft and upload queue |
| Send/Steer | Clear text and attachments after server confirmation | Preserve input; show the error |
| Upload | List processed file and note; enable Send when every file is ready | Per-file error, retry or remove; other files remain |
| Start preview | Show the current app frame | Inline error and retry |
| Inspect | Highlight box and show bounded descriptor | Keep route and selector for correction |
| Add to message | Switch to Activity and append descriptor to existing draft | Do not submit automatically |
| Stop preview | Stop page and dev server | Keep controls available for retry |
| Restore checkpoint | Confirm affected worktree, checkpoint current state, then restore | Keep dialog and show error |

## Input and async behavior

- Documentation assistance is owned by `components/DocumentationSetup.tsx` inside Setup. Public queries require saved network/provider consent and an installed local model. No frontier fallback. Results are transient component state, never browser/query-cache storage. Errors retain the question for correction. "Cancel documentation work" aborts the client request, signals the daemon's cancellation registry for that operation id and suppresses late results; the status says requests already sent to Context7 cannot be undone. Source links open separately.
- The Context7 key is entered in `components/Context7Credential.tsx`: a password field with no autofill, choosing session-only (default) or OS credential storage when available. The field clears after every submit, including failures, so the key never persists in UI state. The saved value is never returned; the panel shows only its source (session, OS storage or environment). Remove is offered for session/OS keys; environment keys show unset-and-restart guidance. Invalid keys, unavailable native storage and failed saves are inline alerts. Automatic documentation handoff to frontier tasks is implemented (see docs/PHASE-DOCUMENTATION.md).

- English copy, system locale number/date formatting, native file picker and project select.
- IME composition never submits a composer or route field.
- Upload limits: ten files per message, 50 MB per file. Both client and daemon enforce limits. Aborted or failed uploads cannot become sendable silently.
- Attachment storage is content-addressed; prompts reference paths instead of inlining content. Text extraction supports PDF, DOCX, PPTX and XLSX. Unsupported binary formats remain files.
- No preview action runs automatically. Clicking the frame inspects while Inspect is on, and clicks the page while off. Selector controls provide keyboard alternatives. Refresh view captures the existing page; Go navigates/reloads it.
- Preview source metadata can be absent. This is shown explicitly.
- Blob URLs are revoked and outstanding reads/uploads canceled on unmount. Failed mutations preserve inputs. New UI uses inline live regions, not transient-only error toasts.
- Draft text and attachment state remain in memory across thread tabs; full application reload is not a durable draft store.
- Modal dialogs use native dialog focus containment, Escape, accessible titles and focus restoration, with an app-owned body and actions.

## Scope

This contract covers the B2 preview and attachment flows and shared controls they use. Older untouched task/list flows retain their current behavior and are not evidence of completed accessibility coverage.
# Phase C additions

- New tasks default to local intake; users can opt out. Clarification cards are persisted before the coding agent starts. All cards require an explicit answer; recommendations are not submitted automatically.
- Answers and intent specs are stored in the append-only thread log. Inline errors retain answer drafts. A duplicate answer submission is rejected by the daemon.
- Tool approval is an explicit one-time decision tied to the current run. Approvals expire on process exit/restart; the UI never silently replays an allow decision.
- Model setup shows download progress, errors, license links, and measured warm latency. Missing models use clearly identified standard rules/cards. A benchmark that misses its target is shown as such.
- File context is selected through the shared dialog with search, keyboard-accessible results, loading, empty, error and retry states. Up to eight paths are passed to the agent; file contents are not automatically inlined.

# Phase D additions

- Desktop creation defaults to a reviewable coordinated plan. Disabling that option preserves direct-agent creation. The CLI opts in with `--workflow`.
- Plan approval names the step count and submits an exact revision. The daemon rejects stale/duplicate approval. Scope, dependencies, checks, models and budgets remain editable before approval; invalid edits retain the dialog and input.
- Plan map nodes and alternate views are buttons with visible text status. Step details expose the exact stored brief and structured handoff. Timeline and diff reads have loading/error/retry states.
- Pause stops current execution. Scope extensions and budget changes are explicit, bounded actions on the selected step; they never resume a paused plan automatically.
- Integration targets the isolated plan branch. The completed screen offers a combined diff and does not imply a PR was created or changes landed.
- Thread URLs retain selection across reload. Missing task links show a recovery action.
- Verified at 1440 and 900 pixels on Windows. This is desktop coverage, not a claim of mobile or product-wide accessibility compliance.

# Phase E additions

Sources: `docs/PLAN.md`, `crates/arbiterd/src/knowledge.rs`, `crates/arbiter-vault/src/lib.rs`, authenticated preview and vault API routes.

- Screenshot review is the default. Fit/actual size/zoom/pan change only image presentation; website viewport presets resize the actual daemon browser. Phone/tablet presets test CSS viewport dimensions, not mobile UA or touch emulation.
- Saved captures display route, source viewport and local capture date. Saving does not attach pixels to an agent. Captures reopen through the authenticated attachment API with the dev server stopped. Images may be downscaled by the attachment pipeline; actual size uses natural pixel dimensions.
- Refresh indicates pending work and blocks image interaction until a current frame arrives. Failed refreshes identify stale/unavailable imagery and provide Retry. Custom viewport limits have inline validation.
- Narrow navigation is a modal drawer. Plan step selection opens details directly. Dialog bodies scroll within the window; image/table/map overflow remains internal.
- Notes are approved by user Save or proposal Accept. Proposals show current and proposed bodies before acceptance; rejection leaves approved memory untouched. Both save and accept verify the base revision. Acceptance and approved content are one event.
- Closing an edited note asks whether to discard the draft. Failed saves preserve input. Graph/list links share note selection. `@` in the task composer exposes note choices; selecting one adds a wikilink and never sends the message.
- Markdown files are a rebuildable mirror of event-backed notes. Editing mirror files directly does not change approved memory. Rebuild reports success/error. Mirrors are excluded from Git changes by the daemon.
- Memory snippets are bounded to 1000 UTF-8 bytes per injection (a conservative budget below 1k tokens), deduplicated by note revision and content. Plan briefs keep the existing 6000-byte total cap. Guards reject recognizable secrets and instruction patterns; notes remain untrusted reference data even after approval.
- Learning displays reported usage and check outcomes, not estimated real-model savings. Unreported model IDs remain labeled default/unknown. Automatic selection requires three samples, at least half passing, and first-try successes exceeding user-rework count. Explicit harness/model settings win; project preferences can override or disable learning.
- Current outcome rows represent each task's latest recorded check result or plan integration/blocking result. A fork does not add a training sample. Marking user rework changes its score through an appended event.

# Phase F1 additions

- Setup saves explicitly at each step, persists on the daemon and resumes at the saved step. Stale writes return conflict; Reload saved setup replaces the draft only with the fetched state. Loading/error/retry and account-check pending states remain visible. Focus moves to each new step heading. Inputs disable during mutations.
- Account status is reported with its source/time; unknown is never rendered as zero usage. Subscription status and reported dollar diagnostics are separate. Protected mode may block execution while the user still prepares projects/tasks.
- Paid usage requires a separate explicit consent control. Saving setup stops active agents; the summary states this effect. Work is retained. Sign-in links use official sites and the harness manages credentials; no account secrets enter the form.
- Model setup shows size/license/benchmark before selection. Downloads can be cancelled and retried with verified partial-file resume. Closing the dialog does not cancel a daemon-owned download. Benchmarks are required before selecting question models; a slow measured model remains an explicit user choice.
- Composer Options and task Options preserve access to existing features. The shared workspace retains selected tasks and narrow navigation. Setup uses local deterministic questions; no frontier inference runs just to configure the app or query usage.

# Simplification pass (2026-09-24)

- **Default composer.** It shows only the request box, Attach, @ context, Options and Start. The default approach is Automatic, which has three parts:
  - Arbiter assesses the request locally.
  - Medium and large work is planned first, for the user to approve.
  - Small work goes to one agent in an isolated branch, and questions are asked only when the request is ambiguous.

  The chosen approach and its reason are recorded as a notice on the task. Options offers Plan first or One agent, plus the agent, access, tools, checkout and Compare controls.
- **Top bar.** There is one: navigation toggle, Search (palette) and Settings. The connection state appears only while reconnecting. Add project lives in the sidebar. "Inbox" is labelled "Needs you" and "Tasks" is labelled "Board".
- **Task header.** It shows the title, status and Options. Options holds the agent and model, access, tools, usage and budget, Ask aside, project memory, local models, and "End agent process".
- **Composer while running.** It has one Stop, which interrupts the turn (Esc); sending a message continues.
- **Next-step card.** After an agent has finished, Activity proposes one action:
  - "Try again" when checks fail or the agent failed;
  - "Review changes" when new files exist, because new files are always reviewed before commit;
  - "Commit" with the drafted message when only tracked changes exist;
  - "Open Changes" to publish when the branch already has commits.

  A local agent's card also offers hand-off to an installed cloud agent. Nothing consequential happens without a click, and publishing keeps its own approval.
- **Settings.**
  - "Set up automatically" checks the installed coding tools (read-only status), then picks either the subscription, when a signed-in account without environment conflicts exists, or local-only. Paid usage is never enabled.
  - A completed setup reopens as Settings: its pages are tabs, moving between them saves nothing, and "Save changes" keeps setup complete. Before this change, Back or Continue in a finished setup un-completed it and gated the workspace.
- **Composer polish.**
  - Focus highlights the whole composer box; the textarea has no inner ring. The global focus ring sits in the base layer so components can replace it.
  - Attach is a compact toolbar button. Files can be dropped anywhere on the composer, and attachments appear as removable chips.
  - Typing `@` opens an inline list of folders and files: tracked plus untracked, with secret and config paths excluded. Inside a task, project-memory notes are listed too.
  - The list filters as you type. Keys: ↑/↓, Enter or Tab to insert, Esc to close. Mentioned files and folders (up to 8) become context paths. Folders are allowed; the agent reads only what it needs. The "@ Add context" button is gone.
- **Agentic composer (replaces Options).** The new-task composer has no settings panel: only the box, Attach and Start.
  - **Approach.** Small work starts with one agent. Large or risky work is planned. Medium work with no risks adds an "Approach" question card (Plan it first, recommended / Just start) to intake's other cards; the answer decides and is not added to the brief.
  - **`/` commands.** Typing `/` offers `/plan`, `/now`, `/claude`, `/codex` (installed only), `/compare` and `/models`. A choice becomes a removable chip; nothing is shown until typed.
- **Project folder picker.**
  - "Open a project folder" opens a browser of folder names only, with Home, common folders and drives, breadcrumbs, Up, and New folder. Hidden and system folders are excluded, and listings are capped at 500.
  - Choosing a folder starts inspection immediately. Inspection is metadata only and runs nothing.
  - "Start a new project" takes a name and a location, creates the folder, then inspects it.
  - "Type a path instead" remains for power users.

