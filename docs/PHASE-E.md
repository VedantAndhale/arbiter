# Phase E checkpoint — 2026-09-24

Status: core implementation delivered; final verification results below. Phase E is not a production-readiness claim.

## Implemented

- Screenshot review with fit, natural-pixel size, zoom, pointer/keyboard pan, fullscreen, manual refresh, stale/error handling, and saved captures. Authenticated captures survive preview shutdown and daemon restart.
- Actual CSS viewport presets (1280×800, 768×1024, 390×844) and bounded custom dimensions. Frame geometry and inspector coordinates use returned viewport metadata.
- Collapsible desktop navigation, narrow modal drawer, compact plan summary and direct selected-step details. Shared dialogs have bounded scrolling.
- New `arbiter-vault`: guarded Markdown mirrors, lexical search, wikilinks and a bounded memory catalog. Notes are event-backed and Markdown can be rebuilt. Mirrors are excluded from Git diffs. External Markdown edits are not imported.
- Automatic approved-plan, structured-handoff and successful-repair notes. Agent decision proposals require revision-checked acceptance/rejection; acceptance stores the approved content atomically with resolution.
- Project memory UI: note editor, search, links/backlinks, keyboard-operable graph, current/proposed comparison, and composer @ note selection.
- MCP `vault_search`, `vault_read`, `vault_propose`; irrelevant browser/plan tools are omitted for non-web/direct tasks. API and CLI counterparts cover notes, proposals, rebuilding, learning, overrides and rework.
- Bounded context: catalog ≤1000 UTF-8 bytes, note-revision/content deduplication, and existing 6000-byte total plan-brief cap. The exact enriched plan brief is persisted. Read calls cap output at 4000 bytes and disclose truncation.
- Latest outcomes per task: task type, selected harness/model, check pass/first try, heal attempts, usage/cost and user rework. Forks do not duplicate samples. Tasks without configured checks do not train routing. Unknown actual model IDs remain labeled default/unreported.
- Automatic routing uses at least three measured samples, minimum 50% passes, and first-try successes exceeding rework count. Explicit choices win. Preferences can select a harness/model or disable learning for a task type.

## Verification

- Workspace regression suite: 97 tests passed, including real Chromium at all three website viewport dimensions; daemon tests use fake harnesses.
- Final learning eligibility change: knowledge and plan integration suites passed (six tests); workspace clippy with warnings denied passed.
- A development hot-reload context error was traced to the App/component import cycle; the shared API context now lives in `ApiContext.ts`. Fresh browser verification reported no page errors.
- Desktop TypeScript/Vite production build passed. Strict frontend audit reported zero findings.
- Browser: app widths 1440×900, 1280×720, 1024×768, 768×720, 640×720 had no document-wide horizontal overflow. Narrow navigation and fullscreen Escape restored focus. Compact step selection opened details directly.
- Simulated frame-request failure showed Retry and disabled stale-image interaction; restoring the request and retrying recovered the frame.
- Screenshot actual-size geometry measured 1280 natural pixels → 1280 displayed CSS pixels; zoom increased it to 1600. Saved capture reopened after daemon restart with the website stopped; timestamp and source viewport persisted.
- At 640×360, the note editor’s primary action remained reachable by keyboard; Escape exposed an unsaved-edit prompt. Its focus placement was corrected during verification.
- A pointer click on the 150% zoomed 390×844 website screenshot selected the expected `#pay` element, confirming UI-to-browser coordinate mapping after resize, zoom and canvas scrolling.
- Browser: approved note save, linked note/backlink, live proposal count, acceptance, keyboard graph selection and composer note insertion passed at desktop/narrow sizes. No message was sent by mention selection.
- Regression tests cover rejected secrets/path traversal, stale revisions and duplicate acceptance, bounded reads, Markdown exclusion from Git, event/projection rebuild, fork sample deduplication, learned routing and user override.

## Token measurement and remaining gates

The deterministic fake-harness test reports 400 input + 20 output tokens before memory, and 100 input + 20 output after a memory hit. This tests accounting/reuse/routing contracts only: those numbers are simulated. **A real-provider repeat-task comparison and the plan's measured real token-savings acceptance gate remain unverified.** No paid harness was run.

The 200% check used CSS zoom and narrow/short reflow emulation. Native desktop browser zoom and a broader accessibility/OS matrix remain release checks. Phone/tablet presets do not emulate mobile UA, touch hardware or a complete remote/mobile product.

Secret/injection guards are conservative pattern checks, not proof that arbitrary text is safe. Notes are never treated as instructions. Search is lexical; there is no embedding index. Graph shows up to 50 notes, list up to the project cap of 500; pending proposals cap at 50. Learning is based on the latest task outcome and selected model, not an experiment controlling for task difficulty. Existing Phase C latency and Phase D scheduler/resource limitations still apply.

## Preview and evidence

Fixture: `cargo run -p arbiterd --example phase_e_preview -- <home> <repo> 7442`. It uses fake agents and preserves an existing fixture home. The live preview uses `target/phase-e/home` and `target/phase-e/repo`; discovery is in `daemon.json`. Do not copy its token into documentation.

Evidence under `target/phase-e/`: `saved-reopened.png`, `compact-step.png`, `vault-note.png`, `proposal-640.png`, `vault-graph.png`. These files are local verification artifacts, not committed product assets.

Next product phase: Phase F review/landing, inline comments, best-of-N, side chat, command palette and notifications. Installer/onboarding hardening and realistic provider trials remain release work.
