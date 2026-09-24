# Phase G checkpoint: builder readiness (2026-09-24)

In progress. The plan and its acceptance criteria are in PLAN.md under "Phase G".

## Done

- **G0.** The folder picker hides profile internals (`AppData`, `Application Data`, `Local Settings`, `node_modules`, `__pycache__`, `Recovery`) as well as hidden and system folders.
- **G1. Show me my app.**
  - **Detection.** Covers plain HTML sites, which arbiterd serves itself with no toolchain; Flask (`python -m flask --app … run`); FastAPI (`python -m uvicorn main:app`); and Django (`manage.py runserver`), alongside the existing JS frameworks.
  - **Built-in static server.** It serves 127.0.0.1 only. It refuses hidden files and path segments that are parent, drive or backslash references, caps files at 50 MB, and sends `no-store`.
  - **Automatic start.** When a turn finishes on a web project with a worktree, the daemon starts the app and records `preview_ready { url }` once per address. A failed start becomes a short notice.
  - **Start command.** `GET/PUT /v1/threads/{id}/preview/command` shows the detected and custom commands. The custom one is stored in Arbiter's home, never in the repository; saving it restarts the server.
  - **Preview tab.**
    - **Live:** an embedded, interactive view with the page path, phone, tablet and desktop widths, Reload, and Open in browser (Tauri opener, with a fallback).
    - **Inspect:** the existing screenshot tools, unchanged.
    - **Not running:** a friendly empty state with "Start my app" and the start command.
  - The Tauri security policy now allows embedding localhost and `blob:` images; the second was a latent bug for screenshots.
- **G2. Plain-language progress.**
  - Statuses read Working, Fixing problems, Needs you, Ready for you, Stopped and Done.
  - A sticky "what's happening" line is derived from events, for example "Editing index.html…", "The checks found problems. Fixing them (attempt 1)…" and "Done. All 3 checks passed. Your app is running."
  - Runs of tool steps fold into "N steps" (with a count of problems). "Show steps" is kept per viewer.
  - Other copy changes:
    - "Your app is running" appears as its own row;
    - the next-step card has "See your app";
    - the branch label reads "Own copy";
    - the self-heal row reads "Fixing problems".
- **G3. Templates (built, then removed).** The user found them unnecessary: the first prompt decides what to build. They were replaced by the single "Add a project" step described below.
- **G4. Prerequisites.** `GET /v1/prerequisites` reports Git, Node, Python, Claude Code and Codex with their versions, plus the global Git identity.
  - **Installs.** An install needs `approved: true` and network access. It refuses a tool that is already installed, and it needs Node before installing the agents. It runs one job at a time in the background.
    - Windows uses winget for Git, Node and Python; macOS uses brew.
    - Claude Code and Codex are installed with npm, using their official packages.
  - **Sign-in.** Opens a visible terminal running `claude auth login` or `codex login`.
  - **Git identity.** A validated form writes `user.name` and `user.email`.
  - **UI.** A "Your computer" panel in Settings, under Accounts & usage.
- **G6. Walkthrough.** A six-step spotlight tour over the real controls. It has Next, Back, Skip and arrow keys, respects reduced motion, shows once on first entry to the workspace, and can be replayed with "Take the tour" in the palette. The card goes below the target, above it, or pinned to an edge, whichever fits.

- **Simplification pass, after user feedback.**
  - **Templates removed.** What to build comes from the first prompt. "Add a project" is a single "Choose folder…" (the picker has New folder), handled by `POST /v1/projects/add`:
    - a repository with history is added untouched;
    - an empty folder is set up, saved as a first version and added;
    - only a folder with files but no history asks the user to choose what the first version includes.
  - **Task screen.** The conversation always stays in view. App and Changes open as a side panel (an overlay on narrow windows), and App opens by itself the first time the app starts. Changes has "What changed" and "All files" views.
  - **Settings.** A completed setup is one scrolling page instead of four tabs.
  - **Sign in.** Only shown when the account check reports signed out; otherwise the panel says "Signed in".
  - **Browser address.** The dev UI finds the daemon through a dev-only Vite endpoint that reads `daemon.json` (from `ARBITER_HOME` or `~/.arbiter`). An explicit `?port=&token=` moves into session storage and leaves the address bar at once; a stale stored connection is dropped. The desktop app never used a URL.
  - **Commit history.** "Co-Authored-By" lines were stripped from all history, with a backup at `backup/before-trailer-strip`; the file contents are identical.
- **G5. Terminal and editor.**
  - **Terminal.** `arbiter-supervisor::pty` wraps `portable-pty` (ConPTY on Windows), keeps a 64 KB replay buffer, and strips daemon-held credentials from the shell's environment.
  - **Daemon.** One shell per task (at most 8) on `GET /v1/threads/{id}/terminal` (a WebSocket), plus `…/terminal/close`.
  - **Terminal UI.** A drawer with xterm.js, toggled by the Terminal button or Ctrl+J.
  - **Editor.** `GET /v1/threads/{id}/files` and `GET`/`PUT …/file` read and save in the task's copy only. Saving requires the file to be unchanged since it was opened (409 otherwise) and is refused while an agent works. Secret paths are refused, and files are capped at 1 MB. The UI is CodeMirror 6 with Ctrl+S, plus an Edit link on each changed file.
  - Both components are lazy-loaded, so the main bundle stays under 500 KB.
  - **Browser check.** PowerShell opened in the worktree and ran `git status`; the editor opened `src/cart.js` with syntax highlighting.

- **Plan screen.** The plan is a checklist:
  - **One action at a time:** Approve plan, Pause, or Resume. Edit and "Ask the planner to revise it" sit under "Change the plan".
  - **Plain statuses:** Waiting, Working, Checking, Needs you, Done.
  - **Steps expand inline** with "Open this step's agent" and "What it did". Scope, checks, agent and brief sit under Details.
  - **Extras:** the map, timeline and combined changes are small links instead of tabs.
- **Board.** Hidden from the sidebar until there are six or more tasks; Ctrl+T and the palette still open it.
- **Wording.** Messages that pointed to "Setup" or "Setup & usage" now say "Settings".

- **Project picker.** A project chip above the new-task composer, like Claude Code's, lists projects and "Add a project…", and remembers the last one used. Each project heading in the sidebar has a "+" for a new task in that project.
- **G8. Local web research.** The frontier agent gets only what it needs.
  - **How it works.** The daemon searches (DuckDuckGo HTML), the local model picks at most two results, the daemon reads them (HTTPS public hosts only, 1 MB and 15 s caps, scripts and styles stripped), and the local model writes a cited summary.
  - **Validation.** Sources and quotes must appear in the pages read, using the same rules as documentation briefs.
  - **Where it's available.** `POST /v1/research`, a thread-scoped route, the `web_research` MCP tool, and a `web` action for the local agent.
  - **Frontier tools.** While it's on, Claude's WebSearch/WebFetch and Codex web search are switched off (lean mode), so raw pages never reach frontier context.
  - **Guards.** Off by default (`web_research_enabled`, plus network). Private-looking questions (paths, secrets, code, localhost) are refused before any request.

- **Usage policy, after user feedback.** A signed-in subscription is admitted even when the CLI cannot report allowance or credit status; the provider enforces the plan's limits.
  - **Still refused:** known paid extras (unless paid usage is allowed), a reported allowance of 90% or more used, conflicts, and stale status. Auto-routing moves work to the other agent.
  - **Strict protection** (the old behaviour) is an opt-in setting.
  - **"Your computer"** now shows sign-in, plan, allowance ("2% left this week, resets in 6 days") and any pause reason per agent. The duplicate account cards are gone, and "Check again" re-checks accounts.
- **Composer.** Pasted images are attached. The project is picked automatically (the project you were just working in, or the last one picked) and shown inline as "in <project> ▾" in the toolbar.
- **Local models.** The one best suited to this computer is marked "★ Recommended": Granite 4.1 3B with 12 GB of memory or more, LFM 2.5 otherwise. Potion is marked as needed.

- **G8 follow-up: dynamic and signed-in sites.**
  - **Dynamic pages.** A fetched page with under 400 characters of text is rendered in headless Chromium on a throwaway profile; the rendered text is used only if the final address is still a public HTTPS host.
  - **Signed-in sites.** Settings → "Sites you are signed into". "Sign in to a site" opens a visible Chromium window on `home/web-profile`, where the user signs in themselves. Sites are kept in `home/web-sites.json` (20 maximum). Only those hosts and their subdomains are read with the profile, one browser at a time; a redirect off the site is refused. "Sign out" deletes the site's cookies.
  - **Audit and permission.** See PLAN.md. Routes: `GET/POST /v1/web/sites`, `DELETE /v1/web/sites/{host}`, `GET /v1/research/held`, `POST /v1/research/held/{id}`. Events: `ShareRequested`, `ShareDecided`. At most 16 summaries are held at once.

## Tests

- `preview_auto.rs`: after a turn the app starts by itself, serves the agent's change and assets, refuses traversal and `.git`, and the start command round-trips without touching the repository.
- `quick_add.rs`: repository, empty folder and loose-files cases, plus a missing folder.
- `editor.rs`: listing, saving, stale-save refusal, secret and traversal refusal.
- Supervisor `pty` test: runs a command in ConPTY and keeps scrollback.
- `prerequisites.rs`: tool listing, the approval requirement, no reinstalls, and Git identity written to a temporary global config.
- Crate tests cover the new detections and the folder filters.
- `signed_in_research.rs`: rendering a script-only page, signing in and out, a private summary held and then kept private, a redirect off the site refused, a task waiting on the permission card (Needs you) and receiving the summary after Allow, and a clean summary shared without asking.
- `real_browser.rs` (real Chromium): script-built text read on a kept profile, and a site's cookies deleted.

## Not done yet

- **G7 deploy.** Deferred to a future plan at the user's request.
- **G8 follow-up still open.** webcmd-style replayable site flows.
- **Live verification.** Real winget, npm and brew installs, real sign-ins, and a real template build with an agent have not been run. They need the user's go-ahead because they install software or spend allowance.
