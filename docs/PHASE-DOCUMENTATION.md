# Local documentation retrieval checkpoint — 2026-09-24

## Resumed 2026-09-24: verification results

- The first commit `f793005` holds the whole tree as it stood at the pause.
- Full workspace, `cargo test --workspace --lib --tests --offline --target-dir target/dev2 -j 4` (with `LIBCLANG_PATH` set): **116 passed, 0 failed.** The one ignored test is the opt-in native credential test. Log: `target/resume-workspace.log`.
- `native_dummy_credential_roundtrip` (run with `--ignored`) **passed** against Windows Credential Manager. Afterwards `cmdkey /list` showed no leftover `Arbiter/*` entries. Run `cmdkey` from PowerShell: Git Bash rewrites `/list` into a path.
- Browser check against an isolated daemon home on port 7447 (Vite dev server on 1420), with cloud agents off and dummy `ctx7sk-dummy-…` keys only:
  - An invalid key is rejected inline and the field clears.
  - A session save reports "until the daemon restarts".
  - An OS save creates exactly one `Arbiter/Context7/<home hash>` entry.
  - `GET /v1/documentation/credential` returns only `configured/source/persistent_available`. The key never appears in the API response or the DOM.
  - Remove deletes the OS entry (0 entries afterwards).
  - Without consent, "Test Context7 connection" is refused with no network call.
  - "Guide me locally" without a model returns setup guidance (~250 ms).
  - At 390 px the document is 390 px wide with no overflowing elements.
- Not exercised live: the Cancel button. Without a model or consent every path finishes in under 300 ms, so the busy state never shows; the daemon cancellation paths are covered by the integration tests. A failed native save was not exercised, because the native store was available.
- `UX-CONTRACT.md` now describes real cancellation and the credential form, replacing "Stop waiting" and "sign-in pending".

## Preparation race audit: fixed 2026-09-24

- **H1.** The idle sweep in `start_run` skips threads that are preparing documentation. `stop_locked` now stops both the preparation and the thread's idle process; before, it returned after the first. Regression: `preparation_races.rs`. With the fix removed, A's message is lost.
- **H2.** The local model is a single queued slot (`local_slot`, which waits up to 5 minutes) instead of `try_lock`.
  - Preparation, retrieval and review all wait their turn.
  - Up to 16 preparations may be pending.
  - The outer bound is 600 s, including queue time.
  - Three parallel plan-style preparations now all dispatch.
- **M1.** A second message during preparation or a busy turn returns 409 (`runs::Busy`). It adds a "Message not delivered" notice and does not mark the thread failed.
- **M2.** Restart recovery detects an unfinished preparation and tells the user the message never reached the agent and must be resent.
- **M3.** The reviewer's documentation lookup has a 180 s limit and tolerates failures: the gap is passed to the reviewer as `documentation_gap` instead of aborting the review.
- **L1.** A missing model or disabled network or Context7 now fails synchronously in `deliver`.
- **L3.** Citation quotes need at least 12 non-space characters.
- **Not changed, with reasons.**
  - **M4 / L2.** `start_run` re-checks `check_cloud` for the harness current at dispatch time, and reads the thread configuration fresh. So stale admission and a harness change during preparation are caught at dispatch. The cost is only the wasted local work.
  - **L4.** Separate cancel-registry limit: deferred as low risk.
  - **Review cache.** The review cache is still keyed on evidence that includes fresh documentation, so it is checked after fetching.
- **Verification.** Workspace: 120 passed, 0 failed, 1 ignored (the opt-in native credential test). Clippy is clean with `-D warnings`, and the UI build passes.

## PAUSED by user — usage exhausted (2026-09-24), superseded by the section above

The user explicitly requested: "out of usage add in doc and stop". Work is paused. Do not continue implementation until the user resumes it. This section supersedes older pending/verification statements below.

### Latest changes saved, not committed

- Added a `CredentialStore` abstraction in arbiter-supervisor, Windows Credential Manager backend, explicit unsupported-platform fallback, and an in-memory fixture backend. Daemon credentials are scoped by daemon home. Authenticated status/save/remove routes never return the key. Session-only storage is supported. New `Context7Credential.tsx` provides password entry, storage choice and removal. Existing environment-key fallback remains, explicitly reported as such.
- Added automatic local documentation preparation before frontier dispatch (`documentation_handoff.rs`). Local model decides if one lookup is needed, fetches/summarizes it, then passes a bounded untrusted evidence brief to the existing frontier launch path. No raw provider response is persisted or forwarded. Failure pauses dispatch; Stop/Interrupt and changed setup cancel preparation. A 240-second outer timeout bounds preparation. Pending preparations count as active tasks and are not automatically replayed after restart.
- Added a bounded cancellation-signal registry (`documentation_operations.rs`) and authenticated cancel route for standalone setup/lookups. Supports cancel-before-POST races without storing response data. UI cancellation now signals the daemon and suppresses late responses.
- Independent reviewer now requests fresh local documentation and can cite only validated exact excerpts. Retrieval output adds up to two bounded citations; absent/mismatched quotes are rejected. No documentation cache was reintroduced.
- Updated run lifecycle locking: `deliver_ready` requires admission lock; `stop_locked` is used inside existing admission-locked paths. This affects dispatch/stop/setup and warrants the pending workspace regression run.

### Verification at pause

- The four focused integration tests in `target/docs2-tests.log` passed: documentation retrieval, credential/handoff/failure/cancellation, local execution and setup policy. This run preceded the last independent-review/citation assertions; rerun those tests on resume.
- Latest targeted Clippy passed (`target/docs2-clippy.log`), and desktop build passed (`target/docs2-ui.log`). No live frontier tokens, real keys or provider calls were used by tests.
- Full `cargo test --workspace --lib --tests --offline --target-dir target/dev2 -j 4` was still compiling (including llama native dependencies) when the user stopped work. It was interrupted; **no workspace-wide pass is claimed**. Log: `target/docs2-workspace.log`.
- The opt-in native dummy credential roundtrip test is added but has NOT run. Latest credential/cancel UI has NOT received browser verification. Earlier browser screenshots below cover the preceding implementation only.
- A disposable preview was started at port 7446 (`target/phase-docs2`), then stopped at this pause. Earlier previews are separate. Never copy its bearer token into documentation.

### Resume here

1. Read AGENTS.md and PLAN.md; inspect saved changes without staging the mostly untracked repository wholesale.
2. Set `LIBCLANG_PATH` to `<repo>/target/intake-tooling/clang/native`. Rerun documentation delivery/review tests, then finish the interrupted workspace tests; use `--offline --target-dir target/dev2 -j 4` and redirect verbose logs.
3. Run the explicitly ignored `native_dummy_credential_roundtrip` test against its unique disposable credential target; it creates/removes a dummy value only. No real user key is required.
4. Verify the new credential form and cancellation in a fresh isolated preview at desktop/narrow widths, including missing native storage, failed save, removal, standard guidance without a model, cancellation, and no key echo. Update UX-CONTRACT.md to replace obsolete "Stop waiting" behavior and secure-sign-in-pending wording.
5. Audit preparation lifecycle: cancellation/stop/config-change races, plan children/root failure propagation, concurrent dispatch, retry/restart, offline or missing model, and no-query path. Test reviewer fresh evidence and exact-quote validation. No real paid provider trials without explicit allowance.
6. Remaining larger scope: native credential backends on other OSes, broader MCP discovery/install/rollback, provider-version and model-quality acceptance, native harness inherited-MCP isolation, and remaining F2-F4/release work. None of these phases is declared complete.

Implementation approved after the user requested Context7-style retrieval, no documentation cache and local ownership of all retrieval work.

## Implemented

- Daemon-owned Context7 remote MCP connection: initialize, tool discovery, resolve-library-id and query-docs. Fixed HTTPS endpoint, no redirects, 20-second request timeout, 128 KB response cap and bounded model context. Supports JSON and SSE response envelopes. No automatic retries or paid fallback.
- An explicitly selected installed local generation model chooses a returned library ID and produces a compact evidence brief. Source URLs must appear in the supplied response; this is attribution validation, not proof that every generated claim is correct. Missing/ambiguous version selection fails visibly.
- No documentation response writes, persistent index, prefetch or idle retrieval. The former project documentation endpoint no longer fetches or reads cached pages. Review no longer reads docs-cache. Old files from earlier development are not consumed; no broad filesystem deletion is performed.
- Local editor can request documentation, at most twice within its eight-action turn. Public-query guards and saved network/provider consent apply. Raw provider content is not written to the task event log. No Context7 tools were added to frontier harnesses; the daemon's Context7 environment credential is removed from Claude/Codex child environments.
- Setup supports local model choice, explicit external-query consent, local setup guidance, connection testing and an optional live query form. Results remain only in the mounted panel. Missing models, disabled network, protocol errors, authentication failure and quota exhaustion are visible states.
- Credentials currently use CONTEXT7_API_KEY in the daemon environment. They are not passed to local model prompts or persisted in setup preferences. The remote connection needs no package install or rewrite of existing harness configs.

## Verification

- Focused daemon unit and integration tests cover JSON/SSE envelopes, private-query rejection, invented source rejection, local setup guidance, required tool discovery, live repeated fetches, no docs-cache creation, offline/auth rejection, adoption and isolated local editing/cancellation. Fixture frontier launcher panics if called.
- All 13 focused daemon tests passed after the local-tool addition, including offline documentation refusal within a local editing turn. UI production build and targeted Clippy with warnings denied passed; logs are target/docs-tests.log, target/docs-ui.log and target/docs-clippy.log.
- Strict premium UI audit: zero findings. Browser checked setup and missing-model feedback, no page errors, 390 CSS pixel viewport with document width 390, and desktop layout. Screenshots in target/phase-docs.
- No real frontier inference, model download, credential entry, provider quota trial or publication was performed. MCP/model fixture success does not establish live provider compatibility or local-model accuracy.

## Remaining before this phase is complete

- Secure guided credential entry/sign-in and durable OS credential storage; broader MCP discovery/install/rollback beyond the fixed remote Context7 service.
- Automatic evidence preparation and delivery across planning, frontier execution and independent final review. The lookup service returns a compact brief, but automatic frontier handoff is not wired. The reviewer currently reports missing documentation evidence rather than reading stale cached pages.
- True server-side cancellation for standalone setup/lookup requests. The UI can stop waiting and discards late results; daemon work remains bounded by existing request/model timeouts. Stopping a local editing task aborts its retrieval future.
- Broader privacy fixtures, rate-limit/auth/slow-transport integration tests, requested-version correctness and citation-entailment evaluation with real local models. Current string guards are not a proof of comprehensive sensitive-data detection.
- Verify native harness isolation against inherited MCP configuration, including non-lean mode; do not claim all third-party MCP access is blocked merely because Arbiter does not register Context7 for frontier agents.

Protocol reference: https://github.com/upstash/context7 (remote endpoint and tool signatures checked on 2026-09-24).
