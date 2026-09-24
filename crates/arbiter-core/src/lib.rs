//! Domain types shared by every Arbiter crate: identifiers, the normalized
//! agent event model, and the append-only event envelope.

pub mod discovery;
pub mod event;
pub mod ids;
pub mod intake;
pub mod task;
pub mod title;

pub use discovery::{DaemonInfo, arbiter_home};
pub use event::{AgentEvent, AttachmentRef, CheckResult, Event, EventKind, PermissionMode, ThreadStatus, Usage};
pub use ids::{ProjectId, RunId, TaskId, ThreadId, WorkspaceId};
pub use intake::{Answer, Assessment, IntentSpec, Question, QuestionKind, QuestionOption, ToolProfile};
pub use task::{Priority, Task, TaskStatus};
pub use title::derive_title;
pub mod plan;

pub mod vault;
