//! Adding a project is one step: a repository with history is added as it is,
//! an empty folder is set up and saved as a first version, and only a folder
//! with files but no history asks the user to review the first version.
use serde_json::{Value, json};
use std::sync::Arc;

#[tokio::test]
async fn folders_are_added_in_one_step_when_safe() {
    let home = tempfile::tempdir().unwrap();
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.launcher = Arc::new(|_, _| panic!("adding a project must not start an agent"));
    let daemon = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    let add = async |path: &std::path::Path| -> Value {
        client
            .post(format!("{base}/v1/projects/add"))
            .bearer_auth(&daemon.token)
            .json(&json!({"path":path}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    };
    let git = arbiter_supervisor::integration::git;

    // A repository with history: added untouched.
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]).await.unwrap();
    std::fs::write(repo.path().join("app.py"), "print('hi')\n").unwrap();
    git(repo.path(), &["add", "."]).await.unwrap();
    git(repo.path(), &["-c", "commit.gpgSign=false", "commit", "-qm", "initial"]).await.unwrap();
    let r = add(repo.path()).await;
    assert_eq!(r["status"], "added", "{r}");
    assert!(!repo.path().join("AGENTS.md").exists(), "an existing repository is not modified");

    // An empty folder: set up, first version saved, added.
    let empty = tempfile::tempdir().unwrap();
    let r = add(empty.path()).await;
    assert_eq!(r["status"], "added", "{r}");
    assert_eq!(git(empty.path(), &["rev-list", "--count", "HEAD"]).await.unwrap().trim(), "1");
    assert!(git(empty.path(), &["ls-files"]).await.unwrap().lines().any(|l| l == "AGENTS.md"));

    // Files but no history: the user reviews what the first version includes.
    let loose = tempfile::tempdir().unwrap();
    std::fs::write(loose.path().join("notes.txt"), "draft").unwrap();
    let r = add(loose.path()).await;
    assert_eq!(r["status"], "review", "{r}");
    assert!(r["proposal"]["id"].is_string());
    assert!(!loose.path().join(".git").exists(), "nothing committed without review");

    let missing = add(&home.path().join("nope")).await;
    assert!(missing["error"].as_str().unwrap().contains("does not exist"), "{missing}");
    let projects: Vec<Value> = client
        .get(format!("{base}/v1/projects"))
        .bearer_auth(&daemon.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(projects.len(), 2);
    daemon.handle.abort();
}
