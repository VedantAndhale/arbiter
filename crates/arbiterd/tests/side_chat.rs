//! Side questions are answered locally from a bounded task summary, recorded
//! on the thread, and never reach the task's agent.
use arbiter_adapters::{Command, HarnessEvent, RunHandle, TurnOutcome};
use arbiter_core::AgentEvent;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn side_question_is_local_bounded_and_not_forwarded() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let git = arbiter_supervisor::integration::git;
    git(repo.path(), &["init", "-q", "-b", "main"]).await.unwrap();
    std::fs::write(repo.path().join("app.py"), "print('hi')\n").unwrap();
    git(repo.path(), &["add", "."]).await.unwrap();
    git(repo.path(), &["-c", "commit.gpgSign=false", "commit", "-qm", "initial"]).await.unwrap();

    let sent = Arc::new(Mutex::new(Vec::<String>::new()));
    let prompts = Arc::new(Mutex::new(Vec::<String>::new()));
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
                        std::fs::write(opts.cwd.join("app.py"), "print('hello')\n").unwrap();
                        let _ = events.send(HarnessEvent::Agent(AgentEvent::Message {
                            text: "Changed the greeting to hello.".into(),
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
    let seen = prompts.clone();
    cfg.local_generator = Some(Arc::new(move |_, prompt, _, _| {
        seen.lock().unwrap().push(prompt);
        Box::pin(async { Ok(json!({"answer":"It changed the greeting in app.py."}).to_string()) })
    }));
    let daemon = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    let post = |p: &str| client.post(format!("{base}{p}")).bearer_auth(&daemon.token);
    let get = |p: &str| client.get(format!("{base}{p}")).bearer_auth(&daemon.token);
    let p: Value = get("/v1/setup").send().await.unwrap().json().await.unwrap();
    let mut p = p["preferences"].clone();
    p["reviewer_model"] = json!("fixture");
    post("/v1/setup").json(&p).send().await.unwrap().error_for_status().unwrap();
    let project: Value =
        post("/v1/projects").json(&json!({"path":repo.path()})).send().await.unwrap().json().await.unwrap();
    let t: Value = post("/v1/threads")
        .json(
            &json!({"project_id":project["id"],"harness":"claude","worktree":true,"message":"Say hello instead of hi"}),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = t["id"].as_str().unwrap();
    for _ in 0..200 {
        let t: Value = get(&format!("/v1/threads/{id}")).send().await.unwrap().json().await.unwrap();
        if !sent.lock().unwrap().is_empty() && t["status"] != "running" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let r = post(&format!("/v1/threads/{id}/side")).json(&json!({"question":"x".repeat(501)})).send().await.unwrap();
    assert_eq!(r.status(), 422);
    let r: Value = post(&format!("/v1/threads/{id}/side"))
        .json(&json!({"question":"What did the agent change?"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["answer"], "It changed the greeting in app.py.", "{r}");
    assert_eq!(r["model"], "fixture");

    // The local model saw a bounded summary: request, latest message, diff stat.
    let prompt = prompts.lock().unwrap().last().cloned().unwrap();
    assert!(prompt.contains("Say hello instead of hi") && prompt.contains("Changed the greeting to hello."));
    assert!(prompt.contains("app.py |"), "{prompt}");
    assert!(prompt.len() <= 8000);

    // Recorded on the thread, but the agent received nothing new.
    let events = get(&format!("/v1/threads/{id}/events")).send().await.unwrap().text().await.unwrap();
    assert!(events.contains("\"type\":\"side_chat\""));
    assert_eq!(sent.lock().unwrap().len(), 1, "side questions never reach the agent");
    // A follow-up sees the earlier exchange.
    post(&format!("/v1/threads/{id}/side")).json(&json!({"question":"Anything else?"})).send().await.unwrap();
    assert!(prompts.lock().unwrap().last().unwrap().contains("What did the agent change?"));
    assert_eq!(sent.lock().unwrap().len(), 1);
    daemon.handle.abort();
}
