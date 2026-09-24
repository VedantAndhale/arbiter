//! F4 landing with a fake agent: diff comments reach a single agent as one
//! bounded message; the landing draft comes from recorded work; a commit takes
//! tracked changes plus only the new files the user ticked, never credentials.
use arbiter_adapters::{Command, HarnessEvent, RunHandle, TurnOutcome};
use arbiter_core::AgentEvent;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn comments_draft_and_selective_commit() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let git = arbiter_supervisor::integration::git;
    git(repo.path(), &["init", "-q", "-b", "main"]).await.unwrap();
    std::fs::write(repo.path().join("calc.py"), "def add(a, b):\n    return a - b\n").unwrap();
    git(repo.path(), &["add", "."]).await.unwrap();
    git(repo.path(), &["-c", "commit.gpgSign=false", "commit", "-qm", "initial"]).await.unwrap();

    let sent = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.heal = false;
    cfg.available = Arc::new(|_| true);
    let messages = sent.clone();
    cfg.launcher = Arc::new(move |_, opts| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (events, ev) = tokio::sync::mpsc::unbounded_channel();
        let messages = messages.clone();
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::Send(s) => {
                        messages.lock().unwrap().push(s);
                        std::fs::write(opts.cwd.join("calc.py"), "def add(a, b):\n    return a + b\n").unwrap();
                        std::fs::write(opts.cwd.join("test_calc.py"), "from calc import add\nassert add(1, 2) == 3\n")
                            .unwrap();
                        std::fs::write(opts.cwd.join(".env.local"), "TOKEN=not-a-real-secret\n").unwrap();
                        let _ = events.send(HarnessEvent::Agent(AgentEvent::Message {
                            text: "Fixed add and added a test.".into(),
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
    let daemon = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    let post = |p: &str| client.post(format!("{base}{p}")).bearer_auth(&daemon.token);
    let get = |p: &str| client.get(format!("{base}{p}")).bearer_auth(&daemon.token);
    let project: Value =
        post("/v1/projects").json(&json!({"path":repo.path()})).send().await.unwrap().json().await.unwrap();
    let t: Value = post("/v1/threads")
        .json(&json!({"project_id":project["id"],"harness":"claude","worktree":true,"title":"Fix addition","message":"Fix add in calc.py"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = t["id"].as_str().unwrap().to_owned();
    let wait_turns = async |n: usize| {
        for _ in 0..200 {
            let t: Value = get(&format!("/v1/threads/{id}")).send().await.unwrap().json().await.unwrap();
            if sent.lock().unwrap().len() >= n && t["status"] != "running" {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("turn {n} did not finish");
    };
    wait_turns(1).await;

    // Review comments: bounded, then delivered as one structured message.
    let comment = |text: &str| json!({"path":"calc.py","line":2,"text":text});
    let too_many: Vec<Value> = (0..9).map(|_| comment("x")).collect();
    let r = post(&format!("/v1/threads/{id}/review")).json(&json!({"comments":too_many})).send().await.unwrap();
    assert_eq!(r.status(), 422);
    let long = "y".repeat(501);
    let r = post(&format!("/v1/threads/{id}/review")).json(&json!({"comments":[comment(&long)]})).send().await.unwrap();
    assert_eq!(r.status(), 422);
    let r: Value = post(&format!("/v1/threads/{id}/review"))
        .json(&json!({"comments":[comment("Handle negative numbers too"), {"path":"test_calc.py","line":2,"text":"Add a case for zero"}]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["kind"], "message", "{r}");
    wait_turns(2).await;
    let review = sent.lock().unwrap()[1].clone();
    assert!(review.starts_with("Review comments on your changes."), "{review}");
    assert!(review.contains("- calc.py:2: Handle negative numbers too\n- test_calc.py:2: Add a case for zero"));

    // Viewing the diff shows new files but must not leave them staged: a
    // later commit would otherwise sweep in the credential file.
    let diff = get(&format!("/v1/threads/{id}/diff")).send().await.unwrap().text().await.unwrap();
    assert!(diff.contains("test_calc.py") && diff.contains(".env.local"), "{diff}");
    let worktree = std::path::PathBuf::from(t["worktree"].as_str().unwrap());
    let index = git(&worktree, &["status", "--porcelain"]).await.unwrap();
    assert!(index.contains("?? .env.local"), "diff left the index marked: {index}");

    // The landing draft is built from recorded work, not a model call.
    let landing: Value = get(&format!("/v1/threads/{id}/landing")).send().await.unwrap().json().await.unwrap();
    assert_eq!(landing["draft"]["title"], "Fix addition", "{landing}");
    let body = landing["draft"]["body"].as_str().unwrap();
    assert!(body.contains("Fixed add and added a test.") && body.contains("calc.py |"), "{body}");
    let untracked: Vec<&str> = landing["untracked"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(untracked, [".env.local", "test_calc.py"]);

    // Credentials cannot be ticked in; a stale fingerprint is refused.
    let commit = |fp: &str, include: Vec<&str>| {
        post(&format!("/v1/threads/{id}/commit"))
            .json(&json!({"fingerprint":fp,"message":"Fix addition","include":include}))
    };
    let fp = landing["fingerprint"].as_str().unwrap();
    let r = commit(fp, vec![".env.local"]).send().await.unwrap();
    assert!(r.text().await.unwrap().contains("credential"));
    assert_eq!(commit("stale", vec![]).send().await.unwrap().status(), 422);

    let done: Value = commit(fp, vec!["test_calc.py"]).send().await.unwrap().json().await.unwrap();
    assert_eq!(done["untracked"], json!([".env.local"]), "only the credential stays out: {done}");
    let files = git(&worktree, &["show", "--name-only", "--format=%s", "HEAD"]).await.unwrap();
    assert_eq!(files.lines().collect::<Vec<_>>(), ["Fix addition", "", "calc.py", "test_calc.py"]);
    let events = get(&format!("/v1/threads/{id}/events")).send().await.unwrap().text().await.unwrap();
    assert!(events.contains("1 new file(s) were left out. Nothing has been pushed."));
    daemon.handle.abort();
}
