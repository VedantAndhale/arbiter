use crate::AppState;
use anyhow::{Context, Result, ensure};
use arbiter_core::{EventKind, ThreadId, ThreadStatus};
use arbiter_supervisor::integration::git;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Deserialize)]
pub(crate) struct ReviewComment {
    pub path: String,
    pub line: u32,
    pub text: String,
}
/// Longest comment text accepted; enough for a concrete instruction.
pub(crate) const COMMENT_CHARS: usize = 500;

pub(crate) fn validate_comments(comments: &[ReviewComment]) -> Result<()> {
    ensure!(!comments.is_empty() && comments.len() <= 8, "submit one to eight review comments");
    for c in comments {
        arbiter_plan::validate_pattern(&c.path)?;
        ensure!(
            !c.path.contains(['*', '?']) && !c.text.trim().is_empty() && c.text.len() <= COMMENT_CHARS && c.line > 0,
            "review comment exceeds bounds"
        );
    }
    Ok(())
}

impl AppState {
    /// Send a batch of diff comments. A finished plan gets one fix node that
    /// must be approved; a single agent gets them as one structured message.
    pub(crate) async fn review(
        &self,
        id: ThreadId,
        revision: Option<u32>,
        comments: Vec<ReviewComment>,
        project: Option<String>,
    ) -> Result<Value> {
        validate_comments(&comments)?;
        let plan = self.plan_state(id)?;
        if plan.plan.is_some() {
            let revision = revision.unwrap_or(plan.revision);
            self.request_review_fixes(id, revision, comments, project).await?;
            return Ok(json!({"kind":"plan","plan":self.plan_state(id)?}));
        }
        ensure!(
            !self.agent_working(id),
            crate::runs::Busy("Wait for the agent to finish before sending review comments".into())
        );
        let mut text =
            String::from("Review comments on your changes. Address each one; keep everything else as it is.\n");
        for c in &comments {
            text.push_str(&format!("- {}:{}: {}\n", c.path, c.line, c.text.trim()));
        }
        self.send_message(id, text, &[]).await?;
        Ok(json!({"kind":"message"}))
    }

    /// A commit message and PR text assembled from what the agents reported:
    /// handoff summaries, the diff stat and the latest checks. Deterministic
    /// and free; the user edits it before anything is committed or published.
    pub(crate) async fn landing_draft(
        &self,
        id: ThreadId,
        path: &std::path::Path,
        project: Option<&str>,
    ) -> Result<Value> {
        let t = self.store(|s| s.thread(id))?.context("task missing")?;
        let plan = self.plan_state(id)?;
        let title = plan.plan.as_ref().map(|p| p.title.clone()).unwrap_or_else(|| t.title.clone());
        let title = title
            .split_once(": ")
            .filter(|(k, _)| k.len() <= 12 && !k.contains(' '))
            .map(|(_, r)| r.to_owned())
            .unwrap_or(title);
        let title = clip(title.trim(), 72);
        let events = self.store(|s| s.events(id, 0))?;
        // Only the steps that changed this project describe its commit.
        let here = |node: &str| {
            plan.plan.as_ref().is_none_or(|p| p.nodes.iter().any(|n| n.id == node && n.project.as_deref() == project))
        };
        let mut summaries: Vec<String> = plan
            .nodes
            .iter()
            .filter(|(id, _)| here(id))
            .filter_map(|(_, n)| n.handoff.as_ref().map(|h| format!("- {}", clip(&h.summary, 300))))
            .collect();
        if summaries.is_empty()
            && let Some(last) = events.iter().rev().find_map(|e| match &e.kind {
                EventKind::Agent { event: arbiter_core::AgentEvent::Message { text }, .. } => Some(text.clone()),
                _ => None,
            })
        {
            summaries.push(clip(last.trim(), 800));
        }
        let base = self.landing_base(id, project)?.unwrap_or_else(|| "HEAD".into());
        let stat = git(path, &["diff", "--stat", "--no-color", &base]).await.unwrap_or_default();
        let checks = events.iter().rev().find_map(|e| match &e.kind {
            EventKind::ChecksRan { results, .. } => Some(
                results
                    .iter()
                    .map(|r| format!("- {}: {}", r.name, if r.ok { "passed" } else { "failed" }))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            _ => None,
        });
        let body = format!(
            "## Summary\n{}\n\n## Changes\n```\n{}\n```\n\n## Checks\n{}\n\nDrafted by Arbiter from agent handoffs; reviewed before publishing.",
            if summaries.is_empty() { "(no agent summary recorded)".into() } else { summaries.join("\n") },
            clip(stat.trim(), 1500),
            checks.unwrap_or_else(|| "No checks were recorded.".into()),
        );
        Ok(json!({"message":title,"title":title,"body":clip(&body, 5000)}))
    }

    /// Where a task's changes started, per project for multi-project plans.
    pub(crate) fn landing_base(&self, id: ThreadId, project: Option<&str>) -> Result<Option<String>> {
        let plan = self.plan_state(id)?;
        Ok(match project {
            Some(p) => plan.integrations.get(p).map(|i| i.base.clone()),
            None => plan.base.clone().or(self.store(|s| s.thread(id))?.context("task missing")?.base),
        })
    }
    pub(crate) async fn landing_path(&self, id: ThreadId, project: Option<&str>) -> Result<(PathBuf, String)> {
        let t = self.store(|s| s.thread(id))?.context("task missing")?;
        ensure!(
            !self.agent_working(id) && !matches!(t.status, ThreadStatus::Running | ThreadStatus::Healing),
            "stop the agent before landing changes"
        );
        let plan = self.plan_state(id)?;
        let path = if plan.plan.is_some() {
            ensure!(plan.completed.is_some(), "finish the plan before landing");
            PathBuf::from(plan.integration(project).context("plan workspace missing")?.0)
        } else {
            ensure!(project.is_none(), "this task has one project");
            PathBuf::from(t.worktree.context("landing requires an isolated worktree")?)
        };
        let branch = git(&path, &["branch", "--show-current"]).await?;
        ensure!(branch.starts_with("arbiter/"), "only Arbiter work branches can be committed or published here");
        Ok((path, branch))
    }
    pub(crate) async fn landing_status(&self, id: ThreadId, project: Option<&str>) -> Result<Value> {
        let (path, branch) = self.landing_path(id, project).await?;
        let head = git(&path, &["rev-parse", "HEAD"]).await?;
        let status = git(&path, &["status", "--porcelain", "-uall"]).await?;
        let diff = git(&path, &["diff", "--no-ext-diff", "--no-textconv", "HEAD"]).await?;
        let fingerprint = arbiter_project::hash(format!("{head}\n{status}\n{diff}").as_bytes());
        // New files: untracked, or only marked intent-to-add (" A"), which a
        // plain `commit -a` would otherwise include without anyone choosing it.
        let untracked: Vec<&str> =
            status.lines().filter_map(|l| l.strip_prefix("?? ").or_else(|| l.strip_prefix(" A "))).take(100).collect();
        let draft = self.landing_draft(id, &path, project).await?;
        // Commits on this branch since it started: tells "committed, ready to
        // publish" apart from "nothing changed" when the tree is clean.
        let ahead = match self.landing_base(id, project)? {
            Some(b) => {
                git(&path, &["rev-list", "--count", &format!("{b}..HEAD")]).await?.trim().parse::<u32>().unwrap_or(0)
            }
            None => 0,
        };
        Ok(json!({"branch":branch,"head":head,"status":status,"fingerprint":fingerprint,"clean":status.is_empty(),
                // Untracked files never go into a publish, so only tracked
                // changes must be committed first.
                "committed":status.lines().all(|l| l.starts_with("?? ") || l.starts_with(" A ")),"untracked":untracked,"draft":draft,"ahead":ahead}))
    }
    pub(crate) async fn commit_reviewed(
        &self,
        id: ThreadId,
        fingerprint: &str,
        message: &str,
        include: &[String],
        project: Option<&str>,
    ) -> Result<Value> {
        let _lock = self.inner.plan_lock.lock().await;
        ensure!(!message.trim().is_empty() && message.len() <= 200, "use a commit message of 1–200 bytes");
        let current = self.landing_status(id, project).await?;
        ensure!(current["fingerprint"] == fingerprint, "changes moved since review; refresh and review again");
        let (path, _) = self.landing_path(id, project).await?;
        // Tracked changes, plus only the new files the user ticked. Never sweep
        // an untracked credential into a commit.
        let untracked: Vec<String> = current["untracked"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        ensure!(include.len() <= 100, "too many new files selected");
        for file in include {
            ensure!(untracked.contains(file), "a selected new file is no longer untracked; refresh and review again");
            ensure!(
                !file.split('/').any(|s| s.starts_with(".env") || ["credentials", "secrets"].contains(&s))
                    && ![".pem", ".key", ".p12", ".pfx"].iter().any(|x| file.to_lowercase().ends_with(x)),
                "{file} looks like a credential file and cannot be committed here"
            );
        }
        // Unmark new files the user did not choose, so `commit -a` leaves them out.
        let skipped: Vec<&str> = untracked.iter().filter(|p| !include.contains(p)).map(String::as_str).collect();
        for chunk in skipped.chunks(20) {
            let mut args = vec!["reset", "-q", "--"];
            args.extend(chunk);
            git(&path, &args).await?;
        }
        for chunk in include.chunks(20) {
            let mut args = vec!["add", "--"];
            args.extend(chunk.iter().map(String::as_str));
            git(&path, &args).await?;
        }
        let left_out = untracked.len() - include.len();
        let hooks = self.inner.home.join("empty-hooks");
        std::fs::create_dir_all(&hooks)?;
        git(
            &path,
            &[
                "-c",
                &format!("core.hooksPath={}", hooks.display()),
                "-c",
                "commit.gpgSign=false",
                "commit",
                "-am",
                message,
            ],
        )
        .await?;
        self.append(
            id,
            EventKind::Notice {
                text: if left_out > 0 {
                    format!("Created the reviewed local commit. {left_out} new file(s) were left out. Nothing has been pushed.")
                } else {
                    "Created the reviewed local commit. Nothing has been pushed.".into()
                },
            },
        )?;
        self.landing_status(id, project).await
    }
}

pub(crate) async fn gh(path: &std::path::Path, args: &[&str]) -> Result<String> {
    let mut command = tokio::process::Command::new("gh");
    command
        .current_dir(path)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = arbiter_supervisor::SupervisedChild::spawn(command)?;
    let mut child = child;
    use tokio::io::AsyncReadExt;
    let mut stdout = child.child.stdout.take().context("PR stdout missing")?;
    let mut stderr = child.child.stderr.take().context("PR stderr missing")?;
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        let mut out = vec![];
        let mut err = vec![];
        tokio::try_join!(stdout.read_to_end(&mut out), stderr.read_to_end(&mut err))?;
        let status = child.child.wait().await?;
        ensure!(
            status.success(),
            "GitHub CLI: {}",
            String::from_utf8_lossy(&err).chars().take(1000).collect::<String>()
        );
        Ok::<_, anyhow::Error>(String::from_utf8_lossy(&out).trim().to_owned())
    })
    .await
    .context("GitHub CLI timed out; check the remote PR before retrying")?
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_owned() } else { s.chars().take(n).collect::<String>() + "…" }
}
