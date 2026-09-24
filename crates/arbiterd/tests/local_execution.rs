use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
async fn local_only_edits_isolated_files_without_cloud_and_can_cancel() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let git = arbiter_supervisor::integration::git;
    git(repo.path(), &["init", "-q", "-b", "main"]).await.unwrap();
    std::fs::write(repo.path().join("calc.py"), "def add(a,b): return a-b\n").unwrap();
    git(repo.path(), &["add", "calc.py"]).await.unwrap();
    git(repo.path(), &["-c", "commit.gpgSign=false", "commit", "-qm", "initial"]).await.unwrap();
    let steps = Arc::new(AtomicUsize::new(0));
    let counter = steps.clone();
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.heal = false;
    cfg.launcher = Arc::new(|_, _| panic!("no cloud launches allowed"));
    cfg.local_generator = Some(Arc::new(move |_, prompt, _, _| {
        let n = counter.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if n > 3 {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
            if n == 0 {
                return Ok(json!({"action":"documentation","path":"react","content":"Effect cleanup","before_hash":"","summary":"Request documentation"}).to_string());
            }
            let evidence: Value =
                serde_json::from_str(prompt.split("Latest tool result:\n").last().unwrap_or("")).unwrap_or(Value::Null);
            Ok(json!({"action":if n==1{"read"}else if n==2{"write"}else{"done"},"path":"calc.py","before_hash":evidence["hash"].as_str().unwrap_or(""),"content":"def add(a,b): return a+b\n","summary":"Fixed addition. Checks were not run."}).to_string())
        })
    }));
    let daemon = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    let post = |path: &str| client.post(format!("{base}{path}")).bearer_auth(&daemon.token);
    let get = |path: &str| client.get(format!("{base}{path}")).bearer_auth(&daemon.token);
    let p: Value = get("/v1/setup").send().await.unwrap().json().await.unwrap();
    let mut p = p["preferences"].clone();
    p["mode"] = json!("local");
    p["network"] = json!(false);
    post("/v1/setup").json(&p).send().await.unwrap().error_for_status().unwrap();
    let project: Value =
        post("/v1/projects").json(&json!({"path":repo.path()})).send().await.unwrap().json().await.unwrap();
    let t:Value=post("/v1/threads").json(&json!({"project_id":project["id"],"harness":"local","model":"fixture","worktree":true,"message":"Fix addition"})).send().await.unwrap().json().await.unwrap();
    let id = t["id"].as_str().unwrap();
    let thread = format!("/v1/threads/{id}");
    for _ in 0..100 {
        let t: Value = get(&thread).send().await.unwrap().json().await.unwrap();
        if t["status"] == "review" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let t: Value = get(&thread).send().await.unwrap().json().await.unwrap();
    assert_eq!(t["status"], "review", "{t}");
    assert_eq!(std::fs::read_to_string(repo.path().join("calc.py")).unwrap(), "def add(a,b): return a-b\n");
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(t["worktree"].as_str().unwrap()).join("calc.py")).unwrap(),
        "def add(a,b): return a+b\n"
    );
    post(&format!("{thread}/messages"))
        .json(&json!({"text":"Continue"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    post(&format!("{thread}/stop")).send().await.unwrap().error_for_status().unwrap();
    let t: Value = get(&thread).send().await.unwrap().json().await.unwrap();
    assert_eq!(t["status"], "idle");
    daemon.handle.abort();
}
