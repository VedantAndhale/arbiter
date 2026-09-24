//! The user's terminal drawer and file editor for a task's working copy.
//! Terminal output and edited files are the user's; nothing here is sent to
//! an agent.
use crate::AppState;
use anyhow::{Context, Result, ensure};
use arbiter_core::ThreadId;
use arbiter_supervisor::pty::Terminal;
use serde_json::{Value, json};
use std::sync::Arc;

/// Shells running at once, across all tasks.
const MAX_TERMINALS: usize = 8;
/// Largest file the editor opens.
const EDIT_CAP: usize = 1024 * 1024;

pub(crate) type Terminals = std::collections::HashMap<ThreadId, Arc<Terminal>>;

impl AppState {
    /// The task's shell, started in its working copy on first use.
    pub(crate) fn terminal(&self, thread: ThreadId, cols: u16, rows: u16) -> Result<Arc<Terminal>> {
        let mut terms = self.inner.terminals.lock().unwrap();
        terms.retain(|_, t| t.is_alive());
        if let Some(t) = terms.get(&thread) {
            t.resize(cols, rows);
            return Ok(t.clone());
        }
        ensure!(terms.len() < MAX_TERMINALS, "Too many terminals are open; close one first");
        let t = self.store(|s| s.thread(thread))?.context("task not found")?;
        let cwd = self.thread_cwd(&t)?;
        // Daemon-held credentials never reach the user's shell environment.
        let term = Terminal::spawn(&cwd, cols, rows, &["CONTEXT7_API_KEY", "ARBITER_TOKEN"])?;
        terms.insert(thread, term.clone());
        Ok(term)
    }

    pub(crate) fn close_terminal(&self, thread: ThreadId) -> bool {
        match self.inner.terminals.lock().unwrap().remove(&thread) {
            Some(t) => {
                t.kill();
                true
            }
            None => false,
        }
    }

    fn editable(&self, thread: ThreadId, rel: &str) -> Result<std::path::PathBuf> {
        let t = self.store(|s| s.thread(thread))?.context("task not found")?;
        let root = self.thread_cwd(&t)?;
        let norm = rel.replace('\\', "/");
        ensure!(!crate::intake::sensitive(&norm), "secret and configuration files cannot be opened here");
        Ok(arbiter_project::safe_path(&root, &norm)?)
    }

    /// Files in the task's working copy: tracked plus new, ignoring build output.
    pub(crate) async fn working_files(&self, thread: ThreadId) -> Result<Vec<String>> {
        let t = self.store(|s| s.thread(thread))?.context("task not found")?;
        let root = self.thread_cwd(&t)?;
        let out =
            arbiter_supervisor::integration::git(&root, &["ls-files", "--cached", "--others", "--exclude-standard"])
                .await?;
        Ok(out
            .lines()
            .filter(|p| !p.is_empty() && !crate::intake::sensitive(p))
            .take(5000)
            .map(str::to_owned)
            .collect())
    }

    pub(crate) fn read_file(&self, thread: ThreadId, rel: &str) -> Result<Value> {
        let path = self.editable(thread, rel)?;
        let meta = std::fs::metadata(&path).context("file not found")?;
        ensure!(meta.is_file() && meta.len() as usize <= EDIT_CAP, "only text files up to 1 MB can be edited here");
        let bytes = std::fs::read(&path)?;
        let text = String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("this file is not text"))?;
        Ok(json!({"path":rel,"content":text,"hash":arbiter_project::hash(text.as_bytes())}))
    }

    /// Save an edit if the file is unchanged since it was opened.
    pub(crate) fn write_file(&self, thread: ThreadId, rel: &str, content: &str, hash: &str) -> Result<Value> {
        ensure!(
            !self.agent_working(thread),
            crate::runs::Busy("The agent is editing files right now; save when it finishes".into())
        );
        ensure!(content.len() <= EDIT_CAP, "the file is too large to save here");
        let path = self.editable(thread, rel)?;
        let current = std::fs::read(&path).unwrap_or_default();
        ensure!(
            arbiter_project::hash(&current) == hash,
            crate::runs::Busy("The file changed since you opened it; reopen it to see the latest version".into())
        );
        let staging = path.with_extension(format!("{}.arbiter-tmp", arbiter_core::RunId::new()));
        std::fs::write(&staging, content)?;
        std::fs::rename(&staging, &path)?;
        Ok(json!({"path":rel,"hash":arbiter_project::hash(content.as_bytes())}))
    }
}
