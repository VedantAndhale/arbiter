# Testing Arbiter yourself

This is for trying the real app on your own computer before it is shared. There is no installer yet, so you run it from the source folder.

## Once: what you need

- Git, Node 22+, pnpm, Rust (stable), CMake and a C++ compiler. See [local runtime setup](PHASE-C.md#development).
- Claude Code and/or Codex installed and signed in. Arbiter's Settings shows what is missing, can install it with your approval, and opens sign-in for you.
- Optional: the GitHub CLI (`gh`), signed in. It enables draft pull requests and a one-click private repository for your memory.

On Windows, point the build at the bundled clang (PowerShell):

```bash
$env:LIBCLANG_PATH = "$pwd/target/intake-tooling/clang/native"
```

## Start it

Build the daemon once (and again after pulling changes):

```bash
cargo build -p arbiterd
```

Start the desktop app. It starts the daemon itself:

```bash
pnpm --dir apps/desktop tauri dev
```

To keep your real data separate while testing, set a test home first (PowerShell), for example:

```bash
$env:ARBITER_HOME = "$env:LOCALAPPDATA/arbiter-test"
```

## What to try, in order

1. **Settings.** Check that Claude Code or Codex shows as signed in with a plan. Under **Your memory**, connect a private GitHub repository or paste a git remote.
2. **One small task.** Add a small practice project with **Choose folder…**, then ask for something small ("add a dark mode toggle"). Watch it work, open **App** to see it running, and open **Changes** to review.
3. **Publish.** Commit, then publish. Pick "Keep it on this computer" first; try a draft PR on a throwaway GitHub repository. Check that the remote gets one commit on `feature/…` and no `arbiter/…` branch.
4. **A planned task.** Type `/plan` and ask for something with two or three parts. Approve the plan and watch the steps.
5. **Two projects.** Type @ and pick two projects (for example an API and a front end). Arbiter plans it across both, and each project gets its own commit.
6. **Memory.** Write a few lines in `guidance/about-me.md` in your memory folder, and check that the next task follows them. Close the app: it asks to save your memory. Say yes.
7. **Optional.** Turn on local web research in Settings and ask an agent to look something up. Try a local model from **Local models**; this downloads a few GB and needs your OK.

## What uses your allowance

Every task that runs Claude Code or Codex uses your subscription, the same as using those tools directly. Arbiter pauses before paid extras and shows your remaining allowance in Settings.

## When something goes wrong

- The task view shows what happened in plain words. **Terminal** (Ctrl+J) opens a shell in the task's copy of the project.
- The daemon's log is in the terminal that ran `tauri dev`.
- Your projects are never changed until you commit or publish; each task works in its own copy.
- Tell me what you did, what you expected and what you saw. A screenshot helps.
