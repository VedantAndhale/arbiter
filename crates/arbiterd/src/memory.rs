//! Your memory, kept apart from your projects. A git repository in
//! `<home>/memory` holds notes, your own guidance, and a text backup of
//! Arbiter's database. Nothing personal is written into project repositories,
//! which other people share. The folder is committed locally every few
//! minutes. The app asks to push it when you close it, and after an unclean
//! stop (a flat battery, say) it pushes on the next start.
use crate::AppState;
use anyhow::{Context, Result, ensure};
use arbiter_core::NoWindow;
use arbiter_supervisor::integration::git;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const COMMIT_EVERY: Duration = Duration::from_secs(600);
const GUIDANCE_TEMPLATE: &str = "# About me\n\nWrite how you like to work: languages, style, tools, things agents should always or never do.\nArbiter gives agents the first 1200 characters of this file. Delete this paragraph when you start.\n";
const GUIDANCE_CHARS: usize = 1200;
const INSTRUCTIONS_TEMPLATE: &str = "# My instructions for this project\n\nYour own additions to the project's AGENTS.md: they stay in your memory, not in the shared repository. Arbiter gives agents the first 1200 characters. Delete this paragraph when you start.\n\nTo change which checks run for you, add `healing.toml` next to this file (same format as `.arbiter/healing.toml`).\n";
const README: &str = "# Arbiter memory\n\nThis folder is yours. It is kept apart from your projects, so nothing personal ends up in a repository you share.\n\n- `guidance/about-me.md`: how you like to work. Agents read the start of it.\n- `projects/<name>/notes/`: notes Arbiter keeps about each project.\n- `projects/<name>/instructions.md`: your own additions to a project's AGENTS.md. `healing.toml` next to it changes which checks run, for you only.\n- `backup/`: a text backup of Arbiter's history, tasks and settings. Anything that looks like a secret is replaced with `[redacted]`.\n\nTo restore on a new computer, clone this repository and run `arbiterd --restore <this folder>` before the first start.\n";

#[derive(Default, Serialize, Deserialize)]
struct Keeper {
    /// Last event id written to `backup/events`.
    cursor: i64,
    /// Set when the app closed normally; cleared on start.
    closed_cleanly: bool,
    last_push: Option<u64>,
}

/// Replace secret-looking strings. Backups may be pushed, so this runs on
/// everything written under `backup/`.
pub(crate) fn redact(text: &str) -> String {
    static PATTERNS: std::sync::OnceLock<Vec<regex::Regex>> = std::sync::OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        [
            r"sk-[A-Za-z0-9_\-]{16,}",
            r"gh[pousr]_[A-Za-z0-9]{20,}",
            r"github_pat_[A-Za-z0-9_]{20,}",
            r"xox[baprs]-[A-Za-z0-9\-]{10,}",
            r"AKIA[0-9A-Z]{16}",
            r"AIza[0-9A-Za-z_\-]{30,}",
            r"(?i)bearer\s+[A-Za-z0-9._\-]{20,}",
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
            r#"(?i)(password|passwd|secret|api[_-]?key|token)(\\?["']?\s*[:=]\s*\\?["']?)[^\s"'\\,}]{6,}"#,
        ]
        .iter()
        .map(|p| regex::Regex::new(p).unwrap())
        .collect()
    });
    let mut out = text.to_owned();
    for (i, p) in patterns.iter().enumerate() {
        out = if i == patterns.len() - 1 {
            p.replace_all(&out, "$1$2[redacted]").into_owned()
        } else {
            p.replace_all(&out, "[redacted]").into_owned()
        };
    }
    out
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').chars().take(40).collect()
}

impl AppState {
    pub(crate) fn memory_dir(&self) -> PathBuf {
        self.inner.home.join("memory")
    }

    fn keeper_file(&self) -> PathBuf {
        self.inner.home.join("memory-state.json")
    }

    fn keeper(&self) -> Keeper {
        std::fs::read(self.keeper_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save_keeper(&self, k: &Keeper) -> Result<()> {
        std::fs::write(self.keeper_file(), serde_json::to_vec_pretty(k)?)?;
        Ok(())
    }

    /// A project's folder inside the memory repository: notes, your
    /// instructions and your check overrides. Never inside the project.
    pub(crate) fn project_folder(&self, project: arbiter_core::ProjectId) -> Result<PathBuf> {
        let p = self.store(|st| st.project(project))?;
        let short: String = project.0.simple().to_string().chars().rev().take(6).collect();
        Ok(self.memory_dir().join("projects").join(format!("{}-{short}", slug(&p.name))))
    }

    /// Where a project's notes live inside the memory folder.
    pub(crate) fn project_memory(&self, project: arbiter_core::ProjectId) -> Result<PathBuf> {
        Ok(self.project_folder(project)?.join("notes"))
    }

    /// Checks for a thread's project: your override in the memory repository,
    /// else a `.arbiter/healing.toml` the team committed, else detection.
    pub(crate) fn heal_config(
        &self,
        thread: Option<arbiter_core::ThreadId>,
        cwd: &Path,
    ) -> Result<arbiter_heal::HealConfig> {
        let personal = thread
            .and_then(|t| self.store(|st| st.thread(t)).ok().flatten())
            .and_then(|t| self.project_folder(t.project_id).ok())
            .map(|d| d.join("healing.toml"));
        arbiter_heal::detect_with(cwd, personal.as_deref())
    }

    /// Your own instructions for one project, layered after its AGENTS.md.
    pub(crate) fn project_instructions(&self, project: arbiter_core::ProjectId) -> Option<String> {
        let text = std::fs::read_to_string(self.project_folder(project).ok()?.join("instructions.md")).ok()?;
        let text = text.trim();
        if text.is_empty() || text.contains("Delete this paragraph when you start") {
            return None;
        }
        arbiter_vault::guard(text).ok()?;
        Some(text.chars().take(GUIDANCE_CHARS).collect())
    }

    /// Create the memory repository the first time. No questions asked.
    pub(crate) async fn ensure_memory(&self) -> Result<PathBuf> {
        // Start-up and the first request can both get here; create it once.
        static INIT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _once = INIT.lock().await;
        let dir = self.memory_dir();
        std::fs::create_dir_all(dir.join("guidance"))?;
        std::fs::create_dir_all(dir.join("backup").join("events"))?;
        if !dir.join(".git").exists() {
            git(&dir, &["init", "-q", "-b", "main"]).await?;
        }
        for (path, text) in [("README.md", README), ("guidance/about-me.md", GUIDANCE_TEMPLATE)] {
            if !dir.join(path).exists() {
                std::fs::write(dir.join(path), text)?;
            }
        }
        Ok(dir)
    }

    /// Your guidance for agents, bounded, or none while it is still the template.
    pub(crate) fn user_guidance(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.memory_dir().join("guidance").join("about-me.md")).ok()?;
        let text = text.trim();
        if text.is_empty() || text == GUIDANCE_TEMPLATE.trim() || text.contains("Delete this paragraph when you start")
        {
            return None;
        }
        arbiter_vault::guard(text).ok()?;
        Some(text.chars().take(GUIDANCE_CHARS).collect())
    }

    /// Write new events, tables, settings and notes into the memory folder.
    async fn export_memory(&self) -> Result<()> {
        let dir = self.ensure_memory().await?;
        let mut keeper = self.keeper();
        loop {
            let batch = self.store(|st| st.backup_events(keeper.cursor, 2000))?;
            let Some(last) = batch.last().map(|e| e.id) else { break };
            let mut files: std::collections::BTreeMap<String, Vec<String>> = Default::default();
            for mut e in batch {
                e.payload = redact(&e.payload);
                let month = e.ts.get(..7).unwrap_or("unknown").to_owned();
                files.entry(month).or_default().push(serde_json::to_string(&e)?);
            }
            for (month, lines) in files {
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join("backup").join("events").join(format!("{month}.jsonl")))?;
                for l in lines {
                    writeln!(f, "{l}")?;
                }
            }
            keeper.cursor = last;
            self.save_keeper(&keeper)?;
        }
        let tables = self.store(|st| st.backup_tables())?;
        std::fs::write(dir.join("backup").join("tables.json"), redact(&serde_json::to_string_pretty(&tables)?))?;
        let prefs = self.inner.setup.lock().unwrap().read()?;
        std::fs::write(dir.join("backup").join("preferences.json"), serde_json::to_vec_pretty(&prefs)?)?;
        let settings = dir.join("backup").join("settings");
        for sub in ["publish", "preview"] {
            let from = self.inner.home.join(sub);
            if let Ok(entries) = std::fs::read_dir(&from) {
                std::fs::create_dir_all(settings.join(sub))?;
                for e in entries.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "json")) {
                    std::fs::copy(e.path(), settings.join(sub).join(e.file_name()))?;
                }
            }
        }
        // Only the list of signed-in sites; cookies stay in the browser profile.
        if self.inner.home.join("web-sites.json").exists() {
            std::fs::copy(self.inner.home.join("web-sites.json"), settings.join("web-sites.json")).ok();
        }
        for p in self.store(|st| st.projects(crate::service::WS))? {
            if let Ok(folder) = self.project_folder(p.id) {
                std::fs::create_dir_all(&folder)?;
                if !folder.join("instructions.md").exists() {
                    std::fs::write(folder.join("instructions.md"), INSTRUCTIONS_TEMPLATE)?;
                }
            }
            let Ok(state) = self.knowledge(p.id) else { continue };
            let Ok(target) = self.project_memory(p.id) else { continue };
            for note in state.notes.values() {
                let _ = arbiter_vault::mirror_to(&target, note);
            }
        }
        Ok(())
    }

    async fn commit_memory(&self, message: &str) -> Result<bool> {
        let dir = self.ensure_memory().await?;
        git(&dir, &["add", "-A"]).await?;
        if git(&dir, &["status", "--porcelain"]).await?.is_empty() {
            return Ok(false);
        }
        git(&dir, &["-c", "commit.gpgSign=false", "commit", "-q", "-m", message]).await?;
        Ok(true)
    }

    /// Commits not yet on the remote, or `None` without a remote.
    async fn memory_unpushed(&self, dir: &Path) -> Option<u32> {
        git(dir, &["remote", "get-url", "origin"]).await.ok()?;
        let range = if git(dir, &["rev-parse", "--verify", "-q", "origin/main"]).await.is_ok() {
            "origin/main..HEAD"
        } else {
            "HEAD"
        };
        Some(git(dir, &["rev-list", "--count", range]).await.ok()?.trim().parse().unwrap_or(0))
    }

    async fn push_memory(&self, dir: &Path) -> Result<()> {
        ensure!(self.inner.setup.lock().unwrap().read()?.network, "network access is disabled");
        git(dir, &["push", "-q", "-u", "origin", "main"]).await.context("pushing your memory failed")?;
        let mut k = self.keeper();
        k.last_push = Some(crate::projects::now());
        self.save_keeper(&k)?;
        Ok(())
    }

    /// Earlier versions kept notes and attachment copies inside projects
    /// (git-excluded). Move the notes into the memory repository and delete
    /// those leftovers. Tracked files are never touched.
    pub(crate) async fn migrate_project_leftovers(&self) -> Result<usize> {
        let mut moved = 0;
        for p in self.store(|st| st.projects(crate::service::WS))? {
            let root = PathBuf::from(&p.path);
            for sub in ["vault", "attachments"] {
                let dir = root.join(".arbiter").join(sub);
                let Ok(meta) = std::fs::symlink_metadata(&dir) else { continue };
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    continue;
                }
                let tracked = git(&root, &["ls-files", "--", &format!(".arbiter/{sub}")]).await.unwrap_or_default();
                if !tracked.trim().is_empty() {
                    continue;
                }
                if sub == "vault" {
                    let target = self.project_memory(p.id)?;
                    for category in
                        std::fs::read_dir(&dir)?.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                    {
                        let to = target.join(category.file_name());
                        std::fs::create_dir_all(&to)?;
                        for note in std::fs::read_dir(category.path())?
                            .flatten()
                            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                        {
                            if !to.join(note.file_name()).exists() {
                                std::fs::copy(note.path(), to.join(note.file_name()))?;
                                moved += 1;
                            }
                        }
                    }
                }
                std::fs::remove_dir_all(&dir)?;
            }
            let arbiter = root.join(".arbiter");
            if std::fs::read_dir(&arbiter).is_ok_and(|mut d| d.next().is_none()) {
                let _ = std::fs::remove_dir(&arbiter);
            }
        }
        Ok(moved)
    }

    /// Arbiter files already committed in projects (for example
    /// `.arbiter/PROJECT.md` from an older setup). Reported, never changed;
    /// a team's own `healing.toml` is not listed.
    async fn committed_in_projects(&self) -> Vec<Value> {
        let mut out = vec![];
        for p in self.store(|st| st.projects(crate::service::WS)).unwrap_or_default() {
            let Ok(files) = git(Path::new(&p.path), &["ls-files", "--", ".arbiter"]).await else { continue };
            let files: Vec<&str> =
                files.lines().filter(|f| !f.is_empty() && *f != ".arbiter/healing.toml").take(10).collect();
            if !files.is_empty() {
                out.push(json!({"project": p.name, "files": files}));
            }
        }
        out
    }

    pub(crate) async fn memory_status(&self) -> Result<Value> {
        let dir = self.ensure_memory().await?;
        let remote = git(&dir, &["remote", "get-url", "origin"]).await.ok();
        let dirty = !git(&dir, &["status", "--porcelain"]).await?.is_empty();
        let unpushed = self.memory_unpushed(&dir).await;
        let last = git(&dir, &["log", "-1", "--format=%cI"]).await.ok().filter(|s| !s.is_empty());
        Ok(json!({
            "path": dir,
            "remote": remote,
            "unpushed": unpushed,
            "dirty": dirty,
            "last_commit": last,
            "last_push": self.keeper().last_push,
            "gh": std::process::Command::new("gh").no_window().arg("--version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success()),
            "guidance": self.user_guidance().is_some(),
            "committed_in_projects": self.committed_in_projects().await,
        }))
    }

    /// Back up now; push when asked and a remote exists. `closing` records a
    /// normal close, so the next start does not push on its own.
    pub(crate) async fn sync_memory(&self, push: bool, closing: bool) -> Result<Value> {
        let _lock = self.inner.memory_lock.lock().await;
        self.export_memory().await?;
        self.commit_memory("Back up memory").await?;
        let dir = self.memory_dir();
        let mut pushed = false;
        if push && self.memory_unpushed(&dir).await.is_some_and(|n| n > 0) {
            self.push_memory(&dir).await?;
            pushed = true;
        }
        if closing {
            let mut k = self.keeper();
            k.closed_cleanly = true;
            self.save_keeper(&k)?;
        }
        let mut status = self.memory_status().await?;
        status["pushed"] = json!(pushed);
        Ok(status)
    }

    /// Use an existing remote (any git URL the user gives).
    pub(crate) async fn set_memory_remote(&self, url: &str) -> Result<Value> {
        let url = url.trim();
        ensure!(
            !url.is_empty() && url.len() <= 500 && !url.starts_with('-') && !url.chars().any(char::is_whitespace),
            "paste a git remote URL"
        );
        let _lock = self.inner.memory_lock.lock().await;
        let dir = self.ensure_memory().await?;
        if git(&dir, &["remote", "get-url", "origin"]).await.is_ok() {
            git(&dir, &["remote", "set-url", "origin", url]).await?;
        } else {
            git(&dir, &["remote", "add", "origin", url]).await?;
        }
        drop(_lock);
        self.sync_memory(true, false).await
    }

    /// Create a private GitHub repository for the memory and push to it.
    pub(crate) async fn create_memory_repo(&self, name: &str) -> Result<Value> {
        ensure!(
            !name.is_empty()
                && name.len() <= 100
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
            "use letters, numbers, dashes, dots or underscores"
        );
        ensure!(self.inner.setup.lock().unwrap().read()?.network, "network access is disabled");
        let dir = self.ensure_memory().await?;
        ensure!(git(&dir, &["remote", "get-url", "origin"]).await.is_err(), "your memory already has a remote");
        {
            let _lock = self.inner.memory_lock.lock().await;
            self.export_memory().await?;
            self.commit_memory("Back up memory").await?;
        }
        crate::landing::gh(
            &dir,
            &["repo", "create", name, "--private", "--source", &dir.to_string_lossy(), "--remote", "origin", "--push"],
        )
        .await
        .context("GitHub CLI could not create the repository. Sign in with `gh auth login` first.")?;
        let mut k = self.keeper();
        k.last_push = Some(crate::projects::now());
        self.save_keeper(&k)?;
        self.memory_status().await
    }

    /// Commit locally every few minutes. On start, an unclean previous stop
    /// pushes first, so a flat battery never loses work.
    pub(crate) fn spawn_memory_keeper(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            let crashed = {
                let mut k = state.keeper();
                let crashed = !k.closed_cleanly && k.cursor > 0;
                k.closed_cleanly = false;
                let _ = state.save_keeper(&k);
                crashed
            };
            match state.migrate_project_leftovers().await {
                Ok(n) if n > 0 => tracing::info!("moved {n} note(s) from projects into the memory folder"),
                Ok(_) => {}
                Err(e) => tracing::warn!("moving old project notes: {e:#}"),
            }
            if let Err(e) = state.sync_memory(crashed, false).await {
                tracing::warn!("memory backup at start: {e:#}");
            }
            let mut timer = tokio::time::interval(COMMIT_EVERY);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            timer.tick().await;
            loop {
                timer.tick().await;
                if let Err(e) = state.sync_memory(false, false).await {
                    tracing::warn!("memory backup: {e:#}");
                }
            }
        });
    }
}

/// Restore Arbiter's history, tasks and settings from a memory folder into a
/// new home before the first start.
pub fn restore(home: &Path, memory: &Path) -> Result<usize> {
    std::fs::create_dir_all(home)?;
    let mut store = arbiter_store::Store::open(home.join("arbiter.db"))?;
    let tables: Value = serde_json::from_slice(
        &std::fs::read(memory.join("backup").join("tables.json")).context("backup/tables.json is missing")?,
    )?;
    let mut events = vec![];
    let mut files: Vec<_> =
        std::fs::read_dir(memory.join("backup").join("events"))?.flatten().map(|e| e.path()).collect();
    files.sort();
    for f in files.into_iter().filter(|p| p.extension().is_some_and(|x| x == "jsonl")) {
        for line in std::fs::read_to_string(&f)?.lines().filter(|l| !l.trim().is_empty()) {
            events.push(serde_json::from_str::<arbiter_store::BackupEvent>(line)?);
        }
    }
    events.retain(|e| serde_json::from_str::<arbiter_core::EventKind>(&e.payload).is_ok());
    events.sort_by_key(|e| e.id);
    events.dedup_by_key(|e| e.id);
    store.restore_backup(&tables, &events)?;
    if let Ok(bytes) = std::fs::read(memory.join("backup").join("preferences.json")) {
        let mut setup = arbiter_setup::Store::open(&home.join("setup.db"))?;
        let mut p: arbiter_setup::Preferences = serde_json::from_slice(&bytes)?;
        p.revision = setup.read()?.revision;
        setup.save(p).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    }
    for sub in ["publish", "preview"] {
        if let Ok(entries) = std::fs::read_dir(memory.join("backup").join("settings").join(sub)) {
            std::fs::create_dir_all(home.join(sub))?;
            for e in entries.flatten() {
                std::fs::copy(e.path(), home.join(sub).join(e.file_name()))?;
            }
        }
    }
    let _ = std::fs::copy(memory.join("backup").join("settings").join("web-sites.json"), home.join("web-sites.json"));
    // Continue in the same memory folder: it becomes this home's memory.
    let target = home.join("memory");
    if memory.canonicalize().ok() != target.canonicalize().ok() && !target.exists() {
        copy_dir(memory, &target)?;
    }
    std::fs::write(
        home.join("memory-state.json"),
        serde_json::to_vec_pretty(&json!({"cursor": events.last().map_or(0, |e| e.id), "closed_cleanly": true}))?,
    )?;
    Ok(events.len())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let path = e.path();
        if e.file_type()?.is_dir() {
            copy_dir(&path, &to.join(e.file_name()))?;
        } else {
            std::fs::copy(&path, to.join(e.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_secrets_but_keeps_text() {
        let s = redact(
            r#"{"text":"use sk-abcdefghijklmnop1234 and ghp_abcdefghijklmnopqrstuvwxyz1234 then password=hunter22long ok"}"#,
        );
        assert!(!s.contains("sk-abcdef") && !s.contains("ghp_abc") && !s.contains("hunter22"), "{s}");
        assert!(s.contains("password=[redacted]") && s.contains(" ok"), "{s}");
        assert_eq!(redact("Fixed the checkout total rounding."), "Fixed the checkout total rounding.");
    }
}
