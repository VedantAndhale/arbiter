//! Memory lives in its own git repository, never in a project. Secrets are
//! redacted from the backup, your guidance reaches an agent once per task,
//! pushing works, an unclean stop pushes on the next start while a normal
//! close does not, and a backup restores into a new home. The remote is a
//! local bare repository.
use arbiter_adapters::{Command, HarnessEvent, RunHandle, TurnOutcome};
use arbiter_core::AgentEvent;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn git(path: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").arg("-C").arg(path).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn config(home: &Path, sent: Arc<Mutex<Vec<String>>>) -> arbiterd::Config {
    let mut cfg = arbiterd::Config::new(home.into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.heal = false;
    cfg.available = Arc::new(|_| true);
    cfg.launcher = Arc::new(move |_, _| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (events, ev) = tokio::sync::mpsc::unbounded_channel();
        let sent = sent.clone();
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::Send(text) => {
                        sent.lock().unwrap().push(text);
                        let _ = events.send(HarnessEvent::Agent(AgentEvent::Message { text: "Done.".into() }));
                        let _ = events.send(HarnessEvent::TurnDone(TurnOutcome::Completed));
                    }
                    Command::Shutdown => break,
                    _ => {}
                }
            }
        });
        Ok(RunHandle::from_channels(tx, ev))
    });
    cfg
}

struct Api {
    base: String,
    token: String,
    http: reqwest::Client,
}
impl Api {
    async fn get(&self, p: &str) -> Value {
        self.http.get(format!("{}{p}", self.base)).bearer_auth(&self.token).send().await.unwrap().json().await.unwrap()
    }
    async fn post(&self, p: &str, body: Value) -> (u16, Value) {
        let r = self.http.post(format!("{}{p}", self.base)).bearer_auth(&self.token).json(&body).send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
}

async fn wait<F: Fn() -> bool>(what: &str, f: F) {
    for _ in 0..200 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test]
async fn memory_is_separate_pushed_recovered_and_restorable() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
    git(repo.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(repo.path().join("app.py"), "print('hi')\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(
        repo.path(),
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-qm",
            "initial",
        ],
    );

    let sent = Arc::new(Mutex::new(Vec::<String>::new()));
    let daemon = arbiterd::start(config(home.path(), sent.clone())).await.unwrap();
    let api =
        Api { base: format!("http://{}", daemon.addr), token: daemon.token.clone(), http: reqwest::Client::new() };

    // The memory repository exists without any setup.
    let memory = home.path().join("memory");
    let status = api.get("/v1/memory").await;
    assert_eq!(status["path"].as_str().map(Path::new), Some(memory.as_path()), "{status}");
    assert!(memory.join(".git").exists() && memory.join("README.md").exists());
    assert!(status["remote"].is_null());
    std::fs::write(
        memory.join("guidance").join("about-me.md"),
        "# About me\n\nPrefer small functions and plain English errors.\n",
    )
    .unwrap();

    let (_, project) = api.post("/v1/projects", json!({"path":repo.path()})).await;
    let (_, t) = api
        .post("/v1/threads", json!({"project_id":project["id"],"harness":"claude","worktree":true,"title":"Greeting","message":"Use key sk-abcdefghijklmnopqrstuvw to test"}))
        .await;
    let id = t["id"].as_str().unwrap().to_owned();
    wait("first turn", || sent.lock().unwrap().len() == 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    api.post(&format!("/v1/threads/{id}/messages"), json!({"text":"Now say hello"})).await;
    wait("second turn", || sent.lock().unwrap().len() == 2).await;
    {
        let sent = sent.lock().unwrap();
        assert!(sent[0].contains("Prefer small functions"), "guidance reaches the agent: {}", sent[0]);
        assert!(!sent[1].contains("Prefer small functions"), "only once per task: {}", sent[1]);
    }

    // Connect a remote: everything is pushed; secrets are redacted.
    let (code, status) = api.post("/v1/memory/remote", json!({"url": remote.path()})).await;
    assert_eq!(code, 200, "{status}");
    assert_eq!(status["pushed"], true);
    assert_eq!(status["unpushed"], 0);
    let files = git(remote.path(), &["ls-tree", "-r", "--name-only", "main"]);
    assert!(files.contains("guidance/about-me.md") && files.contains("backup/tables.json"), "{files}");
    let month = files.lines().find(|f| f.starts_with("backup/events/")).expect("event backup").to_owned();
    let log = git(remote.path(), &["show", &format!("main:{month}")]);
    assert!(log.contains("Greeting") && log.contains("[redacted]") && !log.contains("sk-abcdef"), "{log}");
    // Nothing personal went into the project repository.
    assert!(!repo.path().join(".arbiter").exists());
    assert_eq!(git(repo.path(), &["status", "--porcelain"]), "");

    // Leftovers from older versions: notes and attachment copies (untracked),
    // and a committed .arbiter/PROJECT.md, which is only reported.
    std::fs::create_dir_all(repo.path().join(".arbiter/vault/decisions")).unwrap();
    std::fs::write(
        repo.path().join(".arbiter/vault/decisions/old-choice.md"),
        "# Old choice

Use SQLite.
",
    )
    .unwrap();
    std::fs::create_dir_all(repo.path().join(".arbiter/attachments")).unwrap();
    std::fs::write(repo.path().join(".arbiter/attachments/shot.png"), "png").unwrap();
    std::fs::write(
        repo.path().join(".arbiter/PROJECT.md"),
        "# Project setup
",
    )
    .unwrap();
    git(repo.path(), &["add", ".arbiter/PROJECT.md"]);
    git(
        repo.path(),
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-qm",
            "old setup",
        ],
    );

    // Unclean stop: new work is committed locally but not pushed...
    api.post(&format!("/v1/threads/{id}/messages"), json!({"text":"And goodbye"})).await;
    wait("third turn", || sent.lock().unwrap().len() == 3).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (_, status) = api.post("/v1/memory/sync", json!({"push":false})).await;
    assert!(status["unpushed"].as_u64().unwrap() > 0, "{status}");
    let before = git(remote.path(), &["rev-list", "--count", "main"]);
    daemon.handle.abort();
    // ...so the next start pushes first.
    let daemon = arbiterd::start(config(home.path(), sent.clone())).await.unwrap();
    let api =
        Api { base: format!("http://{}", daemon.addr), token: daemon.token.clone(), http: reqwest::Client::new() };
    let r = remote.path().to_owned();
    wait("push after an unclean stop", || git(&r, &["rev-list", "--count", "main"]) != before).await;
    // The restart moved the old notes into memory and removed the copies.
    assert!(!repo.path().join(".arbiter/vault").exists() && !repo.path().join(".arbiter/attachments").exists());
    assert!(repo.path().join(".arbiter/PROJECT.md").exists(), "committed files are never touched");
    let folder = std::fs::read_dir(memory.join("projects")).unwrap().next().unwrap().unwrap().path();
    assert!(folder.join("notes/decisions/old-choice.md").exists());
    let status = api.get("/v1/memory").await;
    assert_eq!(status["committed_in_projects"][0]["files"], json!([".arbiter/PROJECT.md"]), "{status}");

    // A normal close (the app asked, you said not now) does not push on start.
    api.post(&format!("/v1/threads/{id}/messages"), json!({"text":"One more"})).await;
    wait("fourth turn", || sent.lock().unwrap().len() == 4).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (_, status) = api.post("/v1/memory/sync", json!({"push":false,"closing":true})).await;
    assert!(status["unpushed"].as_u64().unwrap() > 0);
    let before = git(remote.path(), &["rev-list", "--count", "main"]);
    daemon.handle.abort();
    let daemon = arbiterd::start(config(home.path(), sent.clone())).await.unwrap();
    let api =
        Api { base: format!("http://{}", daemon.addr), token: daemon.token.clone(), http: reqwest::Client::new() };
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(git(remote.path(), &["rev-list", "--count", "main"]), before);
    // Closing with push sends it.
    let (_, status) = api.post("/v1/memory/sync", json!({"push":true,"closing":true})).await;
    assert_eq!(status["pushed"], true, "{status}");
    daemon.handle.abort();

    // Restore on a "new computer" from a clone of the remote.
    let clone = tempfile::tempdir().unwrap();
    let copy = clone.path().join("memory");
    git(clone.path(), &["clone", "-q", &remote.path().to_string_lossy(), &copy.to_string_lossy()]);
    let fresh = tempfile::tempdir().unwrap();
    let restored = arbiterd::restore(fresh.path(), &copy).unwrap();
    assert!(restored > 5);
    let daemon = arbiterd::start(config(fresh.path(), Arc::new(Mutex::new(vec![])))).await.unwrap();
    let api =
        Api { base: format!("http://{}", daemon.addr), token: daemon.token.clone(), http: reqwest::Client::new() };
    let threads = api.get("/v1/threads").await;
    assert!(threads.as_array().unwrap().iter().any(|t| t["title"] == "Greeting"), "{threads}");
    let events = api.get(&format!("/v1/threads/{id}/events")).await;
    let texts = events.to_string();
    assert!(texts.contains("One more") && !texts.contains("sk-abcdef"));
    assert!(fresh.path().join("memory").join("guidance").join("about-me.md").exists());
    daemon.handle.abort();
}
