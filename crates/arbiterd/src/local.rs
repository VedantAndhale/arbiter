//! Local model execution uses the same thread log, checkpoints and review path.
use crate::AppState;
use anyhow::{Context, Result, ensure};
use arbiter_core::{AgentEvent, EventKind, PermissionMode, RunId, ThreadId, ThreadStatus};
use serde_json::{Value, json};
use std::{future::Future, pin::Pin, sync::Arc};

/// Fixture seam; production invokes the daemon-owned embedded model runtime.
pub type LocalGenerator =
    Arc<dyn Fn(String, String, String, Value) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> + Send + Sync>;

/// Actions per local turn. Large enough for list, search, read, edit, check,
/// fix, check and done, small enough to stay bounded.
pub(crate) const LOCAL_STEPS: usize = 24;

pub(crate) struct LocalOutcome {
    pub summary: String,
    pub steps: usize,
}

impl AppState {
    pub(crate) fn start_local(&self, thread: ThreadId, text: String) -> Result<()> {
        ensure!(self.inner.setup.lock().unwrap().read()?.complete, "complete Setup first");
        let t = self.store(|s| s.thread(thread))?.context("thread not found")?;
        let cwd = t.worktree.as_deref().context("local coding requires an isolated worktree")?;
        let model = t.model.clone().context("choose an installed local model explicitly")?;
        ensure!(
            self.inner.local_generator.is_some()
                || self.inner.intake.status()["models"]
                    .as_array()
                    .is_some_and(|ms| ms.iter().any(|m| m["model"]["id"] == model && m["ready"] == true)),
            "download the selected local model before coding"
        );
        let mut active = self.inner.local_runs.lock().unwrap();
        ensure!(
            active.is_empty(),
            crate::runs::Busy("The local model is busy with another task; wait or stop it first".into())
        );
        let mut scope = vec!["**".into()];
        let mut read_scope = vec![];
        if let Some(root) = t.plan_root {
            let state = self.plan_state(root)?;
            if let Some((key, _)) = state.nodes.iter().find(|(_, n)| n.thread == Some(thread))
                && let Some(spec) = state.plan.as_ref().and_then(|p| p.nodes.iter().find(|n| &n.id == key))
            {
                scope = spec.scope.clone();
                read_scope = spec.may_read.clone();
            }
        }
        let session = arbiter_local::Session::with_limit(
            std::path::Path::new(cwd),
            scope.clone(),
            read_scope,
            t.permission == PermissionMode::Plan,
            LOCAL_STEPS,
        )?;
        let inventory = arbiter_project::inspect(std::path::Path::new(cwd))?;
        let files = inventory.initial_files.iter().take(60).cloned().collect::<Vec<_>>().join("\n");
        let goal = format!(
            "Request:\n{}\nApproved writable scope: {}\nSome project files (use list and search for more):\n{}",
            text.chars().take(2500).collect::<String>(),
            scope.join(","),
            files.chars().take(2000).collect::<String>()
        );
        let run_id = RunId::new();
        self.append(thread, EventKind::RunStarted { run_id, session_id: None })?;
        self.append(thread, EventKind::StatusChanged { status: ThreadStatus::Running })?;
        self.append(thread,EventKind::Notice{text:format!("Local coding agent: at most {LOCAL_STEPS} actions using file tools and the project's configured checks; no shell or network tools. Usage is local; token counts are currently unavailable.")})?;
        let state = self.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            if rx.await.is_err() {
                return;
            }
            let result = state.local_turn(thread, run_id, model, goal, session).await;
            state.inner.local_runs.lock().unwrap().remove(&thread);
            match result {
                Ok((summary, files)) => {
                    if state.plan_child(thread).is_some_and(|(_, node, repair)| node.is_some() && !repair)
                        && let Err(e) = state.report_handoff(
                            thread,
                            arbiter_core::plan::Handoff {
                                summary: summary.clone(),
                                files,
                                decisions: vec!["Edited locally with bounded file tools".into()],
                                interfaces: vec![],
                                open_issues: vec!["Validate project checks and review model output".into()],
                            },
                        )
                    {
                        state.append_logged(
                            thread,
                            EventKind::Notice { text: format!("Local handoff could not be recorded: {e}") },
                        );
                    }
                    state.append_logged(
                        thread,
                        EventKind::Agent { run_id, event: AgentEvent::Message { text: summary } },
                    );
                    state.append_logged(thread, EventKind::Agent { run_id, event: AgentEvent::Done });
                    if state.inner.setup.lock().unwrap().read().is_ok_and(|p| p.network) {
                        tokio::spawn(state.clone().after_turn(thread));
                    } else {
                        state.append_logged(thread,EventKind::Notice{text:"Project commands were skipped because network access is disabled. Review the diff; checks have not passed.".into()});
                        state.append_logged(thread, EventKind::StatusChanged { status: ThreadStatus::Review });
                    }
                }
                Err(e) => {
                    state.append_logged(
                        thread,
                        EventKind::Agent { run_id, event: AgentEvent::Error { message: format!("{e:#}") } },
                    );
                    state.append_logged(thread, EventKind::StatusChanged { status: ThreadStatus::Failed });
                }
            }
        });
        active.insert(thread, (run_id, task.abort_handle()));
        let _ = tx.send(());
        Ok(())
    }
    async fn local_turn(
        &self,
        thread: ThreadId,
        run_id: RunId,
        model: String,
        goal: String,
        mut session: arbiter_local::Session,
    ) -> Result<(String, Vec<String>)> {
        let t = self.store(|s| s.thread(thread))?.context("thread missing")?;
        let cwd = self.thread_cwd(&t)?;
        self.checkpoint_now(thread, &t, &cwd).await;
        let outcome = self.local_loop(Some((thread, run_id)), &cwd, &model, &goal, &mut session, true).await?;
        Ok((outcome.summary, session.changed))
    }

    /// Hand a local task to a frontier agent, at the user's request only.
    /// The frontier agent gets a compact handoff — the request, the local
    /// summary, the changed files and the open problems — never the local
    /// transcript. Cloud admission applies exactly as for any other launch.
    pub(crate) async fn escalate(&self, thread: ThreadId, harness: &str) -> Result<usize> {
        let t = self.store(|s| s.thread(thread))?.context("thread not found")?;
        ensure!(t.harness == "local", "only a local agent's task can be escalated");
        ensure!(!self.run_is_active(thread), crate::runs::Busy("Stop the local agent before escalating".into()));
        let target: arbiter_adapters::Harness = harness.parse().map_err(anyhow::Error::msg)?;
        self.check_cloud(target)?;
        let events = self.store(|s| s.events(thread, 0))?;
        let request = events
            .iter()
            .find_map(|e| match &e.kind {
                EventKind::UserMessage { text, .. } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_else(|| t.title.clone());
        let summary = events.iter().rev().find_map(|e| match &e.kind {
            EventKind::Agent { event: AgentEvent::Message { text }, .. } => Some(text.clone()),
            _ => None,
        });
        let mut problems: Vec<String> = events
            .iter()
            .rev()
            .filter_map(|e| match &e.kind {
                EventKind::Agent { event: AgentEvent::ToolResult { output, is_error: true, .. }, .. } => {
                    Some(clip(output, 300))
                }
                EventKind::Agent { event: AgentEvent::Error { message }, .. } => Some(clip(message, 300)),
                _ => None,
            })
            .take(3)
            .collect();
        if let Some(failing) = events.iter().rev().find_map(|e| match &e.kind {
            EventKind::ChecksRan { results, .. } => {
                Some(results.iter().filter(|r| !r.ok).map(|r| r.name.clone()).collect::<Vec<_>>())
            }
            _ => None,
        }) && !failing.is_empty()
        {
            problems.insert(0, format!("Checks still failing: {}", failing.join(", ")));
        }
        problems.dedup();
        let cwd = self.thread_cwd(&t)?;
        let base = t.base.clone().unwrap_or_else(|| "HEAD".into());
        let changed = arbiter_supervisor::integration::git(&cwd, &["diff", "--stat", "--no-color", &base])
            .await
            .unwrap_or_default();
        let brief = format!(
            "Continue a task that a local agent started in this worktree. Its changes are already on disk; review them rather than starting over.\n\nOriginal request:\n{}\n\nLocal agent's summary:\n{}\n\nChanged files:\n{}\n\nOpen problems:\n{}\n\nFinish the request, keep the changes minimal, and run the project's checks.",
            clip(&request, 2000),
            summary.as_deref().map(|s| clip(s, 800)).unwrap_or_else(|| "(none; the local agent did not finish)".into()),
            if changed.trim().is_empty() { "(no changes yet)".into() } else { clip(changed.trim(), 1500) },
            if problems.is_empty() { "(none recorded)".into() } else { problems.join("\n") },
        );
        self.append(
            thread,
            EventKind::ConfigChanged {
                harness: harness.into(),
                model: None,
                effort: None,
                permission: t.permission,
                reason: Some("Escalated from the local agent by the user".into()),
            },
        )?;
        self.append(
            thread,
            EventKind::Notice {
                text: format!(
                    "Escalated to {}. It receives a {}-character handoff, not the local transcript; this uses your cloud allowance.",
                    crate::router::name(target),
                    brief.len()
                ),
            },
        )?;
        let len = brief.len();
        self.deliver(thread, brief)?;
        Ok(len)
    }

    /// Record a local tool step on the thread, when there is one (the
    /// capability probe runs without a thread).
    fn local_event(&self, sink: Option<(ThreadId, RunId)>, event: AgentEvent) -> Result<()> {
        if let Some((thread, run_id)) = sink {
            self.append(thread, EventKind::Agent { run_id, event })?;
        }
        Ok(())
    }

    /// The bounded tool loop shared by coding turns and the capability probe.
    pub(crate) async fn local_loop(
        &self,
        sink: Option<(ThreadId, RunId)>,
        cwd: &std::path::Path,
        model: &str,
        goal: &str,
        session: &mut arbiter_local::Session,
        documentation: bool,
    ) -> Result<LocalOutcome> {
        let mut evidence = String::new();
        let mut history: std::collections::VecDeque<String> = Default::default();
        let mut attempts: std::collections::HashMap<String, usize> = Default::default();
        let (mut documentation_calls, mut check_calls) = (0, 0);
        for step in 0..LOCAL_STEPS {
            let recent = history.iter().cloned().collect::<Vec<_>>().join("\n");
            let prompt = format!(
                "{goal}\nChanged files: {}\nSteps so far (oldest first):\n{recent}\nLatest tool result:\n{evidence}",
                session.changed.join(", ")
            );
            let output =
                self.local_generate(model, prompt, arbiter_local::SYSTEM.into(), arbiter_local::schema()).await?;
            let action: arbiter_local::Action =
                serde_json::from_str(&output).context("local model returned invalid action")?;
            // A model stuck on one failing attempt wastes the whole budget.
            let tries = attempts.entry(action.signature()).or_default();
            *tries += 1;
            if *tries >= 3 && action.action != "check" {
                anyhow::bail!(
                    "The local model repeated the same {} action three times. Changes so far are preserved.",
                    action.action
                );
            }
            self.local_event(
                sink,
                AgentEvent::ToolCall {
                    id: format!("local-{step}"),
                    name: action.action.clone(),
                    input: json!({"path":action.path}),
                },
            )?;
            let result = match action.action.as_str() {
                "documentation" if !documentation => {
                    Err(anyhow::anyhow!("Documentation is unavailable here; use the files provided"))
                }
                "documentation" => {
                    documentation_calls += 1;
                    if documentation_calls > 2 {
                        Err(anyhow::anyhow!(
                            "Documentation request limit reached; use existing findings or ask the user"
                        ))
                    } else {
                        self.retrieve_documentation(&action.path, &action.content).await
                    }
                }
                "web" if !documentation => {
                    Err(anyhow::anyhow!("Web lookups are unavailable here; use the files provided"))
                }
                "web" => {
                    documentation_calls += 1;
                    if documentation_calls > 2 {
                        Err(anyhow::anyhow!("Lookup limit reached; use what you found or finish"))
                    } else {
                        self.web_research(&action.content, None).await.map(|(brief, _)| brief)
                    }
                }
                "check" => {
                    check_calls += 1;
                    if check_calls > 3 {
                        Err(anyhow::anyhow!(
                            "Check limit reached for this turn; finish with done and report the result"
                        ))
                    } else {
                        self.local_checks(sink.map(|s| s.0), cwd).await
                    }
                }
                _ => session.execute(&action),
            };
            let error = result.is_err();
            evidence = match result {
                Ok(v) => v.to_string(),
                Err(e) => json!({"error":e.to_string()}).to_string(),
            };
            let line = format!(
                "{step}: {} {} -> {}",
                action.action,
                action.path,
                if error { evidence.chars().take(160).collect::<String>() } else { "ok".into() }
            );
            history.push_back(line);
            if history.len() > 10 {
                history.pop_front();
            }
            // File bodies stay in the local tool loop, not the durable transcript.
            self.local_event(
                sink,
                AgentEvent::ToolResult {
                    id: format!("local-{step}"),
                    output: if error {
                        evidence.chars().take(600).collect()
                    } else {
                        format!("{} completed for {}", action.action, action.path)
                    },
                    is_error: error,
                },
            )?;
            if action.action == "done" && !error {
                return Ok(LocalOutcome { summary: action.summary, steps: step + 1 });
            }
            if evidence.len() > 8000 {
                evidence = json!({"error":"tool result exceeded the local context budget; narrow the request (smaller read window or more specific search)"}).to_string();
            }
        }
        anyhow::bail!("Local action limit reached. Changes are preserved; inspect them before continuing.")
    }
    /// The project's configured checks, run on the local agent's behalf.
    /// Same policy as automatic checks: they execute project code, so they
    /// need network access enabled. Returns failures, never full logs.
    async fn local_checks(&self, thread: Option<ThreadId>, cwd: &std::path::Path) -> Result<Value> {
        ensure!(
            self.inner.setup.lock().unwrap().read()?.network,
            "Project checks are disabled while network access is off; finish with done and say checks did not run"
        );
        let checks = self.heal_config(thread, cwd)?.checks;
        ensure!(!checks.is_empty(), "This project has no configured checks; finish with done and say so");
        let mut results = vec![];
        for check in checks.iter().take(8) {
            let mut check = check.clone();
            check.timeout_secs = check.timeout_secs.clamp(1, 300);
            let run = arbiter_heal::run_check(&check, cwd).await;
            results.push(json!({"name":check.name,"ok":run.ok,"failures":crate::projects::baseline_evidence(&run)}));
        }
        Ok(json!({"passed":results.iter().all(|r| r["ok"] == true),"results":results}))
    }
    pub(crate) async fn local_generate(
        &self,
        model: &str,
        prompt: String,
        system: String,
        schema: Value,
    ) -> Result<String> {
        if let Some(generate) = &self.inner.local_generator {
            generate(model.into(), prompt, system, schema).await
        } else {
            self.inner.intake.structured(model, prompt, system, schema).await
        }
    }
    pub(crate) fn stop_local(&self, thread: ThreadId) -> Result<bool> {
        let Some((_, task)) = self.inner.local_runs.lock().unwrap().remove(&thread) else { return Ok(false) };
        task.abort();
        self.append(
            thread,
            EventKind::Notice { text: "Local execution stopped. Completed file changes are preserved.".into() },
        )?;
        self.append(thread, EventKind::StatusChanged { status: ThreadStatus::Idle })?;
        Ok(true)
    }
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_owned() } else { s.chars().take(n).collect::<String>() + "…" }
}
