//! Harness adapters. Each harness is a long-lived child process speaking a
//! line-delimited JSON protocol; a [`Codec`] translates between that protocol
//! and Arbiter's normalized [`HarnessEvent`]s, and [`runtime`] owns the process.
//!
//! Keeping sessions warm (one process across many turns) is deliberate: steering
//! and follow-ups reuse the harness's prompt cache instead of re-sending context.

pub mod account;
pub mod catalog;
mod claude;
mod codex;
mod runtime;

pub use claude::ClaudeCodec;
pub use codex::CodexCodec;
pub use runtime::RunHandle;

use arbiter_core::NoWindow;
use arbiter_core::{AgentEvent, PermissionMode};
use std::path::PathBuf;

/// Which harness runs a thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Harness {
    Claude,
    Codex,
}

impl std::str::FromStr for Harness {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            _ => Err(format!("unknown harness {s:?}")),
        }
    }
}

#[derive(Clone, Debug)]
pub struct StartOpts {
    pub subscription_only: bool,
    pub cwd: PathBuf,
    pub permission: PermissionMode,
    pub model: Option<String>,
    /// Start without user-level plugins, skills, MCP servers and non-coding
    /// tools (see `claude::lean_args`, `codex::LEAN_DISABLE`).
    pub lean: bool,
    pub tool_profile: arbiter_core::ToolProfile,
    /// Reasoning effort, harness-specific (`--effort` / turn `effort`).
    pub effort: Option<String>,
    /// Harness session to continue.
    pub resume: Option<Resume>,
    /// Arbiter's own MCP server (browser tools…), attached only when useful.
    pub mcp: Option<McpServer>,
    /// Extra folders the agent may read, such as another project a
    /// multi-project plan step depends on.
    pub read_dirs: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct McpServer {
    pub command: PathBuf,
    pub env: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct Resume {
    pub session_id: String,
    /// Branch a copy of the session instead of continuing it (thread forks).
    pub fork: bool,
}

/// Normalized output of a harness process.
#[derive(Clone, Debug, PartialEq)]
pub enum HarnessEvent {
    Approval {
        request_id: String,
        tool: String,
        input: serde_json::Value,
    },
    /// The harness's own session id, for resuming later.
    Session(String),
    Agent(AgentEvent),
    TurnDone(TurnOutcome),
    /// The provider is refusing work until `resets_at` (unix seconds), when known.
    RateLimited {
        resets_at: Option<i64>,
    },
    /// The process exited; `stderr_tail` helps explain unexpected exits.
    Exited {
        code: Option<i32>,
        stderr_tail: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum TurnOutcome {
    Completed,
    Interrupted,
    Failed(String),
}

/// Inputs to a running session.
#[derive(Clone, Debug)]
pub enum Command {
    Approve {
        request_id: String,
        allowed: bool,
    },
    /// A user message: starts a turn, or steers the active one.
    Send(String),
    Interrupt,
    Shutdown,
}

/// Protocol translation for one harness. Pure (no I/O) so it can be tested
/// against recorded transcripts.
pub trait Codec: Send + 'static {
    /// Lines to write to stdin right after spawning (handshakes).
    fn on_start(&mut self) -> Vec<String> {
        Vec::new()
    }
    /// Lines to write for a command. `Shutdown` never reaches the codec.
    fn on_command(&mut self, cmd: &Command) -> Vec<String>;
    /// Parse one stdout line into events plus any protocol replies to write back.
    fn on_line(&mut self, line: &str) -> (Vec<HarnessEvent>, Vec<String>);
}

impl Harness {
    /// Spawn the harness in `opts.cwd` and return a handle to drive it.
    pub fn start(self, opts: StartOpts) -> anyhow::Result<RunHandle> {
        match self {
            Harness::Claude => runtime::spawn(claude::command(&opts)?, ClaudeCodec::new(opts.permission)),
            Harness::Codex => runtime::spawn(codex::command(&opts)?, CodexCodec::new(&opts)),
        }
    }
}

/// Find `name` on PATH. npm-style `.cmd`/`.bat` shims can't be spawned directly
/// on Windows, so they are wrapped in `cmd /C`.
pub(crate) fn resolve(name: &str) -> anyhow::Result<tokio::process::Command> {
    let path = which::which(name).map_err(|_| anyhow::anyhow!("`{name}` not found on PATH; is it installed?"))?;
    let is_script = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    let mut command = if is_script {
        let mut c = tokio::process::Command::new("cmd");
        c.no_window();
        c.arg("/C").arg(path);
        c
    } else {
        let mut c = tokio::process::Command::new(path);
        c.no_window();
        c
    };
    arbiter_supervisor::process::hide_console(&mut command);
    Ok(command)
}

/// Cap stored tool output; the UI shows it, but megabytes of logs help no one.
pub(crate) fn clip(s: &str) -> String {
    const MAX: usize = 16 * 1024;
    if s.len() <= MAX {
        return s.to_owned();
    }
    let mut end = MAX;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… [{} bytes truncated]", &s[..end], s.len() - end)
}
