//! Per-turn snapshots of a worktree, stored as ordinary git commits under
//! `refs/arbiter/cp/...` so they survive gc and never touch the user's index,
//! branch or stash. A snapshot includes untracked (non-ignored) files.

use anyhow::{Context, Result, bail};
use arbiter_core::NoWindow;
use std::path::Path;
use tokio::process::Command;

async fn git(dir: &Path, args: &[&str], index: Option<&Path>) -> Result<String> {
    let mut c = Command::new("git");
    c.no_window();
    c.arg("-C").arg(dir).args(args);
    if let Some(i) = index {
        c.env("GIT_INDEX_FILE", i);
    }
    // Checkpoint commits need an identity even on machines without one.
    c.env("GIT_AUTHOR_NAME", "arbiter")
        .env("GIT_AUTHOR_EMAIL", "arbiter@localhost")
        .env("GIT_COMMITTER_NAME", "arbiter")
        .env("GIT_COMMITTER_EMAIL", "arbiter@localhost");
    let out = c.output().await.context("failed to run git")?;
    if !out.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Commit the worktree's current files (tracked + untracked, respecting
/// .gitignore) to `refname` without touching the real index. Returns the sha.
/// Returns `None` when nothing changed since `parent` (no commit is made).
pub async fn checkpoint(wt: &Path, refname: &str, parent: Option<&str>, message: &str) -> Result<Option<String>> {
    if !refname.starts_with("refs/arbiter/") {
        bail!("checkpoint refs must live under refs/arbiter/");
    }
    let tmp = tempfile_path(wt);
    let result = async {
        git(wt, &["read-tree", "HEAD"], Some(&tmp)).await?;
        git(wt, &["add", "-A", "--", "."], Some(&tmp)).await?;
        let tree = git(wt, &["write-tree"], Some(&tmp)).await?;
        let head = git(wt, &["rev-parse", "HEAD"], None).await?;
        let parent = parent.unwrap_or(&head).to_owned();
        let parent_tree = git(wt, &["rev-parse", &format!("{parent}^{{tree}}")], None).await?;
        if parent_tree == tree {
            return Ok(None);
        }
        let sha = git(wt, &["commit-tree", &tree, "-p", &parent, "-m", message], None).await?;
        git(wt, &["update-ref", refname, &sha], None).await?;
        Ok(Some(sha))
    }
    .await;
    let _ = std::fs::remove_file(&tmp);
    result
}

/// Make the worktree's files match `commit`: restore its files and delete
/// files created since. Callers should checkpoint first so this is undoable.
pub async fn restore(wt: &Path, commit: &str, current: &str) -> Result<()> {
    let added = git(wt, &["diff", "--name-only", "--diff-filter=A", "--no-renames", commit, current], None).await?;
    for f in added.lines().filter(|l| !l.is_empty()) {
        let p = wt.join(f);
        if p.starts_with(wt) {
            let _ = std::fs::remove_file(p);
        }
    }
    git(wt, &["restore", "--source", commit, "--worktree", "--", "."], None).await?;
    Ok(())
}

/// Unified diff between two commits (e.g. consecutive checkpoints = one turn).
pub async fn diff(wt: &Path, from: &str, to: &str) -> Result<String> {
    git(wt, &["diff", "--no-color", from, to], None).await
}

/// Files that differ between `base` and the worktree right now, including
/// untracked files. Paths are repo-relative with `/` separators.
pub async fn changed_files(wt: &Path, base: &str) -> Result<Vec<String>> {
    let mut files: Vec<String> =
        git(wt, &["diff", "--name-only", base], None).await?.lines().map(str::to_owned).collect();
    files.extend(git(wt, &["ls-files", "--others", "--exclude-standard"], None).await?.lines().map(str::to_owned));
    files.retain(|f| !f.is_empty());
    files.sort();
    files.dedup();
    Ok(files)
}

pub async fn head(wt: &Path) -> Result<String> {
    git(wt, &["rev-parse", "HEAD"], None).await
}

fn tempfile_path(wt: &Path) -> std::path::PathBuf {
    let n: u64 =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    std::env::temp_dir().join(format!("arbiter-index-{}-{n}", wt.file_name().and_then(|s| s.to_str()).unwrap_or("wt")))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for args in
            [&["init", "-q", "-b", "main"][..], &["config", "user.email", "t@e.com"], &["config", "user.name", "t"]]
        {
            git(d.path(), args, None).await.unwrap();
        }
        std::fs::write(d.path().join("a.txt"), "one\n").unwrap();
        git(d.path(), &["add", "."], None).await.unwrap();
        git(d.path(), &["commit", "-q", "-m", "init"], None).await.unwrap();
        d
    }

    #[tokio::test]
    async fn checkpoint_diff_restore_roundtrip() {
        let d = repo().await;
        let w = d.path();
        let base = head(w).await.unwrap();

        assert_eq!(checkpoint(w, "refs/arbiter/cp/t/0", None, "no change").await.unwrap(), None);
        std::fs::write(w.join("a.txt"), "two\n").unwrap();
        std::fs::write(w.join("new.txt"), "hi\n").unwrap();
        let cp1 = checkpoint(w, "refs/arbiter/cp/t/1", None, "turn 1").await.unwrap().unwrap();
        assert_eq!(changed_files(w, &base).await.unwrap(), ["a.txt", "new.txt"]);

        std::fs::write(w.join("a.txt"), "three\n").unwrap();
        std::fs::write(w.join("later.txt"), "x\n").unwrap();
        let cp2 = checkpoint(w, "refs/arbiter/cp/t/2", Some(&cp1), "turn 2").await.unwrap().unwrap();
        let d12 = diff(w, &cp1, &cp2).await.unwrap();
        assert!(d12.contains("-two") && d12.contains("+three") && d12.contains("later.txt"), "{d12}");

        // The real index was never touched.
        assert!(git(w, &["diff", "--cached", "--name-only"], None).await.unwrap().is_empty());

        restore(w, &cp1, &cp2).await.unwrap();
        assert_eq!(std::fs::read_to_string(w.join("a.txt")).unwrap().trim(), "two");
        assert!(w.join("new.txt").exists());
        assert!(!w.join("later.txt").exists(), "files created after the checkpoint are removed");
    }

    #[tokio::test]
    async fn refuses_refs_outside_namespace() {
        let d = repo().await;
        assert!(checkpoint(d.path(), "refs/heads/main", None, "x").await.is_err());
    }
}
