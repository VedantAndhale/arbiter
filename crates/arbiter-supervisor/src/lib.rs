//! OS-facing plumbing: isolated git worktrees per task and supervised child
//! processes whose whole tree can be killed reliably on every platform.

pub mod checkpoint;
pub mod credentials;
pub mod folders;
pub mod hardware;
pub mod prereq;
pub mod process;
pub mod pty;
pub mod worktree;

pub use process::{ProcessGroup, SupervisedChild};
pub use worktree::{Worktree, WorktreeManager};
pub mod integration;
