# F3 checkpoint: local and frontier feature delivery (2026-09-24)

In progress. The user approved F2–F4. This checkpoint does not mark F3 complete: acceptance with real local models and real providers is still open (see below).

## Implemented

- **Local coding agent** (`arbiter-local`, `arbiterd/src/local.rs`).
  - **Tools.**
    - `list` and `search` return up to 200 paths or 30 matches, and skip hidden, dependency and credential paths.
    - `read` works in windows of 200 lines or 6,000 characters, on files up to 256 KiB, and returns a hash of the whole file.
    - `edit` replaces text that must match exactly once, and must be preceded by a fresh read.
    - `write` handles new files and files up to 8 KB.
    - `check` runs the project's configured checks through the daemon, with the same network policy as automatic checks. It returns the failures only.
    - `documentation` is limited to 2 per turn.
  - **Limits.**
    - At most 24 actions per turn, and at most 3 check runs.
    - The last 10 steps are passed back into the prompt, so the model knows what it has already done.
    - The same action repeated three times stops the turn, with changes preserved.
    - Tool results over 8,000 characters are replaced with a request to narrow the query.
    - File bodies never enter the event log.
  - After the turn, the existing heal loop and review still run.
- **Local coding check** (`capability.rs`).
  - It runs three fixed tasks through the same loop in a disposable folder under the daemon home, which is removed afterwards:
    - fix a bug in one file;
    - search and fix across files without touching the caller;
    - create a new file.
  - Each task is checked by inspecting the resulting files, not by trusting the model's claim.
  - Results are stored per model and per probe version in `local-capability.json`. Changing the probe version invalidates old results.
  - Routes: `GET /v1/local/capability` and `POST /v1/local/capability {model}`. The UI shows it in Local models as "Check coding ability", with per-task results.
- **Local-first placement** (`router::place`, `runs.rs::place_locally`). This applies only to auto threads.
  - Only a model that passed every current check task is trusted.
  - **Local-only mode:** the task always runs locally, or is blocked with the reason. It never falls back to the cloud.
  - **Otherwise:** the task runs locally when no cloud agent is allowed. It also runs locally when the task is small, has no risks, is not flagged for a frontier model and is not research.
  - Everything else goes to the cloud.
  - The choice and its reason are recorded as a `ConfigChanged` event. The corrected intake assessment is used when one exists.
- **Escalation** (`POST /v1/threads/{id}/escalate {harness}`).
  - It is only ever user-initiated, from the "Continue with …" card on a local task.
  - Cloud admission applies as usual.
  - The frontier agent gets a compact brief, not the local transcript. The brief contains the original request, the local summary, the diff stat, and up to three open problems (errors or failing checks).
  - The notice states the brief's size and that it uses cloud allowance.

## Verification

- Crate tests: windowed read, list and search skipping `node_modules` and `.env`, ambiguous-edit refusal, stale-hash refusal, large-file write refusal, and read scope versus write scope.
- `local_repair.rs`: list → search → read → wrong edit → failing check → reread → fix → passing check → done, with cloud launches forbidden. It also stops the repeated-action loop.
- `hybrid_routing.rs`:
  - local-only mode without a checked model is blocked;
  - the "weak" model scores 0/3 and the competent one 3/3, and the probe folders are removed;
  - local-only mode routes locally;
  - a small task routes locally even when the cloud is allowed;
  - a large or risky task goes to the cloud;
  - escalation delivers a brief under 6 KB with the diff stat and no transcript, and a second escalation is refused.
- Workspace: 124 passed, 0 failed, 1 ignored (the opt-in native credential test). Clippy is clean with `-D warnings`, and the UI build passes. All of this uses fixture models and launchers; no live tokens were spent.

## Not done / open

- **Real-model quality.** Whether Granite 4.1-3B or LFM2.5 pass the coding check on the 5850U, and how long they take, is unmeasured. Fixture success proves the loop, not a model.
- **Plan nodes.** They can run locally when the plan assigns `local`. There is no automatic escalation for a failed local plan node yet; it blocks like any failed node.
- **Restart.** A local turn in progress when the daemon restarts is marked interrupted and its changes are kept. There is no automatic resume.
- **Standing reviewer.** It exists from F2 and is used at plan milestones. The dual-perspective acceptance fixtures (outdated API, version-incompatible feature, trade-off, agreement, malicious document) are not yet written.
- **Acceptance runs.** A full local-only feature run through preview, and an authorized hybrid run with real providers, are both still required. Real provider trials need explicit allowance.
- **Token accounting.** Local token counts are not reported by the runtime.
