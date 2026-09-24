//! Real HTTP/SQLite/Git, deterministic fake usage. No paid model is launched.
use arbiter_adapters::{Command, HarnessEvent, RunHandle, TurnOutcome};
use arbiter_core::{AgentEvent, Usage};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[tokio::test]
async fn guarded_notes_revision_review_memory_and_learning() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        assert!(std::process::Command::new("git").arg("-C").arg(repo.path()).args(args).status().unwrap().success());
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "fixture@example.test"]);
    git(&["config", "user.name", "Fixture"]);
    std::fs::create_dir(repo.path().join(".arbiter")).unwrap();
    std::fs::write(
        repo.path().join(".arbiter/healing.toml"),
        "[[check]]\nname=\"clean\"\nkind=\"test\"\ncmd=\"git diff --check\"\ntimeout_secs=10\n",
    )
    .unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "fixture"]);
    let prompts = Arc::new(Mutex::new(vec![]));
    let seen = prompts.clone();
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.available = Arc::new(|_| true);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.launcher = Arc::new(move |_, opts| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (events, ev) = tokio::sync::mpsc::unbounded_channel();
        let seen = seen.clone();
        tokio::spawn(async move {
            while let Some(c) = rx.recv().await {
                match c {
                    Command::Send(text) => {
                        let hit = text.contains("[[decisions/cart]]");
                        seen.lock().unwrap().push(text);
                        std::fs::write(
                            opts.cwd.join("cart.txt"),
                            format!("cart implementation {}\n", seen.lock().unwrap().len()),
                        )
                        .unwrap();
                        // Simulated usage contract: a memory hit replaces repeated investigation.
                        let _ = events.send(HarnessEvent::Agent(AgentEvent::Usage(Usage {
                            input_tokens: if hit { 100 } else { 400 },
                            output_tokens: 20,
                            ..Default::default()
                        })));
                        let _ = events.send(HarnessEvent::Agent(AgentEvent::Message {
                            text: "Validated the pure cart function.".into(),
                        }));
                        let _ = events.send(HarnessEvent::TurnDone(TurnOutcome::Completed));
                    }
                    Command::Shutdown => break,
                    _ => {}
                }
            }
        });
        Ok(RunHandle::from_channels(tx, ev))
    });
    let d = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", d.addr);
    let http = reqwest::Client::new();
    let post = |path: &str, body: Value| http.post(format!("{base}{path}")).bearer_auth(&d.token).json(&body);
    let get = |path: &str| http.get(format!("{base}{path}")).bearer_auth(&d.token);
    let project: Value = post("/v1/projects", json!({"path":repo.path()}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let create = || {
        post(
            "/v1/threads",
            json!({"project_id":project["id"],"title":"Refactor cart","harness":"codex","worktree":false}),
        )
    };
    let t: Value = create().send().await.unwrap().json().await.unwrap();
    let id = t["id"].as_str().unwrap();
    let path = format!("/v1/threads/{id}");
    assert_eq!(http.get(format!("{base}{path}/vault")).send().await.unwrap().status(), 401);
    let wait_review = |id: String| {
        let http = http.clone();
        let base = base.clone();
        let token = d.token.clone();
        async move {
            for _ in 0..150 {
                let t: Value = http
                    .get(format!("{base}/v1/threads/{id}"))
                    .bearer_auth(&token)
                    .send()
                    .await
                    .unwrap()
                    .json()
                    .await
                    .unwrap();
                if t["status"] == "review" {
                    return t;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            panic!("task did not finish");
        }
    };
    post(&format!("{path}/messages"), json!({"text":"Refactor cart"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let first = wait_review(id.into()).await;
    let note = json!({"id":"decisions/cart","title":"Cart calculation","body":"Reuse the pure cartTotal function. [[files/cart]]","revision":0});
    let proposal: Value = post(&format!("{path}/vault/propose"), json!({"note":note,"base_revision":0}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let before: Value = get(&format!("{path}/vault")).send().await.unwrap().json().await.unwrap();
    assert!(before["notes"].as_array().unwrap().is_empty());
    let accept = json!({"id":proposal["id"],"accepted":true});
    post(&format!("{path}/vault/resolve"), accept.clone()).send().await.unwrap().error_for_status().unwrap();
    assert!(!post(&format!("{path}/vault/resolve"), accept).send().await.unwrap().status().is_success());
    assert!(
        !post(&format!("{path}/vault"), json!({"note":note,"base_revision":0}))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    let bad = json!({"id":"decisions/secret","title":"Secret","body":"password=do-not-store","revision":0});
    assert!(
        !post(&format!("{path}/vault"), json!({"note":bad,"base_revision":0}))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    // Notes go to the memory folder, never into the shared project.
    assert!(!repo.path().join(".arbiter/vault").exists());
    let projects = home.path().join("memory").join("projects");
    let folder = std::fs::read_dir(&projects).unwrap().next().unwrap().unwrap().path();
    assert!(folder.join("notes/decisions/cart.md").exists());
    let status =
        std::process::Command::new("git").arg("-C").arg(repo.path()).args(["status", "--porcelain"]).output().unwrap();
    assert!(
        !String::from_utf8_lossy(&status.stdout).contains("vault"),
        "Markdown mirror must not enter repository diffs"
    );
    let read: Value = post(&format!("{path}/vault/tool"), json!({"op":"read","id":"decisions/cart","budget":100}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(read["text"].as_str().unwrap().len() <= 100);
    let mut last = String::new();
    for _ in 0..2 {
        let t: Value = create().send().await.unwrap().json().await.unwrap();
        last = t["id"].as_str().unwrap().into();
        post(&format!("/v1/threads/{last}/messages"), json!({"text":"Refactor cart"}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
        let repeated = wait_review(last.clone()).await;
        assert!(repeated["input_tokens"].as_u64() < first["input_tokens"].as_u64());
    }
    let evidence: Value = get(&format!("{path}/learning")).send().await.unwrap().json().await.unwrap();
    assert_eq!(evidence["strengths"][0]["runs"], 3, "{evidence}");
    let auto:Value=post("/v1/threads",json!({"project_id":project["id"],"title":"Refactor cart","harness":"auto","worktree":false,"message":"Refactor cart"})).send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
    let auto = wait_review(auto["id"].as_str().unwrap().into()).await;
    assert_eq!(auto["harness"], "codex", "three successful samples should influence auto routing: {evidence}");
    post(&format!("/v1/threads/{last}/learning/rework"), json!({"rework":true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let learned: Value = get(&format!("{path}/learning")).send().await.unwrap().json().await.unwrap();
    assert_eq!(learned["strengths"][0]["rework"], 1);
    post(&format!("{path}/learning"), json!({"task_type":"refactor","harness":"claude","model":null,"enabled":true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let override_task:Value=post("/v1/threads",json!({"project_id":project["id"],"title":"Refactor cart","harness":"auto","worktree":false,"message":"Refactor cart"})).send().await.unwrap().json().await.unwrap();
    assert_eq!(wait_review(override_task["id"].as_str().unwrap().into()).await["harness"], "claude");
    {
        let messages = prompts.lock().unwrap();
        assert!(!messages[0].contains("Project memory"));
        assert!(messages[1].contains("Project memory"));
    }
    let vault: Value = get(&format!("{path}/vault")).send().await.unwrap().json().await.unwrap();
    assert_eq!(vault["notes"].as_array().unwrap().len(), 1);
    assert_eq!(vault["links"][0]["to"], "files/cart");
    let current: Value = get(&path).send().await.unwrap().json().await.unwrap();
    post(&format!("{path}/fork"), json!({"at_seq":current["last_seq"]}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let after_fork: Value = get(&format!("{path}/learning")).send().await.unwrap().json().await.unwrap();
    assert_eq!(after_fork["outcomes"].as_object().unwrap().len(), 5, "fork must not duplicate learning samples");
    d.handle.abort();
    // Store startup/rebuild must preserve approved memory without updating old events.
    let db = home.path().join("arbiter.db");
    assert!(db.exists());
    {
        let mut store = arbiter_store::Store::open(&db).unwrap();
        store.rebuild_projections().unwrap();
        let events = store.knowledge_events(project["id"].as_str().unwrap().parse().unwrap()).unwrap();
        assert!(events.iter().any(|e| matches!(e.kind, arbiter_core::EventKind::VaultResolved { accepted: true, .. })));
    }
}
