# Arbiter: plan (v2, "ultimate orchestrator")

**Paused by user on 2026-09-24 due to usage exhaustion.** Latest credential setup, local documentation handoff, cancellation, and reviewer changes are saved. Full workspace tests were interrupted during compilation. Resume instructions and exact verification limits are at the top of [PHASE-DOCUMENTATION.md](PHASE-DOCUMENTATION.md). Wait for the user to resume building.

Status: Phases A and B are done. B2 and the Phase D core workflow are implemented and verified with fixtures. Phase C's neural latency target remains open. Phase E core is built; real-provider token-savings acceptance remains open (see [Phase E checkpoint](PHASE-E.md)). The user approved F1 on 2026-09-24. Guided setup, CLI status discovery, conservative cloud admission, resumable model downloads and simplified controls are implemented; see [F1 checkpoint](PHASE-F1.md) for verification and provider/platform limits. The revised F1–F4 sequence supersedes conflicting earlier routing, budget, onboarding and phase priorities.

### B2 implementation checkpoint (2026-09-23)
- Delivered: authenticated desktop preview with manual refresh, persistent browser navigation/click/type/scroll, element descriptors to the composer, browser checks in healing, and bounded MCP browser tools.
- Delivered: composer uploads with retry/remove states, content deduplication, image scaling, PDF extraction, and local DOCX/PPTX/XLSX text extraction. Messages carry paths rather than document bodies.
- Source locations use available React/Vue/Svelte development metadata or explicit source annotations. When metadata is absent the inspector says so; general source-map resolution is not implemented.
- Research versus implementation profiles are now explicit settings. Auto resolves from intake; implementation removes harness web search in lean mode, while research retains it. Old event logs retain the research profile for compatibility.
- Document chunking, vault indexing, local summaries and optional vision captions depend on later vault/model work; they are not supplied by the current attachment pipeline.
- Verification uses real installed Chromium and fake agent launchers: browser console regression through healing, persistent form interaction, inspector-to-composer handoff, authenticated frame delivery, and attachment-path delivery. No paid harness run has been performed for this checkpoint.

### C implementation checkpoint (2026-09-23)

- Delivered: daemon-owned model downloads with pinned checksums, setup UI and CLI, local classification, correction-driven retraining, bounded JSON-constrained question generation, persisted clarification cards, intent specs and project-file mentions.
- Delivered: Claude tool-approval cards with explicit allow/deny, run/request scoping, single-use decisions and expiry on process exit or daemon restart. Codex retains its existing sandbox/deny policy.
- Clarification is enabled for new desktop and CLI tasks. Clear requests skip it; ambiguous requests wait for answers before launching a harness. One extra round is available. Direct API callers retain the existing opt-in behavior.
- Current T0 heads train centroids from bundled seed text at model load, with up to 256 local corrections. Shipping separately trained head weights and learning from execution outcomes remain future improvements.
- T1 currently writes short-answer cards. The schema and UI support single/multiple choice, but the model prompt is restricted to short answers for bounded output.
- Verification: 85 workspace tests, workspace clippy, desktop build, strict UI audit, and browser flows for mentions, clarification, clear-request bypass and both approval decisions. Real Potion warm classification measured 0.78 ms in a development build. No paid coding harness run was used.
- The 5850U neural first-card latency acceptance criterion is not met: Granite generated valid constrained questions but measured 2.98 s to first token / 56.19 s for a full response in a development run with concurrent build/test load. It remains excluded from automatic selection. Standard cards with warm Potion took 250 ms through the API without creating a worktree. LFM still requires user license review before measurement. See [implementation and model setup](PHASE-C.md). Phase D planning/dispatch is not implemented by this checkpoint; the saved intent currently starts one agent.

### D implementation checkpoint (2026-09-23)

- Delivered: editable plan approval, dependency map/list/timeline, isolated parallel node worktrees, persisted briefs and structured handoffs, acceptance healing, scope/budget pauses, conflict repair, combined-diff review, parent/child tasks, and CLI/API controls.
- The three-node parallel/dependency workflow passed with real Git and fake agents. A forced daemon kill mid-plan resumed the same child tasks and worktree and completed all three nodes without duplicates.
- This is the Phase D core milestone. Conservative CPU-based concurrency is implemented; RAM-pressure adaptation, learned routing, exact token accounting and a zoomable graph editor remain follow-ups. No paid provider trial has been run. See [implementation, verification and limitations](PHASE-D.md).
- The overall app remains alpha. Phase C's neural latency target is still open; vault/learning (E), review/landing tools (F), and product-wide release polish remain.

## The flow

```
 Composer ──► Intake (local, fast) ──► Clarify (only if ambiguous) ──► Plan (escalate to frontier)
   "describe      classify · risk ·        1–4 question cards:             task graph: subtasks, scope,
    your issue"   ambiguity score          multi-select / short answer     acceptance checks, deps,
    @mentions                                                              model per node + reason
                                                                                    │
      ┌─────────────────────────── you approve / edit the plan once ◄───────────────┘
      ▼
 Dispatch ──► Execute + self-heal ──► Handoff ──► Integrate ──► Review & land ──► Learn
 brief per     checks → excerpt →      structured    merge into      PR from handoff    outcome stats
 subtask,      bounded fix; drift      note → vault  plan branch;    notes; inline       update model
 worktree off  detection; rate-limit   (no raw       merge-healer    review comments     strength
 plan branch   retry / model failover  transcripts)  on conflict     → fix subtasks      profiles
      │
      └─ visible live in the Agent Map (DAG canvas and tree/timeline) and the Kanban board,
         all linked Obsidian-style through the knowledge vault
```

After you approve, you are pulled back in only for: scope expansion, budget overrun, a merge conflict the healer can't resolve, a question an agent can't answer from the vault, and final review. All of these land in the **Inbox**.

## Decisions (Q&A, 2026-09-23)

| Area | Decision |
|---|---|
| Local intake runtime | Embedded in `arbiterd`, and must feel instant on a Ryzen 7 5850U / 16 GB / Vega 8 |
| Local models | **T0** model2vec `potion-base-32M` plus trained heads: routing, ambiguity and risk in under 5 ms. **T1** question writer and drafter: on first run, Arbiter benchmarks **LFM2.5-1.2B** and **Granite 4.1-3B** (Q4_K_M) and keeps whichever meets the latency target. Granite (Apache 2.0) is the fallback when LFM's license (free under $10M revenue) doesn't fit. **T2** LFM2.5-2.6B on demand. **T3** frontier model through the harness. |
| Runtime details | `llama-cpp-2` with JSON-schema constrained decoding through llguidance. The system prompt's KV cache is kept warm so the first token arrives in under 300 ms. Generation runs on the CPU (6 threads); Vulkan is optional and only speeds up prompt processing. Laya is too slow on CPU (190–460 ms) and Jev is API-only, so neither is used. |
| Clarify | Only when T0's ambiguity score is high; at most 4 questions; you can press "Ask me more" |
| Planner | The local model drafts. Anything bigger than a single small task escalates to a frontier model in read-only plan mode, using your CLI subscription. |
| Model assignment | Editable strength profiles, adjusted by learned outcome stats. You can override per node. |
| Integration | A shared **plan branch**. Each subtask gets a worktree off it and merges back when its checks pass. Dependents start from the updated plan branch. Conflicts go to a merge-healer agent. |
| Handoff | A structured handoff note (files, decisions, APIs, open issues), validated and compressed by the local model. The next agent gets brief + note + diff summary, never raw transcripts. |
| Parallelism | Adaptive. Starts at 3 and scales with CPU/RAM and rate-limit headroom. Queued work shows a typed reason. |
| Agent map | Both views: a live DAG canvas and a tree/timeline. Both are linked to the kanban board. |
| Knowledge | A Markdown vault in the repo at `.arbiter/vault/`, with `[[wikilinks]]`, backlinks and a graph view. It opens in Obsidian too, and agents query it through MCP. |
| Scope control | A crafted brief per subtask plus drift detection. Edits outside the declared scope pause the agent and ask for approval. |
| Gates | You approve the plan once; after that it runs autonomously, except for the exceptions listed above. |
| Harnesses (this phase) | Claude Code and Codex. Others (OpenCode, Gemini, ACP) come after the app is done. |

## Components

### Intake (`arbiter-intake`, new crate)
- **Model manager.** Downloads GGUF and model2vec files with checksums, runs the first-run benchmark, unloads idle models after 5 minutes, and shows progress in the UI.
- **T0 heads.** Classify task type (bugfix / feature / refactor / test / docs / research / ops), size (S/M/L), ambiguity (0–1), risk (touches migrations, auth, secrets, CI) and needs-frontier. Heads ship pre-trained on a seed set, then retrain locally from your decisions and outcomes. Their decisions are logged to the event log.
- **T1 question writer.** Emits `{questions:[{header, question, kind: single|multi|short, options[{label, description}], recommended}]}`, schema-constrained so the output is always valid.
- **Intent spec.** Request + answers + @mentioned context → a compact spec that becomes the planner's input and the root node of the vault note.

### Planner and plan engine (`arbiter-plan`, new crate)
- **Plan graph.** Each node records goal, scope (globs it may edit or read), non-goals, acceptance checks (commands and tests), dependencies, suggested model with reason, and a token and $ budget estimate.
- **Plan storage.** Plans are event-sourced in the same log: `PlanProposed`, `PlanApproved`, `NodeQueued{reason}`, `NodeStarted`, `NodeHealed`, `NodeHandedOff`, `NodeMerged`, `NodeBlocked`. The engine is durable: after a crash it resumes from the log, like bb's cached workflow replay.
- **Scheduler.** Topological order and adaptive concurrency. Rate-limit awareness: when a provider reports a reset time, work waits until then, or fails over to the next model in the strength profile.
- **Integration.** Each plan gets a branch (`arbiter/plan/<key>`). Nodes merge into it when green, and a merge-healer agent handles conflicts. A final integration check runs on the plan branch.
- **Subtasks are tasks.** A plan's nodes appear on the kanban board as sub-tasks of the root task. The kanban and the agent map are two views of the same data.

### Brief compiler (`arbiter-brief`, new crate). Prompt crafting.
- Brief sections, in order:
  - project conventions (a stable prefix, so the prompt cache hits);
  - role;
  - goal;
  - context: vault links resolved to short excerpts;
  - in-scope files;
  - may-read files;
  - non-goals;
  - acceptance checks;
  - constraints (budget, style);
  - the **report contract**: a handoff-note JSON schema delivered through an MCP tool;
  - the escalation rule: if blocked or out of scope, call `request_scope` or `ask_user` rather than guessing.
- Hard budget of about 1.5k tokens per brief, with the variable parts last.
- Each compiled brief is stored so you can see exactly what the agent was told.

### Execute and self-heal (`arbiter-heal`, was Weeks 5–6)
- **Checkpoints.** A git snapshot per turn (`refs/arbiter/cp/...`), a diff per turn, and revert of files or conversation to any turn. Adopted from t3code.
- **Code healing.** Detect and run checks (typecheck, lint, test, build), parse the output per tool, and send the agent a minimal excerpt (≤1.5k tokens, deduplicated). Deterministic fixes run first. The loop is bounded: at most N attempts, and it stops if the same failure signature repeats.
- **Run recovery.**
  - Stall, crash or context overflow: resume or compact.
  - Rate limit: retry timed to the provider's reset time, with jitter, or fail over to another model or account. Adopted from bb.
- **Budgets and loop breakers.** Per-node and per-plan token and $ caps, and repetition detection.
- **Drift detection.** Every diff is checked against the node's scope globs. An out-of-scope edit pauses the agent and sends an approval card to the Inbox.
- **Lean mode.** Launch harnesses without your global plugins, skills and MCP servers (the 96k-token finding), attaching only `arbiter-mcp`.

### Web dev toolkit (promoted: the primary user builds full-stack web apps)
- **Preview panel.** Discover the dev server's port, then open the app in a preview panel with an **element inspector**. Picking an element produces a compact descriptor:
  - CSS selector;
  - component name and source file:line, from React/Vue/Svelte dev metadata or source maps;
  - text;
  - bounding box;
  - roughly 10 relevant computed styles.

  Descriptors attach to the composer or to a review comment, as in t3code's annotations. Screenshots are never sent by default.
- **Browser checks as heal checks.** A headless browser loads the configured routes (through CDP on the Edge already installed on Windows, or Chrome; nothing to download). It collects console errors, uncaught exceptions, failed network requests and a basic accessibility pass. Each one becomes a `Failure` and goes through the same deduplicated ≤1.5k-token excerpt as the other checks, so front-end regressions self-heal too.
- **Agent browser tools (MCP).** `browser_errors`, `dom_query(selector)` (compact text), `a11y_snapshot` (trimmed), `click/type/goto`, and `screenshot(max 768px, JPEG)` only when a node needs pixels.
- **Web search.** Nodes that need research keep the harness's own WebSearch/WebFetch; lean mode removes them from implementation nodes. Fetched pages are summarized by the local model into `vault/docs/`, so repeat lookups read the note rather than the page.

### Attachments and media (token-aware)
- Attachments are stored once under `.arbiter/attachments/`, deduplicated by content hash. Briefs reference paths; content is never inlined.
- **Images.**
  - Downscaled to a 1568px long edge (768px for UI screenshots) and recompressed to WebP/JPEG.
  - Each image goes only to the plan nodes whose brief needs it.
  - Optionally, a small local vision model writes a text caption, so text-only handoffs can describe an image without resending it.
- **Documents.** PDFs and Office docs are converted to text locally, chunked and indexed in the vault. Agents read the relevant chunks through `vault_search`.
- **Large text and logs.** Passed as a file path plus a local summary. The agent reads only the parts it needs with its own tools.

### Agent-facing tools (`arbiter-mcp`)
- `vault_search` and `vault_read`, each with a budget.
- `report_handoff`: the structured note.
- `request_scope`: ask to expand what the agent may edit.
- `ask_user`: a question card that lands in the Inbox.
- `run_checks`: run the node's acceptance checks.
- `repo_map`: symbols and files.

### Knowledge vault (`arbiter-vault`, new crate)
- `.arbiter/vault/` holds `plans/`, `tasks/`, `decisions/`, `handoffs/`, `learnings/` and `files/`, linked with `[[...]]`.
- **Automatic notes.** Arbiter writes these, and never writes secrets.
  - A plan note is created when a plan is approved.
  - A handoff note is written per node.
  - A learning note ("failure signature → fix that worked") comes from each successful heal.
- **Proposed notes.** Agents propose decision notes; you accept or reject them with a diff. Adopted from bb docs.
- **Always-on context.** A memory catalog of at most 1k tokens is added to every brief. Text that looks like a secret or a prompt injection is rejected (bb's guards).
- **UI.** A note view with backlinks, a graph view, and @-mentions of notes in the composer.

### Learning
- An outcome table records task type, model, first-try pass, heal attempts, tokens, cost and user rework.
- Scores are recomputed per model and task type and feed the strength profiles. This is shown in the UI: "Claude handled 8/9 refactors first try".

## Features adopted from t3code and bb (confirmed)

**From t3code**
1. Composer-first drafts (done).
2. Provider/model/effort picker (done).
3. **Checkpoints + per-turn revert.**
4. **Inline review comments on diffs, sent as a batch.**
5. **Queue + steer**, with a typed queue.
6. **Approval and question panels**, which also carry intake's clarifying cards.
7. **Proposed-plan card → approve and implement.**
8. **Best-of-N race mode.** For a risky node, run two models in parallel worktrees, compare, and keep the better one. This also feeds learning.
9. **Settle/snooze Inbox.**
10. **Context and usage meters.**
11. **Command palette + rebindable keys.**
12. **Commit / PR with AI message**, built from handoff notes.
13. **Onboarding checks** that the harnesses are installed and logged in.

**From bb**
1. **Parent/child agents.** Children report turns and blockers to the plan node.
2. **Typed queue + concurrency limits.**
3. **Rate-limit-reset retry + failover.**
4. **Durable workflow replay.** This is how the plan engine works. User-scripted workflows come later.
5. **Memory guards + budgeted catalog.**
6. **Agent-proposed vault edits.**
7. **Side chat.** Ask a side question without derailing the agent.
8. **@mentions and live cards** (`::task`) in messages.
9. **CLI/API parity** with `--json` and structured errors.
10. **Secrets form** that writes to `.env` and never reaches the transcript.
11. **Desktop notifications.**

**Deferred until after the core flow ships:**
- terminal drawer → chat (t3code; the preview browser was promoted to Phase B2);
- automations/cron, user-scripted workflows and the plugin system (bb);
- mobile/remote (Pro tier);
- session import;
- more harnesses.

## Phases (solo + AI agents)

### Phase E first milestone: screenshot viewing and small-window usability

Requested on 2026-09-23. **Approved by the user on 2026-09-23; core implementation delivered. See [Phase E checkpoint](PHASE-E.md) for verification and remaining release gates.** Complete this usability milestone before adding the Phase E vault/learning surfaces. It expands Phase E's scope; the original week estimate is provisional.

**Existing capability:** the agent task's Preview tab already displays authenticated screenshot frames from a persistent browser, with manual refresh, element inspection, click/type/scroll and browser checks. Agent screenshot capture is opt-in. A screenshot is a useful visual review artifact; a real browser remains necessary to render the site and verify interaction.

**Confirmed issues and limitations:**

- At a 640×720 app window, the fixed sidebar occupies approximately 255 pixels, leaving 384 pixels for the workspace. No document-wide horizontal overflow was observed, but the dependency map's right-hand nodes require internal sideways scrolling.
- Step details stack below the map at narrower widths and can fall below the fold. The summary header consumes more vertical space as its content wraps. Technical labels at 11–12 pixels are difficult to scan in a compressed window.
- Website preview rendering is fixed at 1280×800. Its displayed screenshot shrinks with the panel; this is not a responsive-site viewport test. Coordinate mapping and inspection boxes also assume that fixed size.
- There is no dedicated fit/actual-size/zoom/fullscreen image viewer or screenshot card with a reopenable capture. Short-window dialog action visibility needs explicit verification; it is not yet a confirmed clipping defect.

**Approved scope:**

1. Add screenshot review mode: fit to panel, actual size, zoom/pan, fullscreen, manual refresh, and save/open a capture. Show capture time, route and viewport dimensions so a saved image is not mistaken for a live page. Expose captures as reopenable task artifacts, preserving authentication. Send pixels to an agent only on explicit attachment/capture requests.
2. Add desktop/tablet/phone viewport presets and custom dimensions. Set the actual browser viewport; derive image geometry, input coordinates and inspector overlays from returned dimensions rather than hard-coded constants. Keep visual review and interactive checks available.
3. Make navigation collapsible and use an accessible drawer for narrow windows. Preserve selected task and keyboard focus. Compact the plan summary, wrap controls, provide a usable step-list alternative, and make selected-step details readily accessible without a long scroll.
4. Verify editor and approval dialogs at short heights, keep primary actions reachable, and improve cramped labels/control spacing. Contain long paths/code and graph scrolling within their panels.

**Acceptance gate:** verify the app at 1440×900, 1280×720, 1024×768, 768×720 and 640×720, plus 200% browser zoom. No document-wide horizontal overflow; primary actions and modal controls remain reachable by keyboard; explicit overflow is confined to map/code/image surfaces. Verify website captures at real 1280×800, 768×1024 and 390×844 browser viewports, including accurate inspect/click mapping after resize and zoom. Cover loading, stale/error/retry and capture reopening. Narrow-window support is distinct from a complete mobile/remote product.

Audit evidence: `target/phase-d/small-screen-audit.png`, the current `Preview.tsx` and `arbiter-browser/src/session.rs` implementation. These are observations from the existing build; implementation and verification progress is tracked in the Phase E checkpoint.

| Phase | Weeks | Scope | Done when |
|---|---|---|---|
| A ✅ | 1–4 (+UX) | core, adapters, tasks, composer, picker, routing | done |
| B ✅ | 5–6 | checkpoints, heal loop, run recovery, budgets, lean mode, Inbox (approval panels → C) | a failing-test fixture goes green with no human input; rate-limit and crash chaos tests recover |
| B2 | 7 | Web dev toolkit: preview panel + element inspector, browser checks in the heal loop, agent browser MCP tools, lean-mode tool profiles (web search kept for research nodes), attachments pipeline (dedupe, downscale, doc-to-text) | a console error introduced by an agent is caught by browser checks and healed; an inspected element reaches the agent as under 300 tokens of text |
| C | 8–9 | model manager + benchmark, T0 heads, T1 question cards, tool-approval panels (Claude `can_use_tool` host prompts), intent spec, @mentions | an ambiguous request gets ≤4 useful cards within 300 ms to first card on the 5850U; clear requests skip Q&A |
| D | 10–12 | planner escalation, plan graph + approval card, durable engine, plan branch + merge-healer, brief compiler, drift detection, adaptive concurrency, handoff notes, agent map (canvas + tree/timeline), plan ↔ kanban | a 3-node plan with one dependency runs end to end in parallel; killing the daemon mid-plan resumes it |
| E — core built; real-provider acceptance open | 13–14, re-estimate expanded scope | screenshot viewer + responsive-window milestone first; then vault + wikilinks + graph, MCP tools, memory catalog + guards, learning loop | screenshot/responsive acceptance gate above passes; the second run of a similar task uses fewer tokens, thanks to vault hits and learned routing (measured) |
| F1 — core implemented; provider/platform acceptance open | re-estimate | guided first-run setup, subscription-aware usage controls, simpler default workspace | fresh installation reaches a verified ready state; disallowed cloud/billing paths cannot launch; interrupted setup resumes; see PHASE-F1.md |
| F2 — proposed | re-estimate | safe project adoption, minimal agent scaffolding, local-first baseline | empty and existing folders become ready without losing user instructions or work; migration is reviewable and reversible |
| F3 — core implemented; real-model acceptance open | re-estimate | local coding executor, capability-based local/frontier routing, complete feature delivery | local-only fixture and authorized hybrid feature both reach checked changes and preview; quota exhaustion preserves progress |
| F4 — core implemented; live publish/provider acceptance open | re-estimate | inline review comments, commit/PR, side chat, command palette, notifications; explicit opt-in best-of-N | review comments become fix nodes that land; optional parallel experiments respect usage policy |

## Local-first product revision (2026-09-24; F1 approved)

The intended product is an end-to-end local-first coding orchestrator. Its normal journey is: guided setup → add/adopt a project → establish a baseline → describe a feature → clarify only what matters → execute/check/repair → preview/review → retain useful local knowledge. Advanced controls remain available through progressive disclosure. Completing backend subsystems alone does not satisfy this product goal.

### Budget reality and usage contract

- The preview's `$5.00` is the fake checkout plan's `budget_usd: 5.0` in `crates/arbiterd/examples/phase_d_preview.rs`. It is a configured threshold against accumulated adapter-reported cost, not subscription credit, a purchase, or a provider-enforced billing limit. Generated plans currently have no default USD cap.
- Claude's adapter accumulates reported `total_cost_usd` deltas. Codex currently reports `cost_usd: 0.0` because its adapter receives no price. Zero here does not establish zero consumption or unlimited access. Existing cost/turn gates are reactive; in-flight work and delayed reporting can overshoot thresholds.
- Separate local execution, included subscription/free allowance, optional provider credits/extra usage, and metered API billing. Record the effective authentication/billing source before dispatch, not merely the selected harness name. Preserve old events with additive optional/defaulted fields; never reinterpret missing telemetry as a known zero.
- Default to local-first with paid API/extra-credit use disabled. Never silently switch from subscription authentication to API keys, bought credits, a different account, or paid fallback. Inspect inherited environment and harness configuration without exposing secrets. If billing source or provider-side extra-usage behavior cannot be verified/controlled, block cloud launch in strict no-extra-spend mode and explain how to resolve it.
- Free/limited-plan support means using officially available harness entitlements and local fallback. A free web-chat account does not automatically grant a coding harness or API access. Detect capabilities and refresh stale status; do not hard-code universal model availability or quotas.
- Show subscription windows, remaining allowance and reset time only when supported telemetry is available, with source/freshness. Otherwise show unknown and use conservative launch/concurrency limits. Keep reported/estimated/unknown tokens, cache/reasoning usage and monetary values distinct. Never convert token counts into invented subscription percentages.
- User-authorized refinement (2026-09-24): obtain usage through installed CLIs when needed. In F1, prefer supported structured CLI/app-server status APIs; use documented read-only CLI status commands where necessary. Capability-detect per harness/version and normalize account scope, windows, reset timestamps, source and observation time. Do not launch an inference turn just to query usage. Bound refresh frequency, deduplicate concurrent queries and back off on failure; stale, unsupported or unparsable output becomes unknown. Fixture-test version differences, partial responses and shared-account limits; keep credentials and raw account output out of transcripts.
- Enforce policy at every daemon launch path: planning, implementation, repair, resume, retry, failover, side chat, benchmarks and background work. Reserve local admission capacity atomically for parallel launches, deduplicate dispatch on replay, bound retries, cancel owned processes, and reconcile completed usage. Provider limits remain authoritative; shared-account usage elsewhere may be invisible.
- Define the user's “zero usage leakage” target as zero unauthorized cloud launches or paid fallback, plus measurable reduction of duplicate work/context. Do not promise zero wasted tokens, exact external billing caps, or recovery of quota already consumed. A strict offline mode must restrict network-capable tools/child processes as well as model routing; label any platform enforcement gap before claiming offline isolation.
- Efficiency defaults: deterministic checks first, bounded cached repository summaries, selective retrieval, lean tool profiles, incremental handoffs, one capable agent before parallel experiments, and no background frontier polling/inference. Pause until reset or use a suitable local model on exhaustion; do not evade provider limits.

### F1: guided setup and a simpler workspace — approved implementation

1. Persist a resumable daemon-owned onboarding state machine. Ask a short adaptive Q&A about intended work, existing accounts, hardware/storage, privacy and autonomy preferences. Use deterministic cards before local models are installed; setup must not require a frontier call.
2. Detect installed supported harnesses, runtime versions, authentication and available capabilities. Offer official installation/sign-in flows with visible progress, actionable failure recovery and cancellation. Credentials stay in supported secure stores and never enter agent context or logs.
3. Reuse the model manager to recommend a hardware-fitting local intake/planning model. Explain purpose, download size, license and resource requirements before installation. Verify artifacts; support interrupted downloads and offline/manual setup. Benchmark locally before enabling a model; do not automatically download larger models because a prompt asks for “smarter” orchestration.
4. Add the usage/authentication contract above, replacing misleading subscription dollar meters with allowance/status views. Paid mode remains an explicit opt-in. Keep any reported API-equivalent value in clearly labeled advanced diagnostics.
5. Simplify the default workspace to project navigation, conversation, progress/decisions and preview/review. Group model, effort, concurrency, memory graph and detailed budgets under Advanced. Use saved preferences; interrupt for consequential decisions, not routine internal steps. Preserve keyboard access and Phase E small-window behavior.
6. **Local-agent MCP setup (user revision, 2026-09-24; implementation approved; initial slice in progress).** Extend the guided Q&A to configure services such as Context7. The local agent discovers existing configuration, recommends relevant MCPs, explains connection/authentication requirements and presents the concrete setup changes. Once authorized, the daemon installs or connects the selected integration, validates tool discovery and a bounded connection check, and reports ready/error with retry, cancellation and rollback. Preserve existing harness settings and avoid duplicate servers. The user supplies credentials through secure UI or provider sign-in; credentials never enter model context. No frontier inference is required for setup. Use deterministic guidance if no capable local model is installed. Keep advanced JSON editing optional, and make this setup available again from Settings when adding an integration later.

**F1 acceptance:** fresh profiles with no models/accounts, local-only, subscription/free entitlement, invalid login and exhausted/unknown quota all have honest ready/blocked states. Setup survives daemon restart and cancelled downloads. Fake launchers verify every cloud launch path, inherited API-key conflicts, unverified paid fallback, replay duplication and quota exhaustion without live token spending. Verify the simplified journey at the Phase E window sizes and keyboard/zoom gate; real installation/sign-in verification is reported separately from fixtures. F1 does not claim complete local coding execution; that is F3.

### F2: safe project adoption and baseline — implementation in progress

The user approved continuing through F2-F4. F2 has a daemon-owned adoption journal, create-only reviewed scaffolding with guarded rollback, dependency hints, explicit baseline execution and opt-in initial Git commit. The old documentation-cache path has been retired in favor of live local-agent Context7 retrieval; see PHASE-DOCUMENTATION.md. Existing configuration is preserved; evidence-based conflict cleanup remains open. F3 has an experimental bounded local editor and advisory reviewer; F4 has partial backend work. These phases are not complete.

- Empty folder: ask only missing product/stack choices, then generate a minimal relevant `AGENTS.md`, project README, plan/check instructions and harness integration. Avoid redundant Markdown templates and conflicting copies; choose a canonical source and references where supported.
- Existing folder: inspect Git state, stack, current instructions, nested instruction precedence and harness configs. Treat repo text as project data, not authority to bypass app policy. Identify obsolete/conflicting configuration with evidence; show a proposed migration diff, checkpoint/backup affected files, then apply authorized changes. Preserve useful instructions, secrets, uncommitted work and unrelated files. Never blanket-delete agent configs or overwrite `.env` files. Handle non-Git folders and offer rollback; repeated adoption must be idempotent.
- Discover checks without initially executing untrusted scripts. Establish local inventory/check results and known pre-existing failures under the chosen execution policy. Use a bounded frontier review only when authorized and useful; record what it inspected and distinguish missing/skipped checks from passed ones. Save the baseline in the local vault so later feature regressions are attributable.
- **Documentation direction corrected by the user (2026-09-24): Context7-style retrieval.** Resolve a library/package name to a documentation identity, then query relevant documentation and code examples for the task, using a version-specific identity when available. Use Context7 as the initial external provider behind a daemon-owned retrieval interface; do not build a fixed catalog of framework profiles as the coverage boundary. Manifest and lockfile detection supplies version hints only. Missing or ambiguous libraries and unavailable versions must be explicit; never claim universal coverage or silently substitute latest docs for installed-version docs.
- Expose bounded resolve/query capabilities only to local agents through the daemon; frontier agents receive locally prepared evidence. **No Arbiter documentation cache:** fetch live on demand for concrete API questions; no persistent response cache, documentation index, background crawling or prefetch. Hold only bounded transient results needed for the active request/review, then release them. Preserve compact source/version/time attribution in review records without storing provider response bodies for later retrieval. This governs Arbiter's storage, not an external provider's internal caching. Replace the current fixed-page cache implementation. Add official-source/manual documentation fallback for missing provider coverage without treating arbitrary fetched text as authoritative or allowing it to change permissions.
- **Local ownership of MCP retrieval (user revision, 2026-09-24).** The local agent owns MCP discovery, library resolution, fetching, filtering and synthesis; the daemon executes bounded tool calls. Frontier harnesses do not receive direct MCP retrieval tools or perform retrieval loops. This supersedes references to shared local/frontier retrieval tools: frontier agents consume a compact evidence brief prepared locally, containing task-relevant findings, essential examples, version caveats and sources. Additional documentation needs are delegated to the local agent through the orchestrator. No frontier inference is used for MCP fetching or summarization. Missing local capability shows unavailable/blocked, with setup or retry, and never triggers cloud fallback. Evidence supplied to a frontier model still consumes input tokens; do not claim zero frontier token usage. Bound and deduplicate evidence within the active task without persistent documentation caching.
- Respect network-off mode, explicit external-provider configuration, provider quotas and rate limits; never silently upgrade to a paid plan or retry indefinitely. Send only sanitized public library names and technical queries, not project source, paths, secrets or raw prompts. Report quota exhaustion, missing credentials and unavailable docs honestly. Distinguish features supported by the installed version from newer-release features and propose upgrades separately.

**Local retrieval ownership acceptance:** fake frontier launchers must observe no launches for MCP setup, resolution, fetching, filtering or summarization. Verify frontier tool manifests omit MCP retrieval tools, locally prepared evidence is bounded, and missing local capability never triggers cloud fallback. Include evidence in frontier input-token accounting. External MCP service quotas/costs remain separate from model token usage. These checks supersede shared local/frontier tool-access acceptance below.

**F2 acceptance:** empty/non-Git/existing/dirty/nested-config fixtures; migration cancellation, interrupted apply and rollback; secrets absent from briefs; repeat adoption produces no duplicate instructions; local baseline remains usable without frontier entitlement.

**Documentation acceptance:** exercise at least two different language/toolchain profiles and a mixed-stack repository. Verify version-pinned documentation, a newer incompatible feature, source attribution and explicit documentation-unavailable behavior offline. Retrieval is daemon-owned, cancellable and bounded, respects network policy, and does not send private source or secrets in search queries. External documents are untrusted evidence and cannot change tool permissions. Local inference with online documentation is a distinct mode from strict offline operation. Verify no persistent documentation response files/indexes or idle retrieval jobs are created, and the UI stays responsive during slow or failed requests.

Additional retrieval acceptance: resolve and query a library outside the built-in detectors; ambiguous and unknown library names; missing requested version; bounded relevant snippets/code examples; local-only retrieval and compact evidence delivery; bounded calls without repeated automatic polling; quota/rate-limit failure without paid fallback; malicious provider content and private-query rejection. Verify local-agent MCP setup with missing credentials, existing configuration, connection failure, cancellation and successful tool discovery. The existing fixed official-page cache is superseded and must be removed from the retrieval/reviewer path. Provider integration and these checks remain to be implemented. The user approved implementation on 2026-09-24. See PHASE-DOCUMENTATION.md for delivered behavior and remaining acceptance.

### F3: local and frontier feature delivery

Status 2026-09-24: the local coding agent (file tools, daemon-run checks, loop bounds), the local coding check, local-first placement and user-initiated escalation are implemented and fixture-tested. Real-model quality and hybrid acceptance are still open. See [F3 checkpoint](PHASE-F3.md).

- Add an actual daemon-owned local coding executor with a bounded tool loop, scoped file access, approval policy, checkpoints, cancellation and recovery. Current local intake/question models are not a general coding harness.
- Route by measured task capability, hardware fit, privacy policy and available allowance. Local models handle suitable planning, summarization, edits and checks; frontier models handle justified escalations with a compact brief. A failed local capability test pauses or proposes escalation rather than claiming success.
- Add a local documentation-backed reviewer as a standing second perspective at planning, consequential implementation choices and final review. Give it the requirements, constraints, relevant versioned sources and check results; have it assess these independently before seeing the implementation agent's conclusion to reduce anchoring. Then compare recommendations, supporting evidence, trade-offs and missing checks. The reviewer can request targeted fresh documentation through the F2 retrieval service, including for newly released features. Measure reviewer capability separately from coding/intake; recommend an additional hardware-fitting model only when needed and with download consent.
- Keep debate bounded and useful: one independent assessment and at most one reconciliation round by default, triggered by milestones or changed evidence rather than every token/edit. Surface a compact “implementation view / reviewer view / evidence / recommendation” card for material disagreements and a collapsed review summary otherwise. Agreement is allowed; never manufacture opposition or describe two models as unbiased. Resolve factual disputes with official versioned sources and executable checks; ask the user when product preferences or unresolved material trade-offs decide the outcome. Preserve the chosen rationale locally. If no capable reviewer is available, show review unavailable/degraded rather than simulating a second opinion; frontier review remains subject to the existing usage policy.
- Complete the feature journey across planning, execution, test/build/browser checks, bounded repair, integrated diff, screenshot/interactive preview, final review and local learning. Preserve progress when offline or quota-limited. Never require parallel frontier agents or automatic best-of-N for the default path.

**F3 acceptance:** complete a bounded feature entirely locally with cloud launches blocked; complete an authorized hybrid feature with accountable handoffs; interrupt/restart and exhaust quota mid-run without losing changes or duplicating calls. Report actual quality, latency, tokens and telemetry gaps. Compare similar tasks against a baseline before claiming savings. Real provider trials require an explicit allowance and remain separate from deterministic test gates.

**Dual-perspective acceptance:** fixtures cover an outdated API recommendation corrected by current official docs, a latest feature incompatible with the project's installed version, a legitimate trade-off, reviewer agreement, unsupported claims, malicious document instructions and unavailable/offline sources. Verify citations support the recommendation, independent assessment precedes reconciliation, debate stops at its bound, and repeated review reuses unchanged evidence. Record review latency/resource usage and whether it catches seeded mistakes without claiming elimination of bias or guaranteed access to the latest information.

### UX simplification revision (user request, 2026-09-24)

The user reported too many visible controls and asked for a more agentic, automated experience. From this revision on, the default path works like this:
- Arbiter decides the approach (plan or one agent) and whether to clarify.
- It proposes the single next step after each agent finishes.
- It offers one-click automatic setup.

Everything else sits behind Options. Consequential actions (commit, hand-off to the cloud, publish, paid usage) still require an explicit click or approval. Details are in UX-CONTRACT.md under "Simplification pass".

### Phase G: builder readiness (user request, 2026-09-24)

Status: G0–G6 are implemented. Templates were dropped in favour of a single "Add a project" step. G8 local web research is implemented, including dynamic and signed-in sites; the webcmd-style replayable flows are not started. G7 is deferred. See [Phase G checkpoint](PHASE-G.md).

**Goal.** A power developer, a novice and a non-technical founder can each go from idea to a running app, all inside Arbiter. Workstreams are listed in the order they will be built.

- **G0. Small fixes.**
  - The folder picker hides `AppData` and other profile-internal folders.
  - Plan and Board screens get the simplification pass.
- **G1. "Show me my app."**
  - When a task on a web project finishes, Arbiter starts the dev server itself and opens a live, interactive view of the app. The screenshot mode stays for inspection.
  - The dev command can be edited in the UI, not only in `.arbiter/healing.toml`.
  - Detection extends beyond JavaScript: static HTML, Python (Flask, FastAPI, Django) and generic `dev` scripts.
  - The custom browser workflow (inspector, browser checks, agent browser tools) keeps working on the same server.
- **G2. Plain-language progress.**
  - Activity leads with outcome sentences built deterministically from events, for example "Working on your pricing page…", "Checks passed: 3 of 3" and "Ready for you to look at". The raw event rows stay one click away.
  - UI copy avoids jargon by default: worktree becomes "its own copy", baseline becomes "starting checks", heal becomes "fixing problems".
  - The Changes view gets a plain summary: files changed, and what the agent says it did.
- **G3. Templates and a guided new project.**
  - "Start a new project" offers starter templates: blank web app, landing page, booking site, online store, dashboard and API service.
  - Each template is a crafted brief plus the scaffolding the agent builds, using the existing plan flow. Starter file skeletons are written only where they are deterministic.
- **G4. Prerequisites handled in-app.**
  - Detect Git, Node, Claude Code and Codex, with versions.
  - On Windows, offer one-click installs through `winget` for official packages, each gated on explicit consent. Sign-in runs in the built-in terminal (G5). Where installation cannot be automated, show clear manual steps.
  - Git setup (user name and email) happens through a form.
- **G5. Terminal and editor, like t3code.**
  - A terminal drawer per task, backed by a daemon-owned PTY (ConPTY on Windows) and xterm.js, bounded and killed with the task.
  - A lightweight file editor (CodeMirror) for quick edits in the task's worktree, with "open in your editor".
  - This needs new dependencies (`portable-pty`, `@xterm/*`, `@codemirror/*`), which require the user's approval to download.
- **G6. Walkthrough.**
  - A smooth first-run tour of a few coach-mark steps: describe, `@` and `/`, Needs you, Changes and Preview, then Commit.
  - It can be skipped or replayed from the palette, and its state is kept per viewer.
- **G7. Deploy.**
  - For supported stacks, deploy a preview through an installed and signed-in provider CLI (Vercel, Netlify), with explicit consent. It returns the URL.
  - It is never automatic, and it is outward-facing, so each deploy is approved individually.
- **G8. Web search and fetch.**
  - Agents keep the harnesses' built-in web search and fetch for research steps.
  - Fetched references are summarized into project memory by the local model, so repeat lookups read the note (planned in the web-dev toolkit section; not built yet).

- **G7 status: deferred.** The user asked for deploy to stay a future plan. It means publishing the user's own app to a URL; it does not change Arbiter, which stays a desktop app.
- **G8 follow-up: dynamic and signed-in sites.**
  - **Dynamic pages.** When a fetched page yields little text (a JavaScript app), render it in a headless browser using a fresh, throwaway profile, then read `document.body.innerText`. The same bounds and validation apply.
  - **Signed-in sites.** Opt in per site. "Sign in to a site" opens a visible browser window on a dedicated Arbiter profile, kept in Arbiter's home and separate from the user's own browser, where the user signs in themselves. Arbiter never sees or stores passwords.
  - **Reading signed-in sites.** Research reuses that profile only for allow-listed hosts; every other site gets a clean profile.
  - **What reaches agents (decided).** Each summary of a signed-in page is audited on this computer: deterministic checks (email addresses, long numbers, key-shaped strings) plus a local-model verdict, where any doubt counts as private. A clean summary is shared as usual. A flagged one is held: the task shows a "Share what was found on <site>?" card (Allow sharing / Keep private), the thread moves to Needs you, and the agent waits up to 10 minutes; no answer means private. Sign-out clears that site's cookies.
  - **Status: implemented** (except the replayable flows below). Agents can pass a `url` to `web_research` to read one page directly.
  - **Repeated site tasks** (webcmd-style): successful action sequences on a site or on the user's own app are saved as named, replayable flows. Later runs replay them without a model and only escalate on failure.

**Acceptance.**
- A non-technical path passes on a fresh Windows profile: install prerequisites, choose a template, get a running app preview, commit, and deploy a preview. Real installs, sign-ins and deploys are reported separately from fixture tests.
- The novice and power-user paths lose no existing capability.

### Multi-project work (implemented)

- **Why.** One feature often spans repositories, such as a data pipeline and the app that reads its output.
- **Composer.** Attach several projects with @ or "+". The first is the main project. More than one always plans the work first.
- **Plan model.** `Plan.projects` lists the other projects (at most 4, all attached to the task). A step's `project` names the repository it changes; scope and checks are relative to it. Dependencies can cross projects.
- **Execution.** Each project gets its own collection branch `arbiter/plan/<id>`. A step branches from its project's collection branch, merges back into it, and its checks run there; final checks run per project.
- **Handover.** A dependent step gets the earlier step's handoff (summary, files, interfaces) in its brief. It can also read the other project's collection worktree (`--add-dir` for Claude; Codex can already read). A collection worktree changed outside a merge pauses the plan.
- **Landing.** Review, commit and publish per project from the plan's "Review and save changes" view.
- **Tests.** `multi_project.rs`: a pipeline step and a dependent app step, each in its own repo, with the interface handed over, and landing and diff per project.

### Publishing and clean-up (implemented)

- **Arbiter's branches stay local.** `arbiter/*` branches, worktrees and `refs/arbiter/*` are Arbiter's scratch space. They are never pushed, and none gets an upstream.
- **One commit.** Publishing squashes the task's committed work, in a temporary detached worktree, into one commit on a normal branch. The commit is authored with the user's Git identity. A later publish of the same task adds one follow-up commit (the diff since the last publish), never a force-push. Untracked files are never included.
- **Message.** The local model writes a Conventional Commits message (`type(scope): subject` plus a short body) from the task's commits, handoffs and diff stat. Any co-author or AI attribution is stripped. When no local model is available, a deterministic message is built from the handoffs. The user can edit it before publishing.
- **Modes.**
  - `pr`: push the branch and open a draft PR. This is the default for GitHub with `gh`.
  - `push`: push the branch only. This is the default for other remotes.
  - `local`: create a local branch; the user pushes. This is the default without a remote.
  - Base defaults to `origin/HEAD`, else `main` or `master`. Branch names use `feature/<slug>`.
  - The last choice is remembered per project in `home/publish/<project>.json`, so nothing is set up twice.
- **Clean-up.** A sweep runs every 10 minutes; `POST /v1/cleanup` runs it now. It removes a task's worktrees, `arbiter/*` branches and refs once the published work is settled:
  - the PR is merged or closed;
  - the remote branch is deleted;
  - or the local branch is merged or deleted.

  It skips tasks with work newer than their last publish. The task is then marked done. `POST /v1/threads/{id}/cleanup` cleans up one task on request.
- **Tests.** `publish_squash.rs` uses a bare local repository as the remote and covers one commit authored as the user, the co-author strip, no `arbiter/*` refs on the remote, a follow-up commit, remembered settings, and clean-up after the branch is gone.

### Memory repository (implemented)

- **Rule.** Nothing Arbiter keeps goes into a project repository, so teammates' workflows are never affected. Everything lives in one separate Git repository, `<home>/memory`. It is created automatically on first start; no setup step.
- **Contents.**
  - `README.md`: what the folder holds and how to restore it.
  - `guidance/about-me.md`: how you like to work. Its first 1200 characters are given to agents once per task, and again after an edit.
  - `projects/<name>-<id>/notes/<category>/<slug>.md`: project notes. These previously went to `<repo>/.arbiter/vault/`.
  - `projects/<name>-<id>/instructions.md`: your additions to the team's `AGENTS.md`, given once per task.
  - `projects/<name>-<id>/healing.toml`: your check overrides. They win over a team-committed `.arbiter/healing.toml`, which is read and never written.
  - `backup/events/<yyyy-mm>.jsonl`: the append-only event log exported incrementally as text, since projections are rebuildable.
  - `backup/tables.json`: projects, tasks and task links.
  - `backup/preferences.json` and `backup/settings/`: publish, preview and signed-in site lists (no cookies).
  - Secrets are redacted before anything is written.
  - The raw SQLite file is not committed: git would store a full copy on every change.
- **Projects stay clean.**
  - Attachments are referenced in Arbiter's store by absolute path, and agents get read access. They are not copied into the checkout.
  - Project setup no longer creates `.arbiter/PROJECT.md`.
  - Publishing leaves out new `.arbiter/` files unless the base already tracks them, and says so.
  - On start, older untracked `.arbiter/vault/` notes are moved into memory and `.arbiter/attachments/` copies are deleted.
  - Committed `.arbiter` files, such as an old `PROJECT.md`, are only reported in Settings.
- **Remote.** Settings → "Your memory" offers "Create a private GitHub repository" (`gh repo create --private`) or any git URL. Pushes go to `main`.
- **When it saves.**
  - It commits locally every 10 minutes.
  - Closing the desktop window asks "Save your memory before closing?" when there are unpushed changes. A second close within 5 seconds always closes.
  - After an unclean stop (crash or flat battery), the next start pushes first. After a normal close where you chose not to push, it does not.
- **Restore.** Clone the memory repository, then run `arbiterd --restore <folder>` into an empty home. This restores events (with original ids and times), tables and settings, and rebuilds projections. That memory folder then becomes the new home's memory.
- **Tests.**
  - `memory_backup.rs`: automatic creation, redaction, guidance once per task, pushing, the push after a crash but not after a normal close, migration of old leftovers, reporting of committed files, and restore.
  - `publish_squash.rs`: a new `.arbiter` file stays out of the published commit.

### Phase R: distribution and updates (plan; start when the product is ready to share)

- **Packaging.**
  - A GitHub Actions release workflow (tauri-action) builds for Windows (MSI and NSIS), macOS (a universal `.dmg`) and Linux (AppImage and `.deb`).
  - `arbiterd` and `arbiter-mcp` ship as Tauri sidecars, so one installer contains everything.
  - Local models are not bundled; they download on demand with checksums, as they do today.
- **Signing.**
  - **Windows:** Authenticode through Azure Trusted Signing or an OV/EV certificate, so SmartScreen trusts the installer.
  - **macOS:** Developer ID signing plus notarization.
  - **Linux:** GPG-signed packages.
  - Signing keys live only in CI secrets.
- **Updates.** The Tauri updater plugin checks a signed `latest.json` (an Ed25519 update key, separate from code signing) published with each GitHub Release.
  - **Channels:** stable and beta.
  - **When it checks:** at start and daily. The download happens in the background.
  - **Installing:** only after the user agrees ("Restart to update").
  - **Before installing:** the daemon stops cleanly first. It finishes or pauses running agents, and their work stays in worktrees.
- **Data safety.**
  - The event log is append-only and additive, so old logs keep working.
  - Projection tables are rebuilt with `rebuild_projections` if a schema changes.
  - Settings migrate forward only, with `serde` defaults.
  - A failed update leaves the previous version runnable. The last installer is kept for rollback.
- **Getting the app.** A download page, plus winget, Homebrew and Scoop manifests.
- **Feedback.** Crash reports are opt-in only, and version and channel are shown in Settings.
- **Acceptance.** A clean install on a fresh Windows profile, updating 1.0 to 1.1 with an agent task mid-way (work preserved), a rollback, and signature verification of a tampered update (refused).
- **Built so far (R0, a tester build).**
  - A per-user NSIS installer (no admin rights) with `arbiterd` and `arbiter-mcp` as sidecars. It is unsigned, so SmartScreen warns.
  - Version handshake. `/v1/health` reports the daemon version. A newer app asks an older daemon to stop (`POST /v1/shutdown`, refused while an agent works), then starts its own. This fixes the "stale background service" 404 seen during development.
  - The installer hook stops a running daemon before replacing files, and on uninstall. The memory keeper treats that as an unclean stop and pushes on the next start.
  - The UI reports an outdated background service in plain words.
- **Rollout order.**
  1. R0 tester builds by hand, for you. **Built.**
  2. R1: CI and releases. **Built.**
     - Every push to `main` runs the tests on three OSes and builds an unsigned Windows installer artifact (`ci.yml`).
     - Publishing a GitHub Release `vX.Y.Z` sets the version from the tag and builds the installer. It signs the update with the Ed25519 key held in `TAURI_SIGNING_PRIVATE_KEY`/`_PASSWORD` secrets, and attaches the installer, `.sig` and `latest.json` (`release.yml`).
     - A beta channel is still to come.
  3. R2: the in-app updater (`tauri-plugin-updater`). **Built.**
     - It checks `releases/latest/download/latest.json` at start and every 6 hours.
     - A banner sits at the top of the window.
     - With "Update Arbiter automatically" (the default), the update downloads in the background and installs when the window closes, or at once with "Restart now".
     - Without it, the banner offers "Update now": download, install and relaunch.
     - Before installing, it pushes memory and stops the daemon, waiting for agents unless you choose "Update anyway". Updates whose signature does not match the built-in public key are refused.
  4. R3: code signing (Azure Trusted Signing for Windows), then macOS and Linux builds.
  5. R4: winget, Homebrew and Scoop manifests, a download page, and opt-in crash reports.
- **Compatibility rules for every release.**
  - The UI and daemon ship together. The daemon keeps old routes working for one minor version.
  - `EventKind` stays additive. Store migrations only move forward, and memory is pushed before a migration runs.
  - A release that changes the backup format also bumps the restore reader, so older backups always restore.

### Release completion and retained work

Status 2026-09-24: diff review comments, landing draft and selective commit, the command palette with rebindable shortcuts, notifications, local side questions and opt-in best-of-N are implemented. See [F4 checkpoint](PHASE-F4.md). F4 retains the former Phase F review/landing and optional power-user tools. Release readiness also requires installer/update/uninstall and recovery coverage, accessibility and native zoom verification, clear empty/error/offline states, responsive layouts, and real supported-harness acceptance. Phase C latency and Phase E real-provider savings gates remain open; this revision does not mark them complete.

Reference check (2026-09-24): [OpenAI plan pricing](https://learn.chatgpt.com/docs/pricing), [Claude Code costs](https://code.claude.com/docs/en/costs), and [Claude subscription authentication](https://support.claude.com/en/articles/11145838-use-claude-code-with-your-pro-or-max-plan). Reverify provider entitlement/authentication behavior during implementation; provider policies can change.

## Verification
- **Unit tests for every new crate.** These include fixture replays for the planner and brief compiler, with golden briefs checked for exact token budgets.
- **Intake latency benchmark in CI**, with a fixed prompt set, plus a local `arb bench` command for your machine.
- **Plan engine chaos tests.** Kill processes, return a rate-limit error, and create merge conflicts, using fake harnesses so no tokens are spent.
- **Token regression suite.** The median tokens per completed node on the fixture repo must not increase by more than 15% between releases.
- **Real end-to-end runs per phase** with small models (Haiku / Luna) on throwaway repos, reported with their cost.
