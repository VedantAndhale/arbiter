//! Tool checks and guarded installs. Nothing is installed here: approval is
//! required, and an installed tool is never reinstalled. Git identity writes go
//! to a temporary global config, never the developer's own.
use serde_json::{Value, json};
use std::sync::Arc;

#[tokio::test]
async fn tools_are_checked_and_installs_are_guarded() {
    let home = tempfile::tempdir().unwrap();
    let gitconfig = home.path().join("gitconfig");
    std::fs::write(&gitconfig, "").unwrap();
    // SAFETY: set before the daemon (and any threads using the environment) starts.
    unsafe { std::env::set_var("GIT_CONFIG_GLOBAL", &gitconfig) };
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.launcher = Arc::new(|_, _| panic!("no agents"));
    let daemon = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    let post = |p: &str| client.post(format!("{base}{p}")).bearer_auth(&daemon.token);
    let get = |p: &str| client.get(format!("{base}{p}")).bearer_auth(&daemon.token);

    let state: Value = get("/v1/prerequisites").send().await.unwrap().json().await.unwrap();
    let tools = state["tools"].as_array().unwrap();
    assert_eq!(
        tools.iter().map(|t| t["id"].as_str().unwrap()).collect::<Vec<_>>(),
        ["git", "node", "python", "claude", "codex"]
    );
    let git = &tools[0];
    assert_eq!(git["installed"], true, "{git}");
    assert_eq!(git["required"], true);
    assert!(tools[3]["sign_in"] == true && git["sign_in"] == false);
    assert!(state["git_identity"]["name"].is_null(), "fresh global config");

    // Approval is required, and installed tools are not reinstalled.
    assert_eq!(post("/v1/prerequisites/git/install").json(&json!({})).send().await.unwrap().status(), 400);
    let r = post("/v1/prerequisites/git/install").json(&json!({"approved":true})).send().await.unwrap();
    assert!(r.text().await.unwrap().contains("already installed"));
    assert!(
        !post("/v1/prerequisites/nope/install")
            .json(&json!({"approved":true}))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );

    // Git identity: validated, saved to the (temporary) global config.
    let bad = post("/v1/git-identity").json(&json!({"name":"Ada","email":"not-an-email"})).send().await.unwrap();
    assert!(!bad.status().is_success());
    post("/v1/git-identity")
        .json(&json!({"name":"Ada Lovelace","email":"ada@example.com"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let state: Value = get("/v1/prerequisites").send().await.unwrap().json().await.unwrap();
    assert_eq!(state["git_identity"]["email"], "ada@example.com");
    assert!(std::fs::read_to_string(&gitconfig).unwrap().contains("Ada Lovelace"));
    daemon.handle.abort();
}
