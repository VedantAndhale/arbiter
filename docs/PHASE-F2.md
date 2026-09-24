# F2 implementation checkpoint

In progress. The user authorized F2–F4; this checkpoint does not mark those phases complete.

## Implemented

- `arbiter-project`: bounded, non-executing metadata inventory; nested instruction discovery; create-only scaffolding; SQLite proposal journal; configuration fingerprint checks; apply retry and rollback that refuses modified files. Existing agent configurations are preserved rather than classified as obsolete without evidence.
- Authenticated daemon routes for inspect/overview/apply/rollback, initial Git setup, baseline and documentation cache. Re-adding the same canonical project path is idempotent.
- Initial Git setup stages only explicitly selected candidate paths, rejects likely secret paths/markers, disables commit hooks/signing for its initial commit, and never initializes an existing repository. Files omitted from the first commit are visibly unavailable to isolated branches. Git history itself is not removed by scaffold rollback.
- Baseline checks require explicit UI trust/command approval, revalidate metadata, skip dependency installation and deterministic fix commands, retain bounded output and record passed/failed/no-checks distinctly. Scripts can still modify files/access the network; network-off refuses this operation rather than claiming OS sandboxing.
- Manifest profiles for Node/React/TypeScript/Next, Rust, Python and Go. npm lockfiles distinguish exact dependency versions from declared ranges. Python minimum-version docs and React 18 archive links avoid blindly selecting current docs for older projects. Other applicability is explicitly unverified.
- The official-source documentation cache has been **removed**, as the user asked. Documentation is retrieved live, locally, through Context7 (see PHASE-DOCUMENTATION.md). The project documentation route now returns version hints only.
- Project preparation UI using existing design tokens, shared controls, inline errors and progressive details. Add project is available from the workspace.

## Resumed 2026-09-24: fixes

- **Inventory scale.**
  - Git repositories are listed with `git ls-files -z --cached --others --exclude-standard`, so ignored build output is never walked.
  - Plain folders are walked to a depth of 8 and at most 20,000 entries. The walk truncates with one warning instead of failing, which replaces the old hard failure at 4,000 entries.
  - Deep folders and linked paths each produce one aggregated warning.
  - Lockfiles (`package-lock.json`, `Cargo.lock`) are read up to 16 MiB, replacing the 256 KiB limit that failed real monorepos.
  - Other oversized metadata is skipped with a warning and still changes the fingerprint.
  - The initial-commit candidate list is capped at 2,000 files, with a warning.
- **Atomic initial commit.** If anything fails after `git init`, the new `.git` directory is removed so the user can retry. The first commit is authored with the user's own `user.name` and `user.email`, and falls back to Arbiter only when those are unset.
- **Register guard.** A repository with no commits is refused with an actionable message. Worktrees need `HEAD`.
- **Baseline evidence.**
  - Each failed check stores its command and a short explanation: up to 5 parsed failures, or the last 8 output lines, capped at 1,500 characters. Raw logs are never kept.
  - The UI shows passes, explains why failures that already exist matter, shows the evidence for each failure, and warns when a baseline predates the current configuration.
- **Tests.**
  - `large_git_repo_inspects_deep_packages_and_big_lockfiles` and `huge_plain_folder_truncates_with_one_warning` (crate).
  - `failed_initial_commit_rolls_back_and_empty_repos_are_refused` (daemon): baseline evidence, rollback after an injected failure, a successful retry, and refusal of a repository with no commits.
  - Both suites pass, clippy is clean with `-D warnings`, and the UI build passes.

## Browser walkthrough, 2026-09-24 (isolated daemon on port 7447, disposable folder)

The full flow was checked in the UI: inspect a folder without Git → apply the three additions → run the baseline with consent → initialize Git with five files → add to workspace.
- The baseline shows the existing failure with its command and the real failing line.
- The commit is authored with the user's own Git identity and contains exactly the selected files.

Fixes from this walkthrough:
- "1 checks" is now pluralized correctly.
- The Git step now comes before the disabled "Add to workspace" button that depends on it.
- The file list has a count and Select all / Select none.
- After initializing, a confirmation says how many files were committed and what to do next.

Checked later the same day:
- Reloading mid-flow restores the saved proposal from `?adoption=`, showing "Folder without Git · proposed".
- At a 390 px viewport the document is 390 px wide and no element overflows.

## Verification so far

- Three initial crate tests passed (preservation/idempotency/rollback, stale nested instructions, mixed-stack profiles). An interrupted-apply/cancellation test was subsequently added and awaits the final suite.
- Daemon integration test passed for authenticated inspection/apply, approved baseline, offline documentation behavior, secret-path rejection, selected initial commit, and idempotent registration; fake coding launcher panics if invoked.
- Desktop build passed. First Clippy pass identified one collapsible conditional, now corrected; final rerun pending.
- Browser preview running at port 7444 with cloud agents disabled; initial inspection and apply verified. Browser/release verification continues.

## Remaining F2 work

- Latest ownership revision: all MCP retrieval is performed by the local agent through the daemon. Frontier agents receive bounded locally prepared evidence only; they do not get retrieval tools. This supersedes shared local/frontier tools below. Verify no frontier calls for retrieval or synthesis and no cloud fallback when local capability is missing. Evidence still counts as frontier input tokens when delivered. Initial retrieval implementation is approved and covered in PHASE-DOCUMENTATION.md.

- Finish UI recovery/resume polish and full browser checks. Baseline evidence and initial-commit recovery are done (see above).
- Evidence-based conflicting-config migrations beyond create-only scaffolding.
- Replace the fixed-profile retrieval direction with the user-requested Context7-style resolve-library → query-docs flow. Context7 is the initial planned external provider; built-in manifest detection supplies version hints, not the library coverage boundary. Latest user revision: no persistent documentation caching, indexes, prefetch or background crawling. Remove the current cache from retrieval and reviewer paths; use bounded transient live results and show unavailable offline. Add local-agent guided MCP setup, connection validation and recovery during onboarding and later from Settings. The cache has been removed and live retrieval is implemented; see PHASE-DOCUMENTATION.md for the delivered slice and open work.
- Final tests, Clippy, UI audit, roadmap reconciliation. No real provider coding inference has been used.
