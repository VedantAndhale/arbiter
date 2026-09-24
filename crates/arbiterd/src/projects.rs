use crate::{AppState, service::WS};
use anyhow::{Context, Result, ensure};
use arbiter_project::{Adoption, Inventory, safe_path};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

impl AppState {
    pub(crate) async fn initialize_project(&self, id: &str, files: Vec<String>) -> Result<Value> {
        let _run =
            self.inner.baseline_lock.try_lock().map_err(|_| anyhow::anyhow!("another project operation is running"))?;
        let p = self.adoption(id)?;
        ensure!(p.state == "applied", "apply the reviewed setup first");
        ensure!(!p.inventory.git, "this is already a Git repository; existing history is never reinitialized");
        let root = Path::new(&p.inventory.root);
        ensure!(!root.join(".git").exists(), "Git state changed; inspect again");
        let current = arbiter_project::inspect(root)?;
        ensure!(current.fingerprint == p.inventory.fingerprint, "project metadata changed; inspect again");
        ensure!(files.len() <= 2000, "too many initial files");
        let mut selected = files;
        selected.sort();
        selected.dedup();
        for file in &selected {
            ensure!(current.initial_files.contains(file), "file is not in the reviewed initial-commit list");
            let bytes = arbiter_project::bounded_read(&safe_path(root, file)?)?;
            let text = String::from_utf8_lossy(&bytes);
            ensure!(
                !["PRIVATE KEY-----", "sk-ant-", "sk-proj-", "ghp_", "AKIA"].iter().any(|s| text.contains(s)),
                "a selected file resembles a secret; remove it from the initial commit"
            );
        }
        // The first commit is the user's history: author it with their own Git
        // identity when configured, and only fall back to Arbiter's.
        let name = user_git(root, &["config", "--get", "user.name"]).await.ok().filter(|s| !s.is_empty());
        let email = user_git(root, &["config", "--get", "user.email"]).await.ok().filter(|s| !s.is_empty());
        let (name, email) = match (name, email) {
            (Some(n), Some(e)) => (n, e),
            _ => ("Arbiter".to_owned(), "arbiter@localhost".to_owned()),
        };
        user_git(root, &["init", "-q", "-b", "main"]).await?;
        // An explicit empty hook directory prevents inherited hooks at this
        // initialization boundary. Only user-selected paths enter the index.
        let hooks = self.inner.home.join("empty-hooks");
        let config = format!("core.hooksPath={}", hooks.display());
        let committed = async {
            std::fs::create_dir_all(&hooks)?;
            for file in &selected {
                user_git(root, &["-c", &config, "add", "--", file]).await?;
            }
            let (n, e) = (format!("user.name={name}"), format!("user.email={email}"));
            user_git(
                root,
                &[
                    "-c",
                    &config,
                    "-c",
                    &n,
                    "-c",
                    &e,
                    "-c",
                    "commit.gpgSign=false",
                    "commit",
                    "--allow-empty",
                    "-qm",
                    "Initialize reviewed project files",
                ],
            )
            .await?;
            anyhow::Ok(())
        }
        .await;
        if let Err(e) = committed {
            // Arbiter created this repository moments ago (checked above), so
            // removing it restores the exact prior state and allows a retry.
            let _ = std::fs::remove_dir_all(root.join(".git"));
            return Err(e.context("the initial commit failed and the new Git repository was removed; nothing else changed, so you can retry"));
        }
        Ok(json!(self.inner.project_setup.lock().unwrap().refresh(id)?))
    }
    pub(crate) fn inspect_project(&self, path: &str, purpose: &str, stack: &str) -> Result<Value> {
        let proposal = self.inner.project_setup.lock().unwrap().propose(Path::new(path), purpose, stack)?;
        let checks = discovered_checks(Path::new(&proposal.inventory.root))?;
        Ok(json!({"proposal":proposal,"checks":checks}))
    }
    pub(crate) fn adoption(&self, id: &str) -> Result<Adoption> {
        Ok(self.inner.project_setup.lock().unwrap().get(id)?)
    }
    pub(crate) fn apply_adoption(&self, id: &str, rollback: bool) -> Result<Value> {
        let store = self.inner.project_setup.lock().unwrap();
        let p = if rollback { store.rollback(id)? } else { store.apply(id)? };
        Ok(json!(p))
    }
    pub(crate) fn project_overview(&self, path: &str) -> Result<Value> {
        let inventory = arbiter_project::inspect(Path::new(path))?;
        let latest = self.inner.project_setup.lock().unwrap().latest(&inventory.root)?;
        let baseline =
            std::fs::read(self.baseline_path(&inventory)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        Ok(
            json!({"inventory":inventory,"latest":latest,"baseline":baseline,"checks":discovered_checks(Path::new(path))?}),
        )
    }
    fn baseline_path(&self, inventory: &Inventory) -> std::path::PathBuf {
        self.inner.home.join("baselines").join(format!("{}.json", arbiter_project::hash(inventory.root.as_bytes())))
    }
    pub(crate) async fn project_baseline(&self, path: &str, fingerprint: &str) -> Result<Value> {
        let _run = self.inner.baseline_lock.try_lock().map_err(|_| anyhow::anyhow!("another baseline is running"))?;
        let inventory = arbiter_project::inspect(Path::new(path))?;
        ensure!(inventory.fingerprint == fingerprint, "project configuration changed; review the checks again");
        // Strict offline execution is not yet available. Do not pretend arbitrary
        // project scripts obey the model-network preference.
        ensure!(
            self.inner.setup.lock().unwrap().read()?.network,
            "baseline scripts may access the network; enable network in Settings or keep the inventory-only baseline"
        );
        let checks = discovered_checks(Path::new(path))?;
        let mut results = vec![];
        for check in &checks {
            let mut check = check.clone();
            check.timeout_secs = check.timeout_secs.clamp(1, 120);
            let run = arbiter_heal::run_check(&check, Path::new(path)).await;
            let mut v = json!(run);
            v["evidence"] = json!(baseline_evidence(&run));
            v["cmd"] = json!(check.cmd);
            results.push(v);
        }
        let all_ok = results.iter().all(|r| r["ok"] == true);
        let baseline = json!({"fingerprint":fingerprint,"at":now(),"results":results,"checks_run":results.len(),"status":if results.is_empty(){"no_checks"}else if all_ok {"passed"}else{"existing_failures"}});
        let target = self.baseline_path(&inventory);
        std::fs::create_dir_all(target.parent().unwrap())?;
        std::fs::write(target, serde_json::to_vec(&baseline)?)?;
        Ok(baseline)
    }
    pub(crate) async fn documentation(&self, path: &str, index: usize, _refresh: bool) -> Result<Value> {
        let inventory = arbiter_project::inspect(Path::new(path))?;
        let profile = inventory.profiles.get(index).context("dependency hint not found")?;
        let network = self.inner.setup.lock().unwrap().read()?.network;
        Ok(
            json!({"profile":profile,"cached":null,"network":network,"note":"Documentation caching is disabled. Use local documentation assistance in Settings for live Context7 queries."}),
        )
    }
    pub(crate) fn register_project(&self, path: &str) -> Result<Value> {
        let root = std::fs::canonicalize(path)?;
        ensure!(
            root.join(".git").exists(),
            "Prepare files first, then initialize Git and review an initial commit before adding this folder to the coding workspace."
        );
        // Worktrees branch from HEAD; a repository without commits would fail
        // every agent later with a confusing error.
        let has_commit = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["rev-parse", "--verify", "-q", "HEAD"])
            .output()
            .is_ok_and(|o| o.status.success());
        ensure!(
            has_commit,
            "This repository has no commits yet. Make an initial commit, then add it to the workspace."
        );
        let path = root.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        if let Some(p) = self.store(|s| s.projects(WS))?.into_iter().find(|p| p.path == path) {
            return Ok(json!(p));
        }
        let name = root.file_name().unwrap_or_default().to_string_lossy();
        Ok(json!(self.store(|s| s.create_project(WS, &name, &path))?))
    }
}
/// Git without Arbiter's identity overrides, for commits in the user's own
/// history. Bounded like the shared helper.
pub(crate) async fn user_git(root: &Path, args: &[&str]) -> Result<String> {
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        tokio::process::Command::new("git").arg("-C").arg(root).args(args).kill_on_drop(true).output(),
    )
    .await
    .context("git timed out")??;
    ensure!(out.status.success(), "git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Why a baseline check failed, in a few lines: parsed failures when the tool
/// is recognised, otherwise the end of its output. Never the full log.
pub(crate) fn baseline_evidence(run: &arbiter_heal::CheckRun) -> Option<String> {
    if run.ok {
        return None;
    }
    if run.timed_out {
        return Some(format!("Timed out after {}s.", run.duration_ms / 1000));
    }
    let failures = arbiter_heal::parse(&run.output);
    let mut text = if failures.is_empty() {
        let clean = arbiter_heal::parse::strip_ansi(&run.output);
        let lines: Vec<&str> = clean.lines().filter(|l| !l.trim().is_empty()).collect();
        lines[lines.len().saturating_sub(8)..].join("\n")
    } else {
        let mut lines: Vec<String> = failures
            .iter()
            .take(5)
            .map(|f| {
                let loc = match (&f.file, f.line) {
                    (Some(p), Some(l)) => format!("{p}:{l} "),
                    (Some(p), None) => format!("{p} "),
                    _ => String::new(),
                };
                format!("{loc}{}", f.message)
            })
            .collect();
        if failures.len() > 5 {
            lines.push(format!("…and {} more", failures.len() - 5));
        }
        lines.join("\n")
    };
    if text.len() > 1500 {
        let mut cut = 1500;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push('…');
    }
    Some(text)
}

pub(crate) fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}
fn discovered_checks(root: &Path) -> Result<Vec<arbiter_heal::Check>> {
    for p in [".arbiter/healing.toml", "package.json", "Cargo.toml", "pyproject.toml", "go.mod"] {
        let path = safe_path(root, p)?;
        if path.exists() {
            arbiter_project::bounded_read(&path)?;
        }
    }
    let checks = arbiter_heal::detect(root)?.checks;
    ensure!(
        checks.len() <= 8 && checks.iter().all(|c| c.cmd.len() <= 1000),
        "too many or oversized baseline commands; simplify project checks"
    );
    Ok(checks)
}
