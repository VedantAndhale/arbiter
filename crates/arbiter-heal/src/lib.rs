//! Self-healing, deterministic half: find a project's checks, run them, turn
//! noisy output into a small, deduplicated excerpt an agent can act on, and a
//! stable signature so the loop can tell "same failure again" from progress.
//!
//! Nothing here calls a model. The daemon decides when to heal and feeds the
//! excerpt back to the agent's existing session.

pub mod detect;
pub mod excerpt;
pub mod parse;
pub mod runner;

pub use detect::{BrowserConfig, Check, CheckKind, HealConfig, STATIC_SERVER, detect, detect_with, is_frontend_file};
pub use excerpt::{Excerpt, build_excerpt};
pub use parse::{Failure, parse};
pub use runner::{CheckRun, run_check, run_setup};

/// Bounds on the loop. Defaults are conservative: every attempt costs a turn.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HealPolicy {
    pub max_attempts: u32,
    /// Stop when the exact same failure set comes back this many times.
    pub same_signature_limit: u32,
    /// Excerpt size cap in characters (~4 chars per token).
    pub excerpt_chars: usize,
}

impl Default for HealPolicy {
    fn default() -> Self {
        Self { max_attempts: 3, same_signature_limit: 2, excerpt_chars: 6000 }
    }
}
