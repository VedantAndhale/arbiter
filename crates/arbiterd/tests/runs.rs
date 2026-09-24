//! Run lifecycle against an in-process fake harness (no tokens spent).

use arbiter_adapters::{Command, Harness, HarnessEvent, RunHandle, StartOpts, TurnOutcome};
use arbiter_core::{AgentEvent, Usage};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// Echo harness. Special messages: "crash" dies mid-turn; "break" writes a bug
/// into a.txt; "limit" is rate limited once; heal messages from Arbiter write
/// the fix into a.txt when `fixes` is true.
fn fake_launcher_with(starts: Arc<Mutex<Vec<StartOpts>>>, fixes: bool) -> arbiterd::Launcher {
    Arc::new(move |_h: Harness, opts: StartOpts| {
        let session = match &opts.resume {
            Some(r) if !r.fork => r.session_id.clone(),
            _ => format!("sess-{}", starts.lock().unwrap().len() + 1),
        };
        let cwd = opts.cwd.clone();
        starts.lock().unwrap().push(opts);
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut announced = false;
            let mut limited = false;
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    Command::Approve { allowed, .. } => {
                        let _ = ev_tx
                            .send(HarnessEvent::Agent(AgentEvent::Message { text: format!("approval:{allowed}") }));
                        let _ = ev_tx.send(HarnessEvent::TurnDone(TurnOutcome::Completed));
                    }
                    Command::Send(text) => {
                        if !announced {
                            announced = true;
                            let _ = ev_tx.send(HarnessEvent::Session(session.clone()));
                        }
                        if text == "break" {
                            std::fs::write(cwd.join("a.txt"), "broken\n").unwrap();
                        }
                        if text == "approval" {
                            let _ = ev_tx.send(HarnessEvent::Approval {
                                request_id: "approval-1".into(),
                                tool: "Bash".into(),
                                input: json!({"command":"echo approved"}),
                            });
                            continue;
                        }
                        if text == "break-web" {
                            std::fs::write(cwd.join("app.js"), "console.error('checkout total is NaN');\n").unwrap();
                        }
                        if text.starts_with("Arbiter ran the project's checks") && fixes {
                            std::fs::write(cwd.join("a.txt"), "fixed\n").unwrap();
                            if cwd.join("app.js").exists() {
                                std::fs::write(
                                    cwd.join("app.js"),
                                    "document.querySelector('h1').textContent='Fixed';\n",
                                )
                                .unwrap();
                            }
                        }
                        if text == "limit" && !limited {
                            limited = true;
                            let _ = ev_tx.send(HarnessEvent::RateLimited { resets_at: None });
                            let _ =
                                ev_tx.send(HarnessEvent::TurnDone(TurnOutcome::Failed("rate limit exceeded".into())));
                            continue;
                        }
                        if text == "crash" {
                            let _ = ev_tx.send(HarnessEvent::Agent(AgentEvent::ToolCall {
                                id: "t1".into(),
                                name: "Bash".into(),
                                input: json!({}),
                            }));
                            let _ = ev_tx.send(HarnessEvent::Exited {
                                code: Some(1),
                                stderr_tail: "boom: out of memory".into(),
                            });
                            return;
                        }
                        let _ = ev_tx.send(HarnessEvent::Agent(AgentEvent::Message { text: format!("echo: {text}") }));
                        let _ = ev_tx.send(HarnessEvent::Agent(AgentEvent::Usage(Usage {
                            input_tokens: 100,
                            output_tokens: 10,
                            cache_read_tokens: 0,
                            cost_usd: 0.01,
                        })));
                        let _ = ev_tx.send(HarnessEvent::TurnDone(TurnOutcome::Completed));
                    }
                    Command::Interrupt => {
                        let _ = ev_tx.send(HarnessEvent::TurnDone(TurnOutcome::Interrupted));
                    }
                    Command::Shutdown => break,
                }
            }
            let _ = ev_tx.send(HarnessEvent::Exited { code: None, stderr_tail: String::new() });
        });
        Ok(RunHandle::from_channels(cmd_tx, ev_rx))
    })
}

struct Api {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl Api {
    async fn get(&self, path: &str) -> Value {
        self.http
            .get(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let r =
            self.http.post(format!("{}{path}", self.base)).bearer_auth(&self.token).json(&body).send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
    /// Poll until `pred` holds. Parallel Windows git/browser startup can take
    /// more than five seconds; this is a deadline, not a fixed sleep.
    async fn wait(&self, tid: &str, what: &str, pred: impl Fn(&Value, &[Value]) -> bool) -> (Value, Vec<Value>) {
        for _ in 0..600 {
            let t = self.get(&format!("/v1/threads/{tid}")).await;
            let ev = self.get(&format!("/v1/threads/{tid}/events")).await.as_array().cloned().unwrap_or_default();
            if pred(&t, &ev) {
                return (t, ev);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let ev = self.get(&format!("/v1/threads/{tid}/events")).await;
        panic!("timed out waiting for {what}; events: {ev:#}");
    }
}

fn kinds(ev: &[Value]) -> Vec<String> {
    ev.iter()
        .map(|e| {
            let k = &e["kind"];
            match k["type"].as_str().unwrap() {
                "agent" => format!("agent:{}", k["event"]["type"].as_str().unwrap()),
                "status_changed" => format!("status:{}", k["status"].as_str().unwrap()),
                t => t.to_owned(),
            }
        })
        .collect()
}

#[tokio::test]
async fn intake_waits_for_answers_and_preserves_context_before_launch() {
    let b = boot(Duration::from_secs(60), |_| true).await;
    let (status, thread) = b.api.post("/v1/threads", json!({"project_id":b.project,"message":"make it better","intake":true,"context_paths":["a.txt"],"worktree":false})).await;
    assert_eq!(status, 200, "{thread}");
    let tid = thread["id"].as_str().unwrap();
    assert_eq!(thread["status"], "needs_approval");
    assert!(b.starts.lock().unwrap().is_empty());
    let events = b.api.get(&format!("/v1/threads/{tid}/events")).await;
    let card =
        events.as_array().unwrap().iter().find(|e| e["kind"]["type"] == "questions_asked").unwrap()["kind"].clone();
    let (status, _) = b.api.post(&format!("/v1/threads/{tid}/messages"), json!({"text":"skip answers"})).await;
    assert_eq!(status, 409);
    let answers: Vec<_> = card["questions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| json!({"question_id":q["id"],"text":"Change a.txt to contain a helpful greeting"}))
        .collect();
    let body = json!({"id":card["id"],"answers":answers});
    assert_eq!(b.api.post(&format!("/v1/threads/{tid}/answers"), body.clone()).await.0, 200);
    assert_eq!(b.api.post(&format!("/v1/threads/{tid}/answers"), body).await.0, 422);
    let (_, events) = b.api.wait(tid, "intake to agent", |t, _| t["status"] == "review").await;
    assert!(
        events.iter().any(|e| e["kind"]["type"] == "intent_ready" && e["kind"]["spec"]["context_paths"][0] == "a.txt")
    );
    assert!(events.iter().any(|e| {
        e["kind"]["event"]["text"].as_str().is_some_and(|s| s.contains("helpful greeting") && s.contains("a.txt"))
    }));
    assert_eq!(b.starts.lock().unwrap().len(), 1);
    assert_eq!(b.starts.lock().unwrap()[0].tool_profile, arbiter_core::ToolProfile::Implementation);
    let (status,_) = b.api.post("/v1/threads", json!({"project_id":b.project,"message":"fix","intake":true,"context_paths":["../secret"],"worktree":false})).await;
    assert_eq!(status, 422);
}

#[tokio::test]
async fn approvals_are_run_scoped_single_use_and_block_messages() {
    let b = boot(Duration::from_secs(60), |_| true).await;
    let (_, thread) =
        b.api.post("/v1/threads", json!({"project_id":b.project,"message":"approval","worktree":false})).await;
    let tid = thread["id"].as_str().unwrap();
    let (_, events) = b.api.wait(tid, "tool approval", |t, _| t["status"] == "needs_approval").await;
    let card = &events.iter().find(|e| e["kind"]["type"] == "approval_requested").unwrap()["kind"];
    assert_eq!(b.api.post(&format!("/v1/threads/{tid}/messages"), json!({"text":"continue"})).await.0, 409);
    assert_eq!(b.api.get(&format!("/v1/threads/{tid}")).await["status"], "needs_approval");
    let body = json!({"run_id":card["run_id"],"request_id":card["request_id"],"allowed":true});
    let invalid = json!({"run_id":arbiter_core::RunId::new(),"request_id":card["request_id"],"allowed":true});
    assert_eq!(b.api.post(&format!("/v1/threads/{tid}/approvals"), invalid).await.0, 422);
    assert_eq!(b.api.post(&format!("/v1/threads/{tid}/approvals"), body.clone()).await.0, 200);
    assert_eq!(b.api.post(&format!("/v1/threads/{tid}/approvals"), body).await.0, 422);
    let (_, events) = b.api.wait(tid, "approved tool completion", |t, _| t["status"] == "review").await;
    assert!(events.iter().any(|e| e["kind"]["event"]["text"] == "approval:true"));
}

#[tokio::test]
async fn clarification_survives_reopen_and_more_is_bounded() {
    let b = boot(Duration::from_secs(60), |_| true).await;
    let (_, thread) = b
        .api
        .post("/v1/threads", json!({"project_id":b.project,"message":"make it better","intake":true,"worktree":false}))
        .await;
    let tid = thread["id"].as_str().unwrap();
    // No harness has started, so reopening the database simulates recovery of a pending intake.
    let mut cfg = arbiterd::Config::new(b._dirs.0.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.launcher = fake_launcher_with(b.starts.clone(), true);
    cfg.available = Arc::new(|_| true);
    let reopened = arbiterd::start(cfg).await.unwrap();
    let api = Api { http: reqwest::Client::new(), base: format!("http://{}", reopened.addr), token: reopened.token };
    assert_eq!(api.get(&format!("/v1/threads/{tid}")).await["status"], "needs_approval");
    for round in 0..2 {
        let events = api.get(&format!("/v1/threads/{tid}/events")).await;
        let card =
            &events.as_array().unwrap().iter().rev().find(|e| e["kind"]["type"] == "questions_asked").unwrap()["kind"];
        let answers: Vec<_> = card["questions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|q| json!({"question_id":q["id"],"text":"Only edit a.txt; success means it contains a greeting."}))
            .collect();
        let body = json!({"id":card["id"],"answers":answers,"more":true});
        let status = api.post(&format!("/v1/threads/{tid}/answers"), body.clone()).await.0;
        if round == 0 {
            assert_eq!(status, 200);
        } else {
            assert_eq!(status, 422);
            assert!(b.starts.lock().unwrap().is_empty());
            let mut final_body = body;
            final_body["more"] = json!(false);
            assert_eq!(api.post(&format!("/v1/threads/{tid}/answers"), final_body).await.0, 200);
        }
    }
    api.wait(tid, "answered after reopening", |t, _| t["status"] == "review").await;
    assert_eq!(b.starts.lock().unwrap().len(), 1);
}

fn init_repo(dir: &Path) {
    for args in [&["init", "-q", "-b", "main"][..], &["config", "user.email", "t@e.com"], &["config", "user.name", "t"]]
    {
        assert!(std::process::Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success());
    }
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    for args in [&["add", "."][..], &["commit", "-q", "-m", "init"]] {
        assert!(std::process::Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success());
    }
}

struct Boot {
    api: Api,
    project: String,
    starts: Arc<Mutex<Vec<StartOpts>>>,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

/// A daemon with a fake harness, one project, and `installed` harnesses.
async fn boot(idle: Duration, installed: fn(Harness) -> bool) -> Boot {
    boot_with(idle, installed, true, false).await
}

/// `with_check` commits a `.arbiter/healing.toml` whose check passes only once
/// a.txt contains "fixed".
async fn boot_with(idle: Duration, installed: fn(Harness) -> bool, fixes: bool, with_check: bool) -> Boot {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    if with_check {
        std::fs::create_dir_all(repo.path().join(".arbiter")).unwrap();
        std::fs::write(
            repo.path().join(".arbiter/healing.toml"),
            "[[check]]\nname = \"unit\"\nkind = \"test\"\ncmd = \"git grep -q fixed -- a.txt || (echo a.txt:1: error: not fixed && exit 1)\"\ntimeout_secs = 30\n",
        )
        .unwrap();
        for args in [&["add", "."][..], &["commit", "-q", "-m", "checks"]] {
            assert!(
                std::process::Command::new("git").arg("-C").arg(repo.path()).args(args).status().unwrap().success()
            );
        }
    }
    let starts = Arc::new(Mutex::new(Vec::new()));
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.launcher = fake_launcher_with(starts.clone(), fixes);
    cfg.available = Arc::new(installed);
    cfg.idle_timeout = idle;
    cfg.retry_scale = Duration::from_millis(1);
    let d = arbiterd::start(cfg).await.unwrap();
    let api = Api { http: reqwest::Client::new(), base: format!("http://{}", d.addr), token: d.token };
    let (_, p) = api.post("/v1/projects", json!({ "path": repo.path() })).await;
    Boot { api, project: p["id"].as_str().unwrap().to_owned(), starts, _dirs: (home, repo) }
}

async fn setup(idle: Duration) -> (Api, String, Arc<Mutex<Vec<StartOpts>>>, Boot) {
    let b = boot(idle, |_| true).await;
    let (_, t) = b
        .api
        .post(
            "/v1/threads",
            json!({ "project_id": b.project, "title": "t", "harness": "claude", "permission": "auto" }),
        )
        .await;
    assert_eq!(t["permission"], "auto");
    let api = Api { http: b.api.http.clone(), base: b.api.base.clone(), token: b.api.token.clone() };
    (api, t["id"].as_str().unwrap().to_owned(), b.starts.clone(), b)
}

#[tokio::test]
async fn message_runs_steers_stops_and_resumes() {
    let (api, tid, starts, _b) = setup(Duration::from_secs(600)).await;

    let (s, _) = api.post(&format!("/v1/threads/{tid}/messages"), json!({ "text": "hello" })).await;
    assert_eq!(s, 200);
    let (t, ev) = api.wait(&tid, "first turn", |t, _| t["status"] == "review").await;
    assert_eq!(
        kinds(&ev)[2..],
        ["user_message", "run_started", "status:running", "session", "agent:message", "agent:usage", "status:review"]
    );
    assert_eq!(t["session_id"], "sess-1");
    assert_eq!(t["input_tokens"], 100);
    {
        let s = starts.lock().unwrap();
        assert!(s[0].resume.is_none());
        assert_eq!(s[0].permission, arbiter_core::PermissionMode::Auto);
        assert!(s[0].cwd.ends_with(t["worktree"].as_str().unwrap().rsplit(['/', '\\']).next().unwrap()));
    }

    // Follow-up goes to the same warm process.
    api.post(&format!("/v1/threads/{tid}/messages"), json!({ "text": "more" })).await;
    api.wait(&tid, "second turn", |t, _| t["input_tokens"] == 200).await;
    assert_eq!(starts.lock().unwrap().len(), 1, "steered, not restarted");

    // Stopping is not reported as a crash.
    let (s, _) = api.post(&format!("/v1/threads/{tid}/stop"), json!({})).await;
    assert_eq!(s, 200);
    let (_, ev) = api.wait(&tid, "run ended", |_, ev| kinds(ev).last().is_some_and(|k| k == "run_ended")).await;
    assert!(!kinds(&ev).contains(&"status:failed".to_owned()));

    // Next message resumes the harness session.
    api.post(&format!("/v1/threads/{tid}/messages"), json!({ "text": "again" })).await;
    api.wait(&tid, "resumed turn", |t, _| t["input_tokens"] == 300).await;
    let s = starts.lock().unwrap();
    assert_eq!(s.len(), 2);
    let r = s[1].resume.as_ref().expect("resume");
    assert_eq!((r.session_id.as_str(), r.fork), ("sess-1", false));
}

#[tokio::test]
async fn browser_failure_heals_and_attachments_reach_the_fake_agent() {
    if arbiter_browser::find_browser().is_none() {
        eprintln!("no Chromium browser found; skipping");
        return;
    }
    let b = boot(Duration::from_secs(600), |_| true).await;
    let repo = b._dirs.1.path();
    std::fs::create_dir_all(repo.join(".arbiter")).unwrap();
    std::fs::write(
        repo.join(".arbiter/healing.toml"),
        "[browser]\ndev = \"node server.cjs\"\nroutes = [\"/\"]\nready_timeout_secs = 15\n",
    )
    .unwrap();
    std::fs::write(repo.join("server.cjs"),r#"const fs=require('fs');require('http').createServer((q,s)=>{s.setHeader('Content-Type',q.url==='/app.js'?'application/javascript':'text/html');s.end(q.url==='/app.js'?fs.readFileSync('app.js'):'<!doctype html><html lang="en"><head><title>Fixture</title></head><body><h1>Checkout</h1><button id="pay">Pay</button><script src="/app.js"></script></body></html>');}).listen(process.env.PORT,'127.0.0.1');"#).unwrap();
    std::fs::write(repo.join("app.js"), "// healthy\n").unwrap();
    for args in [&["add", "."][..], &["commit", "-qm", "browser fixture"]] {
        assert!(std::process::Command::new("git").arg("-C").arg(repo).args(args).status().unwrap().success());
    }
    let (_, t) = b
        .api
        .post("/v1/threads", json!({"project_id":b.project,"harness":"claude","worktree":false,"message":"break-web"}))
        .await;
    let tid = t["id"].as_str().unwrap();
    let mut events = Vec::new();
    for _ in 0..600 {
        events = b.api.get(&format!("/v1/threads/{tid}/events")).await.as_array().unwrap().clone();
        let state = b.api.get(&format!("/v1/threads/{tid}")).await;
        if state["status"] == "review" && events.iter().any(|e| e["kind"]["type"] == "heal") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let heals: Vec<_> = events.iter().filter(|e| e["kind"]["type"] == "heal").collect();
    assert_eq!(heals.len(), 1, "{events:#?}");
    let excerpt = heals[0]["kind"]["excerpt"].as_str().unwrap();
    assert!(excerpt.contains("checkout total is NaN") && excerpt.len() <= 6500, "{excerpt}");
    assert_eq!(b.api.get(&format!("/v1/threads/{tid}")).await["status"], "review");
    let (status, desc) =
        b.api.post(&format!("/v1/threads/{tid}/browser"), json!({"op":"inspect","selector":"#pay"})).await;
    assert_eq!(status, 200, "{desc}");
    assert!(desc["descriptor"].as_str().unwrap().len() < 1200);
    let (status, viewport) =
        b.api.post(&format!("/v1/threads/{tid}/preview/viewport"), json!({"width":390,"height":844})).await;
    assert_eq!(status, 200, "{viewport}");
    let (status, capture) = b.api.post(&format!("/v1/threads/{tid}/preview/captures"), json!({})).await;
    assert_eq!(status, 200, "{capture}");
    assert_eq!(capture["width"], 390);
    assert!(capture["ts"].as_str().unwrap().ends_with('Z'));
    let bytes = b
        .api
        .http
        .get(format!("{}/v1/attachments/{}", b.api.base, capture["id"].as_str().unwrap()))
        .bearer_auth(&b.api.token)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let image = image::load_from_memory(&bytes).unwrap();
    assert_eq!((image.width(), image.height()), (390, 844));
    assert_eq!(b.api.get(&format!("/v1/threads/{tid}/preview/captures")).await[0]["id"], capture["id"]);
    assert_eq!(
        b.api.post(&format!("/v1/threads/{tid}/preview/viewport"), json!({"width":0,"height":844})).await.0,
        422
    );
    assert_eq!(
        b.api.http.get(format!("{}/v1/threads/{tid}/preview/frame", b.api.base)).send().await.unwrap().status(),
        401
    );
    let uploaded: Value = b
        .api
        .http
        .post(format!("{}/v1/attachments?name=brief.txt", b.api.base))
        .bearer_auth(&b.api.token)
        .body("Only adjust the checkout label.")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let (status, _) = b
        .api
        .post(
            &format!("/v1/threads/{tid}/messages"),
            json!({"text":"Use the attached brief","attachments":[uploaded["id"]]}),
        )
        .await;
    assert_eq!(status, 200);
    let (_, ev) = b
        .api
        .wait(tid, "attachment delivery", |_, ev| {
            ev.iter().any(|e| {
                e["kind"]["event"]["text"]
                    .as_str()
                    .is_some_and(|s| s.contains("attachments") && s.contains("brief.txt"))
            })
        })
        .await;
    assert!(ev.iter().any(|e| e["kind"]["attachments"][0]["id"] == uploaded["id"]));
    assert!(!repo.join(".arbiter/attachments").exists(), "attachments stay out of the project");
    b.api.post(&format!("/v1/threads/{tid}/preview/stop"), json!({})).await;
    b.api.post(&format!("/v1/threads/{tid}/stop"), json!({})).await;
}

#[tokio::test]
async fn fork_branches_the_harness_session() {
    let (api, tid, starts, _b) = setup(Duration::from_secs(600)).await;
    api.post(&format!("/v1/threads/{tid}/messages"), json!({ "text": "hello" })).await;
    let (_, ev) = api.wait(&tid, "turn", |t, _| t["status"] == "review").await;

    let at = ev.last().unwrap()["seq"].as_i64().unwrap();
    let (_, fork) = api.post(&format!("/v1/threads/{tid}/fork"), json!({ "at_seq": at })).await;
    let fid = fork["id"].as_str().unwrap().to_owned();
    assert_eq!(fork["session_id"], "sess-1");

    api.post(&format!("/v1/threads/{fid}/messages"), json!({ "text": "branch" })).await;
    let (f, _) = api.wait(&fid, "fork turn", |t, _| t["status"] == "review" && t["session_id"] != "sess-1").await;
    let s = starts.lock().unwrap();
    let r = s[1].resume.as_ref().expect("fork resumes parent session");
    assert!(r.fork && r.session_id == "sess-1");
    assert_eq!(f["session_id"], "sess-2", "fork now has its own session");
}

#[tokio::test]
async fn crash_mid_turn_resumes_the_session_once() {
    let (api, tid, starts, _b) = setup(Duration::from_secs(600)).await;
    api.post(&format!("/v1/threads/{tid}/messages"), json!({ "text": "crash" })).await;
    let (_, ev) = api.wait(&tid, "recovered turn", |t, _| t["status"] == "review").await;
    let notice = ev.iter().find(|e| e["kind"]["type"] == "notice").expect("notice");
    let text = notice["kind"]["text"].as_str().unwrap();
    assert!(
        text.contains("exited unexpectedly") && text.contains("out of memory") && text.contains("Resuming"),
        "{text}"
    );
    let s = starts.lock().unwrap();
    assert_eq!(s.len(), 2, "restarted once");
    assert_eq!(s[1].resume.as_ref().unwrap().session_id, "sess-1", "same session");
}

#[tokio::test]
async fn heal_loop_feeds_back_failures_until_green() {
    let b = boot_with(Duration::from_secs(600), |_| true, true, true).await;
    let (_, t) =
        b.api.post("/v1/threads", json!({ "project_id": b.project, "message": "break", "harness": "claude" })).await;
    let tid = t["id"].as_str().unwrap().to_owned();
    let (t, ev) = b.api.wait(&tid, "healed", |t, _| t["status"] == "review").await;
    let k = kinds(&ev);
    let checks: Vec<&Value> = ev.iter().filter(|e| e["kind"]["type"] == "checks_ran").collect();
    assert_eq!(checks.len(), 2, "{k:?}");
    assert_eq!(checks[0]["kind"]["results"][0]["ok"], false);
    assert_eq!(checks[1]["kind"]["results"][0]["ok"], true);
    let heal = ev.iter().find(|e| e["kind"]["type"] == "heal").expect("heal event");
    let excerpt = heal["kind"]["excerpt"].as_str().unwrap();
    assert!(excerpt.contains("a.txt:1") && excerpt.contains("not fixed") && excerpt.len() < 2000, "{excerpt}");
    assert_eq!(t["heal_attempts"], 1);
    assert!(t["checkpoints"].as_u64().unwrap() >= 2, "one checkpoint per changing turn");
    // The heal feedback went to the agent, not into the log as a user message.
    assert_eq!(k.iter().filter(|x| *x == "user_message").count(), 1);
}

#[tokio::test]
async fn heal_loop_stops_when_the_same_failure_returns() {
    let b = boot_with(Duration::from_secs(600), |_| true, false, true).await;
    let (_, t) =
        b.api.post("/v1/threads", json!({ "project_id": b.project, "message": "break", "harness": "claude" })).await;
    let tid = t["id"].as_str().unwrap().to_owned();
    let (t, ev) = b.api.wait(&tid, "gave up", |t, _| t["status"] == "failed").await;
    assert_eq!(t["heal_attempts"], 1, "one fix attempt, then the repeat breaker");
    let notice = ev.iter().rev().find(|e| e["kind"]["type"] == "notice").unwrap();
    assert!(notice["kind"]["text"].as_str().unwrap().contains("same failures"));
}

#[tokio::test]
async fn checkpoints_diff_and_revert() {
    let b = boot_with(Duration::from_secs(600), |_| true, true, false).await;
    let (_, t) =
        b.api.post("/v1/threads", json!({ "project_id": b.project, "message": "break", "harness": "claude" })).await;
    let tid = t["id"].as_str().unwrap().to_owned();
    let wt = t["worktree"].as_str().unwrap().to_owned();
    b.api.wait(&tid, "turn", |t, _| t["status"] == "review" && t["checkpoints"] == 1).await;
    let cps = b.api.get(&format!("/v1/threads/{tid}/checkpoints")).await;
    assert_eq!(cps.as_array().unwrap().len(), 1);
    let r = b
        .api
        .http
        .get(format!("{}/v1/threads/{tid}/diff?from=0&to=1", b.api.base))
        .bearer_auth(&b.api.token)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(r.contains("+broken"), "{r}");

    let (s, _) = b.api.post(&format!("/v1/threads/{tid}/revert"), json!({ "n": 0 })).await;
    assert_eq!(s, 200);
    assert_eq!(std::fs::read_to_string(Path::new(&wt).join("a.txt")).unwrap().trim(), "a");
    let (_, ev) = b.api.wait(&tid, "reverted", |_, ev| kinds(ev).contains(&"reverted".to_owned())).await;
    assert!(kinds(&ev).contains(&"checkpoint".to_owned()));
}

#[tokio::test]
async fn rate_limit_waits_and_resumes() {
    let (api, tid, _starts, _b) = setup(Duration::from_secs(600)).await;
    api.post(&format!("/v1/threads/{tid}/messages"), json!({ "text": "limit" })).await;
    let (_, ev) = api
        .wait(&tid, "resumed after limit", |t, ev| {
            t["status"] == "review" && kinds(ev).contains(&"rate_limited".to_owned())
        })
        .await;
    let k = kinds(&ev);
    assert!(!k.contains(&"status:failed".to_owned()), "{k:?}");
    let msg = ev.iter().rev().find(|e| e["kind"]["event"]["type"] == "message").unwrap();
    assert!(msg["kind"]["event"]["text"].as_str().unwrap().contains("rate limit has reset"));
}

#[tokio::test]
async fn budget_pauses_for_approval_and_settle_hides_until_change() {
    let (api, tid, _starts, _b) = setup(Duration::from_secs(600)).await;
    let (s, t) = api.patch(&format!("/v1/threads/{tid}"), json!({ "budget_usd": 0.005 })).await;
    assert_eq!((s, t["budget_usd"].as_f64()), (200, Some(0.005)));
    api.post(&format!("/v1/threads/{tid}/messages"), json!({ "text": "hello" })).await;
    let (_, ev) = api.wait(&tid, "paused", |t, _| t["status"] == "needs_approval").await;
    assert!(ev.iter().any(|e| e["kind"]["text"].as_str().is_some_and(|t| t.contains("Budget reached"))));

    let (_, t) = api.post(&format!("/v1/threads/{tid}/settle"), json!({})).await;
    assert_eq!(t["settled"], true);
    api.patch(&format!("/v1/threads/{tid}"), json!({ "budget_usd": null })).await;
    api.post(&format!("/v1/threads/{tid}/messages"), json!({ "text": "more" })).await;
    let (t, _) = api.wait(&tid, "unsettled", |t, _| t["status"] == "review").await;
    assert_eq!(t["settled"], false, "a status change brings it back to the inbox");
    assert!(t["budget_usd"].is_null());
}

#[tokio::test]
async fn idle_sessions_are_reaped_and_resumable() {
    let (api, tid, starts, _b) = setup(Duration::from_millis(150)).await;
    api.post(&format!("/v1/threads/{tid}/messages"), json!({ "text": "hello" })).await;
    let (t, _) =
        api.wait(&tid, "reaped", |t, ev| t["status"] == "review" && kinds(ev).contains(&"run_ended".to_owned())).await;
    assert_eq!(t["status"], "review", "reaping does not change status");

    api.post(&format!("/v1/threads/{tid}/messages"), json!({ "text": "later" })).await;
    api.wait(&tid, "resumed", |t, _| t["input_tokens"] == 200).await;
    assert_eq!(starts.lock().unwrap()[1].resume.as_ref().unwrap().session_id, "sess-1");
}

#[tokio::test]
async fn interrupt_without_run_conflicts() {
    let (api, tid, _starts, _b) = setup(Duration::from_secs(600)).await;
    let (s, _) = api.post(&format!("/v1/threads/{tid}/interrupt"), json!({})).await;
    assert_eq!(s, 409);
}

impl Api {
    async fn patch(&self, path: &str, body: Value) -> (u16, Value) {
        let r =
            self.http.patch(format!("{}{path}", self.base)).bearer_auth(&self.token).json(&body).send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
}

#[tokio::test]
async fn composer_first_thread_is_titled_and_auto_routed() {
    let b = boot(Duration::from_secs(600), |h| h == Harness::Codex).await;
    let (s, t) = b
        .api
        .post(
            "/v1/threads",
            json!({ "project_id": b.project, "message": "please fix the flaky login test in auth.rs" }),
        )
        .await;
    assert_eq!(s, 200, "{t}");
    assert_eq!(t["title"], "Fix the flaky login test in auth.rs");
    let tid = t["id"].as_str().unwrap();
    let (t, ev) = b.api.wait(tid, "routed turn", |t, _| t["status"] == "review").await;
    assert_eq!(t["harness"], "codex", "only codex is installed");
    let routed = ev.iter().find(|e| e["kind"]["type"] == "config_changed").expect("routing recorded");
    assert!(routed["kind"]["reason"].as_str().unwrap().contains("only harness installed"));
}

#[tokio::test]
async fn no_harness_installed_is_a_clear_error() {
    let b = boot(Duration::from_secs(600), |_| false).await;
    let (_, t) = b.api.post("/v1/threads", json!({ "project_id": b.project, "message": "hi" })).await;
    let tid = t["id"].as_str().expect("thread still created");
    let (_, ev) = b.api.wait(tid, "failure", |t, _| t["status"] == "failed").await;
    let notice = ev.iter().find(|e| e["kind"]["type"] == "notice").unwrap();
    assert!(notice["kind"]["text"].as_str().unwrap().contains("Install Claude Code"));
}

#[tokio::test]
async fn config_changes_apply_on_next_turn_with_resume() {
    let (api, tid, starts, _b) = setup(Duration::from_secs(600)).await;
    api.post(&format!("/v1/threads/{tid}/messages"), json!({ "text": "hello" })).await;
    api.wait(&tid, "turn", |t, _| t["status"] == "review").await;

    let (s, t) = api.patch(&format!("/v1/threads/{tid}"), json!({ "model": "haiku", "effort": "low" })).await;
    assert_eq!((s, &t["model"], &t["effort"]), (200, &json!("haiku"), &json!("low")));
    // Idle warm process is restarted so the next turn picks the change up.
    api.wait(&tid, "restart", |_, ev| kinds(ev).last().is_some_and(|k| k == "run_ended")).await;
    api.post(&format!("/v1/threads/{tid}/messages"), json!({ "text": "again" })).await;
    api.wait(&tid, "second turn", |t, _| t["input_tokens"] == 200).await;
    {
        let s = starts.lock().unwrap();
        assert_eq!((s[1].model.as_deref(), s[1].effort.as_deref()), (Some("haiku"), Some("low")));
        assert_eq!(s[1].resume.as_ref().unwrap().session_id, "sess-1");
    }

    // Resetting to default uses null.
    let (_, t) = api.patch(&format!("/v1/threads/{tid}"), json!({ "model": null })).await;
    assert!(t["model"].is_null() && t["effort"] == "low");
}

#[tokio::test]
async fn harness_is_locked_once_started() {
    let (api, tid, _starts, _b) = setup(Duration::from_secs(600)).await;
    let (s, _) = api.patch(&format!("/v1/threads/{tid}"), json!({ "harness": "codex" })).await;
    assert_eq!(s, 200, "allowed before the first session");
    api.post(&format!("/v1/threads/{tid}/messages"), json!({ "text": "hello" })).await;
    api.wait(&tid, "turn", |t, _| t["status"] == "review").await;
    let (s, _) = api.patch(&format!("/v1/threads/{tid}"), json!({ "harness": "claude" })).await;
    assert_eq!(s, 409);
    let (s, t) = api.patch(&format!("/v1/threads/{tid}"), json!({ "title": "Renamed" })).await;
    assert_eq!((s, t["title"].as_str()), (200, Some("Renamed")));
}

#[tokio::test]
async fn task_start_links_thread_and_follows_agent_status() {
    let b = boot(Duration::from_secs(600), |_| true).await;
    let (s, task) = b
        .api
        .post("/v1/tasks", json!({ "project_id": b.project, "title": "Add dark mode", "description": "Use CSS vars.", "priority": "high" }))
        .await;
    assert_eq!(s, 200, "{task}");
    assert_eq!((task["status"].as_str(), task["priority"].as_str()), (Some("todo"), Some("high")));
    let key = task["key"].as_str().unwrap().to_owned();
    assert!(key.ends_with("-1"), "{key}");

    let tid = task["id"].as_str().unwrap();
    let (s, started) = b.api.post(&format!("/v1/tasks/{tid}/start"), json!({ "harness": "claude" })).await;
    assert_eq!(s, 200, "{started}");
    let thread = started["thread"]["id"].as_str().unwrap().to_owned();
    assert_eq!(started["thread"]["title"], format!("{key}: Add dark mode"));

    let (_, ev) = b.api.wait(&thread, "agent done", |t, _| t["status"] == "review").await;
    let seed =
        ev.iter().find(|e| e["kind"]["type"] == "user_message").unwrap()["kind"]["text"].as_str().unwrap().to_owned();
    assert!(seed.contains("Add dark mode") && seed.contains("Use CSS vars."), "{seed}");

    let task = b.api.get(&format!("/v1/tasks/{tid}")).await;
    assert_eq!(task["status"], "in_review", "task follows the agent");
    assert_eq!(task["thread_ids"][0], thread.as_str());

    let (s, t) = b.api.patch(&format!("/v1/tasks/{tid}"), json!({ "status": "done" })).await;
    assert_eq!((s, t["status"].as_str()), (200, Some("done")));
}
