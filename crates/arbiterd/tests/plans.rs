//! Real Git + HTTP + fake agents: no harness subscriptions or live tokens.
use arbiter_adapters::{Command, HarnessEvent, RunHandle, TurnOutcome};
use arbiter_core::AgentEvent;
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

fn git(path: &Path, args: &[&str]) {
    assert!(std::process::Command::new("git").arg("-C").arg(path).args(args).output().unwrap().status.success());
}
struct Fixture {
    _home: tempfile::TempDir,
    repo: tempfile::TempDir,
    base: String,
    token: String,
    http: reqwest::Client,
    peak: Arc<AtomicUsize>,
}
impl Fixture {
    async fn new(mode: &'static str) -> Self {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.name", "fixture"],
            &["config", "user.email", "fixture@localhost"],
        ] {
            git(repo.path(), args);
        }
        std::fs::write(repo.path().join("README.md"), "fixture").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-qm", "initial"]);
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let measure = peak.clone();
        let mut config = arbiterd::Config::new(home.path().into(), 0);
        config.heal = false;
        config.available = Arc::new(|_| true);
        config.fixture_accounts = Some(arbiterd::fixture_accounts());
        config.launcher = Arc::new(move |_, opts| {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let (events, ev) = tokio::sync::mpsc::unbounded_channel();
            let active = active.clone();
            let peak = measure.clone();
            tokio::spawn(async move {
                while let Some(cmd) = rx.recv().await {
                    match cmd {
                        Command::Send(text) => {
                            let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                            peak.fetch_max(count, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(1500)).await;
                            let id = text
                                .lines()
                                .find_map(|s| s.strip_prefix("Step ").and_then(|s| s.split(':').next()))
                                .unwrap_or("a");
                            if id == "c" {
                                assert!(opts.cwd.join("a.txt").exists(), "dependent started before a was merged");
                            }
                            std::fs::write(
                                opts.cwd.join(format!("{id}.txt")),
                                if text.contains("Repair these failed") {
                                    "fixed\n".to_owned()
                                } else {
                                    format!("verified {id}\n")
                                },
                            )
                            .unwrap();
                            if mode == "conflict" {
                                if text.starts_with("Resolve only") || text.starts_with("Continue resolving") {
                                    std::fs::write(opts.cwd.join("README.md"), "verified a\nverified b\n").unwrap();
                                    git(&opts.cwd, &["add", "README.md"]);
                                } else {
                                    std::fs::write(opts.cwd.join("README.md"), format!("verified {id}\n")).unwrap();
                                }
                            }
                            if mode == "drift" {
                                std::fs::write(opts.cwd.join("outside.txt"), "scope expansion").unwrap();
                            }
                            let _=events.send(HarnessEvent::Agent(AgentEvent::Message{text:json!({"handoff":{"summary":format!("Implemented {id}"),"files":[format!("{id}.txt")],"decisions":[],"interfaces":[],"open_issues":[]}}).to_string()}));
                            active.fetch_sub(1, Ordering::SeqCst);
                            let _ = events.send(HarnessEvent::TurnDone(TurnOutcome::Completed));
                        }
                        Command::Shutdown => break,
                        Command::Interrupt => {
                            let _ = events.send(HarnessEvent::TurnDone(TurnOutcome::Interrupted));
                        }
                        Command::Approve { .. } => {}
                    }
                }
                let _ = events.send(HarnessEvent::Exited { code: None, stderr_tail: String::new() });
            });
            Ok(RunHandle::from_channels(tx, ev))
        });
        let daemon = arbiterd::start(config).await.unwrap();
        Self {
            _home: home,
            repo,
            base: format!("http://{}", daemon.addr),
            token: daemon.token,
            http: reqwest::Client::new(),
            peak,
        }
    }
    async fn call(&self, method: reqwest::Method, path: &str, body: Value) -> (u16, Value) {
        let r = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let code = r.status().as_u16();
        (code, r.json().await.unwrap_or(Value::Null))
    }
    async fn get(&self, path: &str) -> Value {
        self.call(reqwest::Method::GET, path, json!({})).await.1
    }
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        self.call(reqwest::Method::POST, path, body).await
    }
    async fn create(&self) -> String {
        let (_, p) = self.post("/v1/projects", json!({"path":self.repo.path()})).await;
        let (_, t) =
            self.post("/v1/threads", json!({"project_id":p["id"],"title":"Three-step plan","worktree":false})).await;
        t["id"].as_str().unwrap().into()
    }
    async fn wait(&self, id: &str, done: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..300 {
            let s = self.get(&format!("/v1/threads/{id}/plan")).await;
            if done(&s) {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("plan did not reach expected state: {}", self.get(&format!("/v1/threads/{id}/plan")).await)
    }
}
fn plan() -> Value {
    json!({"title":"Ship the fixture","goal":"Implement three independent files with a dependency","concurrency":3,"nodes":[
        {"id":"a","title":"First","goal":"Implement a.txt","scope":["a.txt"],"dependencies":[]},
        {"id":"b","title":"Second","goal":"Implement b.txt","scope":["b.txt"],"dependencies":[]},
        {"id":"c","title":"Dependent","goal":"Implement c.txt after a.txt","scope":["c.txt"],"dependencies":["a"]}
    ]})
}
#[tokio::test]
async fn three_nodes_run_in_parallel_integrate_and_link_tasks() {
    let f = Fixture::new("normal").await;
    let id = f.create().await;
    let path = format!("/v1/threads/{id}/plan");
    let (code, state) = f.call(reqwest::Method::PATCH, &path, plan()).await;
    assert_eq!(code, 200, "{state}");
    assert_eq!(f.post(&format!("{path}/approve"), json!({"revision":99})).await.0, 422);
    assert_eq!(f.peak.load(Ordering::SeqCst), 0, "no agent before approval");
    assert_eq!(f.post(&format!("{path}/approve"), json!({"revision":1})).await.0, 200);
    assert_eq!(f.post(&format!("{path}/approve"), json!({"revision":1})).await.0, 422);
    let state = f.wait(&id, |s| !s["completed"].is_null() || !s["paused"].is_null()).await;
    assert!(state["paused"].is_null(), "{state}");
    assert!(!state["completed"].is_null());
    let integrated = Path::new(state["path"].as_str().unwrap());
    for name in ["a", "b", "c"] {
        assert!(integrated.join(format!("{name}.txt")).exists());
        assert!(!f.repo.path().join(format!("{name}.txt")).exists());
        assert_eq!(state["nodes"][name]["status"], "merged");
    }
    let vault = f.get(&format!("/v1/threads/{id}/vault")).await;
    assert_eq!(vault["notes"].as_array().unwrap().len(), 4, "approved plan and three handoffs are mirrored");
    let learned = f.get(&format!("/v1/threads/{id}/learning")).await;
    assert_eq!(learned["outcomes"].as_object().unwrap().len(), 3, "one outcome per integrated child");
    assert!(f.peak.load(Ordering::SeqCst) >= 2, "independent steps should overlap");
    let tasks = f.get("/v1/tasks").await;
    assert_eq!(tasks.as_array().unwrap().len(), 4);
    assert_eq!(tasks.as_array().unwrap().iter().filter(|t| t["status"] == "done").count(), 3);
    assert_eq!(f.post(&format!("/v1/threads/{id}/fork"), json!({"at_seq":2})).await.0, 409);
}
#[tokio::test]
async fn drift_blocks_integration_until_scope_is_explicitly_extended() {
    let f = Fixture::new("drift").await;
    let id = f.create().await;
    let path = format!("/v1/threads/{id}/plan");
    let mut plan = plan();
    plan["nodes"] = json!([plan["nodes"][0].clone()]);
    assert_eq!(f.call(reqwest::Method::PATCH, &path, plan).await.0, 200);
    assert_eq!(f.post(&format!("{path}/approve"), json!({"revision":1})).await.0, 200);
    let state = f.wait(&id, |s| !s["paused"].is_null()).await;
    assert_eq!(state["nodes"]["a"]["status"], "blocked");
    assert!(state["nodes"]["a"]["reason"].as_str().unwrap().contains("outside.txt"));
    assert!(!Path::new(state["path"].as_str().unwrap()).join("outside.txt").exists());
    assert_eq!(f.post(&format!("{path}/scope"), json!({"node":"a","paths":["../outside"]})).await.0, 422);
    assert_eq!(f.post(&format!("{path}/scope"), json!({"node":"a","paths":["outside.txt"]})).await.0, 200);
    assert_eq!(f.post(&format!("{path}/control"), json!({"action":"resume"})).await.0, 200);
    let state = f.wait(&id, |s| !s["completed"].is_null()).await;
    assert!(state["paused"].is_null());
}

#[tokio::test]
async fn custom_acceptance_is_healed_and_repeated_failures_stop() {
    for healable in [true, false] {
        let f = Fixture::new("normal").await;
        let id = f.create().await;
        let path = format!("/v1/threads/{id}/plan");
        let mut p = plan();
        p["nodes"] = json!([p["nodes"][0].clone()]);
        let cmd =
            if healable { if cfg!(windows) { "findstr fixed a.txt" } else { "grep fixed a.txt" } } else { "exit 3" };
        p["nodes"][0]["checks"] = json!([cmd]);
        assert_eq!(f.call(reqwest::Method::PATCH, &path, p).await.0, 200);
        assert_eq!(f.post(&format!("{path}/approve"), json!({"revision":1})).await.0, 200);
        let state = f.wait(&id, |s| !s["completed"].is_null() || !s["paused"].is_null()).await;
        assert_eq!(state["nodes"]["a"]["heal_signatures"].as_array().unwrap().len(), 1, "{state}");
        assert_eq!(!state["completed"].is_null(), healable, "{state}");
        assert_eq!(!state["paused"].is_null(), !healable, "{state}");
    }
}
#[tokio::test]
async fn pause_interrupts_long_checks_and_budget_changes_require_pause() {
    let f = Fixture::new("normal").await;
    let id = f.create().await;
    let path = format!("/v1/threads/{id}/plan");
    let mut p = plan();
    p["nodes"] = json!([p["nodes"][0].clone()]);
    p["nodes"][0]["checks"] = json!([if cfg!(windows) { "ping -n 30 127.0.0.1 > nul" } else { "sleep 30" }]);
    f.call(reqwest::Method::PATCH, &path, p).await;
    f.post(&format!("{path}/approve"), json!({"revision":1})).await;
    f.wait(&id, |s| s["nodes"]["a"]["status"] == "checking").await;
    let start = std::time::Instant::now();
    assert_eq!(f.post(&format!("{path}/control"), json!({"action":"pause"})).await.0, 200);
    assert!(start.elapsed() < Duration::from_secs(2), "pause waited for a running check");
    assert_eq!(f.post(&format!("{path}/budget"), json!({"usd":2.0})).await.0, 200);
    assert_eq!(f.post(&format!("{path}/budget"), json!({"node":"a","usd":1.0,"tokens":4000})).await.0, 200);
    let s = f.get(&path).await;
    assert_eq!(s["plan"]["budget_usd"], 2.0);
    assert_eq!(s["plan"]["nodes"][0]["token_budget"], 4000);
    assert!(s["completed"].is_null());
}
#[tokio::test]
async fn conflicting_changes_use_one_scoped_repair_and_preserve_both_steps() {
    let f = Fixture::new("conflict").await;
    let id = f.create().await;
    let path = format!("/v1/threads/{id}/plan");
    let mut p = plan();
    p["nodes"] = json!([p["nodes"][0].clone(), p["nodes"][1].clone()]);
    for n in p["nodes"].as_array_mut().unwrap() {
        n["scope"].as_array_mut().unwrap().push(json!("README.md"));
    }
    f.call(reqwest::Method::PATCH, &path, p).await;
    f.post(&format!("{path}/approve"), json!({"revision":1})).await;
    let s = f.wait(&id, |s| !s["completed"].is_null() || !s["paused"].is_null()).await;
    assert!(s["paused"].is_null(), "{s}");
    assert_eq!(s["nodes"].as_object().unwrap().values().filter(|n| !n["repair"].is_null()).count(), 1);
    let content = std::fs::read_to_string(Path::new(s["path"].as_str().unwrap()).join("README.md")).unwrap();
    assert!(content.contains("verified a") && content.contains("verified b"));
    assert_eq!(std::fs::read_to_string(f.repo.path().join("README.md")).unwrap(), "fixture");
}
