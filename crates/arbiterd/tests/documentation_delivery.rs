use arbiter_adapters::{Command, RunHandle};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
async fn credentials_handoff_failure_and_cancellation_are_local_and_bounded() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let git = arbiter_supervisor::integration::git;
    git(repo.path(), &["init", "-q", "-b", "main"]).await.unwrap();
    std::fs::write(repo.path().join("README.md"), "fixture").unwrap();
    git(repo.path(), &["add", "README.md"]).await.unwrap();
    git(repo.path(), &["-c", "commit.gpgSign=false", "commit", "-qm", "initial"]).await.unwrap();
    let mode = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(AtomicUsize::new(0));
    let launches = Arc::new(AtomicUsize::new(0));
    let sent = Arc::new(Mutex::new(Vec::<String>::new()));
    let remote = Arc::new(Mutex::new(Vec::<Value>::new()));
    let secrets = Arc::new(arbiter_supervisor::credentials::MemoryCredentials::default());
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.credential_store = Some(secrets.clone());
    cfg.heal = false;
    cfg.available = Arc::new(|_| true);
    let count = launches.clone();
    let messages = sent.clone();
    cfg.launcher = Arc::new(move |_, _| {
        count.fetch_add(1, Ordering::SeqCst);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (evtx, ev) = tokio::sync::mpsc::unbounded_channel();
        let messages = messages.clone();
        tokio::spawn(async move {
            let _keep = evtx;
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::Send(s) => messages.lock().unwrap().push(s),
                    Command::Shutdown => break,
                    _ => {}
                }
            }
        });
        Ok(RunHandle::from_channels(tx, ev))
    });
    let remote_log = remote.clone();
    cfg.documentation_transport = Some(Arc::new(move |q| {
        remote_log.lock().unwrap().push(q.clone());
        Box::pin(async move {
            Ok(match q["method"].as_str().unwrap() {
                "initialize" => json!({"protocolVersion":"2025-03-26"}),
                "notifications/initialized" => Value::Null,
                "tools/list" => json!({"tools":[{"name":"resolve-library-id"},{"name":"query-docs"}]}),
                "tools/call" if q["params"]["name"] == "resolve-library-id" => {
                    json!({"content":[{"text":"Library ID: /facebook/react"}]})
                }
                "tools/call" => {
                    json!({"content":[{"text":"RAW_PROVIDER_MARKER Cleanup runs before reruns. https://react.dev/reference/react/useEffect"}]})
                }
                _ => panic!("unexpected call"),
            })
        })
    }));
    let behavior = mode.clone();
    let generation_started = entered.clone();
    cfg.local_generator = Some(Arc::new(move |_, prompt, _, schema| {
        assert!(!prompt.contains("ctx7sk_fixture_secret"));
        let mode = behavior.load(Ordering::SeqCst);
        generation_started.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if mode == 1 {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
            Ok(if schema["properties"].get("library").is_some() {json!({"library":"react","topic":"Effect cleanup in React 18"})}
            else if schema["properties"].get("library_id").is_some() {json!({"library_id":if mode==2{"/invented/id"}else{"/facebook/react"}})}
            else if schema["properties"].get("recommendation").is_some() {assert!(prompt.contains("Cleanup runs before reruns."));json!({"summary":"Evidence reviewed independently.","recommendation":"proceed","issues":[{"finding":"Cleanup is documented","source":"https://react.dev/reference/react/useEffect","quote":"Cleanup runs before reruns."}],"limitations":"Fixture only"})}
            else if schema["properties"].get("summary").is_some() {json!({"summary":"Cleanup runs before reruns.","sources":["https://react.dev/reference/react/useEffect"],"limitations":"Check installed version.","citations":[{"source":"https://react.dev/reference/react/useEffect","quote":"Cleanup runs before reruns."}]})}
            else {json!({"message":"Use the credential field, then test the connection."})}.to_string())
        })
    }));
    let daemon = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    let post = |p: &str| client.post(format!("{base}{p}")).bearer_auth(&daemon.token);
    let get = |p: &str| client.get(format!("{base}{p}")).bearer_auth(&daemon.token);
    let secret = "ctx7sk_fixture_secret";
    let saved: Value = post("/v1/documentation/credential")
        .json(&json!({"key":secret,"persist":true}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(saved["source"], "os_store");
    assert!(!saved.to_string().contains(secret));
    assert_eq!(
        client
            .post(format!("{base}/v1/documentation/credential"))
            .json(&json!({"key":secret}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let p: Value = get("/v1/setup").send().await.unwrap().json().await.unwrap();
    let mut p = p["preferences"].clone();
    p["context7_enabled"] = json!(true);
    p["documentation_model"] = json!("fixture");
    p["reviewer_model"] = json!("fixture");
    post("/v1/setup").json(&p).send().await.unwrap().error_for_status().unwrap();
    let project: Value =
        post("/v1/projects").json(&json!({"path":repo.path()})).send().await.unwrap().json().await.unwrap();
    let t:Value=post("/v1/threads").json(&json!({"project_id":project["id"],"harness":"claude","message":"Fix effect cleanup for PRIVATE_PROJECT_MARKER"})).send().await.unwrap().json().await.unwrap();
    let id = t["id"].as_str().unwrap();
    for _ in 0..100 {
        if !sent.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    let prompt = sent.lock().unwrap()[0].clone();
    assert!(prompt.contains("Cleanup runs before reruns."));
    assert!(prompt.contains("Untrusted documentation evidence"));
    assert!(!prompt.contains("RAW_PROVIDER_MARKER"));
    assert!(prompt.len() < 8000);
    assert!(!remote.lock().unwrap().iter().any(|r| r.to_string().contains("PRIVATE_PROJECT_MARKER")));
    let events: Value = get(&format!("/v1/threads/{id}/events")).send().await.unwrap().json().await.unwrap();
    assert!(!events.to_string().contains("RAW_PROVIDER_MARKER"));
    assert!(!events.to_string().contains(secret));
    assert!(!home.path().join("docs-cache").exists());
    post(&format!("/v1/threads/{id}/stop")).send().await.unwrap().error_for_status().unwrap();
    mode.store(2, Ordering::SeqCst);
    post(&format!("/v1/threads/{id}/messages"))
        .json(&json!({"text":"Try again"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    for _ in 0..100 {
        let t: Value = get(&format!("/v1/threads/{id}")).send().await.unwrap().json().await.unwrap();
        if t["status"] == "failed" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    let failed: Value = get(&format!("/v1/threads/{id}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(failed["status"], "failed");
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    mode.store(1, Ordering::SeqCst);
    let before = entered.load(Ordering::SeqCst);
    post(&format!("/v1/threads/{id}/messages"))
        .json(&json!({"text":"Try once more"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    for _ in 0..100 {
        if entered.load(Ordering::SeqCst) > before {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    post(&format!("/v1/threads/{id}/stop")).send().await.unwrap().error_for_status().unwrap();
    let stopped: Value = get(&format!("/v1/threads/{id}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(stopped["status"], "idle");
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    // Standalone cancellation must also release the local assistance lock promptly.
    let before = entered.load(Ordering::SeqCst);
    let request = post("/v1/documentation/setup")
        .json(&json!({"operation_id":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","test":false}));
    let request = tokio::spawn(async move { request.send().await.unwrap() });
    for _ in 0..100 {
        if entered.load(Ordering::SeqCst) > before {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    post("/v1/documentation/operations/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee/cancel")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert!(
        !tokio::time::timeout(std::time::Duration::from_secs(2), request).await.unwrap().unwrap().status().is_success()
    );
    mode.store(0, Ordering::SeqCst);
    post("/v1/documentation/setup").json(&json!({"test":false})).send().await.unwrap().error_for_status().unwrap();
    let reviewed: Value = post(&format!("/v1/threads/{id}/second-opinion"))
        .json(&json!({"stage":"planning"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(reviewed["recommendation"], "proceed", "{reviewed}");
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    // Cancel-before-POST cannot accidentally start work.
    let before = entered.load(Ordering::SeqCst);
    post("/v1/documentation/operations/11111111-2222-3333-4444-555555555555/cancel")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert!(
        !post("/v1/documentation/setup")
            .json(&json!({"operation_id":"11111111-2222-3333-4444-555555555555","test":false}))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    assert_eq!(entered.load(Ordering::SeqCst), before);
    for file in ["setup.db", "setup.db-wal", "arbiter.db", "arbiter.db-wal"] {
        if let Ok(bytes) = std::fs::read(home.path().join(file)) {
            assert!(!bytes.windows(secret.len()).any(|w| w == secret.as_bytes()));
        }
    }
    daemon.handle.abort();
    // Same daemon home reads the saved fixture credential after restart, without echoing it.
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.credential_store = Some(secrets);
    let restarted = arbiterd::start(cfg).await.unwrap();
    let url = format!("http://{}/v1/documentation/credential", restarted.addr);
    let status: Value = client.get(&url).bearer_auth(&restarted.token).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["source"], "os_store");
    let removed: Value = client.delete(&url).bearer_auth(&restarted.token).send().await.unwrap().json().await.unwrap();
    assert_ne!(removed["source"], "os_store");
    restarted.handle.abort();
}
