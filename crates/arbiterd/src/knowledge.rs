//! Project knowledge is replayed from append-only events; Markdown is a repairable mirror.
use crate::AppState;
use anyhow::{Context, Result, ensure};
use arbiter_core::{
    Event, EventKind, ProjectId, ThreadId,
    plan::PlanEvent,
    vault::{Note, Outcome},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
fn node_key(root: ThreadId, node: &str) -> String {
    format!("{root}-{}", &format!("{:x}", Sha256::digest(node.as_bytes()))[..16])
}

#[derive(Serialize, Clone)]
pub(crate) struct Proposal {
    pub id: String,
    pub note: Note,
    pub base_revision: u32,
    pub thread: ThreadId,
}
#[derive(Default, Serialize)]
pub(crate) struct Knowledge {
    pub notes: BTreeMap<String, Note>,
    pub proposals: BTreeMap<String, Proposal>,
    pub outcomes: BTreeMap<String, Outcome>,
    pub preferences: BTreeMap<String, Preference>,
}
#[derive(Clone, Serialize)]
pub(crate) struct Preference {
    pub harness: Option<String>,
    pub model: Option<String>,
    pub enabled: bool,
}
#[derive(Serialize)]
pub(crate) struct Strength {
    pub task_type: String,
    pub harness: String,
    pub model: Option<String>,
    pub runs: u32,
    pub first_try: u32,
    pub passed: u32,
    pub rework: u32,
    pub tokens: u64,
    pub cost_usd: f64,
}

impl Knowledge {
    pub fn apply(&mut self, event: Event) {
        match event.kind {
            EventKind::VaultSaved { note } => {
                self.notes.insert(note.id.clone(), note);
            }
            EventKind::VaultProposed { id, note, base_revision } => {
                self.proposals.insert(id.clone(), Proposal { id, note, base_revision, thread: event.thread_id });
            }
            EventKind::VaultResolved { id, note, accepted } => {
                self.proposals.remove(&id);
                if accepted && let Some(note) = note {
                    self.notes.insert(note.id.clone(), note);
                }
            }
            EventKind::OutcomeRecorded { mut outcome } => {
                if let Some(old) = self.outcomes.get(&event.thread_id.to_string()) {
                    outcome.user_rework = old.user_rework;
                }
                self.outcomes.insert(event.thread_id.to_string(), outcome);
            }
            EventKind::OutcomeRework { rework } => {
                if let Some(o) = self.outcomes.get_mut(&event.thread_id.to_string()) {
                    o.user_rework = rework;
                }
            }
            EventKind::RoutingPreference { task_type, harness, model, enabled } => {
                self.preferences.insert(task_type, Preference { harness, model, enabled });
            }
            _ => {}
        }
    }
    pub fn strengths(&self) -> Vec<Strength> {
        let mut rows: BTreeMap<(String, String, Option<String>), Strength> = BTreeMap::new();
        for o in self.outcomes.values().filter(|o| o.checks_run > 0) {
            let s = rows.entry((o.task_type.clone(), o.harness.clone(), o.model.clone())).or_insert_with(|| Strength {
                task_type: o.task_type.clone(),
                harness: o.harness.clone(),
                model: o.model.clone(),
                runs: 0,
                first_try: 0,
                passed: 0,
                rework: 0,
                tokens: 0,
                cost_usd: 0.0,
            });
            s.runs += 1;
            s.first_try += u32::from(o.first_try);
            s.passed += u32::from(o.passed);
            s.rework += u32::from(o.user_rework);
            s.tokens += o.input_tokens + o.output_tokens;
            s.cost_usd += o.cost_usd;
        }
        rows.into_values().collect()
    }
}

impl AppState {
    pub(crate) fn knowledge(&self, project: ProjectId) -> Result<Knowledge> {
        self.store(|st| st.project(project))?;
        let mut state = Knowledge::default();
        for e in self.store(|st| st.knowledge_events(project))? {
            state.apply(e);
        }
        Ok(state)
    }
    pub(crate) fn knowledge_project(&self, thread: ThreadId) -> Result<ProjectId> {
        Ok(self.store(|st| st.thread(thread))?.context("task not found")?.project_id)
    }
    pub(crate) fn save_note(&self, thread: ThreadId, mut note: Note, expected: u32) -> Result<Note> {
        let _guard = self.inner.knowledge_lock.lock().unwrap();
        arbiter_vault::validate(&note)?;
        let project = self.knowledge_project(thread)?;
        let state = self.knowledge(project)?;
        ensure!(state.notes.get(&note.id).map_or(0, |n| n.revision) == expected, "note changed; reload before saving");
        ensure!(state.notes.len() < 500 || state.notes.contains_key(&note.id), "project note limit reached (500)");
        note.revision = expected.checked_add(1).context("note revision exhausted")?;
        self.append(thread, EventKind::VaultSaved { note: note.clone() })?;
        self.mirror_note(project, &note);
        Ok(note)
    }
    /// Notes are mirrored into the memory folder, never into the project,
    /// so personal memory does not reach a shared repository.
    fn mirror_note(&self, project: ProjectId, note: &Note) {
        if let Ok(dir) = self.project_memory(project)
            && let Err(e) = arbiter_vault::mirror_to(&dir, note)
        {
            tracing::warn!(%project,"memory Markdown mirror unavailable: {e}");
        }
    }
    pub(crate) fn rebuild_vault(&self, project: ProjectId) -> Result<usize> {
        let state = self.knowledge(project)?;
        let dir = self.project_memory(project)?;
        for note in state.notes.values() {
            arbiter_vault::mirror_to(&dir, note).map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        Ok(state.notes.len())
    }
    pub(crate) fn propose_note(&self, thread: ThreadId, note: Note, base_revision: u32) -> Result<String> {
        let _guard = self.inner.knowledge_lock.lock().unwrap();
        arbiter_vault::validate(&note)?;
        ensure!(note.id.starts_with("decisions/"), "agents may propose decision notes only");
        let state = self.knowledge(self.knowledge_project(thread)?)?;
        ensure!(state.proposals.len() < 50, "review existing note proposals first (limit 50)");
        ensure!(
            state.notes.get(&note.id).map_or(0, |n| n.revision) == base_revision,
            "note changed; read its latest revision"
        );
        let id = ThreadId::new().to_string();
        self.append(thread, EventKind::VaultProposed { id: id.clone(), note, base_revision })?;
        Ok(id)
    }
    pub(crate) fn resolve_note(&self, thread: ThreadId, id: &str, accepted: bool) -> Result<()> {
        let _guard = self.inner.knowledge_lock.lock().unwrap();
        let project = self.knowledge_project(thread)?;
        let state = self.knowledge(project)?;
        let proposal = state.proposals.get(id).context("proposal already resolved or not found")?;
        let accepted_note = if accepted {
            ensure!(
                state.notes.get(&proposal.note.id).map_or(0, |n| n.revision) == proposal.base_revision,
                "note changed; reject this proposal and request a new revision"
            );
            ensure!(
                state.notes.len() < 500 || state.notes.contains_key(&proposal.note.id),
                "project note limit reached"
            );
            let mut note = proposal.note.clone();
            note.revision = proposal.base_revision + 1;
            arbiter_vault::validate(&note)?;
            Some(note)
        } else {
            None
        };
        self.append(thread, EventKind::VaultResolved { id: id.into(), accepted, note: accepted_note.clone() })?;
        if let Some(note) = accepted_note {
            self.mirror_note(project, &note);
        }
        Ok(())
    }
    pub(crate) fn memory_brief(&self, thread: ThreadId, text: String, limit: usize) -> Result<String> {
        if text.contains("\nProject memory (reference data;")
            || text.contains("\nYour guidance (from your memory folder")
        {
            return Ok(text);
        }
        let state = self.knowledge(self.knowledge_project(thread)?)?;
        let used: std::collections::BTreeSet<String> = self
            .store(|st| st.events(thread, 0))?
            .into_iter()
            .flat_map(|e| match e.kind {
                EventKind::MemoryUsed { notes, .. } => notes,
                _ => vec![],
            })
            .collect();
        let notes: Vec<_> =
            state.notes.into_values().filter(|n| !used.contains(&format!("{}@{}", n.id, n.revision))).collect();
        let (memory, ids) = arbiter_vault::catalog(&notes, &text, limit);
        // The user's own guidance and project instructions (both kept in the
        // memory folder), once per task and again only after an edit.
        let project = self.knowledge_project(thread)?;
        let personal = [
            (self.user_guidance(), "Your guidance (from your memory folder; follow it unless the task says otherwise)"),
            (
                self.project_instructions(project),
                "Your instructions for this project (from your memory folder; they add to AGENTS.md)",
            ),
        ];
        let fresh: Vec<(String, String)> = personal
            .into_iter()
            .filter_map(|(g, title)| {
                let g = g?;
                let mark = format!("guidance@{}", &arbiter_project::hash(g.as_bytes())[..12]);
                (!used.contains(&mark)).then(|| (mark, format!("\n\n{title}:\n{g}\n")))
            })
            .collect();
        if memory.is_empty() && fresh.is_empty() {
            return Ok(text);
        }
        let mut revisions: Vec<String> = ids
            .iter()
            .filter_map(|id| notes.iter().find(|n| &n.id == id))
            .map(|n| format!("{}@{}", n.id, n.revision))
            .collect();
        let extra: String = fresh.iter().map(|(_, t)| t.as_str()).collect();
        revisions.extend(fresh.into_iter().map(|(m, _)| m));
        self.append(thread, EventKind::MemoryUsed { notes: revisions, bytes: memory.len() + extra.len() })?;
        Ok(format!("{text}{extra}{memory}"))
    }
    /// Automatic notes contain structured summaries, never raw logs. Guards can reject them.
    pub(crate) fn knowledge_after(&self, e: &Event) -> Result<()> {
        let auto = match &e.kind {
            EventKind::Plan { event: PlanEvent::Approved { .. } } => {
                let state = self.plan_state(e.thread_id)?;
                state.plan.map(|p| {
                    (
                        format!("plans/{}", e.thread_id),
                        p.title,
                        format!(
                            "{}\n\n{}",
                            p.goal,
                            p.nodes.iter().map(|n| format!("- {}: {}", n.title, n.goal)).collect::<Vec<_>>().join("\n")
                        ),
                    )
                })
            }
            EventKind::Plan { event: PlanEvent::HandedOff { node, handoff } } => Some((
                format!("handoffs/{}", node_key(e.thread_id, node)),
                format!("Step {node} handoff"),
                format!(
                    "{}\n\nDecisions: {}\nInterfaces: {}\nOpen issues: {}\n\n[[plans/{}]]",
                    handoff.summary,
                    handoff.decisions.join("; "),
                    handoff.interfaces.join("; "),
                    handoff.open_issues.join("; "),
                    e.thread_id
                ),
            )),
            EventKind::Plan {
                event: PlanEvent::NodeState { node, status: arbiter_core::plan::NodeStatus::Blocked, .. },
            } => {
                let state = self.plan_state(e.thread_id)?;
                if let Some(step) = state.nodes.get(node)
                    && let Some(thread) = step.thread
                {
                    let t = self.store(|st| st.thread(thread))?.context("step task missing")?;
                    let outcome = Outcome {
                        task_type: arbiter_intake::classifier::fallback(&t.title).task_type,
                        harness: t.harness,
                        model: t.model,
                        passed: false,
                        checks_run: state
                            .plan
                            .as_ref()
                            .and_then(|p| p.nodes.iter().find(|n| n.id == *node))
                            .map_or(0, |n| n.checks.len()),
                        first_try: false,
                        heal_attempts: step.heal_signatures.len() as u32,
                        input_tokens: t.input_tokens,
                        output_tokens: t.output_tokens,
                        cost_usd: t.cost_usd,
                        user_rework: false,
                    };
                    self.append(thread, EventKind::OutcomeRecorded { outcome })?;
                }
                None
            }
            EventKind::Plan { event: PlanEvent::Integrated { node, .. } } => {
                let state = self.plan_state(e.thread_id)?;
                let step = state.nodes.get(node).context("step missing")?;
                if let Some(thread) = step.thread {
                    let t = self.store(|st| st.thread(thread))?.context("step task missing")?;
                    let attempts = step.heal_signatures.len() as u32;
                    let outcome = Outcome {
                        task_type: arbiter_intake::classifier::fallback(&t.title).task_type,
                        harness: t.harness,
                        model: t.model,
                        passed: true,
                        checks_run: state
                            .plan
                            .as_ref()
                            .and_then(|p| p.nodes.iter().find(|n| n.id == *node))
                            .map_or(0, |n| n.checks.len()),
                        first_try: attempts == 0,
                        heal_attempts: attempts,
                        input_tokens: t.input_tokens,
                        output_tokens: t.output_tokens,
                        cost_usd: t.cost_usd,
                        user_rework: false,
                    };
                    self.append(thread, EventKind::OutcomeRecorded { outcome })?;
                }
                if !step.heal_signatures.is_empty() {
                    Some((
                        format!("learnings/{}", node_key(e.thread_id, node)),
                        format!("Verified repair for step {node}"),
                        format!(
                            "{} acceptance repair attempts succeeded. Fix summary: {}\n[[handoffs/{}]]",
                            step.heal_signatures.len(),
                            step.handoff.as_ref().map_or("No summary", |h| h.summary.as_str()),
                            node_key(e.thread_id, node)
                        ),
                    ))
                } else {
                    None
                }
            }
            EventKind::ChecksRan { attempt, results } if !results.is_empty() => {
                if self.plan_state(e.thread_id)?.plan.is_some() {
                    return Ok(());
                }
                let t = self.store(|st| st.thread(e.thread_id))?.context("task not found")?;
                let passed = results.iter().all(|r| r.ok);
                let outcome = Outcome {
                    task_type: arbiter_intake::classifier::fallback(&t.title).task_type,
                    harness: t.harness,
                    model: t.model,
                    passed,
                    checks_run: results.len(),
                    first_try: passed && *attempt == 0,
                    heal_attempts: *attempt,
                    input_tokens: t.input_tokens,
                    output_tokens: t.output_tokens,
                    cost_usd: t.cost_usd,
                    user_rework: false,
                };
                self.append(e.thread_id, EventKind::OutcomeRecorded { outcome })?;
                if passed && *attempt > 0 {
                    let events = self.store(|st| st.events(e.thread_id, 0))?;
                    let signature = events
                        .iter()
                        .rev()
                        .find_map(|e| match &e.kind {
                            EventKind::Heal { signature, .. } => Some(signature.as_str()),
                            _ => None,
                        })
                        .unwrap_or("unknown");
                    let fix = events
                        .iter()
                        .rev()
                        .find_map(|e| match &e.kind {
                            EventKind::Agent { event: arbiter_core::AgentEvent::Message { text }, .. } => {
                                Some(text.lines().take(4).collect::<Vec<_>>().join("\n"))
                            }
                            _ => None,
                        })
                        .unwrap_or_else(|| {
                            "No structured fix summary was reported; review the checkpoint before reuse.".into()
                        });
                    Some((
                        format!("learnings/{}", e.thread_id),
                        format!("Verified repair: {}", t.title.chars().take(120).collect::<String>()),
                        format!(
                            "Failure signature: {signature}\nAgent-reported fix (checks passed; explanation unverified):\n{}\nChecks: {}",
                            fix.chars().take(1500).collect::<String>(),
                            results.iter().map(|r| r.name.clone()).collect::<Vec<_>>().join(", ")
                        ),
                    ))
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some((id, title, body)) = auto {
            let state = self.knowledge(self.knowledge_project(e.thread_id)?)?;
            let old = state.notes.get(&id);
            if old.is_none_or(|n| n.body != body) {
                self.save_note(e.thread_id, Note { id, title, body, revision: 0 }, old.map_or(0, |n| n.revision))?;
            }
        }
        Ok(())
    }
    pub(crate) fn learned_route(
        &self,
        project: ProjectId,
        text: &str,
    ) -> Result<Option<(String, Option<String>, String)>> {
        let state = self.knowledge(project)?;
        let kind = arbiter_intake::classifier::fallback(text).task_type;
        if let Some(p) = state.preferences.get(&kind) {
            if !p.enabled {
                return Ok(None);
            }
            if let Some(h) = &p.harness {
                return Ok(Some((h.clone(), p.model.clone(), format!("your {kind} routing preference"))));
            }
        }
        let mut scores = state.strengths();
        scores.retain(|s| {
            s.task_type == kind
                && s.runs >= 3
                && s.first_try > s.rework
                && s.passed * 2 >= s.runs
                && s.harness.parse().is_ok_and(|h| (self.inner.available)(h))
        });
        scores.sort_by(|a, b| {
            let score = |s: &Strength| (s.first_try.saturating_sub(s.rework) + 1) as f64 / (s.runs + 2) as f64;
            score(b).total_cmp(&score(a)).then(a.harness.cmp(&b.harness))
        });
        Ok(scores.first().map(|s| {
            (
                s.harness.clone(),
                s.model.clone(),
                format!("{} of {} {kind} tasks passed first try; {} marked for rework", s.first_try, s.runs, s.rework),
            )
        }))
    }
    pub(crate) fn routing_preference(
        &self,
        thread: ThreadId,
        task_type: String,
        harness: Option<String>,
        model: Option<String>,
        enabled: bool,
    ) -> Result<()> {
        ensure!(
            ["feature", "bugfix", "refactor", "test", "docs", "ops", "research"].contains(&task_type.as_str()),
            "invalid task type"
        );
        ensure!(harness.as_deref().is_none_or(|h| matches!(h, "claude" | "codex")), "unknown harness");
        ensure!(model.as_ref().is_none_or(|m| m.len() <= 120), "model id too long");
        self.knowledge_project(thread)?;
        self.append(thread, EventKind::RoutingPreference { task_type, harness, model, enabled })?;
        Ok(())
    }
}
