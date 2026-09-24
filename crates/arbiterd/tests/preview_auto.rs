//! "Show me my app": when an agent finishes on a web project, Arbiter starts
//! the app itself and records where it runs. A plain HTML site is served by
//! arbiterd, with no toolchain. The start command can be changed per project
//! without touching the repository.
use arbiter_adapters::{Command, HarnessEvent, RunHandle, TurnOutcome};
use serde_json::{Value, json};
use std::sync::Arc;

#[tokio::test]
async fn finished_task_starts_the_app_and_command_is_editable() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let git = arbiter_supervisor::integration::git;
    git(repo.path(), &["init", "-q", "-b", "main"]).await.unwrap();
    std::fs::write(repo.path().join("index.html"), "<h1>Old title</h1>").unwrap();
    std::fs::create_dir(repo.path().join("css")).unwrap();
    std::fs::write(repo.path().join("css/site.css"), "h1{color:red}").unwrap();
    git(repo.path(), &["add", "."]).await.unwrap();
    git(repo.path(), &["-c", "commit.gpgSign=false", "commit", "-qm", "initial"]).await.unwrap();
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.heal = false;
    cfg.available = Arc::new(|_| true);
    cfg.launcher = Arc::new(|_, opts| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (events, ev) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::Send(_) => {
                        std::fs::write(opts.cwd.join("index.html"), "<h1>Pricing</h1>").unwrap();
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
        .json(
            &json!({"project_id":project["id"],"harness":"claude","worktree":true,"message":"Make the title Pricing"}),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = t["id"].as_str().unwrap();

    let mut url = None;
    for _ in 0..200 {
        let events: Vec<Value> = get(&format!("/v1/threads/{id}/events")).send().await.unwrap().json().await.unwrap();
        url = events.iter().find_map(|e| (e["kind"]["type"] == "preview_ready").then(|| e["kind"]["url"].clone()));
        if url.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let url = url.expect("the app was started after the turn").as_str().unwrap().to_owned();
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    // The built-in server shows the agent's change, and serves assets.
    let page = client.get(&url).send().await.unwrap().text().await.unwrap();
    assert_eq!(page, "<h1>Pricing</h1>");
    let css = client.get(format!("{url}/css/site.css")).send().await.unwrap();
    assert_eq!(css.headers()["content-type"], "text/css; charset=utf-8");
    // Nothing outside the site, and no hidden files.
    assert_eq!(client.get(format!("{url}/..%2F..%2Fsecret")).send().await.unwrap().status(), 404);
    assert_eq!(client.get(format!("{url}/.git/config")).send().await.unwrap().status(), 404);

    // The start command: detected, customizable, clearable; never written to the repo.
    let cmd: Value = get(&format!("/v1/threads/{id}/preview/command")).send().await.unwrap().json().await.unwrap();
    assert_eq!(cmd["detected"], "builtin:static");
    let saved: Value = client
        .put(format!("{base}/v1/threads/{id}/preview/command"))
        .bearer_auth(&daemon.token)
        .json(&json!({"dev":"python -m http.server {port}"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(saved["effective"], "python -m http.server {port}", "{saved}");
    assert!(!repo.path().join(".arbiter").exists());
    let cleared: Value = client
        .put(format!("{base}/v1/threads/{id}/preview/command"))
        .bearer_auth(&daemon.token)
        .json(&json!({"dev":null}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(cleared["effective"], "builtin:static");
    daemon.handle.abort();
}
