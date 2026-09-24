//! Daemon operations shared by the API handlers and the run manager: the one
//! place events are appended (so side effects like task sync can't be skipped).

use crate::{AppState, WsMsg};
use anyhow::{Context, Result, bail};
use arbiter_core::{
    AttachmentRef, Event, EventKind, PermissionMode, ProjectId, Task, TaskId, TaskStatus, ThreadId, ThreadStatus,
    WorkspaceId, derive_title,
};
use arbiter_store::{TaskPatch, ThreadSummary};
use serde::Deserialize;

pub const WS: WorkspaceId = WorkspaceId::LOCAL;

/// Everything needed to open a thread. `message`, when given, is sent right
/// away (composer-first: a thread exists because someone asked for something).
#[derive(Clone, Debug, Deserialize)]
pub struct ThreadSpec {
    pub project_id: ProjectId,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    /// `auto` (default), `claude` or `codex`.
    #[serde(default = "auto")]
    pub harness: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub permission: PermissionMode,
    /// Isolated git worktree (default) or the project checkout itself.
    #[serde(default = "yes")]
    pub worktree: bool,
    /// Attachment ids (from `POST /v1/attachments`) sent with `message`.
    #[serde(default)]
    pub attachments: Vec<String>,
    #[serde(default)]
    pub intake: bool,
    #[serde(default)]
    pub context_paths: Vec<String>,
    #[serde(default)]
    pub tool_profile: Option<arbiter_core::ToolProfile>,
    #[serde(default)]
    pub workflow: bool,
    /// Let Arbiter choose: plan and coordinate for larger work, one agent for
    /// small work, clarifying only when the request is unclear. Overrides
    /// `workflow` and `intake`.
    #[serde(default)]
    pub auto: bool,
    /// Other projects the work may change. More than one project means the
    /// work is planned, one step per project, with dependencies between them.
    #[serde(default)]
    pub projects: Vec<ProjectId>,
}

/// Run settings for an agent started from a task.
#[derive(Clone, Debug, Deserialize)]
pub struct TaskStart {
    #[serde(default = "auto")]
    pub harness: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub permission: PermissionMode,
    #[serde(default = "yes")]
    pub worktree: bool,
}

fn auto() -> String {
    "auto".into()
}

fn yes() -> bool {
    true
}

fn nonempty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

impl AppState {
    /// Append, publish, and run side effects. All event writes go through here.
    pub(crate) fn append(&self, thread: ThreadId, kind: EventKind) -> Result<Event> {
        let status = match &kind {
            EventKind::StatusChanged { status } => Some(*status),
            _ => None,
        };
        let e = self.store(|st| st.append(WS, thread, kind))?;
        self.publish(&e);
        if let Some(s) = status {
            self.sync_tasks(thread, s);
        }
        if let Err(error) = self.knowledge_after(&e) {
            tracing::warn!(%thread, "knowledge update skipped: {error}");
        }
        Ok(e)
    }

    /// Like [`append`](Self::append) for background tasks, where the only
    /// sensible reaction to a failed write is to log it.
    pub(crate) fn append_logged(&self, thread: ThreadId, kind: EventKind) {
        if let Err(e) = self.append(thread, kind) {
            tracing::error!(%thread, "failed to append event: {e:#}");
        }
    }

    pub(crate) async fn create_thread(&self, mut spec: ThreadSpec) -> Result<ThreadSummary> {
        self.validate_attachments(&spec.attachments)?;
        let mut others = vec![];
        for p in std::mem::take(&mut spec.projects) {
            if p != spec.project_id && !others.contains(&p) {
                self.store(|st| st.project(p)).context("an attached project no longer exists")?;
                others.push(p);
            }
        }
        anyhow::ensure!(others.len() <= 4, "a task can use at most 5 projects");
        let approach = match spec.message.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
            Some(_) if !others.is_empty() => {
                spec.workflow = true;
                spec.intake = true;
                spec.worktree = true;
                Some(format!(
                    "This touches {} projects, so Arbiter plans it first: each step works in one project, and steps that need another project's work wait for it.",
                    others.len() + 1
                ))
            }
            Some(m) if spec.auto => {
                let a = self.inner.intake.assess(m.to_owned()).await;
                // Large work is planned; small work starts; medium work is a
                // judgement call, so intake asks the user as a question card.
                let plan = a.size == "L" || (a.size == "M" && !a.risks.is_empty());
                spec.workflow = plan;
                spec.intake = true;
                spec.worktree = true;
                Some(match a.size.as_str() {
                    _ if plan => format!(
                        "Arbiter is planning this {} {} first; you approve the plan, then agents work in isolated branches.",
                        if a.size == "L" { "large" } else { "risky" },
                        a.task_type
                    ),
                    "M" => {
                        format!("This {} could be planned first or started directly; Arbiter will ask.", a.task_type)
                    }
                    _ => format!("Arbiter gave this small {} to one agent in an isolated branch.", a.task_type),
                })
            }
            _ => None,
        };
        if !matches!(spec.harness.as_str(), "auto" | "claude" | "codex" | "local") {
            bail!("unknown harness {:?} (expected auto, claude or codex)", spec.harness);
        }
        let project = self.store(|st| st.project(spec.project_id))?;
        let message = nonempty(spec.message);
        if spec.intake || spec.workflow {
            anyhow::ensure!(
                message.as_ref().is_some_and(|m| m.len() <= 12000),
                "intake needs a request of at most 12000 bytes"
            );
        }
        let context_paths = crate::intake::validate_paths(std::path::Path::new(&project.path), &spec.context_paths)?;
        let title = nonempty(spec.title)
            .or_else(|| message.as_deref().map(derive_title))
            .unwrap_or_else(|| "New thread".into());
        let thread_id = ThreadId::new();
        let wt = if spec.worktree && !spec.workflow {
            // Short key keeps worktree paths short on Windows. Take the random
            // tail: a UUIDv7's leading hex digits are the timestamp.
            let key = thread_id.0.simple().to_string()[20..].to_owned();
            Some(self.inner.worktrees.create(std::path::Path::new(&project.path), &key, None).await?)
        } else {
            None
        };
        let base = match &wt {
            Some(w) => arbiter_supervisor::checkpoint::head(&w.path).await.ok(),
            None => arbiter_supervisor::checkpoint::head(std::path::Path::new(&project.path)).await.ok(),
        };
        // A model or effort only means something for a concrete harness.
        let concrete = spec.harness != "auto";
        self.append(
            thread_id,
            EventKind::ThreadCreated {
                project_id: spec.project_id,
                title,
                harness: spec.harness,
                worktree: wt.as_ref().map(|w| w.path.to_string_lossy().into_owned()),
                branch: wt.as_ref().map(|w| w.branch.clone()),
                parent: None,
                permission: spec.permission,
                model: nonempty(spec.model).filter(|_| concrete),
                effort: nonempty(spec.effort).filter(|_| concrete),
                base,
            },
        )?;
        self.append(
            thread_id,
            EventKind::ToolProfileChanged { profile: spec.tool_profile.unwrap_or(arbiter_core::ToolProfile::Auto) },
        )?;
        if !others.is_empty() {
            self.append(thread_id, EventKind::ProjectsAttached { projects: others })?;
        }
        if spec.workflow {
            self.append(thread_id, EventKind::WorkflowRequested)?;
        }
        if let Some(text) = approach {
            if text.contains("Arbiter will ask") {
                self.append(thread_id, EventKind::ApproachAsked)?;
            }
            self.append(thread_id, EventKind::Notice { text })?;
        }
        if let Some(text) = message {
            // A failed start is recorded in the thread; return the thread so
            // the user lands where the explanation is.
            if spec.intake || spec.workflow {
                self.begin_intake(thread_id, text, context_paths, spec.attachments).await?;
            } else {
                let mut text = text;
                if !context_paths.is_empty() {
                    text.push_str("\n\nReferenced project paths:\n");
                    text.push_str(&context_paths.join("\n"));
                }
                let _ = self.send_message(thread_id, text, &spec.attachments).await;
            }
        }
        self.store(|st| st.thread(thread_id))?.context("thread vanished")
    }

    /// Record a user message and hand it to the harness. Start failures are
    /// recorded in the thread (and returned) rather than lost.
    pub(crate) async fn send_message(&self, thread: ThreadId, text: String, attachments: &[String]) -> Result<Event> {
        anyhow::ensure!(!self.has_pending_approval(thread), "resolve the pending tool approval first");
        self.validate_attachments(attachments)?;
        self.reset_heal(thread);
        // Attachments are copied next to the agent and referenced by path.
        let (refs, lines) = if attachments.is_empty() {
            (Vec::new(), String::new())
        } else {
            let t = self.store(|st| st.thread(thread))?.context("thread not found")?;
            let cwd = self.thread_cwd(&t)?;
            crate::attach::materialize(&self.inner.home, &cwd, attachments).await?
        };
        let refs =
            refs.into_iter().map(|a| AttachmentRef { id: a.id, name: a.name, kind: a.kind, note: a.note }).collect();
        let event = self.append(thread, EventKind::UserMessage { text: text.clone(), attachments: refs })?;
        if let Err(e) = self.deliver(thread, text + &lines) {
            if e.downcast_ref::<crate::runs::Busy>().is_some() {
                self.append_logged(thread, EventKind::Notice { text: format!("Message not delivered: {e}") });
                return Err(e);
            }
            self.append_logged(thread, EventKind::Notice { text: format!("Could not start agent: {e:#}") });
            self.append_logged(thread, EventKind::StatusChanged { status: ThreadStatus::Failed });
            return Err(e);
        }
        Ok(event)
    }

    fn validate_attachments(&self, ids: &[String]) -> Result<()> {
        anyhow::ensure!(ids.len() <= 10, "at most 10 attachments per message");
        for id in ids {
            let (_, path) = crate::attach::get(&self.inner.home, id)?;
            anyhow::ensure!(path.is_file(), "attachment file is missing");
        }
        Ok(())
    }

    /// Start an agent on a task: a new thread seeded with the task, linked to it.
    pub(crate) async fn start_task(&self, id: TaskId, cfg: TaskStart) -> Result<(Task, ThreadSummary)> {
        let task = self.store(|st| st.task(id))?;
        let spec = ThreadSpec {
            project_id: task.project_id,
            title: Some(format!("{}: {}", task.key, task.title)),
            message: None, // sent after linking, so the first status change syncs the task
            harness: cfg.harness,
            model: cfg.model,
            effort: cfg.effort,
            permission: cfg.permission,
            worktree: cfg.worktree,
            attachments: Vec::new(),
            intake: false,
            context_paths: Vec::new(),
            tool_profile: None,
            workflow: false,
            auto: false,
            projects: vec![],
        };
        let thread = self.create_thread(spec).await?;
        self.store(|st| st.link_task_thread(id, thread.id))?;
        // Delivery failures are recorded on the thread itself.
        let _ = self.send_message(thread.id, seed_prompt(&task), &[]).await;
        self.publish_tasks();
        let thread = self.store(|st| st.thread(thread.id))?.context("thread vanished")?;
        Ok((self.store(|st| st.task(id))?, thread))
    }

    /// Task status follows its agents: work starts → in progress, agent
    /// finishes → in review. Humans (or agents) decide done/canceled.
    fn sync_tasks(&self, thread: ThreadId, status: ThreadStatus) {
        let result = self.store(|st| -> arbiter_store::Result<bool> {
            let mut changed = false;
            for id in st.tasks_for_thread(thread)? {
                let task = st.task(id)?;
                let next = match (status, task.status) {
                    (ThreadStatus::Running | ThreadStatus::Healing, TaskStatus::Backlog | TaskStatus::Todo) => {
                        Some(TaskStatus::InProgress)
                    }
                    (ThreadStatus::Review, TaskStatus::InProgress) => Some(TaskStatus::InReview),
                    (ThreadStatus::Merged, TaskStatus::InProgress | TaskStatus::InReview) => Some(TaskStatus::Done),
                    _ => None,
                };
                if let Some(s) = next {
                    st.update_task(id, TaskPatch { status: Some(s), ..Default::default() })?;
                    changed = true;
                }
            }
            Ok(changed)
        });
        match result {
            Ok(true) => self.publish_tasks(),
            Ok(false) => {}
            Err(e) => tracing::error!(%thread, "task sync failed: {e}"),
        }
    }

    pub(crate) fn publish_tasks(&self) {
        let _ = self.inner.events.send(WsMsg::TasksChanged);
    }
}

/// Kept short on purpose: every token here is paid on every turn of the thread.
fn seed_prompt(t: &Task) -> String {
    let mut p = format!("Task {}: {}\n", t.key, t.title);
    if !t.description.trim().is_empty() {
        p.push('\n');
        p.push_str(t.description.trim());
        p.push('\n');
    }
    p.push_str("\nWhen you are done, reply with a short summary of what you changed and anything left to do.");
    p
}
