//! With `auto`, Arbiter chooses the approach itself: small work goes to one
//! agent, larger work is planned first; the choice and its reason are recorded.
use arbiter_adapters::{Command, HarnessEvent, RunHandle, TurnOutcome};
use serde_json::{Value, json};
use std::sync::Arc;

#[tokio::test]
async fn auto_picks_single_agent_or_plan_and_says_why() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let git = arbiter_supervisor::integration::git;
    git(repo.path(), &["init", "-q", "-b", "main"]).await.unwrap();
    std::fs::write(repo.path().join("README.md"), "# Demo\nTeh demo.\n").unwrap();
    git(repo.path(), &["add", "."]).await.unwrap();
    git(repo.path(), &["-c", "commit.gpgSign=false", "commit", "-qm", "initial"]).await.unwrap();
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.heal = false;
    cfg.available = Arc::new(|_| true);
    cfg.launcher = Arc::new(|_, _| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (events, ev) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::Send(_) => {
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
    let start = async |message: &str| -> String {
        let t: Value = post("/v1/threads")
            .json(&json!({"project_id":project["id"],"auto":true,"worktree":true,"message":message}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let id = t["id"].as_str().unwrap_or_else(|| panic!("{t}")).to_owned();
        get(&format!("/v1/threads/{id}/events")).send().await.unwrap().text().await.unwrap()
    };

    let small = start("Fix the typo in README.md").await;
    assert!(small.contains("gave this small"), "{small}");
    assert!(!small.contains("workflow_requested"));

    let large = start(
        "Build a complete multi-tenant billing system with subscriptions, invoices, payment webhooks, an admin dashboard and a database migration for existing customers",
    )
    .await;
    assert!(large.contains("Arbiter is planning this"), "{large}");
    assert!(large.contains("workflow_requested"));
    // Medium work is a judgement call: Arbiter asks as a question card, and
    // the answer decides whether it plans.
    // Over 45 words, no risk terms: medium for the rules classifier.
    let medium = start(
        "Add a settings page to the app where people can switch between light and dark themes, pick a preferred language from a short list, choose how many items appear on each page, and see a preview of each choice before saving, then remember those choices for the next visit and show a small confirmation after saving the settings",
    )
    .await;
    assert!(medium.contains("Arbiter will ask"), "{medium}");
    let events: Vec<Value> = serde_json::from_str(&medium).unwrap();
    let asked = events.iter().rev().find(|e| e["kind"]["type"] == "questions_asked").expect("question cards");
    let questions = asked["kind"]["questions"].as_array().unwrap();
    assert_eq!(questions[0]["id"], "approach");
    let id = asked["thread_id"].as_str().unwrap();
    let answers: Vec<Value> = questions
        .iter()
        .map(|q| {
            let text = if q["id"] == "approach" {
                "Plan it first".to_owned()
            } else {
                q["options"][0]["label"].as_str().map(str::to_owned).unwrap_or_else(|| "Keep it simple".into())
            };
            json!({"question_id":q["id"],"text":text})
        })
        .collect();
    let r = post(&format!("/v1/threads/{id}/answers"))
        .json(&json!({"id":asked["kind"]["id"],"answers":answers}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "{}", r.text().await.unwrap());
    let after = get(&format!("/v1/threads/{id}/events")).send().await.unwrap().text().await.unwrap();
    assert!(after.contains("workflow_requested"), "planning chosen by the answer: {after}");
    assert!(!after.contains("approach: Plan it first"), "the approach answer is not part of the brief");
    daemon.handle.abort();
}
