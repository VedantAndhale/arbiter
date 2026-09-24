//! One git worktree + branch per task. Uses the `git` CLI (not libgit2) so
//! behaviour matches what users and agents see on the command line.

use anyhow::{Context, Result, bail};
use arbiter_core::NoWindow;
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
}

pub struct WorktreeManager {
    /// Root for all worktrees. Kept short (e.g. `~/.arbiter/wt`) to stay
    /// well under Windows MAX_PATH even for deep repos.
    root: PathBuf,
}

impl WorktreeManager {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Create `<root>/<key>` on a new branch `arbiter/<key>` from `base` (default: HEAD).
    pub async fn create(&self, repo: &Path, key: &str, base: Option<&str>) -> Result<Worktree> {
        if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            bail!("invalid worktree key {key:?}");
        }
        tokio::fs::create_dir_all(&self.root).await?;
        let path = self.root.join(key);
        let branch = format!("arbiter/{key}");
        git(repo, &["config", "core.longpaths", "true"]).await?;
        let path_str = path.to_str().context("non-utf8 worktree path")?;
        git(repo, &["worktree", "add", "-b", &branch, path_str, base.unwrap_or("HEAD")]).await?;
        Ok(Worktree { path, branch })
    }

    /// Remove the worktree directory; the branch is kept so work is never lost.
    pub async fn remove(&self, repo: &Path, wt: &Worktree) -> Result<()> {
        let path_str = wt.path.to_str().context("non-utf8 worktree path")?;
        git(repo, &["worktree", "remove", "--force", path_str]).await?;
        Ok(())
    }

    /// Unified diff of the worktree against the commit it branched from.
    pub async fn diff(&self, repo: &Path, wt: &Worktree) -> Result<String> {
        let base = git(repo, &["merge-base", "HEAD", &wt.branch]).await?;
        // Show untracked files by marking them intent-to-add for this diff
        // only, then unmark them. Leaving the marks would let a later
        // `commit -a` sweep in files nobody chose (credentials included).
        let untracked = git(&wt.path, &["ls-files", "-z", "--others", "--exclude-standard"]).await?;
        let paths: Vec<&str> = untracked.split('\0').filter(|p| !p.is_empty()).collect();
        for chunk in paths.chunks(50) {
            let mut args = vec!["add", "--intent-to-add", "--"];
            args.extend(chunk);
            git(&wt.path, &args).await?;
        }
        let diff = git(&wt.path, &["diff", base.trim()]).await;
        for chunk in paths.chunks(50) {
            let mut args = vec!["reset", "-q", "--"];
            args.extend(chunk);
            let _ = git(&wt.path, &args).await;
        }
        diff
    }
}

async fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .no_window()
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .await
        .context("failed to run git; is it installed and on PATH?")?;
    if !out.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn init_repo(dir: &Path) {
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.email", "t@example.com"],
            &["config", "user.name", "t"],
        ] {
            git(dir, args).await.unwrap();
        }
        tokio::fs::write(dir.join("a.txt"), "one\n").await.unwrap();
        git(dir, &["add", "."]).await.unwrap();
        git(dir, &["commit", "-q", "-m", "init"]).await.unwrap();
    }

    #[tokio::test]
    async fn create_diff_remove() {
        let repo = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        init_repo(repo.path()).await;

        let mgr = WorktreeManager::new(root.path());
        let wt = mgr.create(repo.path(), "t1", None).await.unwrap();
        assert!(wt.path.join("a.txt").exists());
        assert_eq!(wt.branch, "arbiter/t1");

        tokio::fs::write(wt.path.join("a.txt"), "two\n").await.unwrap();
        tokio::fs::write(wt.path.join("new.txt"), "hi\n").await.unwrap();
        let diff = mgr.diff(repo.path(), &wt).await.unwrap();
        assert!(diff.contains("+two"), "{diff}");
        assert!(diff.contains("new.txt"), "untracked files included: {diff}");

        mgr.remove(repo.path(), &wt).await.unwrap();
        assert!(!wt.path.exists());
    }

    #[tokio::test]
    async fn rejects_path_traversal_keys() {
        let mgr = WorktreeManager::new("/tmp/unused");
        assert!(mgr.create(Path::new("."), "../evil", None).await.is_err());
    }
}
