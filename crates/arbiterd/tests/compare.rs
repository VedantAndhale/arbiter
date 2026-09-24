//! Opt-in best-of-N with fake harnesses: explicit approval and the parallel
//! cloud-run limit are enforced before anything is created; candidates run in
//! separate worktrees; keeping one settles the others without deleting work.
use arbiter_adapters::{Command, Harness, HarnessEvent, RunHandle, TurnOutcome};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn comparison_is_opt_in_bounded_and_keeps_one() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let git = arbiter_supervisor::integration::git;
    git(repo.path(), &["init", "-q", "-b", "main"]).await.unwrap();
    std::fs::write(repo.path().join("app.py"), "print('hi')\n").unwrap();
    git(repo.path(), &["add", "."]).await.unwrap();
    git(repo.path(), &["-c", "commit.gpgSign=false", "commit", "-qm", "initial"]).await.unwrap();

    let sent = Arc::new(Mutex::new(Vec::<(Harness, String)>::new()));
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.heal = false;
    cfg.available = Arc::new(|_| true);
    let messages = sent.clone();
    cfg.launcher = Arc::new(move |harness, opts| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (events, ev) = tokio::sync::mpsc::unbounded_channel();
        let messages = messages.clone();
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::Send(s) => {
                        messages.lock().unwrap().push((harness, s));
                        // Each candidate solves it differently, in its own worktree.
                        let body = if harness == Harness::Claude {
                            "print('hello')\n"
                        } else {
                            "print('hello')\nprint('!')\n"
                        };
                        std::fs::write(opts.cwd.join("app.py"), body).unwrap();
                        let _ = events.send(HarnessEvent::TurnDone(TurnOutcome::Completed));
                    }
                    Command::Shutdown => break,
                    _ => {}
                }
            }
        });
        Ok(RunHandle::from_channels(tx, ev))
    });
    let daemon = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    let post = |p: &str| client.post(format!("{base}{p}")).bearer_auth(&daemon.token);
    let get = |p: &str| client.get(format!("{base}{p}")).bearer_auth(&daemon.token);
    let project: Value =
        post("/v1/projects").json(&json!({"path":repo.path()})).send().await.unwrap().json().await.unwrap();
    let request = |approved: bool| {
        json!({"project_id":project["id"],"message":"Say hello instead of hi","approved":approved,
            "candidates":[{"harness":"claude"},{"harness":"codex"}]})
    };
    let threads =
        async || -> usize { get("/v1/threads").send().await.unwrap().json::<Vec<Value>>().await.unwrap().len() };

    let set_limit = async |n: usize| {
        let p: Value = get("/v1/setup").send().await.unwrap().json().await.unwrap();
        let mut p = p["preferences"].clone();
        p["max_cloud_runs"] = json!(n);
        post("/v1/setup").json(&p).send().await.unwrap().error_for_status().unwrap();
    };
    set_limit(1).await;
    // Not approved, or over the parallel limit: refused, nothing created.
    assert_eq!(post("/v1/comparisons").json(&request(false)).send().await.unwrap().status(), 400);
    let r = post("/v1/comparisons").json(&request(true)).send().await.unwrap();
    assert_eq!(r.status(), 422);
    assert!(r.text().await.unwrap().contains("needs at least 2 parallel cloud runs"));
    assert_eq!(threads().await, 0);
    assert!(sent.lock().unwrap().is_empty());

    set_limit(2).await;
    let started: Value = post("/v1/comparisons").json(&request(true)).send().await.unwrap().json().await.unwrap();
    let id = started["id"].as_str().expect("comparison id").to_owned();
    assert_eq!(started["threads"].as_array().unwrap().len(), 2);

    let mut view = Value::Null;
    for _ in 0..200 {
        view = get(&format!("/v1/comparisons/{id}")).send().await.unwrap().json().await.unwrap();
        let rows = view["candidates"].as_array().unwrap();
        if rows.iter().all(|r| r["status"] != "running" && !r["changes"].as_str().unwrap_or("").is_empty()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let rows = view["candidates"].as_array().unwrap().clone();
    assert_eq!(rows.iter().map(|r| r["label"].as_str().unwrap()).collect::<Vec<_>>(), ["A", "B"], "{view}");
    assert_eq!(rows[0]["harness"], "claude");
    assert!(rows[0]["changes"].as_str().unwrap().contains("1 insertion"), "{view}");
    assert!(rows[1]["changes"].as_str().unwrap().contains("2 insertions"), "{view}");
    {
        let sent = sent.lock().unwrap();
        assert_eq!(sent.len(), 2);
        assert!(sent.iter().all(|(_, m)| m == "Say hello instead of hi"));
    }

    // Keep A: B is settled and told why; its worktree stays on disk.
    let a = rows[0]["thread"].as_str().unwrap();
    let b = rows[1]["thread"].as_str().unwrap();
    let kept: Value = post(&format!("/v1/comparisons/{id}/keep"))
        .json(&json!({"thread":a}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(kept["candidates"][0]["kept"], true, "{kept}");
    let bt: Value = get(&format!("/v1/threads/{b}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(bt["settled"], true);
    assert!(std::path::Path::new(bt["worktree"].as_str().unwrap()).join("app.py").exists());
    let events = get(&format!("/v1/threads/{b}/events")).send().await.unwrap().text().await.unwrap();
    assert!(events.contains("Not kept: candidate A was chosen"));
    let again = post(&format!("/v1/comparisons/{id}/keep")).json(&json!({"thread":b})).send().await.unwrap();
    assert!(!again.status().is_success());
    daemon.handle.abort();
}
