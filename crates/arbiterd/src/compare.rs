//! Opt-in best-of-N: the same request to two or three chosen agents, each in
//! its own worktree, compared side by side; the user keeps one. Never
//! automatic, because every candidate spends its own allowance.
use crate::{AppState, service::WS};
use anyhow::{Result, bail, ensure};
use arbiter_core::{EventKind, ThreadId};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
pub(crate) struct Candidate {
    pub harness: String,
    #[serde(default)]
    pub model: Option<String>,
}

const LABELS: [&str; 3] = ["A", "B", "C"];

impl AppState {
    pub(crate) async fn start_comparison(
        &self,
        project: arbiter_core::ProjectId,
        message: &str,
        candidates: Vec<Candidate>,
    ) -> Result<Value> {
        let message = message.trim();
        ensure!(!message.is_empty() && message.len() <= 12000, "describe the task to compare (up to 12000 bytes)");
        ensure!((2..=3).contains(&candidates.len()), "compare two or three agents");
        let cloud = candidates.iter().filter(|c| c.harness != "local").count();
        ensure!(
            candidates.iter().filter(|c| c.harness == "local").count() <= 1,
            "the local model can take one candidate"
        );
        let limit = self.max_cloud_runs()?;
        ensure!(
            cloud <= limit,
            "Comparing {cloud} cloud agents needs at least {cloud} parallel cloud runs; Setup allows {limit}."
        );
        // Check every candidate before creating anything, so a refusal leaves no
        // half-started comparison behind.
        for c in &candidates {
            match c.harness.as_str() {
                "claude" | "codex" => self.check_cloud(c.harness.parse().map_err(anyhow::Error::msg)?)?,
                "local" => ensure!(
                    c.model.as_deref().is_some_and(|m| self.local_model_installed(m)),
                    "choose an installed local model for the local candidate"
                ),
                other => bail!("compare explicit agents (claude, codex or local), not {other:?}"),
            }
        }
        let id = arbiter_core::RunId::new().to_string();
        let title: String = message.lines().next().unwrap_or(message).chars().take(60).collect();
        let mut threads = vec![];
        for (c, label) in candidates.into_iter().zip(LABELS) {
            let t = self
                .create_thread(crate::service::ThreadSpec {
                    project_id: project,
                    title: Some(format!("Compare {label}: {title}")),
                    message: None,
                    harness: c.harness,
                    model: c.model,
                    effort: None,
                    permission: Default::default(),
                    worktree: true,
                    attachments: vec![],
                    intake: false,
                    context_paths: vec![],
                    tool_profile: None,
                    workflow: false,
                    auto: false,
                    projects: vec![],
                })
                .await?;
            self.append(t.id, EventKind::ComparisonJoined { comparison: id.clone(), label: label.into() })?;
            threads.push(t.id);
        }
        for t in &threads {
            // A candidate that cannot start records why on its own thread; the
            // others still run.
            let _ = self.send_message(*t, message.to_owned(), &[]).await;
        }
        Ok(json!({"id":id,"threads":threads}))
    }

    fn comparison_threads(&self, id: &str) -> Result<Vec<(ThreadId, String, bool)>> {
        let mut out = vec![];
        for t in self.store(|s| s.threads(WS))? {
            let events = self.store(|s| s.events(t.id, 0))?;
            let mut label = None;
            let mut kept = false;
            for e in &events {
                match &e.kind {
                    EventKind::ComparisonJoined { comparison, label: l } if comparison == id => label = Some(l.clone()),
                    EventKind::ComparisonDecided { comparison, kept: k } if comparison == id => kept = *k == t.id,
                    _ => {}
                }
            }
            if let Some(label) = label {
                out.push((t.id, label, kept));
            }
        }
        out.sort_by(|a, b| a.1.cmp(&b.1));
        ensure!(!out.is_empty(), "comparison not found");
        Ok(out)
    }

    pub(crate) async fn comparison(&self, id: &str) -> Result<Value> {
        let mut rows = vec![];
        for (thread, label, kept) in self.comparison_threads(id)? {
            let t = self.store(|s| s.thread(thread))?.ok_or_else(|| anyhow::anyhow!("task missing"))?;
            let events = self.store(|s| s.events(thread, 0))?;
            let checks = events.iter().rev().find_map(|e| match &e.kind {
                EventKind::ChecksRan { results, .. } => Some(json!({
                    "passed":results.iter().filter(|r| r.ok).count(),"total":results.len()
                })),
                _ => None,
            });
            let stat = match self.thread_cwd(&t) {
                Ok(cwd) => arbiter_supervisor::integration::git(
                    &cwd,
                    &["diff", "--shortstat", t.base.as_deref().unwrap_or("HEAD")],
                )
                .await
                .unwrap_or_default(),
                Err(_) => String::new(),
            };
            rows.push(json!({
                "thread":thread,"label":label,"kept":kept,"harness":t.harness,"model":t.model,"status":t.status,
                "input_tokens":t.input_tokens,"output_tokens":t.output_tokens,"cost_usd":t.cost_usd,
                "checks":checks,"changes":stat.trim(),"working":self.agent_working(thread)
            }));
        }
        Ok(json!({"id":id,"candidates":rows}))
    }

    /// Keep one candidate. The others are stopped and settled; their worktrees
    /// stay on disk so nothing is lost.
    pub(crate) async fn keep_candidate(&self, id: &str, keep: ThreadId) -> Result<Value> {
        let members = self.comparison_threads(id)?;
        ensure!(members.iter().any(|(t, _, _)| *t == keep), "that task is not part of this comparison");
        ensure!(!members.iter().any(|(_, _, kept)| *kept), "this comparison already has a kept result");
        let winner = members.iter().find(|(t, _, _)| *t == keep).map(|(_, l, _)| l.clone()).unwrap_or_default();
        for (thread, label, _) in &members {
            self.append(*thread, EventKind::ComparisonDecided { comparison: id.into(), kept: keep })?;
            if *thread == keep {
                self.append(
                    *thread,
                    EventKind::Notice { text: format!("Kept candidate {label} from the comparison.") },
                )?;
            } else {
                let _ = self.stop(*thread);
                self.append(
                    *thread,
                    EventKind::Notice {
                        text: format!("Not kept: candidate {winner} was chosen. This worktree is left untouched."),
                    },
                )?;
                self.append(*thread, EventKind::Settled)?;
            }
        }
        self.comparison(id).await
    }
}
