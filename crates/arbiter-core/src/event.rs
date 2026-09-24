use crate::ids::{ProjectId, RunId, ThreadId, WorkspaceId};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Envelope persisted in the append-only log. `seq` is monotonically
/// increasing per thread and assigned by the store.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub workspace_id: WorkspaceId,
    pub thread_id: ThreadId,
    pub seq: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub ts: OffsetDateTime,
    pub kind: EventKind,
}

/// Everything that can happen to a thread. New variants are additive; old
/// logs must always deserialize, so never rename or remove a variant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    /// The task's work went out as one squashed commit on a normal branch
    /// (`arbiter/*` branches are never pushed). `project` is set for a
    /// plan's other projects; `source` is the Arbiter commit it came from.
    Published {
        project: Option<String>,
        mode: String,
        branch: String,
        base: String,
        commit: String,
        source: String,
        url: Option<String>,
    },
    /// Local copies and arbiter/ branches of finished work were removed.
    WorkCleaned {
        copies: u32,
    },
    /// Other projects this task may change besides its own; its work is
    /// planned so each step targets one project.
    ProjectsAttached {
        projects: Vec<ProjectId>,
    },
    /// A summary from a site the user is signed into looked private, so it
    /// is held until the user decides whether the agent may see it.
    ShareRequested {
        id: String,
        host: String,
        question: String,
        reasons: Vec<String>,
    },
    ShareDecided {
        id: String,
        allowed: bool,
    },
    /// The task's app is running and can be opened.
    PreviewReady {
        url: String,
    },
    /// Arbiter could not decide between planning and starting one agent, so
    /// intake asks the user as a question card.
    ApproachAsked,
    /// This thread is one candidate in an opt-in comparison of agents.
    ComparisonJoined {
        comparison: String,
        label: String,
    },
    /// The user kept one candidate of the comparison.
    ComparisonDecided {
        comparison: String,
        kept: ThreadId,
    },
    /// A question asked beside the task and answered locally. Never forwarded
    /// to the task's agent.
    SideChat {
        question: String,
        answer: String,
        model: String,
    },
    SecondOpinion {
        stage: String,
        model: String,
        evidence_hash: String,
        report: serde_json::Value,
    },
    VaultSaved {
        note: crate::vault::Note,
    },
    VaultProposed {
        id: String,
        note: crate::vault::Note,
        base_revision: u32,
    },
    VaultResolved {
        id: String,
        accepted: bool,
        #[serde(default)]
        note: Option<crate::vault::Note>,
    },
    MemoryUsed {
        notes: Vec<String>,
        bytes: usize,
    },
    OutcomeRecorded {
        outcome: crate::vault::Outcome,
    },
    OutcomeRework {
        rework: bool,
    },
    RoutingPreference {
        task_type: String,
        harness: Option<String>,
        model: Option<String>,
        enabled: bool,
    },
    PreviewViewportChanged {
        width: u32,
        height: u32,
    },
    PreviewCaptured {
        id: String,
        route: String,
        width: u32,
        height: u32,
    },
    WorkflowRequested,
    Plan {
        event: crate::plan::PlanEvent,
    },
    PlanChild {
        root: ThreadId,
        node: Option<String>,
        #[serde(default)]
        repair: bool,
    },
    ToolProfileChanged {
        profile: crate::ToolProfile,
    },
    IntakeAssessed {
        assessment: crate::Assessment,
    },
    QuestionsAsked {
        id: String,
        questions: Vec<crate::Question>,
        request: String,
        context_paths: Vec<String>,
        attachments: Vec<String>,
    },
    QuestionsAnswered {
        id: String,
        answers: Vec<crate::Answer>,
    },
    IntentReady {
        spec: crate::IntentSpec,
    },
    ApprovalRequested {
        run_id: RunId,
        request_id: String,
        tool: String,
        input: serde_json::Value,
    },
    ApprovalResolved {
        run_id: RunId,
        request_id: String,
        allowed: bool,
        reason: String,
    },
    ThreadCreated {
        project_id: ProjectId,
        title: String,
        harness: String,
        worktree: Option<String>,
        branch: Option<String>,
        /// Set when this thread was forked from another at a given seq.
        parent: Option<(ThreadId, i64)>,
        #[serde(default)]
        permission: PermissionMode,
        /// Harness-specific model name; `None` uses the harness default.
        #[serde(default)]
        model: Option<String>,
        /// Reasoning effort (harness-specific values); `None` uses the default.
        #[serde(default)]
        effort: Option<String>,
        /// Commit the thread's work is measured against (worktree creation point).
        #[serde(default)]
        base: Option<String>,
    },
    /// New run settings. `harness` may change only until the first session
    /// exists; everything else applies from the next turn. `reason` explains
    /// automatic changes (routing, recovery).
    ConfigChanged {
        harness: String,
        model: Option<String>,
        effort: Option<String>,
        permission: PermissionMode,
        #[serde(default)]
        reason: Option<String>,
    },
    Renamed {
        title: String,
    },
    UserMessage {
        text: String,
        /// Files sent with the message (stored once, referenced by path).
        #[serde(default)]
        attachments: Vec<AttachmentRef>,
    },
    RunStarted {
        run_id: RunId,
        session_id: Option<String>,
    },
    /// The harness reported its own session/thread id, used to resume later.
    Session {
        run_id: RunId,
        session_id: String,
    },
    Agent {
        run_id: RunId,
        event: AgentEvent,
    },
    RunEnded {
        run_id: RunId,
        exit_code: Option<i32>,
    },
    StatusChanged {
        status: ThreadStatus,
    },
    Heal {
        attempt: u32,
        signature: String,
        excerpt: String,
    },
    /// Something Arbiter itself wants the user to know (recovery, crashes).
    Notice {
        text: String,
    },
    /// Project checks run after an agent turn (attempt 0 = first check, before any heal).
    ChecksRan {
        attempt: u32,
        results: Vec<CheckResult>,
    },
    /// Snapshot of the working files after turn `n` (a commit under refs/arbiter/cp).
    Checkpoint {
        n: u32,
        commit: String,
    },
    /// Worktree files were restored to checkpoint `n`.
    Reverted {
        n: u32,
    },
    /// The provider refused work; Arbiter will retry at `resets_at` (unix seconds).
    RateLimited {
        resets_at: Option<i64>,
        message: String,
    },
    /// Spending cap for the thread (None = no cap).
    BudgetSet {
        usd: Option<f64>,
    },
    /// Hidden from the Inbox until the thread's status changes again.
    Settled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttachmentRef {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub ok: bool,
    pub duration_ms: u64,
    pub failures: u32,
    #[serde(default)]
    pub timed_out: bool,
    /// A deterministic fixer ran before this result.
    #[serde(default)]
    pub fixed: bool,
}

impl EventKind {
    /// The serde tag, stored in its own column so the log can be filtered without parsing payloads.
    pub fn tag(&self) -> &'static str {
        match self {
            Self::SideChat { .. } => "side_chat",
            Self::ApproachAsked => "approach_asked",
            Self::PreviewReady { .. } => "preview_ready",
            Self::ShareRequested { .. } => "share_requested",
            Self::ProjectsAttached { .. } => "projects_attached",
            Self::Published { .. } => "published",
            Self::WorkCleaned { .. } => "work_cleaned",
            Self::ShareDecided { .. } => "share_decided",
            Self::ComparisonJoined { .. } => "comparison_joined",
            Self::ComparisonDecided { .. } => "comparison_decided",
            Self::SecondOpinion { .. } => "second_opinion",
            Self::VaultSaved { .. } => "vault_saved",
            Self::VaultProposed { .. } => "vault_proposed",
            Self::VaultResolved { .. } => "vault_resolved",
            Self::MemoryUsed { .. } => "memory_used",
            Self::OutcomeRecorded { .. } => "outcome_recorded",
            Self::OutcomeRework { .. } => "outcome_rework",
            Self::RoutingPreference { .. } => "routing_preference",
            Self::PreviewViewportChanged { .. } => "preview_viewport_changed",
            Self::PreviewCaptured { .. } => "preview_captured",
            Self::WorkflowRequested => "workflow_requested",
            Self::Plan { .. } => "plan",
            Self::PlanChild { .. } => "plan_child",
            Self::ToolProfileChanged { .. } => "tool_profile_changed",
            Self::IntakeAssessed { .. } => "intake_assessed",
            Self::QuestionsAsked { .. } => "questions_asked",
            Self::QuestionsAnswered { .. } => "questions_answered",
            Self::IntentReady { .. } => "intent_ready",
            Self::ApprovalRequested { .. } => "approval_requested",
            Self::ApprovalResolved { .. } => "approval_resolved",
            Self::ThreadCreated { .. } => "thread_created",
            Self::ConfigChanged { .. } => "config_changed",
            Self::Renamed { .. } => "renamed",
            Self::UserMessage { .. } => "user_message",
            Self::RunStarted { .. } => "run_started",
            Self::Session { .. } => "session",
            Self::Agent { .. } => "agent",
            Self::RunEnded { .. } => "run_ended",
            Self::StatusChanged { .. } => "status_changed",
            Self::Heal { .. } => "heal",
            Self::Notice { .. } => "notice",
            Self::ChecksRan { .. } => "checks_ran",
            Self::Checkpoint { .. } => "checkpoint",
            Self::Reverted { .. } => "reverted",
            Self::RateLimited { .. } => "rate_limited",
            Self::BudgetSet { .. } => "budget_set",
            Self::Settled => "settled",
        }
    }
}

/// Harness-agnostic view of what an agent did; adapters map their native
/// stream formats onto this.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Message { text: String },
    ToolCall { id: String, name: String, input: serde_json::Value },
    ToolResult { id: String, output: String, is_error: bool },
    Usage(Usage),
    Error { message: String },
    Done,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost_usd: f64,
}

impl std::ops::AddAssign for Usage {
    fn add_assign(&mut self, o: Self) {
        self.input_tokens += o.input_tokens;
        self.output_tokens += o.output_tokens;
        self.cache_read_tokens += o.cache_read_tokens;
        self.cost_usd += o.cost_usd;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    #[default]
    Idle,
    Running,
    Healing,
    NeedsApproval,
    Review,
    Failed,
    Merged,
}

/// How much an agent may do without asking. Mapped per harness by the adapters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Read and edit files in the worktree; anything else is denied.
    #[default]
    Safe,
    /// Everything allowed (the worktree is the blast radius).
    Auto,
    /// Read-only: plan, don't change anything.
    Plan,
}

impl PermissionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Auto => "auto",
            Self::Plan => "plan",
        }
    }
}

impl std::str::FromStr for PermissionMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "safe" => Ok(Self::Safe),
            "auto" => Ok(Self::Auto),
            "plan" => Ok(Self::Plan),
            _ => Err(format!("unknown permission mode {s:?} (expected safe, auto or plan)")),
        }
    }
}

impl ThreadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Healing => "healing",
            Self::NeedsApproval => "needs_approval",
            Self::Review => "review",
            Self::Failed => "failed",
            Self::Merged => "merged",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_kind_roundtrips_with_stable_tag() {
        let k = EventKind::Agent {
            run_id: RunId::new(),
            event: AgentEvent::Usage(Usage { input_tokens: 10, ..Default::default() }),
        };
        let json = serde_json::to_value(&k).unwrap();
        assert_eq!(json["type"], "agent");
        assert_eq!(json["event"]["type"], "usage");
        assert_eq!(json["type"], k.tag());
        assert_eq!(serde_json::from_value::<EventKind>(json).unwrap(), k);
    }

    #[test]
    fn status_as_str_matches_serde() {
        for s in [ThreadStatus::Idle, ThreadStatus::NeedsApproval, ThreadStatus::Merged] {
            assert_eq!(serde_json::to_value(s).unwrap(), s.as_str());
        }
    }
}
