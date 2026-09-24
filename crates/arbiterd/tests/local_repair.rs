//! The local agent explores, edits, runs the project's checks, repairs its own
//! mistake and finishes — with cloud launches forbidden. A model stuck on one
//! failing action is stopped early. Fake model only.
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn evidence(prompt: &str) -> Value {
    serde_json::from_str(prompt.split("Latest tool result:\n").last().unwrap_or("")).unwrap_or(Value::Null)
}

#[tokio::test]
async fn local_agent_checks_repairs_and_stops_repeating() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let git = arbiter_supervisor::integration::git;
    git(repo.path(), &["init", "-q", "-b", "main"]).await.unwrap();
    std::fs::write(repo.path().join("calc.py"), "def add(a,b): return a-b\n").unwrap();
    std::fs::create_dir(repo.path().join(".arbiter")).unwrap();
    std::fs::write(
        repo.path().join(".arbiter/healing.toml"),
        "[[check]]\nname=\"adds\"\nkind=\"test\"\ncmd=\"git grep -qF a+b -- calc.py\"\ntimeout_secs=20\n",
    )
    .unwrap();
    git(repo.path(), &["add", "."]).await.unwrap();
    git(repo.path(), &["-c", "commit.gpgSign=false", "commit", "-qm", "initial"]).await.unwrap();

    let step = Arc::new(AtomicUsize::new(0));
    let prompts = Arc::new(Mutex::new(Vec::<String>::new()));
    let (counter, seen) = (step.clone(), prompts.clone());
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.heal = false;
    cfg.launcher = Arc::new(|_, _| panic!("no cloud launches allowed"));
    cfg.local_generator = Some(Arc::new(move |_, prompt, _, _| {
        let n = counter.fetch_add(1, Ordering::SeqCst);
        seen.lock().unwrap().push(prompt.clone());
        let last = evidence(&prompt);
        let hash = last["hash"].as_str().unwrap_or("").to_owned();
        let stuck = prompt.contains("Request:\nStuck task");
        Box::pin(async move {
            let a = |action: &str, path: &str| json!({"action":action,"path":path,"before_hash":"","content":"","summary":""});
            let mut v = if stuck {
                a("read", "../outside.txt")
            } else {
                match n {
                    0 => a("list", ""),
                    1 => json!({"action":"search","path":"","before_hash":"","content":"def add","summary":""}),
                    2 | 5 => a("read", "calc.py"),
                    3 => {
                        json!({"action":"edit","path":"calc.py","before_hash":hash,"find":"a-b","content":"a*b","summary":""})
                    }
                    6 => {
                        json!({"action":"edit","path":"calc.py","before_hash":hash,"find":"a*b","content":"a+b","summary":""})
                    }
                    4 | 7 => a("check", ""),
                    _ => {
                        json!({"action":"done","path":"","before_hash":"","content":"","summary":"Fixed add; the adds check passes."})
                    }
                }
            };
            if v["find"].is_null() {
                v["find"] = json!("");
            }
            Ok(v.to_string())
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
    post("/v1/setup").json(&p).send().await.unwrap().error_for_status().unwrap();
    let project: Value =
        post("/v1/projects").json(&json!({"path":repo.path()})).send().await.unwrap().json().await.unwrap();
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

    let t: Value = post("/v1/threads")
        .json(&json!({"project_id":project["id"],"harness":"local","model":"fixture","worktree":true,"message":"Fix addition in calc.py"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let t = wait(t["id"].as_str().unwrap(), "review").await;
    let worktree = std::path::Path::new(t["worktree"].as_str().unwrap());
    // Git may check the worktree out with CRLF; edits keep the file's endings.
    assert_eq!(std::fs::read_to_string(worktree.join("calc.py")).unwrap().trim_end(), "def add(a,b): return a+b");
    let prompts = prompts.lock().unwrap().clone();
    assert_eq!(prompts.len(), 9, "list, search, read, edit, check, read, edit, check, done");
    assert!(evidence(&prompts[1])["files"].as_array().unwrap().iter().any(|f| f == "calc.py"));
    assert_eq!(evidence(&prompts[2])["matches"][0], "calc.py:1: def add(a,b): return a-b");
    // The failed check reaches the model as a short failure, not a log.
    let failed = evidence(&prompts[5]);
    assert_eq!(failed["passed"], false, "{failed}");
    assert_eq!(evidence(&prompts[8])["passed"], true, "{}", evidence(&prompts[8]));
    // The model sees what it already did.
    assert!(prompts[8].contains("4: check  -> ok") && prompts[8].contains("6: edit calc.py -> ok"), "{}", prompts[8]);
    assert!(prompts.iter().all(|p| p.len() < 12_000));

    // A model that keeps repeating one failing action is stopped at three.
    let before = step.load(Ordering::SeqCst);
    let stuck: Value = post("/v1/threads")
        .json(&json!({"project_id":project["id"],"harness":"local","model":"fixture","worktree":true,"message":"Stuck task"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = stuck["id"].as_str().unwrap();
    wait(id, "failed").await;
    assert_eq!(step.load(Ordering::SeqCst) - before, 3);
    let events: Value = get(&format!("/v1/threads/{id}/events")).send().await.unwrap().json().await.unwrap();
    assert!(events.to_string().contains("repeated the same read action three times"));
    daemon.handle.abort();
}
