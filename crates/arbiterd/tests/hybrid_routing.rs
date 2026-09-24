//! F3 hybrid delivery with fakes only: the local coding check decides whether a
//! local model is trusted; auto threads go local for small work or in
//! local-only mode, and to the cloud otherwise; a user escalation hands the
//! local work to a frontier agent as a compact brief.
use arbiter_adapters::{Command, HarnessEvent, RunHandle, TurnOutcome};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

fn evidence(prompt: &str) -> Value {
    serde_json::from_str(prompt.split("Latest tool result:\n").last().unwrap_or("")).unwrap_or(Value::Null)
}

/// A scripted "competent" model: reads, edits and finishes each known task.
fn competent(prompt: &str) -> Value {
    let last = evidence(prompt);
    let hash = last["hash"].as_str().unwrap_or("");
    let act = |action: &str, path: &str, find: &str, content: &str| json!({"action":action,"path":path,"before_hash":hash,"find":find,"content":content,"summary":"Done; see diff."});
    if last.get("changed").is_some() {
        return act("done", "", "", "");
    }
    if prompt.contains("shout helper") {
        return match last["path"].as_str() {
            Some(p) if p.ends_with("shout.py") => act("edit", p, ".lower()", ".upper()"),
            _ if last.get("matches").is_some() => act("read", "src/text/shout.py", "", ""),
            _ => act("search", "", "", "def shout"),
        };
    }
    if prompt.contains("greet.py") {
        return match last["path"].as_str() {
            Some(p) => act("write", p, "", "def greet(name):\n    return \"Hello, \" + name\n"),
            None => act("read", "src/text/greet.py", "", ""),
        };
    }
    match last["path"].as_str() {
        Some(p) => act("edit", p, "a - b", "a + b"),
        None => act("read", "calc.py", "", ""),
    }
}

#[tokio::test]
async fn capability_check_routes_locally_and_escalation_hands_off() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let git = arbiter_supervisor::integration::git;
    git(repo.path(), &["init", "-q", "-b", "main"]).await.unwrap();
    std::fs::write(repo.path().join("calc.py"), "def add(a, b):\n    return a - b\n").unwrap();
    git(repo.path(), &["add", "."]).await.unwrap();
    git(repo.path(), &["-c", "commit.gpgSign=false", "commit", "-qm", "initial"]).await.unwrap();

    let sent = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.heal = false;
    cfg.available = Arc::new(|_| true);
    let messages = sent.clone();
    cfg.launcher = Arc::new(move |_, _| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (events, ev) = tokio::sync::mpsc::unbounded_channel();
        let messages = messages.clone();
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::Send(s) => {
                        messages.lock().unwrap().push(s);
                        let _ = events.send(HarnessEvent::TurnDone(TurnOutcome::Completed));
                    }
                    Command::Shutdown => break,
                    _ => {}
                }
            }
        });
        Ok(RunHandle::from_channels(tx, ev))
    });
    cfg.local_generator = Some(Arc::new(move |model, prompt, _, _| {
        Box::pin(async move {
            // The "weak" model claims success without doing anything.
            let v = if model == "weak" {
                json!({"action":"done","path":"","before_hash":"","find":"","content":"","summary":"All fixed."})
            } else {
                competent(&prompt)
            };
            Ok(v.to_string())
        })
    }));
    let daemon = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    let post = |p: &str| client.post(format!("{base}{p}")).bearer_auth(&daemon.token);
    let get = |p: &str| client.get(format!("{base}{p}")).bearer_auth(&daemon.token);
    let set_mode = async |mode: &str| {
        let p: Value = get("/v1/setup").send().await.unwrap().json().await.unwrap();
        let mut p = p["preferences"].clone();
        p["mode"] = json!(mode);
        post("/v1/setup").json(&p).send().await.unwrap().error_for_status().unwrap();
    };
    let events = async |id: &str| -> String {
        get(&format!("/v1/threads/{id}/events")).send().await.unwrap().text().await.unwrap()
    };
    let wait = async |id: &str, want: &str| -> Value {
        for _ in 0..200 {
            let t: Value = get(&format!("/v1/threads/{id}")).send().await.unwrap().json().await.unwrap();
            if t["status"] == want {
                return t;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("thread {id} never reached {want}");
    };
    let project: Value =
        post("/v1/projects").json(&json!({"path":repo.path()})).send().await.unwrap().json().await.unwrap();
    let start = async |message: &str| -> String {
        let r = post("/v1/threads")
            .json(&json!({"project_id":project["id"],"harness":"auto","worktree":true,"message":message}))
            .send()
            .await
            .unwrap();
        let t: Value = r.json().await.unwrap();
        t["id"].as_str().map(str::to_owned).unwrap_or_else(|| panic!("{t}"))
    };

    // Local-only mode with no checked model blocks instead of using the cloud.
    set_mode("local").await;
    let blocked = start("Fix add in calc.py so it returns the sum").await;
    wait(&blocked, "failed").await;
    assert!(events(&blocked).await.contains("passed the coding check"));

    // The coding check separates a model that works from one that only claims to.
    let weak: Value =
        post("/v1/local/capability").json(&json!({"model":"weak"})).send().await.unwrap().json().await.unwrap();
    assert_eq!(weak["passed"], false, "{weak}");
    assert_eq!(weak["score"], 0);
    let good: Value =
        post("/v1/local/capability").json(&json!({"model":"fixture"})).send().await.unwrap().json().await.unwrap();
    assert_eq!(good["passed"], true, "{good}");
    assert_eq!(good["score"], 3);
    let report: Value = get("/v1/local/capability").send().await.unwrap().json().await.unwrap();
    assert_eq!(report["results"].as_array().unwrap().len(), 2);
    assert!(!home.path().join("probes").read_dir().is_ok_and(|mut d| d.next().is_some()), "probe folders removed");

    // Local-only mode now routes to the checked model and finishes locally.
    let local = start("Fix add in calc.py so it returns the sum").await;
    let t = wait(&local, "review").await;
    assert_eq!(t["harness"], "local");
    assert_eq!(t["model"], "fixture");
    assert!(events(&local).await.contains("Routed to the local agent: Local-only mode"));
    let worktree = std::path::Path::new(t["worktree"].as_str().unwrap());
    assert!(std::fs::read_to_string(worktree.join("calc.py")).unwrap().contains("a + b"));
    assert!(sent.lock().unwrap().is_empty(), "no cloud launch so far");

    // With cloud allowed, large or risky work goes to the cloud.
    set_mode("subscription").await;
    // Even with the cloud available, a small, clear fix stays local.
    let small = start("Fix add in calc.py so it returns the sum").await;
    wait(&small, "review").await;
    assert!(events(&small).await.contains("Routed to the local agent: Small, low-risk"), "{}", events(&small).await);
    assert!(sent.lock().unwrap().is_empty());
    let big = start("Redesign the authentication flow and migrate the user database schema across every service").await;
    let t = wait(&big, "review").await;
    assert_ne!(t["harness"], "local", "{t}");
    assert_eq!(sent.lock().unwrap().len(), 1);

    // The user escalates the local task; the frontier agent gets a compact brief.
    let r: Value = post(&format!("/v1/threads/{local}/escalate"))
        .json(&json!({"harness":"claude"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["ok"], true, "{r}");
    let t = wait(&local, "review").await;
    assert_eq!(t["harness"], "claude");
    let brief = sent.lock().unwrap().last().cloned().unwrap();
    assert!(brief.contains("Original request:\nFix add in calc.py"), "{brief}");
    assert!(brief.contains("calc.py |"), "diff stat of the local change: {brief}");
    assert!(brief.len() < 6000, "compact handoff, not a transcript");
    assert!(!brief.contains("Latest tool result"), "no local transcript");
    // Escalating again is refused: the thread is no longer local.
    let again = post(&format!("/v1/threads/{local}/escalate")).json(&json!({"harness":"codex"})).send().await.unwrap();
    assert!(!again.status().is_success());
    daemon.handle.abort();
}
