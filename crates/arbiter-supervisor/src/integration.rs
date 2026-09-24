//! Git mutations for isolated plan worktrees. The user's checkout is never reset.
use anyhow::{Context, Result, ensure};
use arbiter_core::NoWindow;
use std::path::{Path, PathBuf};
use tokio::process::Command;

pub async fn git(path: &Path, args: &[&str]) -> Result<String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        Command::new("git")
            .no_window()
            .arg("-C")
            .arg(path)
            .args(args)
            .env("GIT_AUTHOR_NAME", "arbiter")
            .env("GIT_AUTHOR_EMAIL", "arbiter@localhost")
            .env("GIT_COMMITTER_NAME", "arbiter")
            .env("GIT_COMMITTER_EMAIL", "arbiter@localhost")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("git operation timed out")??;
    ensure!(
        output.status.success(),
        "git {}: {}",
        args.first().unwrap_or(&""),
        String::from_utf8_lossy(&output.stderr).chars().take(1200).collect::<String>()
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
pub async fn ensure_worktree(repo: &Path, root: &Path, key: &str, branch: &str, base: &str) -> Result<PathBuf> {
    ensure!(!key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'), "invalid worktree key");
    ensure!(
        branch.starts_with("arbiter/")
            && !branch.contains("..")
            && branch.bytes().all(|b| b.is_ascii_alphanumeric() || b"/-_".contains(&b)),
        "invalid branch"
    );
    tokio::fs::create_dir_all(root).await?;
    let path = root.join(key);
    if path.exists() {
        ensure!(
            git(&path, &["branch", "--show-current"]).await? == branch,
            "existing worktree belongs to another branch"
        );
        return Ok(path);
    }
    let target = path.to_str().context("invalid worktree path")?;
    if git(repo, &["show-ref", "--verify", &format!("refs/heads/{branch}")]).await.is_ok() {
        git(repo, &["worktree", "add", target, branch]).await?;
    } else {
        git(repo, &["worktree", "add", "-b", branch, target, base]).await?;
    }
    Ok(path)
}
pub async fn contains(path: &Path, commit: &str) -> bool {
    git(path, &["merge-base", "--is-ancestor", commit, "HEAD"]).await.is_ok()
}
pub async fn conflicts(path: &Path) -> Result<Vec<String>> {
    Ok(git(path, &["diff", "--name-only", "--diff-filter=U"]).await?.lines().map(str::to_owned).collect())
}
pub async fn merge(path: &Path, commit: &str) -> Result<Vec<String>> {
    if contains(path, commit).await {
        return Ok(vec![]);
    }
    if git(path, &["rev-parse", "--verify", "MERGE_HEAD"]).await.is_ok() {
        let unresolved = conflicts(path).await?;
        ensure!(!unresolved.is_empty(), "pending merge needs validation and finish_merge before integration");
        return Ok(unresolved);
    }
    let result = git(path, &["merge", "--no-ff", "--no-edit", commit]).await;
    if result.is_err() {
        let files = conflicts(path).await?;
        if !files.is_empty() {
            return Ok(files);
        }
        result?;
    }
    Ok(vec![])
}
pub async fn finish_merge(path: &Path) -> Result<String> {
    ensure!(conflicts(path).await?.is_empty(), "merge still has unresolved files");
    if git(path, &["rev-parse", "--verify", "MERGE_HEAD"]).await.is_ok() {
        git(path, &["add", "-A"]).await?;
        git(path, &["commit", "--no-edit"]).await?;
    }
    git(path, &["rev-parse", "HEAD"]).await
}
