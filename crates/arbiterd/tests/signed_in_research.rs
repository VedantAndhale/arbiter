//! Pages that need JavaScript are rendered; sites the user signed into are
//! read with the Arbiter profile, audited locally, and held for the user when
//! the summary looks private. Everything browser-shaped goes through the fake
//! web transport.
use arbiter_adapters::{Command, HarnessEvent, RunHandle, TurnOutcome};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn dynamic_and_signed_in_pages() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let git = arbiter_supervisor::integration::git;
    git(repo.path(), &["init", "-q", "-b", "main"]).await.unwrap();
    std::fs::write(repo.path().join("app.py"), "print('hi')\n").unwrap();
    git(repo.path(), &["add", "."]).await.unwrap();
    git(repo.path(), &["-c", "commit.gpgSign=false", "commit", "-qm", "initial"]).await.unwrap();

    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let private = Arc::new(Mutex::new(true));
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
    let log = calls.clone();
    cfg.web_transport = Some(Arc::new(move |kind, arg| {
        log.lock().unwrap().push(format!("{kind}:{arg}"));
        Box::pin(async move {
            Ok(match kind {
                "fetch" => "<html><div id=root></div><script src=app.js></script></html>".to_owned(),
                "render" => json!({"url": arg, "text": "Rendered docs: call createApp() before mount() to start the app."})
                    .to_string(),
                "render_signed_in" if arg.contains("elsewhere") => {
                    json!({"url":"https://login.other.example/","text":"Sign in"}).to_string()
                }
                "render_signed_in" => json!({"url": arg, "text": "Ticket 42 from jane.doe@example.com: the export button fails with error E17 on large files."}).to_string(),
                _ => String::new(),
            })
        })
    }));
    let flag = private.clone();
    cfg.local_generator = Some(Arc::new(move |_, prompt, _, schema| {
        let flag = flag.clone();
        Box::pin(async move {
            let p = *flag.lock().unwrap();
            Ok(if schema["properties"].get("private").is_some() {
                json!({"private": p, "reasons": if p { vec!["mentions a customer ticket"] } else { vec![] }})
            } else if prompt.contains("createApp") {
                json!({"summary":"Call createApp() before mount().","sources":["https://docs.example.com/start"],"limitations":"","citations":[]})
            } else if p {
                json!({"summary":"Ticket 42 from jane.doe@example.com: export fails with E17 on large files.","sources":["https://app.example.com/t/42"],"limitations":"","citations":[]})
            } else {
                json!({"summary":"The export button fails with error E17 on large files.","sources":["https://app.example.com/t/42"],"limitations":"","citations":[]})
            }
            .to_string())
        })
    }));
    let daemon = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    let post = |p: &str| client.post(format!("{base}{p}")).bearer_auth(&daemon.token);
    let get = |p: &str| client.get(format!("{base}{p}")).bearer_auth(&daemon.token);
    let p: Value = get("/v1/setup").send().await.unwrap().json().await.unwrap();
    let mut p = p["preferences"].clone();
    p["web_research_enabled"] = json!(true);
    p["documentation_model"] = json!("fixture");
    post("/v1/setup").json(&p).send().await.unwrap().error_for_status().unwrap();
    let ask = async |q: &str, url: &str| -> Value {
        post("/v1/research").json(&json!({"question":q,"url":url})).send().await.unwrap().json().await.unwrap()
    };
    let events = async |thread: &str| -> Vec<Value> {
        let v: Value = get(&format!("/v1/threads/{thread}/events")).send().await.unwrap().json().await.unwrap();
        v.as_array().unwrap().clone()
    };

    // A page that is empty without JavaScript is rendered on a throwaway profile.
    let brief = ask("How do I start the app?", "https://docs.example.com/start").await;
    assert_eq!(brief["summary"], "Call createApp() before mount().", "{brief}");
    assert!(calls.lock().unwrap().contains(&"render:https://docs.example.com/start".to_owned()));

    // Signing in opens the browser for the user and remembers the site.
    let r = post("/v1/web/sites").json(&json!({"url":"app.example.com"})).send().await.unwrap();
    assert!(r.status().is_success());
    assert!(calls.lock().unwrap().contains(&"sign_in:https://app.example.com".to_owned()));
    let r = post("/v1/web/sites").json(&json!({"url":"http://127.0.0.1"})).send().await.unwrap();
    assert!(r.status().is_client_error());
    let sites: Value = get("/v1/web/sites").send().await.unwrap().json().await.unwrap();
    assert_eq!(sites["sites"], json!(["app.example.com"]));

    // A private-looking summary is held, not returned.
    let held = ask("Why does export fail?", "https://app.example.com/t/42").await;
    assert!(held["summary"].is_null(), "{held}");
    assert_eq!(held["host"], "app.example.com");
    let reasons = held["reasons"].to_string();
    assert!(reasons.contains("email") && reasons.contains("ticket"), "{reasons}");
    assert!(calls.lock().unwrap().iter().any(|c| c == "render_signed_in:https://app.example.com/t/42"));
    let id = held["held"].as_str().unwrap();
    let list: Value = get("/v1/research/held").send().await.unwrap().json().await.unwrap();
    assert_eq!(list["held"][0]["id"], id);
    let kept: Value = post(&format!("/v1/research/held/{id}"))
        .json(&json!({"allow":false}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(kept["kept_private"], true);
    let again = post(&format!("/v1/research/held/{id}")).json(&json!({"allow":true})).send().await.unwrap();
    assert!(again.status().is_client_error());

    // A redirect off the signed-in site is not read.
    let r = post("/v1/research")
        .json(&json!({"question":"Open it","url":"https://app.example.com/elsewhere"}))
        .send()
        .await
        .unwrap();
    assert!(r.text().await.unwrap().contains("left app.example.com"));

    // For a task, the agent waits while the user decides on a card.
    let project: Value =
        post("/v1/projects").json(&json!({"path":repo.path()})).send().await.unwrap().json().await.unwrap();
    let t: Value = post("/v1/threads")
        .json(&json!({"project_id":project["id"],"harness":"claude","worktree":true,"message":"Fix the export"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let thread = t["id"].as_str().unwrap().to_owned();
    // Let the fake agent's first turn finish, so its status change cannot
    // land after the permission card's (a real agent waits for research).
    for _ in 0..200 {
        let t: Value = get(&format!("/v1/threads/{thread}")).send().await.unwrap().json().await.unwrap();
        if t["status"] == "review" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let research = {
        let client = client.clone();
        let url = format!("{base}/v1/threads/{thread}/research");
        let token = daemon.token.clone();
        tokio::spawn(async move {
            client
                .post(url)
                .bearer_auth(token)
                .json(&json!({"question":"Why does export fail?","url":"https://app.example.com/t/42"}))
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()
        })
    };
    let mut share = None;
    for _ in 0..200 {
        share = events(&thread).await.into_iter().find(|e| e["kind"]["type"] == "share_requested");
        if share.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let share = share.expect("a permission card");
    assert_eq!(share["kind"]["host"], "app.example.com");
    let id = share["kind"]["id"].as_str().unwrap();
    let waiting: Value = get(&format!("/v1/threads/{thread}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(waiting["status"], "needs_approval");
    post(&format!("/v1/research/held/{id}"))
        .json(&json!({"allow":true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let brief = research.await.unwrap();
    assert!(brief["summary"].as_str().unwrap().contains("E17"), "{brief}");
    assert!(events(&thread).await.iter().any(|e| e["kind"]["type"] == "share_decided" && e["kind"]["allowed"] == true));
    let after: Value = get(&format!("/v1/threads/{thread}")).send().await.unwrap().json().await.unwrap();
    assert_ne!(after["status"], "needs_approval");

    // Nothing private: shared without asking.
    *private.lock().unwrap() = false;
    let brief = ask("Why does export fail?", "https://app.example.com/t/42").await;
    assert_eq!(brief["summary"], "The export button fails with error E17 on large files.", "{brief}");

    // Signing out forgets the site and clears its cookies.
    let r: Value = client
        .delete(format!("{base}/v1/web/sites/app.example.com"))
        .bearer_auth(&daemon.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["sites"], json!([]));
    assert!(calls.lock().unwrap().contains(&"forget:app.example.com".to_owned()));
    daemon.handle.abort();
}
