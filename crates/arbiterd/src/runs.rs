//! Live harness runs, one per thread at most. The first message on a thread
//! starts a run (resuming the harness session when there is one); later
//! messages steer it. A pump task turns harness output into log events.

use crate::AppState;
use crate::heal::{RESUME_AFTER_CRASH, is_rate_limit};
use crate::router::{self, Candidate};
use anyhow::{Context, Result, bail};
use arbiter_adapters::{Command, Harness, HarnessEvent, Resume, RunHandle, StartOpts, TurnOutcome, catalog};
use arbiter_core::{AgentEvent, EventKind, RunId, ThreadId, ThreadStatus};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

/// How long a finished session stays warm (cheap follow-ups via prompt cache)
/// before its process is stopped. It can always be resumed later.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Starts harness processes. Swappable so tests can use in-process fakes.
pub type Launcher = Arc<dyn Fn(Harness, StartOpts) -> Result<RunHandle> + Send + Sync>;

/// Which harnesses can run here. Swappable for tests.
pub type Availability = Arc<dyn Fn(Harness) -> bool + Send + Sync>;

pub fn real_launcher() -> Launcher {
    Arc::new(|h: Harness, o: StartOpts| h.start(o))
}

pub fn real_availability() -> Availability {
    Arc::new(catalog::installed)
}

#[derive(Default)]
pub struct Runs {
    active: Mutex<HashMap<ThreadId, Active>>,
}

#[derive(Clone)]
struct Active {
    run_id: RunId,
    harness: Harness,
    tx: UnboundedSender<Command>,
    flags: Arc<Flags>,
}

#[derive(Default)]
struct Flags {
    approvals: Mutex<std::collections::HashSet<String>>,
    /// The agent is mid-turn.
    busy: AtomicBool,
    /// Settings changed: restart (resuming the session) once the turn ends.
    restart: AtomicBool,
}

impl Runs {
    fn get(&self, t: ThreadId) -> Option<Active> {
        self.active.lock().unwrap().get(&t).cloned()
    }

    /// Unregister `run` if it is still the thread's live run; true if it was.
    fn remove_if(&self, t: ThreadId, run: RunId) -> bool {
        let mut map = self.active.lock().unwrap();
        let hit = map.get(&t).is_some_and(|a| a.run_id == run);
        if hit {
            map.remove(&t);
        }
        hit
    }

    fn count(&self, h: Harness) -> usize {
        self.active.lock().unwrap().values().filter(|a| a.harness == h).count()
    }
}

/// Written when documentation preparation starts; restart recovery keys on it.
pub(crate) const PREPARING_NOTICE: &str = "Local agent is checking documentation needs before frontier dispatch.";

/// The thread cannot take this message right now. Nothing was started or
/// broken, so callers must not mark the thread failed.
#[derive(Debug)]
pub(crate) struct Busy(pub String);
impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Busy {}

impl AppState {
    pub(crate) fn run_is_active(&self, thread: ThreadId) -> bool {
        self.inner.runs.get(thread).is_some()
            || self.inner.local_runs.lock().unwrap().contains_key(&thread)
            || self.inner.documentation_preparing.lock().unwrap().contains_key(&thread)
    }
    /// Something is changing the thread's files right now. A warm, idle
    /// frontier process between turns does not count.
    pub(crate) fn agent_working(&self, thread: ThreadId) -> bool {
        self.inner.runs.get(thread).is_some_and(|a| a.flags.busy.load(Ordering::SeqCst))
            || self.inner.local_runs.lock().unwrap().contains_key(&thread)
            || self.inner.documentation_preparing.lock().unwrap().contains_key(&thread)
    }
    /// Any agent or local model changing files anywhere.
    pub(crate) fn anything_working(&self) -> bool {
        self.inner.runs.active.lock().unwrap().values().any(|a| a.flags.busy.load(Ordering::SeqCst))
            || !self.inner.local_runs.lock().unwrap().is_empty()
            || !self.inner.documentation_preparing.lock().unwrap().is_empty()
    }
    pub(crate) fn cloud_slots_available(&self) -> Result<bool> {
        let busy =
            self.inner.runs.active.lock().unwrap().values().filter(|a| a.flags.busy.load(Ordering::SeqCst)).count();
        Ok(busy < self.max_cloud_runs()?)
    }
    /// Deliver a user message: steer the live run, or start one.
    pub(crate) fn deliver(&self, thread: ThreadId, text: String) -> Result<()> {
        let _admission = self.inner.admission_lock.lock().unwrap();
        let t = self.store(|s| s.thread(thread))?.context("thread missing")?;
        if t.harness != "local" && self.inner.setup.lock().unwrap().read()?.context7_enabled {
            if let Some(a) = self.inner.runs.get(thread) {
                anyhow::ensure!(
                    !a.flags.busy.load(Ordering::SeqCst),
                    Busy("Wait for the active frontier turn or interrupt it before preparing another documentation brief".into())
                );
                self.check_cloud(a.harness)?;
            } else if t.harness != "auto" {
                self.check_cloud(t.harness.parse().map_err(anyhow::Error::msg)?)?;
            }
            // Fail now, visibly, rather than after the preparation task starts.
            self.documentation_policy()?;
            self.documentation_model()?;
            return self.start_documentation_handoff(thread, text);
        }
        self.deliver_ready(thread, text)
    }
    /// Caller holds admission_lock. Never performs documentation retrieval.
    pub(crate) fn deliver_ready(&self, thread: ThreadId, text: String) -> Result<()> {
        if self.store(|s| s.thread(thread))?.is_some_and(|t| t.harness == "local")
            || self.place_locally(thread, &text)?
        {
            return self.start_local(thread, text);
        }
        if let Some(a) = self.inner.runs.get(thread) {
            self.check_cloud(a.harness)?;
            anyhow::ensure!(a.flags.approvals.lock().unwrap().is_empty(), "resolve the pending tool approval first");
        }
        let budget =
            if text.contains("Role: implementation agent") { 6000usize.saturating_sub(text.len()) } else { 1000 };
        let text = self.memory_brief(thread, text, budget)?;
        if let Some(a) = self.inner.runs.get(thread)
            && a.tx.send(Command::Send(text.clone())).is_ok()
        {
            a.flags.busy.store(true, Ordering::SeqCst);
            self.append(thread, EventKind::StatusChanged { status: ThreadStatus::Running })?;
            return Ok(());
        }
        self.start_run(thread, text)
    }

    /// For an auto thread, decide local vs cloud first. Records the choice and
    /// its reason on the thread; true means the local agent takes it.
    fn place_locally(&self, thread: ThreadId, text: &str) -> Result<bool> {
        let Some(t) = self.store(|s| s.thread(thread))? else { return Ok(false) };
        if t.harness != "auto" || self.inner.runs.get(thread).is_some() {
            return Ok(false);
        }
        let assessment = self
            .store(|s| s.events(thread, 0))?
            .into_iter()
            .rev()
            .find_map(|e| match e.kind {
                EventKind::IntakeAssessed { assessment } => Some(assessment),
                _ => None,
            })
            .unwrap_or_else(|| arbiter_intake::classifier::fallback(text));
        let local_only = self.inner.setup.lock().unwrap().read()?.mode == arbiter_setup::Mode::Local;
        let cloud = [Harness::Claude, Harness::Codex]
            .into_iter()
            .any(|h| (self.inner.available)(h) && self.check_cloud(h).is_ok());
        let model = self.capable_local_model();
        match router::place(local_only, model.as_deref(), t.worktree.is_some(), &assessment, cloud) {
            router::Placement::Cloud => Ok(false),
            router::Placement::Blocked(why) => bail!(why),
            router::Placement::Local(reason) => {
                self.append(
                    thread,
                    EventKind::ConfigChanged {
                        harness: "local".into(),
                        model,
                        effort: None,
                        permission: t.permission,
                        reason: Some(format!("Routed to the local agent: {reason}")),
                    },
                )?;
                Ok(true)
            }
        }
    }

    fn start_run(&self, thread: ThreadId, text: String) -> Result<()> {
        let mut t = self.store(|st| st.thread(thread))?.context("thread not found")?;
        if t.harness == "auto" {
            let candidates: Vec<Candidate> = [Harness::Claude, Harness::Codex]
                .into_iter()
                .map(|h| Candidate {
                    harness: h,
                    installed: (self.inner.available)(h) && self.check_cloud(h).is_ok(),
                    active_runs: self.inner.runs.count(h),
                })
                .collect();
            let learned = self.learned_route(t.project_id, &t.title)?;
            let (h, learned_model, reason) = if let Some((name, model, reason)) = learned.filter(|(name, _, _)| {
                name.parse().is_ok_and(|h| (self.inner.available)(h) && self.check_cloud(h).is_ok())
            }) {
                (name.parse().map_err(anyhow::Error::msg)?, model, reason)
            } else {
                if !candidates.iter().any(|c| c.installed)
                    && [Harness::Claude, Harness::Codex].iter().any(|h| (self.inner.available)(*h))
                {
                    bail!(
                        "No agent meets your usage policy. Open Settings to review login, allowance and paid-fallback status."
                    );
                }
                let (h, why) = router::route(&candidates).map_err(anyhow::Error::msg)?;
                (h, None, why)
            };
            t.harness = harness_id(h).to_owned();
            self.append(
                thread,
                EventKind::ConfigChanged {
                    harness: t.harness.clone(),
                    model: learned_model.clone(),
                    effort: None,
                    permission: t.permission,
                    reason: Some(format!("Auto-routed to {}: {reason}", router::name(h))),
                },
            )?;
            t.model = learned_model;
            t.effort = None;
        }
        let harness: Harness = t.harness.parse().map_err(anyhow::Error::msg)?;
        self.check_cloud(harness)?;
        // A thread preparing documentation is about to use its idle run; reaping
        // it would cancel that preparation on the other thread's behalf.
        let preparing: std::collections::HashSet<ThreadId> =
            self.inner.documentation_preparing.lock().unwrap().keys().copied().collect();
        let idle: Vec<_> = self
            .inner
            .runs
            .active
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, a)| {
                !preparing.contains(id)
                    && !a.flags.busy.load(Ordering::SeqCst)
                    && a.flags.approvals.lock().unwrap().is_empty()
            })
            .map(|(id, _)| *id)
            .collect();
        for id in idle {
            self.stop_locked(id)?;
        }
        anyhow::ensure!(
            self.inner.runs.active.lock().unwrap().len() < self.max_cloud_runs()?,
            "Cloud concurrency limit reached. Stop an idle agent or wait before resuming."
        );
        let cwd = match &t.worktree {
            Some(w) => PathBuf::from(w),
            None => PathBuf::from(self.store(|st| st.project(t.project_id))?.path),
        };
        // A fork still carries its parent's harness session until its first run
        // reports its own; branch the session instead of continuing the parent's.
        let parent_session = match t.parent {
            Some((p, _)) => self.store(|st| st.thread(p))?.and_then(|p| p.session_id),
            None => None,
        };
        let resume = t
            .session_id
            .clone()
            .map(|session_id| Resume { fork: parent_session.as_deref() == Some(session_id.as_str()), session_id });

        let mcp = self.mcp_for(thread, &cwd);
        let local_web = {
            let p = self.inner.setup.lock().unwrap().read()?;
            p.network && p.web_research_enabled
        } && mcp.is_some();
        let mut read_dirs = self.plan_read_dirs(thread);
        read_dirs.push(crate::attach::attachment_dir(&self.inner.home));
        let opts = StartOpts {
            read_dirs,
            subscription_only: self.inner.setup.lock().unwrap().read()?.mode == arbiter_setup::Mode::Subscription,
            cwd,
            permission: t.permission,
            model: t.model,
            effort: t.effort,
            resume,
            lean: self.inner.lean,
            tool_profile: if local_web {
                // Looking things up goes through the local research tool, so
                // raw web pages never reach the frontier agent.
                arbiter_core::ToolProfile::Implementation
            } else {
                match t.tool_profile {
                    arbiter_core::ToolProfile::Auto => {
                        if arbiter_intake::classifier::fallback(&text).task_type == "research" {
                            arbiter_core::ToolProfile::Research
                        } else {
                            arbiter_core::ToolProfile::Implementation
                        }
                    }
                    p => p,
                }
            },
            mcp,
        };
        let handle = (self.inner.launcher)(harness, opts)?;
        let run_id = RunId::new();
        let tx = handle.sender();
        if tx.send(Command::Send(text)).is_err() {
            bail!("harness exited immediately");
        }
        let flags = Arc::new(Flags {
            busy: AtomicBool::new(true),
            restart: AtomicBool::new(false),
            approvals: Default::default(),
        });
        self.inner.runs.active.lock().unwrap().insert(thread, Active { run_id, harness, tx, flags: flags.clone() });
        self.append(thread, EventKind::RunStarted { run_id, session_id: None })?;
        self.append(thread, EventKind::StatusChanged { status: ThreadStatus::Running })?;
        tokio::spawn(self.clone().pump(thread, run_id, t.harness, handle, flags));
        Ok(())
    }

    /// Attach arbiter-mcp for web projects (its tool list costs tokens every
    /// turn, so other projects don't get it). Needs the binary next to arbiterd.
    fn mcp_for(&self, thread: ThreadId, cwd: &std::path::Path) -> Option<arbiter_adapters::McpServer> {
        let web = self.heal_config(Some(thread), cwd).ok().and_then(|c| c.browser).is_some_and(|b| b.enabled);
        let url = self.inner.base_url.get()?;
        let planned = self
            .store(|st| st.events(thread, 0))
            .is_ok_and(|events| events.iter().any(|e| matches!(e.kind, EventKind::PlanChild { .. })));

        let research = self.inner.setup.lock().unwrap().read().is_ok_and(|p| p.network && p.web_research_enabled);
        let exe =
            std::env::current_exe().ok()?.with_file_name(if cfg!(windows) { "arbiter-mcp.exe" } else { "arbiter-mcp" });
        exe.exists().then(|| arbiter_adapters::McpServer {
            command: exe,
            env: vec![
                ("ARBITER_URL".into(), url.clone()),
                ("ARBITER_TOKEN".into(), self.inner.token.clone()),
                ("ARBITER_THREAD".into(), thread.to_string()),
                ("ARBITER_WEB_TOOLS".into(), web.to_string()),
                ("ARBITER_PLAN_TOOLS".into(), planned.to_string()),
                ("ARBITER_RESEARCH_TOOL".into(), research.to_string()),
            ],
        })
    }

    pub(crate) fn interrupt(&self, thread: ThreadId) -> bool {
        let _admission = self.inner.admission_lock.lock().unwrap();
        if self.stop_documentation_handoff(thread).unwrap_or(false) {
            return true;
        }
        if self.stop_local(thread).unwrap_or(false) {
            return true;
        }
        self.inner.runs.get(thread).is_some_and(|a| a.tx.send(Command::Interrupt).is_ok())
    }

    pub(crate) fn stop_all_for_setup(&self) -> Result<()> {
        for (_, tx) in self.inner.documentation_operations.lock().unwrap().values() {
            tx.send_replace(true);
        }
        let preparing: Vec<_> = self.inner.documentation_preparing.lock().unwrap().keys().copied().collect();
        for id in preparing {
            self.stop_documentation_handoff(id)?;
        }
        let local: Vec<_> = self.inner.local_runs.lock().unwrap().keys().copied().collect();
        for id in local {
            self.stop_local(id)?;
        }
        let ids: Vec<_> = self.inner.runs.active.lock().unwrap().keys().copied().collect();
        for id in ids {
            self.stop_locked(id)?;
        }
        Ok(())
    }

    pub(crate) fn approve_tool(&self, thread: ThreadId, run_id: RunId, request_id: &str, allowed: bool) -> Result<()> {
        let a =
            self.inner.runs.get(thread).context("the agent is no longer running; resume to request approval again")?;
        anyhow::ensure!(a.run_id == run_id, "approval belongs to an earlier run");
        let mut pending = a.flags.approvals.lock().unwrap();
        anyhow::ensure!(pending.contains(request_id), "approval was already resolved or expired");
        // Persist before sending; never re-send an allow decision after restart.
        self.append(
            thread,
            EventKind::ApprovalResolved {
                run_id,
                request_id: request_id.into(),
                allowed,
                reason: "User decision".into(),
            },
        )?;
        pending.remove(request_id);
        a.tx.send(Command::Approve { request_id: request_id.into(), allowed }).context("agent disconnected")?;
        if pending.is_empty() {
            self.append(thread, EventKind::StatusChanged { status: ThreadStatus::Running })?;
        }
        Ok(())
    }

    pub(crate) fn has_pending_approval(&self, thread: ThreadId) -> bool {
        self.inner.runs.get(thread).is_some_and(|a| !a.flags.approvals.lock().unwrap().is_empty())
    }

    pub(crate) fn expire_approvals(&self, thread: ThreadId, reason: &str) -> Result<()> {
        let events = self.store(|st| st.events(thread, 0))?;
        let mut pending = std::collections::HashSet::new();
        for e in events {
            match e.kind {
                EventKind::ApprovalRequested { run_id, request_id, .. } => {
                    pending.insert((run_id, request_id));
                }
                EventKind::ApprovalResolved { run_id, request_id, .. } => {
                    pending.remove(&(run_id, request_id));
                }
                _ => {}
            }
        }
        if !pending.is_empty() {
            for (run_id, request_id) in pending {
                self.append(
                    thread,
                    EventKind::ApprovalResolved { run_id, request_id, allowed: false, reason: reason.into() },
                )?;
            }
            self.append(thread, EventKind::StatusChanged { status: ThreadStatus::Idle })?;
        }
        Ok(())
    }

    /// Kill the thread's harness process tree. The session stays resumable.
    pub(crate) fn stop(&self, thread: ThreadId) -> Result<bool> {
        let _admission = self.inner.admission_lock.lock().unwrap();
        self.stop_locked(thread)
    }
    fn stop_locked(&self, thread: ThreadId) -> Result<bool> {
        // A thread can be preparing documentation while its previous frontier
        // process idles; stop both.
        let prepared = self.stop_documentation_handoff(thread)?;
        if self.stop_local(thread)? {
            return Ok(true);
        }
        let Some(a) = self.inner.runs.get(thread) else { return Ok(prepared) };
        self.inner.runs.remove_if(thread, a.run_id);
        let _ = a.tx.send(Command::Shutdown);
        self.append(thread, EventKind::Notice { text: "Stopped by user.".into() })?;
        self.append(thread, EventKind::StatusChanged { status: ThreadStatus::Idle })?;
        Ok(true)
    }

    /// New settings take effect on a fresh process that resumes the session:
    /// now if the agent is idle, otherwise right after its current turn.
    pub(crate) fn restart_for_config(&self, thread: ThreadId) {
        let Some(a) = self.inner.runs.get(thread) else { return };
        if a.flags.busy.load(Ordering::SeqCst) {
            a.flags.restart.store(true, Ordering::SeqCst);
        } else {
            self.inner.runs.remove_if(thread, a.run_id);
            let _ = a.tx.send(Command::Shutdown);
        }
    }

    async fn pump(self, thread: ThreadId, run_id: RunId, harness: String, mut handle: RunHandle, flags: Arc<Flags>) {
        let mut stopping = false;
        let mut stall_noted = false;
        let mut budget_hit = false;
        let mut last_reset: Option<i64> = None;
        loop {
            let busy = flags.busy.load(Ordering::SeqCst);
            let wait = if stopping || !flags.approvals.lock().unwrap().is_empty() {
                None
            } else if busy {
                Some(self.inner.stall_timeout)
            } else {
                Some(self.inner.idle_timeout)
            };
            let next = match wait {
                None => handle.events.recv().await,
                Some(d) => match tokio::time::timeout(d, handle.events.recv()).await {
                    Ok(e) => e,
                    Err(_) if busy => {
                        // Stalled: say so once per turn; the user decides (Interrupt/Stop).
                        if !stall_noted {
                            stall_noted = true;
                            let mins = self.inner.stall_timeout.as_secs() / 60;
                            self.append_logged(
                                thread,
                                EventKind::Notice {
                                    text: format!(
                                        "No activity from {harness} for {mins} min. It may be stuck; you can interrupt or stop it."
                                    ),
                                },
                            );
                        }
                        continue;
                    }
                    Err(_) => {
                        // Idle long enough: stop the warm process (resumable later).
                        // Unregister first so a message racing the shutdown starts a fresh run.
                        self.inner.runs.remove_if(thread, run_id);
                        stopping = true;
                        handle.send(Command::Shutdown);
                        continue;
                    }
                },
            };
            let Some(ev) = next else { break };
            match ev {
                HarnessEvent::Approval { request_id, tool, input } => {
                    if flags.approvals.lock().unwrap().insert(request_id.clone()) {
                        self.append_logged(thread, EventKind::ApprovalRequested { run_id, request_id, tool, input });
                        self.append_logged(thread, EventKind::StatusChanged { status: ThreadStatus::NeedsApproval });
                    }
                }
                HarnessEvent::Session(session_id) => {
                    self.append_logged(thread, EventKind::Session { run_id, session_id })
                }
                HarnessEvent::RateLimited { resets_at } => last_reset = resets_at.or(last_reset),
                HarnessEvent::Agent(event) => {
                    flags.busy.store(true, Ordering::SeqCst);
                    stall_noted = false;
                    let is_usage = matches!(event, AgentEvent::Usage(_));
                    self.append_logged(thread, EventKind::Agent { run_id, event });
                    // Over budget: let this turn finish (Claude reports usage at turn end,
                    // so interrupting here would hit the next turn), then pause.
                    if is_usage && !budget_hit && self.over_budget(thread) {
                        budget_hit = true;
                    }
                }
                HarnessEvent::TurnDone(outcome) => {
                    for request_id in flags.approvals.lock().unwrap().drain() {
                        self.append_logged(
                            thread,
                            EventKind::ApprovalResolved {
                                run_id,
                                request_id,
                                allowed: false,
                                reason: "Turn ended before a decision".into(),
                            },
                        );
                    }
                    flags.busy.store(false, Ordering::SeqCst);
                    stall_noted = false;
                    if budget_hit {
                        budget_hit = false;
                        self.append_logged(
                            thread,
                            EventKind::Notice {
                                text: "Budget reached; the agent was paused. Raise the budget or send a message to continue."
                                    .into(),
                            },
                        );
                        self.append_logged(thread, EventKind::StatusChanged { status: ThreadStatus::NeedsApproval });
                    } else {
                        match outcome {
                            // Checkpoint + checks decide between review and another fix round.
                            TurnOutcome::Completed => {
                                tokio::spawn(self.clone().after_turn(thread));
                            }
                            TurnOutcome::Interrupted => {
                                self.append_logged(thread, EventKind::StatusChanged { status: ThreadStatus::Idle })
                            }
                            TurnOutcome::Failed(message) if is_rate_limit(&message) || last_reset.is_some() => {
                                self.schedule_rate_limit_retry(thread, last_reset.take(), message);
                            }
                            TurnOutcome::Failed(message) => {
                                self.append_logged(
                                    thread,
                                    EventKind::Agent { run_id, event: AgentEvent::Error { message } },
                                );
                                self.append_logged(thread, EventKind::StatusChanged { status: ThreadStatus::Failed });
                            }
                        }
                    }
                    if flags.restart.swap(false, Ordering::SeqCst) {
                        self.inner.runs.remove_if(thread, run_id);
                        stopping = true;
                        handle.send(Command::Shutdown);
                    }
                }
                HarnessEvent::Exited { code, stderr_tail } => {
                    let awaiting_approval = !flags.approvals.lock().unwrap().is_empty();
                    for request_id in flags.approvals.lock().unwrap().drain() {
                        self.append_logged(
                            thread,
                            EventKind::ApprovalResolved {
                                run_id,
                                request_id,
                                allowed: false,
                                reason: "Agent process ended".into(),
                            },
                        );
                    }
                    // Not registered any more means someone stopped it on purpose.
                    let registered = self.inner.runs.remove_if(thread, run_id);
                    let crashed = flags.busy.load(Ordering::SeqCst) && !stopping && registered;
                    self.append_logged(thread, EventKind::RunEnded { run_id, exit_code: code });
                    if crashed && !awaiting_approval {
                        self.on_crash(thread, &harness, code, stderr_tail.trim(), last_reset);
                    } else if awaiting_approval && registered {
                        self.append_logged(thread, EventKind::StatusChanged { status: ThreadStatus::Idle });
                    }
                    break;
                }
            }
        }
        let registered = self.inner.runs.remove_if(thread, run_id);
        let pending: Vec<_> = flags.approvals.lock().unwrap().drain().collect();
        let had_pending = !pending.is_empty();
        for request_id in pending {
            self.append_logged(
                thread,
                EventKind::ApprovalResolved {
                    run_id,
                    request_id,
                    allowed: false,
                    reason: "Agent connection closed".into(),
                },
            );
        }
        if registered && had_pending {
            self.append_logged(thread, EventKind::StatusChanged { status: ThreadStatus::Idle });
        }
    }

    /// A harness died mid-turn: rate limits wait for the reset, other crashes
    /// resume the session once automatically, then give up visibly.
    fn on_crash(&self, thread: ThreadId, harness: &str, code: Option<i32>, tail: &str, last_reset: Option<i64>) {
        let detail = if tail.is_empty() { String::new() } else { format!(": {}", last_lines(tail, 6)) };
        if is_rate_limit(tail) || last_reset.is_some() {
            self.schedule_rate_limit_retry(thread, last_reset, format!("{harness} exited: rate limited"));
            return;
        }
        let has_session = self.store(|st| st.thread(thread)).ok().flatten().is_some_and(|t| t.session_id.is_some());
        let retry = has_session
            && self.heal_state(thread, |h| {
                h.crash_retries += 1;
                h.crash_retries <= 1
            });
        if retry {
            self.append_logged(
                thread,
                EventKind::Notice {
                    text: format!(
                        "{harness} exited unexpectedly (code {code:?}){detail}. Resuming the session automatically."
                    ),
                },
            );
            if let Err(e) = self.deliver(thread, RESUME_AFTER_CRASH.into()) {
                self.append_logged(thread, EventKind::Notice { text: format!("Automatic resume failed: {e:#}") });
                self.append_logged(thread, EventKind::StatusChanged { status: ThreadStatus::Failed });
            }
        } else {
            self.append_logged(
                thread,
                EventKind::Notice { text: format!("{harness} exited unexpectedly (code {code:?}){detail}") },
            );
            self.append_logged(thread, EventKind::StatusChanged { status: ThreadStatus::Failed });
        }
    }

    fn over_budget(&self, thread: ThreadId) -> bool {
        self.store(|st| st.thread(thread)).ok().flatten().is_some_and(|t| t.budget_usd.is_some_and(|b| t.cost_usd >= b))
    }

    /// After a daemon restart no harness processes survive; threads that were
    /// mid-run are marked so the user knows to continue them.
    pub(crate) fn recover_interrupted(&self) -> Result<()> {
        for t in self.store(|st| st.threads(crate::service::WS))? {
            let events = self.store(|st| st.events(t.id, 0))?;
            // A preparation that had started but neither dispatched nor ended
            // was lost with the process; its frontier request was never sent.
            let mut preparing = false;
            let mut pending = std::collections::HashSet::new();
            for e in events {
                match &e.kind {
                    EventKind::Notice { text } if text.starts_with(PREPARING_NOTICE) => preparing = true,
                    EventKind::Notice { text } if text.starts_with("Local documentation") => preparing = false,
                    EventKind::RunStarted { .. } | EventKind::UserMessage { .. } => preparing = false,
                    _ => {}
                }
                match e.kind {
                    EventKind::ApprovalRequested { run_id, request_id, .. } => {
                        pending.insert((run_id, request_id));
                    }
                    EventKind::ApprovalResolved { run_id, request_id, .. } => {
                        pending.remove(&(run_id, request_id));
                    }
                    _ => {}
                }
            }
            if !pending.is_empty() {
                for (run_id, request_id) in pending {
                    self.append(
                        t.id,
                        EventKind::ApprovalResolved {
                            run_id,
                            request_id,
                            allowed: false,
                            reason: "Expired after daemon restart; resume to request again".into(),
                        },
                    )?;
                }
                self.append(t.id, EventKind::StatusChanged { status: ThreadStatus::Idle })?;
            }
            if matches!(t.status, ThreadStatus::Running | ThreadStatus::Healing) {
                self.append(
                    t.id,
                    EventKind::Notice {
                        text: if preparing {
                            "Documentation preparation was interrupted by a daemon restart, so your last message never reached the agent. Send it again to continue.".into()
                        } else {
                            "Interrupted by a daemon restart. Send a message to resume.".into()
                        },
                    },
                )?;
                self.append(t.id, EventKind::StatusChanged { status: ThreadStatus::Idle })?;
            }
        }
        Ok(())
    }
}

pub fn harness_id(h: Harness) -> &'static str {
    match h {
        Harness::Claude => "claude",
        Harness::Codex => "codex",
    }
}

fn last_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join(" | ")
}
