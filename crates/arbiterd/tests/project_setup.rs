use serde_json::{Value, json};
use std::sync::Arc;

#[tokio::test]
async fn adoption_baseline_offline_docs_and_initial_commit() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(
        repo.path().join("package.json"),
        r#"{"scripts":{"test":"exit 0"},"dependencies":{"react":"^18.0.0"}}"#,
    )
    .unwrap();
    std::fs::write(repo.path().join(".env"), "SECRET=preserved").unwrap();
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.launcher = Arc::new(|_, _| panic!("project setup must never invoke a coding model"));
    let daemon = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    let post = |path: &str| client.post(format!("{base}{path}")).bearer_auth(&daemon.token);
    assert_eq!(
        client
            .post(format!("{base}/v1/project-setup/inspect"))
            .json(&json!({"path":repo.path()}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let inspected: Value = post("/v1/project-setup/inspect")
        .json(&json!({"path":repo.path(),"purpose":"demo"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = inspected["proposal"]["id"].as_str().unwrap();
    assert!(!inspected.to_string().contains("preserved"));
    assert!(!inspected["proposal"]["inventory"]["initial_files"].as_array().unwrap().contains(&json!(".env")));
    assert!(!repo.path().join("AGENTS.md").exists());
    let applied: Value = post(&format!("/v1/project-setup/{id}/apply")).send().await.unwrap().json().await.unwrap();
    assert_eq!(applied["state"], "applied");
    let baseline: Value = post("/v1/project-setup/baseline")
        .json(&json!({"path":repo.path(),"fingerprint":applied["inventory"]["fingerprint"]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(baseline["status"], "passed", "{baseline}");
    let prefs: Value =
        client.get(format!("{base}/v1/setup")).bearer_auth(&daemon.token).send().await.unwrap().json().await.unwrap();
    let mut prefs = prefs["preferences"].clone();
    prefs["network"] = json!(false);
    assert!(post("/v1/setup").json(&prefs).send().await.unwrap().status().is_success());
    let docs: Value = post("/v1/project-setup/docs")
        .json(&json!({"path":repo.path(),"index":0,"refresh":true}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(docs["network"], false);
    assert!(docs["cached"].is_null());
    assert!(
        !post(&format!("/v1/project-setup/{id}/initialize"))
            .json(&json!({"files":[".env"]}))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    assert!(!repo.path().join(".git").exists());
    let init = post(&format!("/v1/project-setup/{id}/initialize"))
        .json(&json!({"files":["AGENTS.md","README.md","package.json"]}))
        .send()
        .await
        .unwrap();
    assert!(init.status().is_success(), "{}", init.text().await.unwrap());
    let project: Value =
        post("/v1/projects").json(&json!({"path":repo.path()})).send().await.unwrap().json().await.unwrap();
    let duplicate: Value =
        post("/v1/projects").json(&json!({"path":repo.path()})).send().await.unwrap().json().await.unwrap();
    assert_eq!(project["id"], duplicate["id"]);
    let tracked = arbiter_supervisor::integration::git(repo.path(), &["ls-files"]).await.unwrap();
    assert!(tracked.contains("package.json"));
    assert!(!tracked.contains(".env"));
    assert_eq!(std::fs::read_to_string(repo.path().join(".env")).unwrap(), "SECRET=preserved");
    daemon.handle.abort();
}

/// A failure after `git init` must not leave a half-made repository behind:
/// the retry must work, and a commit-less repository must never be registered.
#[tokio::test]
async fn failed_initial_commit_rolls_back_and_empty_repos_are_refused() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("package.json"), r#"{"scripts":{"test":"exit 1"}}"#).unwrap();
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.launcher = Arc::new(|_, _| panic!("project setup must never invoke a coding model"));
    let daemon = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    let post = |path: &str| client.post(format!("{base}{path}")).bearer_auth(&daemon.token);
    let inspected: Value = post("/v1/project-setup/inspect")
        .json(&json!({"path":repo.path()}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = inspected["proposal"]["id"].as_str().unwrap().to_owned();
    let applied: Value = post(&format!("/v1/project-setup/{id}/apply")).send().await.unwrap().json().await.unwrap();
    assert_eq!(applied["state"], "applied");

    // Baseline failures carry a short explanation, not just "failed".
    let baseline: Value = post("/v1/project-setup/baseline")
        .json(&json!({"path":repo.path(),"fingerprint":applied["inventory"]["fingerprint"]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(baseline["status"], "existing_failures", "{baseline}");
    assert!(baseline["results"][0]["evidence"].is_string(), "{baseline}");
    assert!(baseline["results"][0]["cmd"].as_str().unwrap().contains("test"));

    // Inject a failure after `git init`: the hooks directory cannot be created.
    std::fs::write(home.path().join("empty-hooks"), "not a directory").unwrap();
    let files = json!({"files":["package.json"]});
    let r = post(&format!("/v1/project-setup/{id}/initialize")).json(&files).send().await.unwrap();
    assert!(!r.status().is_success());
    assert!(r.text().await.unwrap().contains("removed"));
    assert!(!repo.path().join(".git").exists(), "half-initialized repository was rolled back");

    // The retry succeeds once the cause is gone.
    std::fs::remove_file(home.path().join("empty-hooks")).unwrap();
    let r = post(&format!("/v1/project-setup/{id}/initialize")).json(&files).send().await.unwrap();
    assert!(r.status().is_success(), "{}", r.text().await.unwrap());
    assert!(post("/v1/projects").json(&json!({"path":repo.path()})).send().await.unwrap().status().is_success());

    // A repository without commits cannot be added: worktrees need HEAD.
    let empty = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("git").arg("-C").arg(empty.path()).args(["init", "-q"]).status().unwrap().success()
    );
    let r = post("/v1/projects").json(&json!({"path":empty.path()})).send().await.unwrap();
    assert!(!r.status().is_success());
    assert!(r.text().await.unwrap().contains("no commits"));
    daemon.handle.abort();
}
