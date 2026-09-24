//! End-to-end API test against a real daemon on a free port and a real git repo.

use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success();
    assert!(ok, "git {args:?}");
}

fn init_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "t"]);
    std::fs::write(dir.join("README.md"), "hello\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "init"]);
}

#[tokio::test]
async fn project_thread_message_fork_diff() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());

    // A harness that accepts input and never answers: this test is about the API, and must not spend tokens.
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.launcher = std::sync::Arc::new(|_, _| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (ev_tx, ev) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            let _keep = ev_tx;
            while rx.recv().await.is_some() {}
        });
        Ok(arbiter_adapters::RunHandle::from_channels(tx, ev))
    });
    cfg.available = std::sync::Arc::new(|_| true);
    let d = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", d.addr);
    let http = reqwest::Client::new();
    let authed = |rb: reqwest::RequestBuilder| rb.bearer_auth(&d.token);

    // Discovery file matches the running daemon.
    let info = arbiterd::DaemonInfo::read(home.path()).unwrap();
    assert_eq!(info.port, d.addr.port());

    // Unauthenticated access is rejected; health is public.
    assert_eq!(http.get(format!("{base}/v1/threads")).send().await.unwrap().status(), 401);
    assert!(http.get(format!("{base}/v1/health")).send().await.unwrap().status().is_success());

    let project: Value = authed(http.post(format!("{base}/v1/projects")))
        .json(&json!({ "path": repo.path() }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let project_id = project["id"].as_str().unwrap();

    let thread: Value = authed(http.post(format!("{base}/v1/threads")))
        .json(&json!({ "project_id": project_id, "title": "add docs", "harness": "claude" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let tid = thread["id"].as_str().unwrap().to_owned();
    let wt = thread["worktree"].as_str().expect("worktree created").to_owned();
    assert_eq!(thread["status"], "idle");

    let r = authed(http.post(format!("{base}/v1/threads/{tid}/messages")))
        .json(&json!({ "text": "write docs" }))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());

    let t: Value = authed(http.get(format!("{base}/v1/threads/{tid}"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(t["status"], "running");
    let initial_seq = thread["last_seq"].as_i64().unwrap();
    assert_eq!(t["last_seq"].as_i64().unwrap(), initial_seq + 3, "user_message, run_started, status");

    let events: Value = authed(http.get(format!("{base}/v1/threads/{tid}/events?after={initial_seq}")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(events.as_array().unwrap().len(), 3);
    assert_eq!(events[0]["kind"]["type"], "user_message");

    // Worktree edits show up in the diff.
    std::fs::write(Path::new(&wt).join("README.md"), "hello\nworld\n").unwrap();
    let diff = authed(http.get(format!("{base}/v1/threads/{tid}/diff"))).send().await.unwrap().text().await.unwrap();
    assert!(diff.contains("+world"), "{diff}");

    let fork: Value = authed(http.post(format!("{base}/v1/threads/{tid}/fork")))
        .json(&json!({ "at_seq": 2 }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fork["parent"][0], tid.as_str());
    assert_eq!(fork["last_seq"], 2);

    let threads: Value = authed(http.get(format!("{base}/v1/threads"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(threads.as_array().unwrap().len(), 2);
}
