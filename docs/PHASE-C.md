# Local intake and approvals

The daemon owns intake, model downloads, classification, question generation and approval decisions. Desktop and CLI clients use authenticated endpoints; no local model logic lives in Tauri.

## User flow

New desktop tasks clarify unclear requests by default. Clear requests proceed directly. An unclear task is stored with `IntakeAssessed` and `QuestionsAsked` events and appears in the Inbox. It does not launch a coding harness until every question is answered. One extra clarification round is available. The saved `IntentReady` record contains the request, answers, referenced paths, assessment and planning recommendation.

Use **@ Add context** for up to eight project files. References are validated against the project root, including symlink resolution. Content is not inserted into the agent prompt. Files excluded as secrets are not listed. Isolated worktrees use their committed versions.

Classification corrections are append-only assessment events. A bounded, deduplicated local training set retains the most recent 256 corrected examples. The next classifier load retrains its centroid heads.

Claude `can_use_tool` requests appear as **Allow once / Deny** cards. A decision is tied to both the run and request id, cannot be reused, and expires if the process exits or the daemon restarts. Codex continues to use its existing sandbox/deny behavior; Codex approval cards are not implemented in this phase.

## Models

**Local models** opens setup, progress, errors, selection and benchmarks. The catalog pins public Hugging Face revisions, byte sizes and SHA-256 checksums. Downloads are staged, size bounded and hash checked. No private task text leaves the machine for classification or questions.

- Potion base 32M supplies static embeddings to seed-trained task type, size, ambiguity and risk heads. Explicit risk terms remain as a conservative guard. User corrections retrain type, size and ambiguity heads.
- LFM 2.5 1.2B Instruct and Granite 4.1 3B Q4_K_M are supported question writers. LFM setup requires the user to review and accept its license. Granite is available under Apache 2.0.
- llama-cpp-2 runs CPU inference with six threads and llguidance JSON constraints. Context is capped at 2,048 tokens, output at 500 tokens, and generation at 90 seconds. A cached system-prefix state is restored between requests. One worker bounds simultaneous model memory; models unload after five idle minutes.
- Missing models or invalid output use explicitly identified rule-based assessment and standard clarification cards. This is not represented as neural inference.

Setup automatically runs a benchmark. Three fixed prompts are measured after warm-up. The recorded value is worst-case completion latency, with first-token latency recorded separately; this deliberately does not mislabel first-token time as a completed question card. Targets are 5 ms for classification and 300 ms for complete question generation. The UI reports when a target is unmet. Only qualifying question models become automatic defaults; standard cards remain available when neither qualifies. Users may explicitly select a slower installed model. CPU and build configuration affect results.

## Development

### Verification checkpoint: Ryzen 7 PRO 5850U (2026-09-23)

In the Windows development build, Potion's worst warm classification over three fixed prompts was **0.7751 ms**. Granite produced valid schema-constrained questions on warm-up and three measured prompts; worst first-token latency was **2,981.93 ms**, and worst full response was **56,191.31 ms**. Development builds/tests were running during parts of this measurement, so this is not an isolated release-performance result. It does not establish the 300 ms target; Granite therefore remains unselected by default.

A separate warm API request using Potion plus standard question cards completed in **250 ms** with `worktree=false`. Cold classifier loading took about **7.9 s** in this build. These are individual end-to-end observations, not a substitute for the neural first-card acceptance benchmark. LFM was not downloaded or benchmarked because its license acceptance is left to the user. The current T1 prompt requests short answers; option-based card rendering is supported by the API/UI but is not generated yet.

The embedded runtime requires a C++ compiler, CMake and libclang. On Windows with MSVC/CMake already installed, an isolated build dependency can be installed with:

```powershell
python -m pip install --target target/intake-tooling libclang==18.1.1
$env:LIBCLANG_PATH = "$pwd/target/intake-tooling/clang/native"
cargo test --workspace
```

On Linux install `clang libclang-dev cmake`; on macOS install LLVM and set `LIBCLANG_PATH` to its `lib` directory if it is not detected.

`arb models --json` reports setup and benchmark results. `arb install-model potion` downloads and verifies a model; `arb select-model granite` selects an installed question writer. `arb bench potion` (or `lfm`, `granite`) starts a benchmark in the daemon. Use `arb log THREAD --json` to inspect pending cards, `arb answer THREAD CARD answers.json` to submit an array of `{question_id,text}` answers, and `arb approve THREAD RUN REQUEST --allow` to allow a tool once (omit `--allow` to deny). Unit and daemon tests use fixed inputs and fake coding harnesses. They do not download models or spend subscription tokens.

For an isolated, explicit public-model benchmark:

```powershell
cargo run -p arbiter-intake --example bench -- target/model-bench potion
cargo run -p arbiter-intake --example bench -- target/model-bench granite
```

The catalog updater is `scripts/update-model-catalog.py`. Review revision, license and size changes before shipping an updated catalog.

## Sources

- [Model2Vec Rust implementation](https://github.com/MinishLab/model2vec-rs)
- [llama-cpp-2 runtime](https://docs.rs/llama-cpp-2/0.1.157/llama_cpp_2/)
- [Claude SDK permission protocol](https://code.claude.com/docs/en/agent-sdk/permissions)
- [Codex web search configuration](https://learn.chatgpt.com/docs/config-file/config-basic)
