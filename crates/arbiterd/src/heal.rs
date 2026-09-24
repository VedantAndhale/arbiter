//! What happens after an agent says it is done: snapshot the files, run the
//! project's checks, and either hand the thread to the user or feed the agent
//! a minimal excerpt of what broke, within strict loop bounds.
//!
//! Also the per-thread recovery state (crash auto-resume, rate-limit retry).

use crate::AppState;
use anyhow::{Context, Result};
use arbiter_core::{CheckResult, EventKind, PermissionMode, ThreadId, ThreadStatus};
use arbiter_heal::excerpt::Failed;
use arbiter_heal::{Check, CheckKind, CheckRun, build_excerpt, is_frontend_file, parse, run_check, run_setup};
use arbiter_supervisor::checkpoint;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Per-thread loop and recovery bookkeeping. Reset whenever the user speaks.
#[derive(Default, Debug)]
pub(crate) struct ThreadHeal {
    pub attempts: u32,
    last_signature: Option<String>,
    repeats: u32,
    pub crash_retries: u32,
    pub rate_limit_retries: u32,
    /// Bumped on every user message; a scheduled retry only fires if unchanged.
    pub generation: u64,
}

pub(crate) const RESUME_AFTER_CRASH: &str =
    "Your previous process exited unexpectedly. Continue the task from where you left off; don't redo finished work.";
pub(crate) const RESUME_AFTER_LIMIT: &str = "The rate limit has reset. Continue the task from where you left off.";

impl AppState {
    pub(crate) fn heal_state<T>(&self, t: ThreadId, f: impl FnOnce(&mut ThreadHeal) -> T) -> T {
        f(self.inner.heal.lock().unwrap().entry(t).or_default())
    }

    /// A human message starts a fresh budget of heal attempts and retries.
    pub(crate) fn reset_heal(&self, t: ThreadId) {
        self.heal_state(t, |h| {
            let generation = h.generation + 1;
            *h = ThreadHeal { generation, ..Default::default() };
        });
    }

    pub(crate) fn thread_cwd(&self, t: &arbiter_store::ThreadSummary) -> Result<PathBuf> {
        Ok(match &t.worktree {
            Some(w) => PathBuf::from(w),
            None => PathBuf::from(self.store(|st| st.project(t.project_id))?.path),
        })
    }

    /// Entry point after a completed turn. Never leaves the thread stuck in
    /// `healing`: any internal error is reported and the thread goes to review.
    pub(crate) async fn after_turn(self, thread: ThreadId) {
        if let Err(e) = self.after_turn_inner(thread).await {
            self.append_logged(thread, EventKind::Notice { text: format!("Checks could not run: {e:#}") });
            self.append_logged(thread, EventKind::StatusChanged { status: ThreadStatus::Review });
        }
        self.auto_preview(thread).await;
    }

    async fn after_turn_inner(&self, thread: ThreadId) -> Result<()> {
        let t = self.store(|st| st.thread(thread))?.context("thread not found")?;
        let cwd = self.thread_cwd(&t)?;
        self.checkpoint_now(thread, &t, &cwd).await;

        let review = |s: &Self| s.append(thread, EventKind::StatusChanged { status: ThreadStatus::Review }).map(drop);
        if !self.inner.heal_enabled || t.permission == PermissionMode::Plan {
            return review(self);
        }
        if let Some(cap) = t.budget_usd
            && t.cost_usd >= cap
        {
            self.append(thread, EventKind::Notice { text: format!("Budget of ${cap:.2} reached; checks skipped.") })?;
            return self.append(thread, EventKind::StatusChanged { status: ThreadStatus::NeedsApproval }).map(drop);
        }
        let cfg = self.heal_config(Some(thread), &cwd)?;
        if !cfg.enabled || (cfg.checks.is_empty() && cfg.browser.is_none()) {
            return review(self);
        }
        let base = match &t.base {
            Some(b) => b.clone(),
            None => checkpoint::head(&cwd).await?,
        };
        let changed = checkpoint::changed_files(&cwd, &base).await?;
        if changed.is_empty() {
            return review(self);
        }

        self.append(thread, EventKind::StatusChanged { status: ThreadStatus::Healing })?;
        if let Some(setup) = &cfg.setup {
            let r = run_setup(setup, &cwd).await;
            if !r.ok {
                let tail: String =
                    r.output.lines().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
                self.append(
                    thread,
                    EventKind::Notice {
                        text: format!("Environment setup failed (`{setup}`), so checks were skipped:\n{tail}"),
                    },
                )?;
                return review(self);
            }
        }

        let mut results = Vec::new();
        let mut failed_runs = Vec::new();
        for check in &cfg.checks {
            let mut run = run_check(check, &cwd).await;
            let mut fixed = false;
            // Deterministic fixers are free; try them before spending a turn.
            if !run.ok
                && let Some(fix) = &check.fix
            {
                let fixer = Check { cmd: fix.clone(), ..check.clone() };
                if run_check(&fixer, &cwd).await.ok {
                    fixed = true;
                    run = run_check(check, &cwd).await;
                }
            }
            let failures = if run.ok { Vec::new() } else { parse(&run.output) };
            results.push(CheckResult {
                name: check.name.clone(),
                ok: run.ok,
                duration_ms: run.duration_ms,
                failures: failures.len() as u32,
                timed_out: run.timed_out,
                fixed,
            });
            if !run.ok {
                failed_runs.push((check.clone(), run, failures));
            }
        }
        // Front-end changes: load the app and treat runtime errors as failures.
        if changed.iter().any(|f| is_frontend_file(f))
            && let Some((result, failures, label)) = self.browser_checks(thread, &cwd).await
        {
            if !result.ok {
                let check =
                    Check { name: "browser".into(), kind: CheckKind::Test, cmd: label, fix: None, timeout_secs: 0 };
                let run = CheckRun {
                    name: "browser".into(),
                    ok: false,
                    exit_code: None,
                    output: String::new(),
                    duration_ms: result.duration_ms,
                    timed_out: false,
                };
                failed_runs.push((check, run, failures));
            }
            results.push(result);
        }
        let attempt = self.heal_state(thread, |h| h.attempts);
        self.append(thread, EventKind::ChecksRan { attempt, results })?;
        if failed_runs.is_empty() {
            return review(self);
        }

        let policy = &self.inner.heal_policy;
        let failed: Vec<Failed> =
            failed_runs.iter().map(|(c, r, f)| Failed { check: c, run: r, failures: f.clone() }).collect();
        let ex = build_excerpt(&failed, &cwd, &changed, policy.excerpt_chars, attempt + 1, policy.max_attempts);
        if ex.count == 0 {
            self.append(
                thread,
                EventKind::Notice {
                    text: format!(
                        "{} pre-existing failure(s) in files the agent didn't touch were ignored.",
                        ex.ignored
                    ),
                },
            )?;
            return review(self);
        }

        enum Next {
            Heal(u32),
            Stop(String),
        }
        let next = self.heal_state(thread, |h| {
            h.repeats = if h.last_signature.as_deref() == Some(ex.signature.as_str()) { h.repeats + 1 } else { 1 };
            h.last_signature = Some(ex.signature.clone());
            if h.attempts >= policy.max_attempts {
                Next::Stop(format!("Healing stopped after {} attempts; checks still fail.", h.attempts))
            } else if h.repeats >= policy.same_signature_limit {
                Next::Stop("Healing stopped: the same failures came back after a fix attempt.".into())
            } else {
                h.attempts += 1;
                Next::Heal(h.attempts)
            }
        });
        match next {
            Next::Stop(why) => {
                self.append(
                    thread,
                    EventKind::Notice { text: format!("{why} Review the failures or give the agent a hint.") },
                )?;
                self.append(thread, EventKind::StatusChanged { status: ThreadStatus::Failed })?;
            }
            Next::Heal(n) => {
                self.append(
                    thread,
                    EventKind::Heal { attempt: n, signature: ex.signature.clone(), excerpt: ex.text.clone() },
                )?;
                self.deliver(thread, ex.text)?;
            }
        }
        Ok(())
    }

    /// Snapshot the working files after a turn; unchanged trees are skipped.
    pub(crate) async fn checkpoint_now(&self, thread: ThreadId, t: &arbiter_store::ThreadSummary, cwd: &Path) {
        let n = t.checkpoints + 1;
        let refname = format!("refs/arbiter/cp/{}/{n}", thread.0.simple());
        match checkpoint::checkpoint(cwd, &refname, t.last_checkpoint.as_deref(), &format!("arbiter turn {n}")).await {
            Ok(Some(commit)) => self.append_logged(thread, EventKind::Checkpoint { n, commit }),
            Ok(None) => {}
            Err(e) => tracing::warn!(%thread, "checkpoint failed: {e:#}"),
        }
    }

    /// Wait for a rate-limit reset (or back off), then continue the thread
    /// unless the user has said something in the meantime.
    pub(crate) fn schedule_rate_limit_retry(&self, thread: ThreadId, resets_at: Option<i64>, message: String) {
        let (generation, n) = self.heal_state(thread, |h| {
            h.rate_limit_retries += 1;
            (h.generation, h.rate_limit_retries)
        });
        if n > 6 {
            self.append_logged(
                thread,
                EventKind::Notice { text: "Still rate limited after 6 retries; giving up.".into() },
            );
            self.append_logged(thread, EventKind::StatusChanged { status: ThreadStatus::Failed });
            return;
        }
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        // Known reset: wait for it (+jitter). Unknown: 1, 2, 4… minutes, capped at 30.
        let wait = match resets_at {
            Some(at) if at > now => (at - now) as u64 + 5 + (thread.0.as_u128() % 20) as u64,
            _ => (60u64 << (n - 1).min(5)).min(1800),
        }
        .min(6 * 3600);
        let when = time::OffsetDateTime::now_utc() + Duration::from_secs(wait);
        self.append_logged(thread, EventKind::RateLimited { resets_at: Some(when.unix_timestamp()), message });
        self.append_logged(thread, EventKind::StatusChanged { status: ThreadStatus::Idle });
        let s = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(s.inner.retry_scale.mul_f64(wait as f64)).await;
            if s.heal_state(thread, |h| h.generation) != generation {
                return; // the user took over
            }
            let _ = s.deliver(thread, RESUME_AFTER_LIMIT.into());
        });
    }
}

/// Provider-agnostic rate-limit detection for error text.
pub(crate) fn is_rate_limit(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    ["rate limit", "rate_limit", "usage limit", "too many requests", "429", "overloaded", "quota"]
        .iter()
        .any(|k| m.contains(k))
}

#[cfg(test)]
mod tests {
    #[test]
    fn rate_limit_detection() {
        assert!(super::is_rate_limit("API Error: 429 Too Many Requests"));
        assert!(super::is_rate_limit("You've hit your usage limit. Try again at 3pm."));
        assert!(!super::is_rate_limit("TypeError: x is undefined"));
    }
}
