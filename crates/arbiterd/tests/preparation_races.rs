//! Races around local documentation preparation: parallel preparations queue
//! for the single local model, a second message is refused without failing the
//! thread, the idle sweep leaves preparing threads alone, and a restart says
//! the pending request never reached the agent. Fake harness and model only.
use arbiter_adapters::{Command, HarnessEvent, RunHandle, TurnOutcome};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};
use std::time::Duration;

struct Fixture {
    sent: Arc<Mutex<Vec<String>>>,
    shutdowns: Arc<AtomicUsize>,
    delay_ms: Arc<AtomicU64>,
}

fn config(home: &std::path::Path, f: &Fixture) -> arbiterd::Config {
    let mut cfg = arbiterd::Config::new(home.into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.credential_store = Some(Arc::new(arbiter_supervisor::credentials::MemoryCredentials::default()));
    cfg.heal = false;
    cfg.available = Arc::new(|_| true);
    let (sent, shutdowns) = (f.sent.clone(), f.shutdowns.clone());
    cfg.launcher = Arc::new(move |_, _| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (events, ev) = tokio::sync::mpsc::unbounded_channel();
        let (sent, shutdowns) = (sent.clone(), shutdowns.clone());
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::Send(s) => {
                        sent.lock().unwrap().push(s);
                        // Finish the turn so the run idles, like a real harness.
                        let _ = events.send(HarnessEvent::TurnDone(TurnOutcome::Completed));
                    }
                    Command::Shutdown => {
                        shutdowns.fetch_add(1, Ordering::SeqCst);
                        break;
                    }
                    _ => {}
                }
            }
        });
        Ok(RunHandle::from_channels(tx, ev))
    });
    let delay = f.delay_ms.clone();
    // The local model decides no lookup is needed, after a configurable delay.
    cfg.local_generator = Some(Arc::new(move |_, _, _, _| {
        let ms = delay.load(Ordering::SeqCst);
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            Ok(json!({"library":"","topic":""}).to_string())
        })
    }));
    cfg
}

async fn until(what: &str, mut f: impl AsyncFnMut() -> bool) {
    for _ in 0..200 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test]
async fn preparations_queue_refuse_duplicates_and_survive_the_idle_sweep() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let git = arbiter_supervisor::integration::git;
    git(repo.path(), &["init", "-q", "-b", "main"]).await.unwrap();
    std::fs::write(repo.path().join("README.md"), "fixture").unwrap();
    git(repo.path(), &["add", "README.md"]).await.unwrap();
    git(repo.path(), &["-c", "commit.gpgSign=false", "commit", "-qm", "initial"]).await.unwrap();
    let f =
        Fixture { sent: Default::default(), shutdowns: Default::default(), delay_ms: Arc::new(AtomicU64::new(300)) };
    let daemon = arbiterd::start(config(home.path(), &f)).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    let post = |p: &str| client.post(format!("{base}{p}")).bearer_auth(&daemon.token);
    let get = |p: &str| client.get(format!("{base}{p}")).bearer_auth(&daemon.token);
    let p: Value = get("/v1/setup").send().await.unwrap().json().await.unwrap();
    let mut p = p["preferences"].clone();
    p["context7_enabled"] = json!(true);
    p["documentation_model"] = json!("fixture");
    p["max_cloud_runs"] = json!(3);
    post("/v1/setup").json(&p).send().await.unwrap().error_for_status().unwrap();
    let project: Value =
        post("/v1/projects").json(&json!({"path":repo.path()})).send().await.unwrap().json().await.unwrap();
    let status = async |id: &str| -> String {
        let t: Value = get(&format!("/v1/threads/{id}")).send().await.unwrap().json().await.unwrap();
        t["status"].as_str().unwrap_or_default().to_owned()
    };
    let notices = async |id: &str| -> Vec<String> {
        let events: Value = get(&format!("/v1/threads/{id}/events")).send().await.unwrap().json().await.unwrap();
        events
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e["kind"]["text"].as_str().or(e["text"].as_str()).map(str::to_owned))
            .collect()
    };

    // Three parallel preparations share one local model. Before the fix the
    // third was refused ("busy") and its thread failed; now they take turns.
    let mut ids = vec![];
    for n in 0..3 {
        let t: Value = post("/v1/threads")
            .json(&json!({"project_id":project["id"],"harness":"claude","message":format!("Fix the effect cleanup bug number {n} in src/app.ts")}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        ids.push(t["id"].as_str().unwrap().to_owned());
    }
    let sent = f.sent.clone();
    until("all three preparations to dispatch", async || sent.lock().unwrap().len() == 3).await;
    for id in &ids {
        assert_ne!(status(id).await, "failed", "{:?}", notices(id).await);
    }

    // Give A a live, idle process (starting it sweeps the others' idle runs).
    let a = &ids[0];
    post(&format!("/v1/threads/{a}/messages"))
        .json(&json!({"text":"Warm up the cleanup fix in src/app.ts"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    until("A's warm-up", async || sent.lock().unwrap().iter().any(|m| m.contains("Warm up"))).await;
    until("A to finish its turn", async || status(a).await != "running").await;

    // B prepares first and holds the local model; A then prepares with its
    // idle process still alive and queues behind B. When B's preparation ends,
    // B starts a run, which sweeps idle runs. Before the fix that sweep
    // cancelled A's queued preparation; A's process must survive to receive it.
    f.delay_ms.store(1500, Ordering::SeqCst);
    let b = &ids[1];
    let swept = f.shutdowns.load(Ordering::SeqCst);
    post(&format!("/v1/threads/{b}/messages"))
        .json(&json!({"text":"Now fix the second cleanup in src/app.ts"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    post(&format!("/v1/threads/{a}/messages"))
        .json(&json!({"text":"Also handle the retry path in src/app.ts"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    // A second message during preparation is refused with 409 and does not
    // fail the thread.
    let again = post(&format!("/v1/threads/{a}/messages"))
        .json(&json!({"text":"And the logging in src/app.ts"}))
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), 409);
    assert_ne!(status(a).await, "failed");
    until("B's run", async || sent.lock().unwrap().iter().any(|m| m.contains("second cleanup"))).await;
    until("A's steered message", async || sent.lock().unwrap().iter().any(|m| m.contains("retry path"))).await;
    assert_eq!(f.shutdowns.load(Ordering::SeqCst), swept, "A's idle process was reaped while it was preparing");
    let a_notices = notices(a).await;
    assert!(!a_notices.iter().any(|n| n.contains("preparation cancelled")), "{a_notices:?}");
    assert!(a_notices.iter().any(|n| n.starts_with("Message not delivered")), "{a_notices:?}");

    // Stop during preparation stops both the preparation and the idle process.
    f.delay_ms.store(5000, Ordering::SeqCst);
    post(&format!("/v1/threads/{a}/messages"))
        .json(&json!({"text":"One more change in src/app.ts"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let before = f.shutdowns.load(Ordering::SeqCst);
    post(&format!("/v1/threads/{a}/stop")).send().await.unwrap().error_for_status().unwrap();
    let shutdowns = f.shutdowns.clone();
    until("A's process to shut down", async || shutdowns.load(Ordering::SeqCst) > before).await;
    assert!(notices(a).await.iter().any(|n| n.contains("preparation cancelled")));
    assert_eq!(status(a).await, "idle");

    // A restart loses an in-flight preparation; the thread says so plainly.
    let c = &ids[2];
    post(&format!("/v1/threads/{c}/messages"))
        .json(&json!({"text":"Last change in src/app.ts"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    daemon.handle.abort();
    let _ = daemon.handle.await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let daemon = arbiterd::start(config(home.path(), &f)).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let events: Value = client
        .get(format!("{base}/v1/threads/{c}/events"))
        .bearer_auth(&daemon.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(events.to_string().contains("never reached the agent"), "{events}");
    daemon.handle.abort();
}
