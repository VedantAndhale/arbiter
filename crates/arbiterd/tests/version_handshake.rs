//! An updated app finds an older background service by its version and asks
//! it to stop. Stopping needs the token. (The success path exits the process,
//! so it is exercised by the desktop shell, not here.)
use serde_json::Value;

#[tokio::test]
async fn health_reports_version_and_shutdown_needs_the_token() {
    let home = tempfile::tempdir().unwrap();
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.launcher = std::sync::Arc::new(|_, _| panic!("no agent in this test"));
    let daemon = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let http = reqwest::Client::new();
    let health: Value = http.get(format!("{base}/v1/health")).send().await.unwrap().json().await.unwrap();
    assert_eq!(health["ok"], true);
    assert_eq!(health["version"], env!("CARGO_PKG_VERSION"));
    let r = http.post(format!("{base}/v1/shutdown")).send().await.unwrap();
    assert_eq!(r.status(), 401);
    daemon.handle.abort();
}
