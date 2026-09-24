//! Pure plan validation and event replay. Processes and Git belong to the daemon.
use arbiter_core::{TaskId, ThreadId, plan::*};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Error(pub String);
type Result<T> = std::result::Result<T, Error>;
fn require(ok: bool, message: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(Error(message.into())) }
}

pub fn validate(plan: &Plan) -> Result<()> {
    require(
        !plan.title.trim().is_empty() && plan.title.len() <= 160 && plan.goal.len() <= 3000,
        "plan title/goal exceeds bounds",
    )?;
    require(!plan.nodes.is_empty() && plan.nodes.len() <= 24, "a plan needs 1–24 steps")?;
    require((1..=6).contains(&plan.concurrency), "concurrency must be 1–6")?;
    valid_budget(plan.budget_usd)?;
    require(
        plan.projects.len() <= 4 && plan.projects.iter().all(|p| !p.is_empty() && p.len() <= 64),
        "a plan can use at most 4 other projects",
    )?;
    let mut ids = HashSet::new();
    for n in &plan.nodes {
        require(
            !n.id.is_empty()
                && n.id.len() <= 40
                && n.id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                && ids.insert(n.id.as_str()),
            "step ids must be unique short identifiers",
        )?;
        require(
            !n.title.trim().is_empty() && n.title.len() <= 160 && !n.goal.trim().is_empty() && n.goal.len() <= 2000,
            "step title/goal is missing or too long",
        )?;
        require(
            !n.scope.is_empty() && n.scope.len() <= 16 && n.may_read.len() <= 16,
            "declare 1–16 writable scope patterns",
        )?;
        for pattern in n.scope.iter().chain(&n.may_read) {
            validate_pattern(pattern)?;
        }
        require(n.non_goals.len() <= 8 && n.non_goals.iter().all(|s| s.len() <= 200), "non-goals exceed bounds")?;
        require(
            n.checks.len() <= 8
                && n.checks.iter().all(|s| !s.trim().is_empty() && s.len() <= 500 && !s.contains(['\n', '\r'])),
            "checks must be 1-line commands, at most 8",
        )?;
        require(["auto", "claude", "codex", "local"].contains(&n.harness.as_str()), "unknown harness")?;
        require(
            n.model.as_ref().is_none_or(|s| s.len() <= 120) && n.model_reason.len() <= 300,
            "model settings exceed bounds",
        )?;
        require(n.dependencies.len() <= 24, "too many dependencies")?;
        require(
            n.project.as_ref().is_none_or(|p| plan.projects.contains(p)),
            "a step's project must be one of the plan's projects",
        )?;
        valid_budget(n.budget_usd)?;
        require(n.token_budget.is_none_or(|v| v > 0), "token budget must be positive")?;
    }
    let mut done = HashSet::new();
    for n in &plan.nodes {
        require(n.dependencies.iter().all(|d| ids.contains(d.as_str()) && d != &n.id), "unknown or self dependency")?;
    }
    loop {
        let before = done.len();
        for n in &plan.nodes {
            if n.dependencies.iter().all(|d| done.contains(d.as_str())) {
                done.insert(n.id.as_str());
            }
        }
        if done.len() == plan.nodes.len() {
            break;
        }
        require(done.len() > before, "dependency graph contains a cycle")?;
    }
    Ok(())
}
fn valid_budget(v: Option<f64>) -> Result<()> {
    require(v.is_none_or(|v| v.is_finite() && v > 0.0 && v <= 10000.0), "budget must be a finite positive amount")
}
pub fn validate_pattern(p: &str) -> Result<()> {
    require(
        !p.is_empty()
            && p.len() <= 240
            && !p.starts_with('/')
            && !p.contains(['\\', ':', '\n', '\r', '[', ']', '{', '}'])
            && !p.split('/').any(|s| s == ".." || s == "."),
        "scope must use relative paths with * or ** wildcards",
    )
}
/// Component-aware matching: * stays within one path segment; ** crosses directories.
pub fn matches(pattern: &str, path: &str) -> bool {
    fn segment(p: &[u8], s: &[u8]) -> bool {
        let mut row = vec![false; s.len() + 1];
        row[0] = true;
        for &c in p {
            let mut next = vec![false; s.len() + 1];
            if c == b'*' {
                next[0] = row[0];
            }
            for j in 1..=s.len() {
                next[j] = if c == b'*' { row[j] || next[j - 1] } else { row[j - 1] && (c == b'?' || c == s[j - 1]) };
            }
            row = next;
        }
        row[s.len()]
    }
    let p: Vec<_> = pattern.split('/').collect();
    let s: Vec<_> = path.split('/').collect();
    let mut row = vec![false; s.len() + 1];
    row[0] = true;
    for part in p {
        let mut next = vec![false; s.len() + 1];
        if part == "**" {
            next[0] = row[0];
        }
        for j in 1..=s.len() {
            next[j] = if part == "**" {
                row[j] || next[j - 1]
            } else {
                row[j - 1] && segment(part.as_bytes(), s[j - 1].as_bytes())
            };
        }
        row = next;
    }
    row[s.len()]
}
pub fn drift(scope: &[String], paths: &[String]) -> Vec<String> {
    paths.iter().filter(|p| !scope.iter().any(|s| matches(s, p))).cloned().collect()
}
pub fn validate_handoff(h: &Handoff) -> Result<()> {
    require(!h.summary.trim().is_empty() && h.summary.len() <= 1200, "handoff needs a summary under 1200 bytes")?;
    for list in [&h.files, &h.decisions, &h.interfaces, &h.open_issues] {
        require(list.len() <= 16 && list.iter().all(|s| s.len() <= 250), "handoff list exceeds bounds")?;
    }
    require(serde_json::to_vec(h).map_err(|e| Error(e.to_string()))?.len() <= 5000, "handoff exceeds 5000 bytes")
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Node {
    pub status: NodeStatus,
    pub reason: String,
    pub thread: Option<ThreadId>,
    pub task: Option<TaskId>,
    pub base: Option<String>,
    pub brief: Option<String>,
    pub handoff: Option<Handoff>,
    pub commit: Option<String>,
    pub extra_scope: Vec<String>,
    pub repair: Option<ThreadId>,
    pub repair_scope: Vec<String>,
    pub repair_baseline: Option<String>,
    pub heal_signatures: Vec<String>,
}
/// The collection branch of one of the plan's other projects.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Integration {
    pub path: String,
    pub branch: String,
    pub base: String,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct State {
    pub attachments: Vec<String>,
    pub plan: Option<Plan>,
    pub revision: u32,
    pub source: String,
    pub approved: bool,
    pub path: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub task: Option<TaskId>,
    pub nodes: BTreeMap<String, Node>,
    pub paused: Option<String>,
    pub completed: Option<String>,
    pub planner: Option<ThreadId>,
    /// Collection branches of the plan's other projects, by project id.
    pub integrations: BTreeMap<String, Integration>,
}
impl State {
    /// Worktree path and branch that collect `project`'s steps (none: the
    /// task's own project).
    pub fn integration(&self, project: Option<&str>) -> Option<(&str, &str)> {
        match project {
            None => Some((self.path.as_deref()?, self.branch.as_deref()?)),
            Some(p) => self.integrations.get(p).map(|i| (i.path.as_str(), i.branch.as_str())),
        }
    }
    /// Projects a plan touches besides the task's own.
    pub fn other_projects(&self) -> Vec<String> {
        let Some(plan) = &self.plan else { return vec![] };
        let mut out = plan.projects.clone();
        for p in plan.nodes.iter().filter_map(|n| n.project.clone()) {
            if !out.contains(&p) {
                out.push(p);
            }
        }
        out
    }
    pub fn apply(&mut self, e: &PlanEvent) {
        match e {
            PlanEvent::ReviewRequested { node, revision } => {
                if let Some(plan) = &mut self.plan
                    && !plan.nodes.iter().any(|n| n.id == node.id)
                {
                    plan.nodes.push(node.clone());
                }
                self.nodes.entry(node.id.clone()).or_default();
                self.revision = *revision;
                self.approved = false;
                self.completed = None;
                self.paused = None;
                self.source = "Review comments".into();
            }
            PlanEvent::Context { attachments } => self.attachments = attachments.clone(),
            PlanEvent::PlannerFinished => self.planner = None,
            PlanEvent::Proposed { plan, revision, source } => {
                self.plan = Some(plan.clone());
                self.revision = *revision;
                self.source = source.clone();
                self.approved = false;
                self.nodes = plan.nodes.iter().map(|n| (n.id.clone(), Node::default())).collect();
                self.paused = None;
                self.planner = None;
            }
            PlanEvent::PlannerStarted { thread } => self.planner = Some(*thread),
            PlanEvent::Approved { revision } if *revision == self.revision => {
                self.approved = true;
                self.paused = None;
            }
            PlanEvent::Prepared { path, branch, base, project: Some(project), .. } => {
                self.integrations.insert(
                    project.clone(),
                    Integration { path: path.clone(), branch: branch.clone(), base: base.clone() },
                );
            }
            PlanEvent::Prepared { path, branch, base, task, project: None } => {
                self.path = Some(path.clone());
                self.branch = Some(branch.clone());
                self.base = Some(base.clone());
                self.task = Some(*task);
            }
            PlanEvent::NodePrepared { node, thread, task, base } => {
                let n = self.nodes.entry(node.clone()).or_default();
                n.thread = Some(*thread);
                n.task = Some(*task);
                n.base = Some(base.clone());
            }
            PlanEvent::NodeState { node, status, reason } => {
                let n = self.nodes.entry(node.clone()).or_default();
                n.status = *status;
                n.reason = reason.clone();
            }
            PlanEvent::BriefCompiled { node, text } => {
                self.nodes.entry(node.clone()).or_default().brief = Some(text.clone())
            }
            PlanEvent::HandedOff { node, handoff } => {
                self.nodes.entry(node.clone()).or_default().handoff = Some(handoff.clone())
            }
            PlanEvent::Integrated { node, commit } => {
                let n = self.nodes.entry(node.clone()).or_default();
                n.status = NodeStatus::Merged;
                n.commit = Some(commit.clone());
                n.reason = "Integrated into plan branch".into();
            }
            PlanEvent::ScopeApproved { node, paths } => {
                let n = self.nodes.entry(node.clone()).or_default();
                for p in paths {
                    if !n.extra_scope.contains(p) {
                        n.extra_scope.push(p.clone());
                    }
                }
            }
            PlanEvent::RepairStarted { node, thread } => {
                self.nodes.entry(node.clone()).or_default().repair = Some(*thread)
            }
            PlanEvent::RepairScope { node, paths, baseline } => {
                let n = self.nodes.entry(node.clone()).or_default();
                n.repair_scope = paths.clone();
                n.repair_baseline = Some(baseline.clone());
            }
            PlanEvent::NodeHealed { node, signature } => {
                let n = self.nodes.entry(node.clone()).or_default();
                n.heal_signatures.push(signature.clone());
                n.handoff = None;
            }
            PlanEvent::BudgetApproved { node, usd, tokens } => {
                if let Some(plan) = &mut self.plan {
                    if let Some(id) = node {
                        if let Some(n) = plan.nodes.iter_mut().find(|n| &n.id == id) {
                            n.budget_usd = *usd;
                            n.token_budget = *tokens;
                        }
                    } else {
                        plan.budget_usd = *usd;
                    }
                }
            }
            PlanEvent::Paused { reason } => self.paused = Some(reason.clone()),
            PlanEvent::Resumed => {
                self.paused = None;
                for n in self.nodes.values_mut() {
                    if n.status == NodeStatus::Blocked {
                        n.status = NodeStatus::Queued;
                        n.reason = "Resume requested".into();
                    }
                }
            }
            PlanEvent::Completed { commit } => self.completed = Some(commit.clone()),
            _ => {}
        }
    }
    pub fn ready(&self, id: &str) -> bool {
        self.plan.as_ref().and_then(|p| p.nodes.iter().find(|n| n.id == id)).is_some_and(|n| {
            n.dependencies.iter().all(|d| self.nodes.get(d).is_some_and(|n| n.status == NodeStatus::Merged))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_is_component_aware() {
        assert!(matches("src/**/*.rs", "src/lib.rs"));
        assert!(matches("src/**/*.rs", "src/a/b.rs"));
        assert!(!matches("src/*.rs", "src/a/b.rs"));
        assert!(!matches("src/**", "secrets/key"));
        assert!(validate_pattern("../outside").is_err());
    }
    #[test]
    fn cyclic_plans_are_rejected() {
        let raw = r#"{"title":"demo","goal":"g","nodes":[{"id":"a","title":"A","goal":"A","scope":["src/**"],"dependencies":["b"]},{"id":"b","title":"B","goal":"B","scope":["test/**"],"dependencies":["a"]}]}"#;
        let mut plan: Plan = serde_json::from_str(raw).unwrap();
        assert!(validate(&plan).is_err());
        plan.nodes[0].dependencies.clear();
        assert!(validate(&plan).is_ok());
        let mut state = State::default();
        state.apply(&PlanEvent::Proposed { plan, revision: 1, source: "test".into() });
        assert!(state.ready("a"));
        assert!(!state.ready("b"));
        state.apply(&PlanEvent::Integrated { node: "a".into(), commit: "sha".into() });
        assert!(state.ready("b"));
    }
    #[test]
    fn steps_can_target_other_projects() {
        let raw = r#"{"title":"demo","goal":"g","projects":["p2"],"nodes":[{"id":"a","title":"A","goal":"A","scope":["**"],"project":"p2"},{"id":"b","title":"B","goal":"B","scope":["src/**"],"dependencies":["a"]}]}"#;
        let mut plan: Plan = serde_json::from_str(raw).unwrap();
        assert!(validate(&plan).is_ok());
        plan.nodes[0].project = Some("elsewhere".into());
        assert!(validate(&plan).is_err());
        plan.nodes[0].project = Some("p2".into());
        let mut state = State::default();
        state.apply(&PlanEvent::Proposed { plan, revision: 1, source: "test".into() });
        assert_eq!(state.other_projects(), vec!["p2".to_owned()]);
        let task = TaskId::new();
        let prepared = |project: Option<&str>, path: &str| PlanEvent::Prepared {
            path: path.into(),
            branch: format!("b-{path}"),
            base: "base".into(),
            task,
            project: project.map(str::to_owned),
        };
        state.apply(&prepared(None, "main"));
        state.apply(&prepared(Some("p2"), "other"));
        assert_eq!(state.integration(None), Some(("main", "b-main")));
        assert_eq!(state.integration(Some("p2")), Some(("other", "b-other")));
        assert_eq!(state.task, Some(task));
        // Old logs have no project on Prepared.
        let old: PlanEvent = serde_json::from_str(
            r#"{"type":"prepared","path":"x","branch":"y","base":"z","task":"00000000-0000-0000-0000-000000000000"}"#,
        )
        .unwrap();
        assert!(matches!(old, PlanEvent::Prepared { project: None, .. }));
    }
}
