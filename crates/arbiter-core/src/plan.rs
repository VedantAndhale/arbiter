use crate::{TaskId, ThreadId, ToolProfile};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub title: String,
    pub goal: String,
    pub nodes: Vec<PlanNode>,
    #[serde(default = "concurrency")]
    pub concurrency: usize,
    #[serde(default)]
    pub budget_usd: Option<f64>,
    /// Other projects this plan may change, by id. The task's own project is
    /// always included and is not listed here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<String>,
}
fn concurrency() -> usize {
    3
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanNode {
    pub id: String,
    pub title: String,
    pub goal: String,
    pub scope: Vec<String>,
    #[serde(default)]
    pub may_read: Vec<String>,
    #[serde(default)]
    pub non_goals: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub checks: Vec<String>,
    #[serde(default = "auto")]
    pub harness: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub model_reason: String,
    #[serde(default)]
    pub tool_profile: ToolProfile,
    #[serde(default)]
    pub budget_usd: Option<f64>,
    #[serde(default)]
    pub token_budget: Option<u64>,
    /// The project this step changes (one of `Plan::projects`); none means
    /// the task's own project. Scope and checks are relative to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}
fn auto() -> String {
    "auto".into()
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Handoff {
    pub summary: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub interfaces: Vec<String>,
    #[serde(default)]
    pub open_issues: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    #[default]
    Queued,
    Running,
    Checking,
    Blocked,
    Merging,
    Merged,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlanEvent {
    ReviewRequested {
        node: PlanNode,
        revision: u32,
    },
    Context {
        attachments: Vec<String>,
    },
    PlannerFinished,
    Proposed {
        plan: Plan,
        revision: u32,
        source: String,
    },
    PlannerStarted {
        thread: ThreadId,
    },
    Approved {
        revision: u32,
    },
    /// A collection branch is ready. `project` is set for the plan's other
    /// projects; older logs only have the task's own project.
    Prepared {
        path: String,
        branch: String,
        base: String,
        task: TaskId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
    },
    NodePrepared {
        node: String,
        thread: ThreadId,
        task: TaskId,
        base: String,
    },
    NodeState {
        node: String,
        status: NodeStatus,
        reason: String,
    },
    BriefCompiled {
        node: String,
        text: String,
    },
    HandedOff {
        node: String,
        handoff: Handoff,
    },
    Integrated {
        node: String,
        commit: String,
    },
    ScopeApproved {
        node: String,
        paths: Vec<String>,
    },
    RepairStarted {
        node: String,
        thread: ThreadId,
    },
    RepairScope {
        node: String,
        paths: Vec<String>,
        baseline: String,
    },
    NodeHealed {
        node: String,
        signature: String,
    },
    BudgetApproved {
        node: Option<String>,
        usd: Option<f64>,
        tokens: Option<u64>,
    },
    Paused {
        reason: String,
    },
    Resumed,
    Completed {
        commit: String,
    },
}
