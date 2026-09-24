//! Durable plan coordinator. Replays the root thread log on each tick; Git and
//! child events reconcile work interrupted between external effects and appends.
use crate::{AppState, service::WS};
use anyhow::{Context, Result, ensure};
use arbiter_core::{EventKind, IntentSpec, PermissionMode, TaskId, TaskStatus, ThreadId, ThreadStatus, plan::*};
use arbiter_plan::{Node, State};
use arbiter_store::{NewTask, TaskPatch};
use arbiter_supervisor::{checkpoint, integration as git};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

struct ChildSpec<'a> {
    root: ThreadId,
    project: arbiter_core::ProjectId,
    node: Option<String>,
    repair: bool,
    path: &'a Path,
    branch: &'a str,
    permission: PermissionMode,
    title: &'a str,
    harness: &'a str,
    model: Option<String>,
}
fn short(id: ThreadId) -> String {
    id.0.simple().to_string()[20..].to_owned()
}
fn cut(text: &str, bytes: usize) -> String {
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].into()
}
fn json_object(text: &str) -> Result<serde_json::Value> {
    ensure!(text.len() <= 40000, "structured report exceeds 40 KB");
    let start = text.find('{').context("expected a JSON object")?;
    let end = text.rfind('}').context("incomplete JSON object")?;
    Ok(serde_json::from_str(&text[start..=end])?)
}

impl AppState {
    pub(crate) async fn request_review_fixes(
        &self,
        root: ThreadId,
        revision: u32,
        comments: Vec<crate::landing::ReviewComment>,
        project: Option<String>,
    ) -> Result<()> {
        let _lock = self.inner.plan_lock.lock().await;
        let state = self.plan_state(root)?;
        ensure!(
            state.completed.is_some() && state.revision == revision,
            "finish the plan and refresh before requesting review fixes"
        );
        let mut plan = state.plan.context("plan missing")?;
        crate::landing::validate_comments(&comments)?;
        let mut scope = vec![];
        let mut goal = String::from("Address these review comments; preserve accepted behavior:\n");
        for c in comments {
            if !scope.contains(&c.path) {
                scope.push(c.path.clone());
            }
            goal.push_str(&format!("{}:{}: {}\n", c.path, c.line, c.text));
        }
        ensure!(project.as_ref().is_none_or(|p| plan.projects.contains(p)), "that project is not part of this plan");
        let previous =
            plan.nodes.iter().rev().find(|n| n.project == project).or(plan.nodes.last()).context("empty plan")?;
        let node = PlanNode {
            id: format!("review-{}", revision + 1),
            title: "Address review comments".into(),
            goal,
            scope,
            may_read: vec!["**".into()],
            non_goals: vec!["Do not redo accepted work".into()],
            dependencies: plan.nodes.iter().map(|n| n.id.clone()).collect(),
            checks: previous.checks.clone(),
            harness: previous.harness.clone(),
            model: previous.model.clone(),
            model_reason: "Continue with the reviewed agent assignment".into(),
            tool_profile: previous.tool_profile,
            budget_usd: None,
            token_budget: None,
            project,
        };
        plan.nodes.push(node.clone());
        arbiter_plan::validate(&plan)?;
        self.plan_event(root, PlanEvent::ReviewRequested { node, revision: revision + 1 })?;
        self.append(root, EventKind::StatusChanged { status: ThreadStatus::NeedsApproval })?;
        Ok(())
    }
    /// Projects attached to a task besides its own.
    pub(crate) fn attached_projects(&self, root: ThreadId) -> Result<Vec<arbiter_core::ProjectId>> {
        Ok(self
            .store(|st| st.events(root, 0))?
            .into_iter()
            .rev()
            .find_map(|e| match e.kind {
                EventKind::ProjectsAttached { projects } => Some(projects),
                _ => None,
            })
            .unwrap_or_default())
    }
    /// The project a step changes: its own, or the task's.
    fn step_project(
        &self,
        root: ThreadId,
        project: Option<&str>,
    ) -> Result<(arbiter_core::ProjectId, arbiter_store::Project)> {
        let id = match project {
            Some(p) => p.parse().context("a step names an unknown project")?,
            None => self.store(|st| st.thread(root))?.context("root missing")?.project_id,
        };
        Ok((id, self.store(|st| st.project(id)).context("a step's project no longer exists")?))
    }
    /// A collection worktree must only change through merges. Agents may read
    /// other projects' collection worktrees, so refuse to go on if one of them
    /// was edited directly.
    async fn ensure_untouched(&self, path: &Path, project: &str) -> Result<()> {
        let status = git::git(path, &["status", "--porcelain"]).await?;
        ensure!(
            status.trim().is_empty(),
            "Files in {project}'s plan copy were changed directly, outside a step. Review {} before resuming.",
            path.display()
        );
        Ok(())
    }
    /// Read-only folders an agent of this plan may look at: for the planner,
    /// the other projects; for a step, the collection worktrees of the other
    /// projects it depends on.
    pub(crate) fn plan_read_dirs(&self, thread: ThreadId) -> Vec<PathBuf> {
        let Some((root, node, repair)) = self.plan_child(thread) else { return vec![] };
        let Ok(state) = self.plan_state(root) else { return vec![] };
        if repair {
            return vec![];
        }
        let Some(node) = node else {
            return self
                .attached_projects(root)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|p| self.store(|st| st.project(p)).ok())
                .map(|p| PathBuf::from(p.path))
                .collect();
        };
        let Some(plan) = &state.plan else { return vec![] };
        let Some(spec) = plan.nodes.iter().find(|n| n.id == node) else { return vec![] };
        let mut dirs = vec![];
        for dep in plan.nodes.iter().filter(|n| spec.dependencies.contains(&n.id) && n.project != spec.project) {
            if let Some((path, _)) = state.integration(dep.project.as_deref()) {
                let path = PathBuf::from(path);
                if !dirs.contains(&path) {
                    dirs.push(path);
                }
            }
        }
        dirs
    }
    pub(crate) fn plan_state(&self, root: ThreadId) -> Result<State> {
        self.store(|st| st.thread(root))?.context("task not found")?;
        let mut state = State::default();
        for e in self.store(|st| st.events(root, 0))? {
            if let EventKind::Plan { event } = e.kind {
                state.apply(&event);
            }
        }
        Ok(state)
    }
    fn plan_event(&self, root: ThreadId, event: PlanEvent) -> Result<()> {
        self.append(root, EventKind::Plan { event }).map(drop)
    }
    fn node_state(&self, root: ThreadId, id: &str, status: NodeStatus, reason: &str) -> Result<()> {
        let state = self.plan_state(root)?;
        if state.nodes.get(id).is_none_or(|n| n.status != status || n.reason != reason) {
            self.plan_event(root, PlanEvent::NodeState { node: id.into(), status, reason: cut(reason, 1000) })?;
        }
        Ok(())
    }
    fn pause_plan(&self, root: ThreadId, reason: &str) -> Result<()> {
        if self.plan_state(root)?.paused.is_none() {
            self.plan_event(root, PlanEvent::Paused { reason: cut(reason, 1200) })?;
        }
        for node in self.plan_state(root)?.nodes.values() {
            if let Some(thread) = node.thread {
                let _ = self.stop(thread);
            }
            if let Some(thread) = node.repair {
                let _ = self.stop(thread);
            }
        }
        self.append(root, EventKind::StatusChanged { status: ThreadStatus::NeedsApproval })?;
        Ok(())
    }
    pub(crate) fn plan_child(&self, thread: ThreadId) -> Option<(ThreadId, Option<String>, bool)> {
        self.store(|st| st.events(thread, 0)).ok()?.into_iter().find_map(|e| match e.kind {
            EventKind::PlanChild { root, node, repair } => Some((root, node, repair)),
            _ => None,
        })
    }
    pub(crate) async fn draft_plan(&self, root: ThreadId, intent: &IntentSpec, attachments: &[String]) -> Result<()> {
        let root_thread = self.store(|st| st.thread(root))?.context("root missing")?;
        let project = self.store(|st| st.project(root_thread.project_id))?;
        let request = crate::intake::validate_paths(&PathBuf::from(project.path), &intent.context_paths)?;
        let full_goal = arbiter_intake::questions::brief(intent);
        let goal = if full_goal.len() <= 2000 {
            full_goal.clone()
        } else {
            "Read the complete approved intent and produce an implementation plan before execution.".into()
        };
        let others = self.attached_projects(root)?;
        let mut plan = Plan {
            title: arbiter_core::derive_title(&intent.request),
            goal: cut(&goal, 700),
            concurrency: 3,
            budget_usd: None,
            projects: others.iter().map(|p| p.to_string()).collect(),
            nodes: vec![PlanNode {
                id: "implement".into(),
                title: "Implement and verify the request".into(),
                goal,
                scope: if request.is_empty() { vec!["**".into()] } else { request.clone() },
                may_read: vec!["**".into()],
                non_goals: vec!["No publishing or unrelated changes".into()],
                dependencies: vec![],
                checks: vec![],
                harness: root_thread.harness.clone(),
                model: root_thread.model.clone(),
                model_reason: "Uses your task's agent selection; override before approving".into(),
                tool_profile: arbiter_core::ToolProfile::Implementation,
                budget_usd: None,
                token_budget: None,
                project: None,
            }],
        };
        // Several projects: a starting step per project; the planner orders them.
        if !others.is_empty() {
            let first = plan.nodes[0].clone();
            plan.nodes[0].title = format!("Changes in {}", project.name);
            plan.nodes[0].scope = vec!["**".into()];
            for (i, p) in others.iter().enumerate() {
                let name = self.store(|st| st.project(*p)).map(|p| p.name).unwrap_or_default();
                plan.nodes.push(PlanNode {
                    id: format!("implement-{}", i + 2),
                    title: cut(&format!("Changes in {name}"), 160),
                    scope: vec!["**".into()],
                    project: Some(p.to_string()),
                    ..first.clone()
                });
            }
        }
        self.plan_event(root, PlanEvent::Context { attachments: attachments.to_vec() })?;
        self.propose_plan(root, plan, "Local template; review scope and add acceptance checks")?;
        if intent.needs_frontier || full_goal.len() > 2000 || !others.is_empty() {
            self.start_planner(root).await?;
        }
        Ok(())
    }
    pub(crate) fn propose_plan(&self, root: ThreadId, plan: Plan, source: &str) -> Result<()> {
        arbiter_plan::validate(&plan)?;
        let attached: Vec<String> = self.attached_projects(root)?.iter().map(|p| p.to_string()).collect();
        ensure!(
            plan.projects.iter().all(|p| attached.contains(p)),
            "a plan can only use the projects attached to its task"
        );
        // Reserve the maximum convention prefix before accepting an editable draft.
        for node in &plan.nodes {
            arbiter_brief::compile(&"x".repeat(2400), &plan.goal, node, &[])?;
        }
        let current = self.plan_state(root)?;
        ensure!(!current.approved, "approved plans cannot be rewritten; pause and resolve individual steps");
        self.plan_event(root, PlanEvent::Proposed { plan, revision: current.revision + 1, source: source.into() })?;
        self.append(root, EventKind::StatusChanged { status: ThreadStatus::NeedsApproval })?;
        Ok(())
    }
    pub(crate) async fn start_planner(&self, root: ThreadId) -> Result<()> {
        let state = self.plan_state(root)?;
        ensure!(!state.approved && state.planner.is_none(), "planning is already active or approved");
        let plan = state.plan.context("draft a plan first")?;
        let t = self.store(|st| st.thread(root))?.context("task missing")?;
        let project = self.store(|st| st.project(t.project_id))?;
        let repo = Path::new(&project.path);
        let key = format!("r-{}", short(root));
        let branch = format!("arbiter/planner/{}", short(root));
        let path = git::ensure_worktree(repo, &self.inner.home.join("wt"), &key, &branch, "HEAD").await?;
        let child = self
            .new_plan_child(ChildSpec {
                root,
                project: t.project_id,
                node: None,
                repair: false,
                path: &path,
                branch: &branch,
                permission: PermissionMode::Plan,
                title: "Plan the work",
                harness: &t.harness,
                model: t.model.clone(),
            })
            .await?;
        self.plan_event(root, PlanEvent::PlannerStarted { thread: child })?;
        self.append(root, EventKind::StatusChanged { status: ThreadStatus::Running })?;
        let intent = self
            .store(|st| st.events(root, 0))?
            .into_iter()
            .rev()
            .find_map(|e| match e.kind {
                EventKind::IntentReady { spec, .. } => Some(arbiter_intake::questions::brief(&spec)),
                _ => None,
            })
            .unwrap_or_default();
        ensure!(intent.len() <= 16000, "intent is too large for planning; shorten the request");
        let others = self.attached_projects(root)?;
        let projects = if others.is_empty() {
            String::new()
        } else {
            let mut s = format!(
                "\nThis work spans several projects. The main project, {}, is this worktree; nodes for it omit `project`. Other projects (read-only for you):\n",
                project.name
            );
            for p in &others {
                if let Ok(o) = self.store(|st| st.project(*p)) {
                    s.push_str(&format!("- id {p}: {} at {}\n", o.name, o.path));
                }
            }
            s.push_str("Each node changes exactly one project: set its `project` to that id. Scope and checks are relative to that project. Keep \"projects\" in the plan as given. When a step needs another project's work (for example an API or a data format), make it depend on that step and ask the earlier step to describe the interface in its handoff.\n");
            s
        };
        let prompt = format!(
            "Complete approved intent:\n{intent}\nYou are a read-only planner. Inspect the repository without editing it. Split this request into at most 12 concrete steps with dependencies. Return ONLY JSON matching the supplied Plan example. Each node needs precise relative writable scope patterns, non-goals, runnable acceptance commands, a harness (auto/claude/codex), model or null, and a short model_reason. No deployment. Unknown budgets remain null. The user will edit and approve before any implementation.{projects}\nRequest and initial draft:\n{}",
            serde_json::to_string(&plan)?
        );
        if let Err(e) = self.send_message(child, prompt, &state.attachments).await {
            self.plan_event(root, PlanEvent::PlannerFinished)?;
            self.pause_plan(root, &format!("Planner could not start: {e:#}. Edit the local draft or retry planning."))?;
        }
        Ok(())
    }
    async fn new_plan_child(&self, spec: ChildSpec<'_>) -> Result<ThreadId> {
        let ChildSpec { root, project, node, repair, path, branch, permission, title, harness, model } = spec;
        self.store(|st| st.thread(root))?.context("root missing")?;
        let id = ThreadId::new();
        self.append(
            id,
            EventKind::ThreadCreated {
                project_id: project,
                title: title.into(),
                harness: harness.into(),
                worktree: Some(path.to_string_lossy().into()),
                branch: Some(branch.into()),
                parent: None,
                permission,
                model,
                effort: None,
                base: Some(checkpoint::head(path).await?),
            },
        )?;
        self.append(id, EventKind::PlanChild { root, node, repair })?;
        Ok(id)
    }
    pub(crate) async fn approve_plan(&self, root: ThreadId, revision: u32) -> Result<()> {
        let _lock = self.inner.plan_lock.lock().await;
        let s = self.plan_state(root)?;
        ensure!(
            !s.approved && s.revision == revision,
            "plan changed or was already approved; review the latest revision"
        );
        ensure!(s.planner.is_none(), "wait for the planner before approving");
        let p = s.plan.context("no plan proposed")?;
        arbiter_plan::validate(&p)?;
        self.review_milestone(root, "planning").await;
        self.plan_event(root, PlanEvent::Approved { revision })?;
        self.append(root, EventKind::StatusChanged { status: ThreadStatus::Running })?;
        Ok(())
    }
    pub(crate) async fn control_plan(&self, root: ThreadId, action: &str) -> Result<()> {
        if action == "pause" {
            let state = self.plan_state(root)?;
            ensure!(state.approved && state.completed.is_none(), "plan is not executing");
            return self.pause_plan(root, "Paused by you");
        }
        let _lock = self.inner.plan_lock.lock().await;
        let s = self.plan_state(root)?;
        ensure!(s.approved && s.completed.is_none(), "plan is not executing");
        match action {
            "pause" => {
                self.pause_plan(root, "Paused by you")?;
                for n in s.nodes.values() {
                    if let Some(t) = n.thread {
                        let _ = self.stop(t);
                    }
                    if let Some(t) = n.repair {
                        let _ = self.stop(t);
                    }
                }
            }
            "resume" => {
                self.plan_event(root, PlanEvent::Resumed)?;
                self.append(root, EventKind::StatusChanged { status: ThreadStatus::Running })?;
            }
            _ => anyhow::bail!("unknown plan action"),
        }
        Ok(())
    }
    pub(crate) async fn approve_scope(&self, root: ThreadId, node: &str, paths: Vec<String>) -> Result<()> {
        let _lock = self.inner.plan_lock.lock().await;
        let s = self.plan_state(root)?;
        ensure!(s.approved && s.nodes.contains_key(node) && paths.len() <= 16, "invalid scope request");
        for path in &paths {
            arbiter_plan::validate_pattern(path)?;
        }
        self.plan_event(root, PlanEvent::ScopeApproved { node: node.into(), paths })?;
        Ok(())
    }
    pub(crate) async fn approve_plan_budget(
        &self,
        root: ThreadId,
        node: Option<String>,
        usd: Option<f64>,
        tokens: Option<u64>,
    ) -> Result<()> {
        let _lock = self.inner.plan_lock.lock().await;
        let state = self.plan_state(root)?;
        ensure!(state.approved && state.paused.is_some(), "pause the plan before changing budgets");
        let mut plan = state.plan.context("plan missing")?;
        if let Some(id) = &node {
            let spec = plan.nodes.iter_mut().find(|n| &n.id == id).context("step missing")?;
            spec.budget_usd = usd;
            spec.token_budget = tokens;
        } else {
            plan.budget_usd = usd;
        }
        arbiter_plan::validate(&plan)?;
        self.plan_event(root, PlanEvent::BudgetApproved { node: node.clone(), usd, tokens })?;
        if let Some(thread) = node.and_then(|id| state.nodes.get(&id).and_then(|n| n.thread)) {
            self.append(thread, EventKind::BudgetSet { usd })?;
        }
        Ok(())
    }
    pub(crate) fn report_handoff(&self, thread: ThreadId, handoff: Handoff) -> Result<()> {
        arbiter_plan::validate_handoff(&handoff)?;
        let (root, node, repair) = self.plan_child(thread).context("this task is not a plan step")?;
        ensure!(!repair, "merge repair reports through its final response");
        let node = node.context("planner cannot report a step handoff")?;
        self.plan_event(root, PlanEvent::HandedOff { node, handoff })
    }
    pub(crate) fn request_plan_scope(&self, thread: ThreadId, reason: &str) -> Result<()> {
        ensure!(!reason.trim().is_empty() && reason.len() <= 1200, "scope reason exceeds bounds");
        let (root, node, _) = self.plan_child(thread).context("not a plan step")?;
        if let Some(n) = node {
            self.node_state(root, &n, NodeStatus::Blocked, reason)?;
        }
        self.pause_plan(root, reason)
    }
    pub(crate) fn spawn_plan_engine(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(Duration::from_secs(1));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                timer.tick().await;
                let _lock = state.inner.plan_lock.lock().await;
                let threads = state.store(|st| st.threads(WS)).unwrap_or_default();
                for t in threads {
                    let Ok(s) = state.plan_state(t.id) else { continue };
                    if s.plan.is_none() || s.completed.is_some() || s.paused.is_some() {
                        continue;
                    }
                    if let Err(e) = state.tick_plan(t.id, s).await {
                        let _ = state.pause_plan(t.id, &format!("Plan paused: {e:#}"));
                    }
                }
            }
        });
    }
    async fn tick_plan(&self, root: ThreadId, mut state: State) -> Result<()> {
        if let Some(planner) = state.planner {
            let t = self.store(|st| st.thread(planner))?.context("planner missing")?;
            if t.status == ThreadStatus::Review {
                let result = self
                    .final_object(planner)
                    .and_then(|v| Ok(serde_json::from_value::<Plan>(v.get("plan").cloned().unwrap_or(v))?));
                let _ = self.stop(planner);
                self.plan_event(root, PlanEvent::PlannerFinished)?;
                match result.and_then(|p| self.propose_plan(root, p, "Read-only planner")) {
                    Ok(()) => {}
                    Err(e) => self.pause_plan(
                        root,
                        &format!("Planner output needs correction: {e:#}. Edit the draft or retry."),
                    )?,
                }
            } else if t.status == ThreadStatus::Failed {
                self.plan_event(root, PlanEvent::PlannerFinished)?;
                self.pause_plan(root, "Planner failed. Inspect its activity, edit the draft or retry.")?;
            } else if t.status == ThreadStatus::Idle && !self.run_is_active(planner) {
                self.send_message(planner, "Continue read-only planning. Return the complete plan JSON.".into(), &[])
                    .await?;
            }
            return Ok(());
        }
        if !state.approved {
            return Ok(());
        }
        if state.path.is_none() {
            self.prepare_plan(root, &state).await?;
            state = self.plan_state(root)?;
        }
        for (i, project) in state.other_projects().into_iter().enumerate() {
            if !state.integrations.contains_key(&project) {
                self.prepare_other(root, &state, i, &project).await?;
                state = self.plan_state(root)?;
            }
        }
        let plan = state.plan.clone().unwrap();
        let cost: f64 = state
            .nodes
            .values()
            .flat_map(|n| [n.thread, n.repair].into_iter().flatten())
            .filter_map(|id| self.store(|st| st.thread(id)).ok().flatten())
            .map(|t| t.cost_usd)
            .sum();
        if plan.budget_usd.is_some_and(|b| cost >= b) {
            self.pause_plan(root, "Plan budget reached. Review spending before continuing.")?;
            for n in state.nodes.values() {
                if let Some(t) = n.thread {
                    let _ = self.stop(t);
                }
            }
            return Ok(());
        }
        // Observe running steps before dispatching new ones. A blocked step stops new dispatch.
        for spec in &plan.nodes {
            let node = self.plan_state(root)?.nodes[&spec.id].clone();
            if node.status == NodeStatus::Merged {
                continue;
            }
            if self.plan_state(root)?.paused.is_some() {
                return Ok(());
            }
            if let Some(thread) = node.thread
                && self.observe_node(root, spec, &node, thread).await?
            {
                return Ok(());
            }
        }
        state = self.plan_state(root)?;
        if state.paused.is_some() {
            return Ok(());
        }
        if state.nodes.values().all(|n| n.status == NodeStatus::Merged) {
            // Each project's checks run in its own collection worktree.
            let projects: Vec<Option<String>> =
                std::iter::once(None).chain(state.other_projects().into_iter().map(Some)).collect();
            for project in &projects {
                let (at, _) = state.integration(project.as_deref()).context("plan worktree missing")?;
                if let Some(p) = project {
                    let name = self.step_project(root, Some(p)).map(|(_, p)| p.name).unwrap_or_default();
                    self.ensure_untouched(Path::new(at), &name).await?;
                }
                let checks: Vec<String> = plan
                    .nodes
                    .iter()
                    .filter(|n| &n.project == project)
                    .flat_map(|n| n.checks.clone())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect();
                self.run_acceptance(root, Path::new(at), &checks).await?;
            }
            let path = Path::new(state.path.as_ref().unwrap());
            self.review_milestone(root, "final").await;
            self.plan_event(root, PlanEvent::Completed { commit: checkpoint::head(path).await? })?;
            self.append(root, EventKind::StatusChanged { status: ThreadStatus::Review })?;
            return Ok(());
        }
        let cpu = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
        let healthy = state.nodes.values().filter(|n| n.status == NodeStatus::Merged).count();
        let cap = plan.concurrency.min((cpu / 2).max(1)).min(3 + healthy / 2).min(self.max_cloud_runs()?);
        let mut active = state
            .nodes
            .values()
            .filter(|n| matches!(n.status, NodeStatus::Running | NodeStatus::Checking | NodeStatus::Merging))
            .count();
        for spec in &plan.nodes {
            let node = &state.nodes[&spec.id];
            if node.status != NodeStatus::Queued || node.thread.is_some() {
                continue;
            }
            if !state.ready(&spec.id) {
                self.node_state(root, &spec.id, NodeStatus::Queued, "Waiting for dependencies to integrate")?;
                continue;
            }
            if active >= cap || !self.cloud_slots_available()? {
                self.node_state(root, &spec.id, NodeStatus::Queued, "Waiting for an execution slot")?;
                continue;
            }
            self.dispatch_node(root, &state, spec).await?;
            active += 1;
        }
        Ok(())
    }
    fn final_object(&self, thread: ThreadId) -> Result<serde_json::Value> {
        let events = self.store(|st| st.events(thread, 0))?;
        let text = events
            .iter()
            .rev()
            .find_map(|e| match &e.kind {
                EventKind::Agent { event: arbiter_core::AgentEvent::Message { text }, .. } => Some(text),
                _ => None,
            })
            .context("agent returned no final message")?;
        json_object(text)
    }
    async fn prepare_plan(&self, root: ThreadId, state: &State) -> Result<()> {
        let t = self.store(|st| st.thread(root))?.unwrap();
        let p = self.store(|st| st.project(t.project_id))?;
        let key = format!("p-{}", short(root));
        let branch = format!("arbiter/plan/{}", short(root));
        let path = git::ensure_worktree(Path::new(&p.path), &self.inner.home.join("wt"), &key, &branch, "HEAD").await?;
        let plan = state.plan.as_ref().unwrap();
        let task = self.plan_task(t.project_id, None, &format!("plan:{root}"), &plan.title, &plan.goal)?;
        self.store(|st| st.link_task_thread(task, root))?;
        self.plan_event(
            root,
            PlanEvent::Prepared {
                path: path.to_string_lossy().into(),
                branch,
                base: checkpoint::head(&path).await?,
                task,
                project: None,
            },
        )?;
        self.append(root, EventKind::StatusChanged { status: ThreadStatus::Running })?;
        self.publish_tasks();
        Ok(())
    }
    /// The collection branch in one of the plan's other projects.
    async fn prepare_other(&self, root: ThreadId, state: &State, index: usize, project: &str) -> Result<()> {
        let (_, p) = self.step_project(root, Some(project))?;
        let key = format!("p{}-{}", index + 2, short(root));
        let branch = format!("arbiter/plan/{}", short(root));
        let path = git::ensure_worktree(Path::new(&p.path), &self.inner.home.join("wt"), &key, &branch, "HEAD").await?;
        self.plan_event(
            root,
            PlanEvent::Prepared {
                path: path.to_string_lossy().into(),
                branch,
                base: checkpoint::head(&path).await?,
                task: state.task.context("plan task missing")?,
                project: Some(project.into()),
            },
        )
    }
    fn plan_task(
        &self,
        project: arbiter_core::ProjectId,
        parent: Option<TaskId>,
        label: &str,
        title: &str,
        description: &str,
    ) -> Result<TaskId> {
        if let Some(t) =
            self.store(|st| st.tasks(WS, Some(project)))?.iter().find(|t| t.labels.iter().any(|l| l == label))
        {
            return Ok(t.id);
        }
        Ok(self
            .store(|st| {
                st.create_task(
                    WS,
                    NewTask {
                        project_id: project,
                        title: title.into(),
                        description: description.into(),
                        parent_id: parent,
                        labels: vec![label.into()],
                        ..Default::default()
                    },
                )
            })?
            .id)
    }
    async fn dispatch_node(&self, root: ThreadId, state: &State, spec: &PlanNode) -> Result<()> {
        if self.plan_state(root)?.paused.is_some() {
            return Ok(());
        }
        let (project_id, project) = self.step_project(root, spec.project.as_deref())?;
        let (collect, _) = state.integration(spec.project.as_deref()).context("the step's plan copy is missing")?;
        let base = checkpoint::head(Path::new(collect)).await?;
        let key = format!(
            "n-{}-{}",
            short(root),
            state.plan.as_ref().unwrap().nodes.iter().position(|n| n.id == spec.id).unwrap()
        );
        let branch = format!("arbiter/node/{}/{}", short(root), spec.id);
        let path =
            git::ensure_worktree(Path::new(&project.path), &self.inner.home.join("wt"), &key, &branch, &base).await?;
        let child = self
            .new_plan_child(ChildSpec {
                root,
                project: project_id,
                node: Some(spec.id.clone()),
                repair: false,
                path: &path,
                branch: &branch,
                permission: PermissionMode::Safe,
                title: &spec.title,
                harness: &spec.harness,
                model: spec.model.clone(),
            })
            .await?;
        self.append(child, EventKind::ToolProfileChanged { profile: spec.tool_profile })?;
        if let Some(usd) = spec.budget_usd {
            self.append(child, EventKind::BudgetSet { usd: Some(usd) })?;
        }
        let task =
            self.plan_task(project_id, state.task, &format!("plan:{root}:{}", spec.id), &spec.title, &spec.goal)?;
        self.store(|st| st.link_task_thread(task, child))?;
        self.plan_event(root, PlanEvent::NodePrepared { node: spec.id.clone(), thread: child, task, base })?;
        let conventions = std::fs::metadata(path.join("AGENTS.md"))
            .ok()
            .filter(|m| m.len() <= 32000)
            .and_then(|_| std::fs::read(path.join("AGENTS.md")).ok())
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        let deps: Vec<_> = spec.dependencies.iter().filter_map(|id| state.nodes.get(id)?.handoff.clone()).collect();
        let mut brief = arbiter_brief::compile(&conventions, &state.plan.as_ref().unwrap().goal, spec, &deps)?;
        brief.push_str(&self.cross_project_note(root, state, spec, &project.name));
        let remaining = 6000usize.saturating_sub(brief.len());
        let brief = self.memory_brief(child, brief, remaining)?;
        self.plan_event(root, PlanEvent::BriefCompiled { node: spec.id.clone(), text: brief.clone() })?;
        self.node_state(root, &spec.id, NodeStatus::Running, "Agent is implementing this step")?;
        if self.plan_state(root)?.paused.is_none() {
            self.send_message(child, brief, &state.attachments).await?;
        }
        self.publish_tasks();
        Ok(())
    }
    /// For plans over several projects: where this step works, and where to
    /// read the finished work it depends on in other projects. Bounded.
    fn cross_project_note(&self, root: ThreadId, state: &State, spec: &PlanNode, name: &str) -> String {
        let Some(plan) = &state.plan else { return String::new() };
        if plan.projects.is_empty() {
            return String::new();
        }
        let mut note = format!(
            "\n\nThis plan spans several projects. This step works only in {name}; change files only in this worktree."
        );
        for dep in plan.nodes.iter().filter(|n| spec.dependencies.contains(&n.id) && n.project != spec.project) {
            let Some((path, _)) = state.integration(dep.project.as_deref()) else { continue };
            let other = self.step_project(root, dep.project.as_deref()).map(|(_, p)| p.name).unwrap_or_default();
            note.push_str(&format!(
                "\nStep \"{}\" was done in {other}. Its finished code is at {path}. Read it there if the handoff is not enough; never edit it.",
                cut(&dep.title, 80)
            ));
        }
        cut(&note, 1500)
    }
    async fn observe_node(&self, root: ThreadId, spec: &PlanNode, node: &Node, thread: ThreadId) -> Result<bool> {
        let t = self.store(|st| st.thread(thread))?.context("step task missing")?;
        let cwd = self.thread_cwd(&t)?;
        if node.status == NodeStatus::Merging {
            self.integrate_node(root, spec, node, thread).await?;
            return Ok(false);
        }
        if spec.token_budget.is_some_and(|b| t.input_tokens + t.output_tokens >= b)
            || spec.budget_usd.is_some_and(|b| t.cost_usd >= b)
        {
            let _ = self.stop(thread);
            self.node_state(root, &spec.id, NodeStatus::Blocked, "Step budget reached")?;
            self.pause_plan(root, "A step reached its budget. Review its activity.")?;
            return Ok(true);
        }
        let mut scope = spec.scope.clone();
        scope.extend(node.extra_scope.clone());
        let changed = checkpoint::changed_files(&cwd, node.base.as_deref().context("step base missing")?).await?;
        let outside = arbiter_plan::drift(&scope, &changed);
        if !outside.is_empty() {
            let _ = self.stop(thread);
            self.node_state(
                root,
                &spec.id,
                NodeStatus::Blocked,
                &format!("Outside approved scope: {}", outside.iter().take(16).cloned().collect::<Vec<_>>().join(", ")),
            )?;
            self.pause_plan(root,"Scope changed. Review the affected files, then explicitly extend scope or correct the step before resuming.")?;
            return Ok(true);
        }
        match t.status {
            ThreadStatus::Running | ThreadStatus::Healing => {
                self.node_state(
                    root,
                    &spec.id,
                    NodeStatus::Running,
                    if t.status == ThreadStatus::Healing { "Healing failed checks" } else { "Agent is working" },
                )?;
            }
            ThreadStatus::NeedsApproval => {
                self.node_state(root, &spec.id, NodeStatus::Running, "Waiting for your approval in the agent task")?;
            }
            ThreadStatus::Failed => {
                self.node_state(root, &spec.id, NodeStatus::Blocked, "Agent failed; inspect its activity")?;
                self.pause_plan(root, "A step failed. Review it before resuming.")?;
                return Ok(true);
            }
            ThreadStatus::Idle => {
                let events = self.store(|st| st.events(thread, 0))?;
                let reset = events.iter().rev().find_map(|e| match e.kind {
                    EventKind::RateLimited { resets_at, .. } => resets_at,
                    _ => None,
                });
                if reset.is_some_and(|at| at > time::OffsetDateTime::now_utc().unix_timestamp()) {
                    self.node_state(
                        root,
                        &spec.id,
                        NodeStatus::Running,
                        "Waiting for the provider rate limit to reset",
                    )?;
                    return Ok(false);
                }
                // The persisted brief, session id and worktree make this restart-safe.
                self.node_state(root, &spec.id, NodeStatus::Running, "Resuming interrupted step")?;
                let brief = if let Some(brief) = &node.brief {
                    brief.clone()
                } else {
                    let state = self.plan_state(root)?;
                    let deps: Vec<_> =
                        spec.dependencies.iter().filter_map(|id| state.nodes.get(id)?.handoff.clone()).collect();
                    let brief = arbiter_brief::compile(
                        "Read and follow AGENTS.md in this worktree.",
                        &state.plan.as_ref().unwrap().goal,
                        spec,
                        &deps,
                    )?;
                    self.plan_event(root, PlanEvent::BriefCompiled { node: spec.id.clone(), text: brief.clone() })?;
                    brief
                };
                self.send_message(thread, format!("Resume the interrupted step using the existing worktree. Inspect and preserve completed work.\n{brief}"), &[]).await?;
            }
            ThreadStatus::Review => {
                if self.plan_state(root)?.nodes.iter().any(|(id, n)| id != &spec.id && n.status == NodeStatus::Merging)
                {
                    self.node_state(root, &spec.id, NodeStatus::Checking, "Waiting for the current merge to finish")?;
                    return Ok(false);
                }
                self.node_state(root, &spec.id, NodeStatus::Checking, "Running acceptance checks")?;
                if let Err(error) = self.run_acceptance(root, &cwd, &spec.checks).await {
                    if self.plan_state(root)?.paused.is_some() {
                        return Ok(true);
                    }
                    let feedback = cut(&format!("{error:#}"), 1800);
                    if node.heal_signatures.len() >= 2 || node.heal_signatures.contains(&feedback) {
                        self.node_state(
                            root,
                            &spec.id,
                            NodeStatus::Blocked,
                            "Acceptance checks still fail after bounded repair",
                        )?;
                        self.pause_plan(root, &feedback)?;
                        return Ok(true);
                    }
                    self.plan_event(
                        root,
                        PlanEvent::NodeHealed { node: spec.id.clone(), signature: feedback.clone() },
                    )?;
                    self.node_state(root, &spec.id, NodeStatus::Running, "Repairing failed acceptance checks")?;
                    self.send_message(thread, format!("Repair these failed acceptance checks within the approved scope, then report an updated structured handoff.\n{feedback}"), &[]).await?;
                    return Ok(false);
                }
                let mut handoff = node.handoff.clone();
                if handoff.is_none() {
                    handoff =
                        self.final_object(thread).ok().and_then(|v| serde_json::from_value(v["handoff"].clone()).ok());
                }
                let Some(handoff) = handoff else {
                    self.node_state(
                        root,
                        &spec.id,
                        NodeStatus::Blocked,
                        "Missing structured handoff; ask the agent to report it",
                    )?;
                    self.pause_plan(root, "A completed step needs a structured handoff before it can integrate.")?;
                    return Ok(true);
                };
                arbiter_plan::validate_handoff(&handoff)?;
                if !handoff.open_issues.is_empty() {
                    self.node_state(root, &spec.id, NodeStatus::Blocked, "Handoff contains unresolved issues")?;
                    self.pause_plan(root, "A handoff reports unresolved issues. Review the step.")?;
                    return Ok(true);
                }
                self.plan_event(root, PlanEvent::HandedOff { node: spec.id.clone(), handoff })?;
                let _ = self.stop(thread);
                let changed =
                    checkpoint::changed_files(&cwd, node.base.as_deref().context("step base missing")?).await?;
                ensure!(
                    arbiter_plan::drift(&scope, &changed).is_empty(),
                    "acceptance checks changed files outside approved scope"
                );
                let commit = checkpoint::checkpoint(
                    &cwd,
                    &format!("refs/arbiter/plan/{}/{}", short(root), spec.id),
                    None,
                    &format!("Arbiter step: {}", spec.title),
                )
                .await?
                .unwrap_or(checkpoint::head(&cwd).await?);
                // Record the exact commit before attempting the merge; retry reconciles ancestry.
                self.append(thread, EventKind::Checkpoint { n: t.checkpoints + 1, commit })?;
                self.node_state(root, &spec.id, NodeStatus::Merging, "Integrating verified changes")?;
                self.integrate_node(root, spec, &self.plan_state(root)?.nodes[&spec.id], thread).await?;
            }
            ThreadStatus::Merged => {}
        }
        Ok(false)
    }
    async fn run_acceptance(&self, root: ThreadId, cwd: &Path, commands: &[String]) -> Result<()> {
        let mut results = vec![];
        let mut failures = vec![];
        for (i, cmd) in commands.iter().enumerate() {
            let check = arbiter_heal::Check {
                name: format!("acceptance {}", i + 1),
                kind: arbiter_heal::CheckKind::Test,
                cmd: cmd.clone(),
                fix: None,
                timeout_secs: 120,
            };
            let r = tokio::select! {
                r = arbiter_heal::run_check(&check, cwd) => r,
                _ = async { loop {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    if self.plan_state(root).is_ok_and(|s| s.paused.is_some()) { break; }
                }} => anyhow::bail!("Checks interrupted because the plan is paused"),
            };
            results.push(arbiter_core::CheckResult {
                name: check.name,
                ok: r.ok,
                duration_ms: r.duration_ms,
                failures: u32::from(!r.ok),
                timed_out: r.timed_out,
                fixed: false,
            });
            if !r.ok {
                failures.push(format!("{}: {}", cmd, cut(&r.output, 1200)));
            }
        }
        if !results.is_empty() {
            self.append(root, EventKind::ChecksRan { attempt: 0, results })?;
        }
        ensure!(failures.is_empty(), "Acceptance failed: {}", cut(&failures.join("\n"), 1800));
        Ok(())
    }
    async fn integrate_node(&self, root: ThreadId, spec: &PlanNode, node: &Node, thread: ThreadId) -> Result<()> {
        let state = self.plan_state(root)?;
        if state.paused.is_some() {
            return Ok(());
        }
        let (at, collect_branch) = state.integration(spec.project.as_deref()).context("plan worktree missing")?;
        let path = Path::new(at);
        let t = self.store(|st| st.thread(thread))?.context("step missing")?;
        let commit = t.last_checkpoint.context("verified snapshot missing")?;
        if let Some(repair) = node.repair
            && !git::contains(path, &commit).await
        {
            let r = self.store(|st| st.thread(repair))?.context("repair task missing")?;
            if let Some(baseline) = &node.repair_baseline {
                let changed = checkpoint::changed_files(path, baseline).await?;
                ensure!(
                    arbiter_plan::drift(&node.repair_scope, &changed).is_empty(),
                    "merge repair changed files outside its approved conflict scope"
                );
            }
            if r.status == ThreadStatus::Review {
                ensure!(git::conflicts(path).await?.is_empty(), "merge repair left unresolved conflicts");
                self.run_acceptance(
                    root,
                    path,
                    &state
                        .plan
                        .as_ref()
                        .unwrap()
                        .nodes
                        .iter()
                        .filter(|n| {
                            n.project == spec.project
                                && (n.id == spec.id || state.nodes[&n.id].status == NodeStatus::Merged)
                        })
                        .flat_map(|n| n.checks.clone())
                        .collect::<Vec<_>>(),
                )
                .await?;
                git::finish_merge(path).await?;
                let _ = self.stop(repair);
            } else if r.status == ThreadStatus::Idle && !self.run_is_active(repair) {
                self.send_message(repair,"Continue resolving the existing merge. Do not abort or reset it. Run the approved checks and report completion.".into(),&[]).await?;
                return Ok(());
            } else if r.status == ThreadStatus::Failed {
                anyhow::bail!("merge repair failed; inspect its task");
            } else {
                return Ok(());
            }
        }
        if node.repair.is_none() && spec.project.is_some() {
            let name = self.step_project(root, spec.project.as_deref()).map(|(_, p)| p.name).unwrap_or_default();
            self.ensure_untouched(path, &name).await?;
        }
        let conflicts = git::merge(path, &commit).await?;
        if !conflicts.is_empty() {
            ensure!(node.repair.is_none(), "merge remains conflicted after one repair attempt");
            let repair = self
                .new_plan_child(ChildSpec {
                    root,
                    project: self.step_project(root, spec.project.as_deref())?.0,
                    node: Some(spec.id.clone()),
                    repair: true,
                    path,
                    branch: collect_branch,
                    permission: PermissionMode::Safe,
                    title: &format!("Resolve merge: {}", spec.title),
                    harness: &spec.harness,
                    model: spec.model.clone(),
                })
                .await?;
            let baseline = checkpoint::checkpoint(
                path,
                &format!("refs/arbiter/repair/{}/{}", short(root), spec.id),
                None,
                "Merge repair baseline",
            )
            .await?
            .unwrap_or(checkpoint::head(path).await?);
            self.plan_event(
                root,
                PlanEvent::RepairScope { node: spec.id.clone(), paths: conflicts.clone(), baseline },
            )?;
            self.plan_event(root, PlanEvent::RepairStarted { node: spec.id.clone(), thread: repair })?;
            let prompt = format!(
                "Resolve only the current Git merge conflicts in {}. Preserve both approved changes. Do not abort, reset, publish or commit the merge; the daemon commits after validation. Run these acceptance commands: {}. Summarize the resolution.",
                conflicts.iter().take(16).cloned().collect::<Vec<_>>().join(", "),
                spec.checks.join("; ")
            );
            self.send_message(repair, prompt, &[]).await?;
            return Ok(());
        }
        let integrated = checkpoint::head(path).await?;
        self.plan_event(root, PlanEvent::Integrated { node: spec.id.clone(), commit: integrated })?;
        self.append(thread, EventKind::StatusChanged { status: ThreadStatus::Merged })?;
        if let Some(task) = node.task {
            self.store(|st| st.update_task(task, TaskPatch { status: Some(TaskStatus::Done), ..Default::default() }))?;
        }
        self.publish_tasks();
        Ok(())
    }
}
