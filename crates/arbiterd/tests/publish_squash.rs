//! Publishing squashes a task into one commit authored by the user, with a
//! message from the local model, on a normal branch. arbiter/ branches never
//! reach the remote. A second publish adds one follow-up commit, the choice
//! is remembered, and once the published branch is gone the local copies
//! are cleaned up. The "remote" is a local bare repository.
use arbiter_adapters::{Command, HarnessEvent, RunHandle, TurnOutcome};
use arbiter_core::AgentEvent;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{Arc, Mutex};

fn git(path: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").arg("-C").arg(path).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[tokio::test]
async fn one_commit_on_a_normal_branch_then_clean_up() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["config", "user.name", "Dev Person"]);
    git(repo.path(), &["config", "user.email", "dev@example.com"]);
    std::fs::write(repo.path().join("orders.py"), "ORDERS = []\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["-c", "commit.gpgSign=false", "commit", "-qm", "initial"]);
    git(repo.path(), &["remote", "add", "origin", &remote.path().to_string_lossy()]);
    git(repo.path(), &["push", "-q", "origin", "main"]);
    git(repo.path(), &["fetch", "-q", "origin"]);
    git(repo.path(), &["remote", "set-head", "origin", "main"]);

    let turns = Arc::new(Mutex::new(0usize));
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.heal = false;
    cfg.available = Arc::new(|_| true);
    let count = turns.clone();
    cfg.launcher = Arc::new(move |_, opts| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (events, ev) = tokio::sync::mpsc::unbounded_channel();
        let count = count.clone();
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::Send(_) => {
                        let n = {
                            let mut c = count.lock().unwrap();
                            *c += 1;
                            *c
                        };
                        if n == 1 {
                            // An agent writing Arbiter-looking data into the project.
                            std::fs::create_dir_all(opts.cwd.join(".arbiter")).unwrap();
                            std::fs::write(
                                opts.cwd.join(".arbiter").join("scratch.md"),
                                "agent notes
",
                            )
                            .unwrap();
                        }
                        let file = opts.cwd.join("export.py");
                        let mut text = std::fs::read_to_string(&file).unwrap_or_default();
                        text.push_str(&format!("def step_{n}():\n    return {n}\n"));
                        std::fs::write(&file, text).unwrap();
                        let _ =
                            events.send(HarnessEvent::Agent(AgentEvent::Message { text: format!("Added step {n}.") }));
                        let _ = events.send(HarnessEvent::TurnDone(TurnOutcome::Completed));
                    }
                    Command::Shutdown => break,
                    _ => {}
                }
            }
        });
        Ok(RunHandle::from_channels(tx, ev))
    });
    cfg.local_generator = Some(Arc::new(|_, _, _, schema| {
        Box::pin(async move {
            assert!(schema["properties"].get("subject").is_some());
            Ok(json!({"type":"feat","scope":"orders","subject":"add csv export","body":"Orders can be exported as CSV.\nCo-Authored-By: Some Bot <bot@example.com>"}).to_string())
        })
    }));
    let daemon = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    let post = |p: &str| client.post(format!("{base}{p}")).bearer_auth(&daemon.token);
    let get = |p: &str| client.get(format!("{base}{p}")).bearer_auth(&daemon.token);
    let p: Value = get("/v1/setup").send().await.unwrap().json().await.unwrap();
    let mut p = p["preferences"].clone();
    p["documentation_model"] = json!("fixture");
    post("/v1/setup").json(&p).send().await.unwrap().error_for_status().unwrap();
    let project: Value =
        post("/v1/projects").json(&json!({"path":repo.path()})).send().await.unwrap().json().await.unwrap();
    let t: Value = post("/v1/threads")
        .json(&json!({"project_id":project["id"],"harness":"claude","worktree":true,"title":"CSV export","message":"Add CSV export"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = t["id"].as_str().unwrap().to_owned();
    let worktree = std::path::PathBuf::from(t["worktree"].as_str().unwrap());
    let arbiter_branch = t["branch"].as_str().unwrap().to_owned();
    let wait_turns = async |n: usize| {
        for _ in 0..200 {
            let t: Value = get(&format!("/v1/threads/{id}")).send().await.unwrap().json().await.unwrap();
            if *turns.lock().unwrap() >= n && t["status"] != "running" {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("turn {n} did not finish");
    };
    let commit_all = async |message: &str| {
        let l: Value = get(&format!("/v1/threads/{id}/landing")).send().await.unwrap().json().await.unwrap();
        let include: Vec<Value> = l["untracked"].as_array().cloned().unwrap_or_default();
        let r = post(&format!("/v1/threads/{id}/commit"))
            .json(&json!({"fingerprint":l["fingerprint"],"message":message,"include":include}))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success(), "{}", r.text().await.unwrap());
    };
    wait_turns(1).await;
    commit_all("wip 1").await;
    post(&format!("/v1/threads/{id}/messages")).json(&json!({"text":"Add another step"})).send().await.unwrap();
    wait_turns(2).await;
    commit_all("wip 2").await;

    // The draft: detected standard settings and a squashed message from the
    // local model, without any co-author line.
    let draft: Value = get(&format!("/v1/threads/{id}/publish")).send().await.unwrap().json().await.unwrap();
    assert_eq!(draft["mode"], "push", "{draft}");
    assert_eq!(draft["base"], "main");
    assert_eq!(draft["branch"], "feature/add-csv-export");
    assert_eq!(draft["message"], "feat(orders): add csv export\n\nOrders can be exported as CSV.");
    assert_eq!(draft["written_by_local_model"], true);
    let publish = |draft: &Value, mode: &str, branch: &str| {
        post(&format!("/v1/threads/{id}/publish")).json(&json!({
            "fingerprint":draft["fingerprint"],"mode":mode,"base":"main","branch":branch,
            "message":draft["message"],"title":draft["title"],"body":draft["body"],"approved":true}))
    };
    let r = publish(&draft, "push", "arbiter/sneaky").send().await.unwrap();
    assert!(r.text().await.unwrap().contains("stay on this computer"));
    let r: Value = publish(&draft, "push", "feature/add-csv-export").send().await.unwrap().json().await.unwrap();
    assert_eq!(r["left_out"], 1, "new .arbiter files stay out of the published commit: {r}");

    // One commit on top of main, authored by the user; nothing arbiter/ pushed.
    let log = git(remote.path(), &["log", "--format=%an|%s|%b", "main..feature/add-csv-export"]);
    assert_eq!(log.lines().filter(|l| l.contains('|')).count(), 1, "{log}");
    assert!(log.starts_with("Dev Person|feat(orders): add csv export|Orders can be exported as CSV."), "{log}");
    assert!(!log.to_lowercase().contains("co-authored"));
    let heads = git(remote.path(), &["for-each-ref", "--format=%(refname)"]);
    assert!(!heads.contains("arbiter"), "{heads}");
    let files = git(remote.path(), &["show", "--name-only", "--format=", "feature/add-csv-export"]);
    assert_eq!(files, "export.py");
    // The local arbiter branch has no upstream.
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&worktree)
            .args(["rev-parse", "--abbrev-ref", "@{u}"])
            .output()
            .unwrap()
            .status
            .code()
            != Some(0)
    );

    // Nothing new yet: refused. Then a follow-up adds exactly one commit.
    let again: Value = get(&format!("/v1/threads/{id}/publish")).send().await.unwrap().json().await.unwrap();
    assert_eq!(again["branch"], "feature/add-csv-export");
    let r = publish(&again, "push", "feature/add-csv-export").send().await.unwrap();
    assert!(r.text().await.unwrap().contains("Nothing new"));
    post(&format!("/v1/threads/{id}/messages")).json(&json!({"text":"One more"})).send().await.unwrap();
    wait_turns(3).await;
    commit_all("wip 3").await;
    let again: Value = get(&format!("/v1/threads/{id}/publish")).send().await.unwrap().json().await.unwrap();
    let r = publish(&again, "push", "feature/add-csv-export").send().await.unwrap();
    assert!(r.status().is_success(), "{}", r.text().await.unwrap());
    assert_eq!(git(remote.path(), &["rev-list", "--count", "main..feature/add-csv-export"]), "2");
    let last = git(remote.path(), &["show", "--format=", "feature/add-csv-export"]);
    assert!(last.contains("step_3") && !last.contains("+def step_1"), "{last}");
    // The choice is remembered for the project.
    let prefs =
        std::fs::read_to_string(home.path().join("publish").join(format!("{}.json", project["id"].as_str().unwrap())))
            .unwrap();
    assert!(prefs.contains("\"push\"") && prefs.contains("feature/"), "{prefs}");

    // Still published and open: nothing is cleaned up.
    let r: Value = post("/v1/cleanup").send().await.unwrap().json().await.unwrap();
    assert_eq!(r["cleaned"], json!([]));
    assert!(worktree.exists());
    // Merged and deleted on the remote: the local copy and branch go away.
    git(remote.path(), &["branch", "-D", "feature/add-csv-export"]);
    let r: Value = post("/v1/cleanup").send().await.unwrap().json().await.unwrap();
    assert_eq!(r["cleaned"], json!([id]), "{r}");
    assert!(!worktree.exists());
    assert!(git(repo.path(), &["branch", "--list", &arbiter_branch]).is_empty());
    let t: Value = get(&format!("/v1/threads/{id}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(t["status"], "merged");
    // The user's checkout was never touched.
    assert_eq!(git(repo.path(), &["status", "--porcelain"]), "");
    assert!(!repo.path().join("export.py").exists());
    daemon.handle.abort();
}
