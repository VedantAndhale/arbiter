//! Usage protection is exercised with fake launchers; no real CLI or paid tokens.
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
async fn setup_policy_persists_blocks_launches_and_stops_warm_sessions() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.name", "test"],
        &["config", "user.email", "test@localhost"],
        &["commit", "--allow-empty", "-qm", "initial"],
    ] {
        assert!(std::process::Command::new("git").arg("-C").arg(repo.path()).args(args).status().unwrap().success());
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.available = Arc::new(|_| true);
    cfg.heal = false;
    cfg.launcher = Arc::new(move |_, _| {
        count.fetch_add(1, Ordering::SeqCst);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (events, ev) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            let _keep = events;
            while let Some(c) = rx.recv().await {
                if matches!(c, arbiter_adapters::Command::Shutdown) {
                    break;
                }
            }
        });
        Ok(arbiter_adapters::RunHandle::from_channels(tx, ev))
    });
    let daemon = arbiterd::start(cfg).await.unwrap();
    let url = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    assert_eq!(client.get(format!("{url}/v1/setup")).send().await.unwrap().status(), 401);
    let get = |path: &str| client.get(format!("{url}{path}")).bearer_auth(&daemon.token);
    let post = |path: &str| client.post(format!("{url}{path}")).bearer_auth(&daemon.token);
    let state: Value = get("/v1/setup").send().await.unwrap().json().await.unwrap();
    let mut prefs = state["preferences"].clone();
    prefs["mode"] = json!("local");
    let saved: Value = post("/v1/setup").json(&prefs).send().await.unwrap().json().await.unwrap();
    assert!(!post("/v1/setup").json(&prefs).send().await.unwrap().status().is_success());
    let project: Value =
        post("/v1/projects").json(&json!({"path":repo.path()})).send().await.unwrap().json().await.unwrap();
    let thread: Value = post("/v1/threads")
        .json(&json!({"project_id":project["id"],"harness":"codex","worktree":false}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let path = format!("/v1/threads/{}/messages", thread["id"].as_str().unwrap());
    assert!(!post(&path).json(&json!({"text":"write docs"})).send().await.unwrap().status().is_success());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    prefs = saved["preferences"].clone();
    prefs["mode"] = json!("subscription");
    let saved: Value = post("/v1/setup").json(&prefs).send().await.unwrap().json().await.unwrap();
    assert!(post(&path).json(&json!({"text":"continue"})).send().await.unwrap().status().is_success());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    prefs = saved["preferences"].clone();
    prefs["mode"] = json!("local");
    prefs["network"] = json!(false);
    assert!(post("/v1/setup").json(&prefs).send().await.unwrap().status().is_success());
    let _ = post(&path).json(&json!({"text":"continue again"})).send().await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        !post("/v1/models/potion/install")
            .json(&json!({"accept_license":true}))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    assert!(!post("/v1/setup/refresh").send().await.unwrap().status().is_success());
    let persisted = arbiter_setup::Store::open(&home.path().join("setup.db")).unwrap().read().unwrap();
    assert_eq!(persisted.mode, arbiter_setup::Mode::Local);
    assert!(!persisted.network);
    daemon.handle.abort();
}
