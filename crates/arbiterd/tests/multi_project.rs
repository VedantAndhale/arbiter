//! A plan over two projects: a data pipeline and the app that uses it. Each
//! step works in its own project's worktree, the app step waits for the
//! pipeline step and can read its finished code, and each project collects
//! and lands its own changes. Real Git, fake agents.
use arbiter_adapters::{Command, HarnessEvent, RunHandle, TurnOutcome};
use arbiter_core::AgentEvent;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

fn git(path: &Path, args: &[&str]) {
    assert!(std::process::Command::new("git").arg("-C").arg(path).args(args).output().unwrap().status.success());
}
fn repo(file: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.name", "fixture"],
        &["config", "user.email", "fixture@localhost"],
    ] {
        git(dir.path(), args);
    }
    std::fs::write(dir.path().join(file), "start\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "initial"]);
    dir
}

#[derive(Default)]
struct Seen {
    /// (step id, cwd, read dirs, brief)
    runs: Vec<(String, PathBuf, Vec<PathBuf>, String)>,
}

#[tokio::test]
async fn steps_work_in_their_own_projects_and_hand_over() {
    let home = tempfile::tempdir().unwrap();
    let app = repo("app.py");
    let pipeline = repo("pipeline.py");
    let seen = Arc::new(Mutex::new(Seen::default()));
    let mut config = arbiterd::Config::new(home.path().into(), 0);
    config.heal = false;
    config.available = Arc::new(|_| true);
    config.fixture_accounts = Some(arbiterd::fixture_accounts());
    let log = seen.clone();
    config.launcher = Arc::new(move |_, opts| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (events, ev) = tokio::sync::mpsc::unbounded_channel();
        let log = log.clone();
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::Send(text) => {
                        let id = text
                            .lines()
                            .find_map(|s| s.strip_prefix("Step ").and_then(|s| s.split(':').next()))
                            .unwrap_or("?")
                            .to_owned();
                        log.lock().unwrap().runs.push((id.clone(), opts.cwd.clone(), opts.read_dirs.clone(), text));
                        let (file, interface) = if id == "schema" {
                            ("schema.json", "orders table: id, total_cents")
                        } else {
                            ("dashboard.py", "")
                        };
                        std::fs::write(opts.cwd.join(file), format!("{id}\n")).unwrap();
                        let handoff = json!({"summary":format!("Implemented {id}"),"files":[file],"decisions":[],"interfaces": if interface.is_empty() { vec![] } else { vec![interface] },"open_issues":[]});
                        let _ = events.send(HarnessEvent::Agent(AgentEvent::Message {
                            text: json!({ "handoff": handoff }).to_string(),
                        }));
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
    let base = format!("http://{}", daemon.addr);
    let http = reqwest::Client::new();
    let call = async |method: reqwest::Method, path: &str, body: Value| -> (u16, Value) {
        let r =
            http.request(method, format!("{base}{path}")).bearer_auth(&daemon.token).json(&body).send().await.unwrap();
        let code = r.status().as_u16();
        (code, r.json().await.unwrap_or(Value::Null))
    };
    let (_, a) = call(reqwest::Method::POST, "/v1/projects", json!({"path":app.path()})).await;
    let (_, p) = call(reqwest::Method::POST, "/v1/projects", json!({"path":pipeline.path()})).await;
    let (a, p) = (a["id"].as_str().unwrap().to_owned(), p["id"].as_str().unwrap().to_owned());

    let (code, t) = call(
        reqwest::Method::POST,
        "/v1/threads",
        json!({"project_id":a,"projects":[p],"title":"Orders dashboard","worktree":false}),
    )
    .await;
    assert_eq!(code, 200, "{t}");
    let id = t["id"].as_str().unwrap().to_owned();
    let path = format!("/v1/threads/{id}/plan");
    let plan = json!({"title":"Orders dashboard","goal":"Export orders from the pipeline and chart them in the app","projects":[p],"nodes":[
        {"id":"schema","title":"Export orders","goal":"Write schema.json","scope":["schema.json"],"project":p},
        {"id":"dashboard","title":"Chart orders","goal":"Write dashboard.py from the export","scope":["dashboard.py"],"dependencies":["schema"]}
    ]});
    // Only projects attached to the task may be used.
    let mut stray = plan.clone();
    stray["projects"] = json!(["00000000-0000-0000-0000-000000000000"]);
    stray["nodes"][0]["project"] = json!("00000000-0000-0000-0000-000000000000");
    assert_eq!(call(reqwest::Method::PATCH, &path, stray).await.0, 422);
    let (code, state) = call(reqwest::Method::PATCH, &path, plan).await;
    assert_eq!(code, 200, "{state}");
    assert_eq!(call(reqwest::Method::POST, &format!("{path}/approve"), json!({"revision":1})).await.0, 200);

    let mut state = Value::Null;
    for _ in 0..300 {
        state = call(reqwest::Method::GET, &path, json!({})).await.1;
        if !state["completed"].is_null() || !state["paused"].is_null() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(state["paused"].is_null() && !state["completed"].is_null(), "{state}");

    // Each project collected its own step, and nothing touched the originals.
    let app_copy = PathBuf::from(state["path"].as_str().unwrap());
    let pipe_copy = PathBuf::from(state["integrations"][&p]["path"].as_str().unwrap());
    assert!(app_copy.join("dashboard.py").exists() && !app_copy.join("schema.json").exists());
    assert!(pipe_copy.join("schema.json").exists() && !pipe_copy.join("dashboard.py").exists());
    assert!(!app.path().join("dashboard.py").exists() && !pipe_copy.join("app.py").exists());
    assert!(!pipeline.path().join("schema.json").exists());

    let runs = std::mem::take(&mut seen.lock().unwrap().runs);
    let schema = runs.iter().find(|r| r.0 == "schema").expect("schema ran");
    let dash = runs.iter().find(|r| r.0 == "dashboard").expect("dashboard ran");
    assert!(schema.1.join("pipeline.py").exists(), "the pipeline step works in the pipeline repo");
    assert!(dash.1.join("app.py").exists(), "the app step works in the app repo");
    // The app step started after the export was collected, can read it, and
    // got its interface in the brief.
    // Besides Arbiter's attachment store, it may read the pipeline's copy.
    let extra = |dirs: &Vec<PathBuf>| dirs.iter().filter(|d| !d.ends_with("attachments")).cloned().collect::<Vec<_>>();
    assert_eq!(extra(&dash.2), vec![pipe_copy.clone()]);
    assert!(dash.3.contains("orders table: id, total_cents"), "{}", dash.3);
    assert!(dash.3.contains(&pipe_copy.display().to_string()), "{}", dash.3);
    assert!(extra(&schema.2).is_empty());

    // Each project lands separately.
    let (code, landing) = call(reqwest::Method::GET, &format!("/v1/threads/{id}/landing?project={p}"), json!({})).await;
    assert_eq!(code, 200, "{landing}");
    assert!(landing["draft"]["body"].as_str().unwrap().contains("Implemented schema"));
    assert!(!landing["draft"]["body"].as_str().unwrap().contains("Implemented dashboard"));
    let (_, main) = call(reqwest::Method::GET, &format!("/v1/threads/{id}/landing"), json!({})).await;
    assert!(main["draft"]["body"].as_str().unwrap().contains("Implemented dashboard"));
    let diff = http
        .get(format!("{base}/v1/threads/{id}/plan/diff?project={p}"))
        .bearer_auth(&daemon.token)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(diff.contains("schema.json") && !diff.contains("dashboard.py"), "{diff}");
    daemon.handle.abort();
}
