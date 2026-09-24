//! The task file editor: lists the working copy, opens text files, saves only
//! when the file is unchanged since it was opened, and never opens secrets.
use serde_json::{Value, json};
use std::sync::Arc;

#[tokio::test]
async fn edit_files_in_a_task_copy_safely() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let git = arbiter_supervisor::integration::git;
    git(repo.path(), &["init", "-q", "-b", "main"]).await.unwrap();
    std::fs::create_dir(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("src/app.js"), "console.log('hi')\n").unwrap();
    git(repo.path(), &["add", "."]).await.unwrap();
    git(repo.path(), &["-c", "commit.gpgSign=false", "commit", "-qm", "initial"]).await.unwrap();
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.launcher = Arc::new(|_, _| panic!("no agents"));
    let daemon = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    let post = |p: &str| client.post(format!("{base}{p}")).bearer_auth(&daemon.token);
    let get = |p: &str| client.get(format!("{base}{p}")).bearer_auth(&daemon.token);
    let put = |p: &str| client.put(format!("{base}{p}")).bearer_auth(&daemon.token);
    let project: Value =
        post("/v1/projects").json(&json!({"path":repo.path()})).send().await.unwrap().json().await.unwrap();
    let t: Value = post("/v1/threads")
        .json(&json!({"project_id":project["id"],"harness":"claude","worktree":true,"title":"Edit"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = t["id"].as_str().unwrap();
    let wt = std::path::PathBuf::from(t["worktree"].as_str().unwrap());
    std::fs::write(wt.join(".env"), "SECRET=1").unwrap();
    std::fs::write(wt.join("src/new.js"), "export {}\n").unwrap();

    let files: Vec<String> = get(&format!("/v1/threads/{id}/files")).send().await.unwrap().json().await.unwrap();
    assert!(files.contains(&"src/app.js".into()) && files.contains(&"src/new.js".into()), "{files:?}");
    assert!(!files.iter().any(|f| f.starts_with(".env")), "secrets are not listed");

    let file: Value =
        get(&format!("/v1/threads/{id}/file?path=src/app.js")).send().await.unwrap().json().await.unwrap();
    let hash = file["hash"].as_str().unwrap().to_owned();
    let saved: Value = put(&format!("/v1/threads/{id}/file"))
        .json(&json!({"path":"src/app.js","content":"console.log('hello')\n","hash":hash}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(saved["hash"].is_string(), "{saved}");
    assert_eq!(std::fs::read_to_string(wt.join("src/app.js")).unwrap(), "console.log('hello')\n");
    assert_eq!(
        std::fs::read_to_string(repo.path().join("src/app.js")).unwrap(),
        "console.log('hi')\n",
        "only the task copy"
    );

    // A stale save is refused rather than overwriting someone else's change.
    let stale = put(&format!("/v1/threads/{id}/file"))
        .json(&json!({"path":"src/app.js","content":"x","hash":hash}))
        .send()
        .await
        .unwrap();
    assert_eq!(stale.status(), 409);
    for bad in [".env", "../outside.txt"] {
        let r = get(&format!("/v1/threads/{id}/file?path={bad}")).send().await.unwrap();
        assert!(!r.status().is_success(), "{bad}");
    }
    let closed: Value = post(&format!("/v1/threads/{id}/terminal/close")).send().await.unwrap().json().await.unwrap();
    assert_eq!(closed["closed"], false, "no terminal was open");
    daemon.handle.abort();
}
