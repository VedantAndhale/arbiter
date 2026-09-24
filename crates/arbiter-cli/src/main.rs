//! `arb`: thin client over the daemon's HTTP API. Anything the desktop app can
//! do should be possible here too.

use anyhow::{Context, Result, bail};
use arbiter_core::{DaemonInfo, arbiter_home};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "arb", version, about = "Arbiter: self-healing orchestrator for coding agents")]
struct Cli {
    /// Print raw JSON responses.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Inspect, revise and control an orchestration plan.
    #[command(subcommand)]
    Plan(PlanCmd),
    /// Read, edit and review project memory.
    #[command(subcommand)]
    Vault(VaultCmd),
    /// Inspect measured model strengths and routing preferences.
    Learning { thread: String },
    /// Update routing preferences from a JSON file.
    Routing { thread: String, file: PathBuf },
    /// Mark a measured task as needing user rework.
    Rework {
        thread: String,
        #[arg(long)]
        clear: bool,
    },
    /// List local model downloads and measured latency.
    Models,
    /// Download a pinned local model and run its benchmark.
    InstallModel {
        #[arg(value_parser = ["potion", "lfm", "granite", "qwen", "lfm-8b"])]
        model: String,
        /// Confirm that you reviewed and accept the LFM license.
        #[arg(long)]
        accept_license: bool,
    },
    /// Select an installed question model.
    SelectModel {
        #[arg(value_parser = ["lfm", "granite", "qwen", "lfm-8b"])]
        model: String,
    },
    /// Answer clarification cards using a JSON array of {question_id,text} objects.
    Answer {
        thread: String,
        id: String,
        file: PathBuf,
        #[arg(long)]
        more: bool,
    },
    /// Benchmark an installed local intake model (results appear in `arb models --json`).
    Bench {
        #[arg(value_parser = ["potion", "lfm", "granite", "qwen", "lfm-8b"])]
        model: String,
    },
    /// Resolve one pending tool request. Decisions are scoped to this run.
    Approve {
        thread: String,
        run: String,
        request: String,
        #[arg(long)]
        allow: bool,
    },
    /// Check that the daemon is reachable.
    Status,
    /// Manage projects (git repositories).
    #[command(subcommand)]
    Project(ProjectCmd),
    /// Start an agent on a task (the text is its first message; the title is derived).
    New {
        title: String,
        /// Project id (defaults to the only/first project).
        #[arg(long, short)]
        project: Option<String>,
        #[arg(long, default_value = "auto", value_parser = ["auto", "claude", "codex"])]
        harness: String,
        /// Run in the project checkout instead of a new worktree.
        #[arg(long)]
        no_worktree: bool,
        /// safe: edit files only; auto: anything (inside the worktree); plan: read-only.
        #[arg(long, default_value = "safe", value_parser = ["safe", "auto", "plan"])]
        permission: String,
        /// Harness model name (e.g. sonnet, haiku). Defaults to the harness default.
        #[arg(long)]
        model: Option<String>,
        /// Start directly without the local clarification step.
        #[arg(long)]
        no_intake: bool,
        /// Propose a reviewable orchestration plan before implementing.
        #[arg(long)]
        workflow: bool,
    },
    /// List threads.
    Ls,
    /// Send a message to a thread (starts, resumes or steers its agent).
    Send {
        thread: String,
        text: String,
        /// Stream events until the agent finishes its turn.
        #[arg(long, short)]
        watch: bool,
    },
    /// Stream a thread's events until its agent is no longer running.
    Watch { thread: String },
    /// Interrupt the agent's current turn.
    Interrupt { thread: String },
    /// Stop the agent process (the session stays resumable).
    Stop { thread: String },
    /// Print a thread's events.
    Log {
        thread: String,
        #[arg(long, default_value_t = 0)]
        after: i64,
    },
    /// Show a thread's worktree diff.
    Diff { thread: String },
    /// Fork a thread at an event seq.
    Fork { thread: String, at_seq: i64 },
}

#[derive(Subcommand)]
enum VaultCmd {
    List {
        thread: String,
    },
    Search {
        thread: String,
        query: String,
        #[arg(long, default_value_t = 1000)]
        budget: usize,
    },
    Read {
        thread: String,
        id: String,
        #[arg(long, default_value_t = 1000)]
        budget: usize,
    },
    /// File contains {note:{id,title,body,revision},base_revision}.
    Save {
        thread: String,
        file: PathBuf,
    },
    Propose {
        thread: String,
        file: PathBuf,
    },
    Resolve {
        thread: String,
        id: String,
        #[arg(long)]
        accept: bool,
    },
    Rebuild {
        thread: String,
    },
}

#[derive(Subcommand)]
enum PlanCmd {
    Show {
        thread: String,
    },
    Edit {
        thread: String,
        file: PathBuf,
    },
    Refine {
        thread: String,
    },
    Approve {
        thread: String,
        #[arg(long)]
        revision: u32,
    },
    Pause {
        thread: String,
    },
    Resume {
        thread: String,
    },
    Scope {
        thread: String,
        node: String,
        #[arg(required = true)]
        paths: Vec<String>,
    },
    Budget {
        thread: String,
        #[arg(long)]
        node: Option<String>,
        #[arg(long)]
        usd: Option<f64>,
        #[arg(long)]
        tokens: Option<u64>,
    },
    Diff {
        thread: String,
    },
}

#[derive(Subcommand)]
enum ProjectCmd {
    Add {
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
    Ls,
}

struct Client {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl Client {
    fn connect() -> Result<Self> {
        let info = DaemonInfo::read(&arbiter_home()).context("start it with `arbiterd`")?;
        Ok(Self { http: reqwest::Client::new(), base: info.base_url(), token: info.token })
    }

    async fn send(&self, rb: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let res = rb.bearer_auth(&self.token).send().await.context("daemon not reachable; is arbiterd running?")?;
        if !res.status().is_success() {
            let status = res.status();
            let body: Value = res.json().await.unwrap_or(Value::Null);
            bail!("{status}: {}", body["error"].as_str().unwrap_or("request failed"));
        }
        Ok(res)
    }

    async fn get(&self, path: &str) -> Result<Value> {
        Ok(self.send(self.http.get(format!("{}{path}", self.base))).await?.json().await?)
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        Ok(self.send(self.http.post(format!("{}{path}", self.base)).json(&body)).await?.json().await?)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let c = Client::connect()?;
    let out = match cli.cmd {
        Cmd::Learning { thread } => {
            let value = c.get(&format!("/v1/threads/{thread}/learning")).await?;
            if !cli.json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
            value
        }
        Cmd::Routing { thread, file } => {
            c.post(&format!("/v1/threads/{thread}/learning"), serde_json::from_slice(&std::fs::read(file)?)?).await?
        }
        Cmd::Rework { thread, clear } => {
            c.post(&format!("/v1/threads/{thread}/learning/rework"), json!({"rework":!clear})).await?
        }
        Cmd::Vault(cmd) => {
            let v = match cmd {
                VaultCmd::List { thread } => c.get(&format!("/v1/threads/{thread}/vault")).await?,
                VaultCmd::Search { thread, query, budget } => {
                    c.post(
                        &format!("/v1/threads/{thread}/vault/tool"),
                        json!({"op":"search","query":query,"budget":budget}),
                    )
                    .await?
                }
                VaultCmd::Read { thread, id, budget } => {
                    c.post(&format!("/v1/threads/{thread}/vault/tool"), json!({"op":"read","id":id,"budget":budget}))
                        .await?
                }
                VaultCmd::Save { thread, file } => {
                    c.post(&format!("/v1/threads/{thread}/vault"), serde_json::from_slice(&std::fs::read(file)?)?)
                        .await?
                }
                VaultCmd::Propose { thread, file } => {
                    c.post(
                        &format!("/v1/threads/{thread}/vault/propose"),
                        serde_json::from_slice(&std::fs::read(file)?)?,
                    )
                    .await?
                }
                VaultCmd::Resolve { thread, id, accept } => {
                    c.post(&format!("/v1/threads/{thread}/vault/resolve"), json!({"id":id,"accepted":accept})).await?
                }
                VaultCmd::Rebuild { thread } => {
                    c.post(&format!("/v1/threads/{thread}/vault/rebuild"), json!({})).await?
                }
            };
            if !cli.json {
                println!("{}", serde_json::to_string_pretty(&v)?);
            }
            v
        }
        Cmd::Plan(cmd) => {
            let v = match cmd {
                PlanCmd::Show { thread } => c.get(&format!("/v1/threads/{thread}/plan")).await?,
                PlanCmd::Edit { thread, file } => {
                    let plan: Value = serde_json::from_slice(&std::fs::read(file)?)?;
                    c.send(c.http.patch(format!("{}/v1/threads/{thread}/plan", c.base)).json(&plan))
                        .await?
                        .json()
                        .await?
                }
                PlanCmd::Refine { thread } => c.post(&format!("/v1/threads/{thread}/plan/draft"), json!({})).await?,
                PlanCmd::Approve { thread, revision } => {
                    c.post(&format!("/v1/threads/{thread}/plan/approve"), json!({"revision":revision})).await?
                }
                PlanCmd::Pause { thread } => {
                    c.post(&format!("/v1/threads/{thread}/plan/control"), json!({"action":"pause"})).await?
                }
                PlanCmd::Resume { thread } => {
                    c.post(&format!("/v1/threads/{thread}/plan/control"), json!({"action":"resume"})).await?
                }
                PlanCmd::Scope { thread, node, paths } => {
                    c.post(&format!("/v1/threads/{thread}/plan/scope"), json!({"node":node,"paths":paths})).await?
                }
                PlanCmd::Budget { thread, node, usd, tokens } => {
                    c.post(&format!("/v1/threads/{thread}/plan/budget"), json!({"node":node,"usd":usd,"tokens":tokens}))
                        .await?
                }
                PlanCmd::Diff { thread } => {
                    let diff =
                        c.send(c.http.get(format!("{}/v1/threads/{thread}/plan/diff", c.base))).await?.text().await?;
                    if !cli.json {
                        println!("{diff}");
                    }
                    if cli.json {
                        println!("{}", serde_json::to_string(&diff)?);
                    }
                    return Ok(());
                }
            };
            if !cli.json {
                println!("{}", serde_json::to_string_pretty(&v)?);
            }
            v
        }
        Cmd::InstallModel { model, accept_license } => {
            c.post(&format!("/v1/models/{model}/install"), json!({"accept_license":accept_license})).await?
        }
        Cmd::SelectModel { model } => c.post(&format!("/v1/models/{model}/select"), json!({})).await?,
        Cmd::Answer { thread, id, file, more } => {
            let answers: Vec<arbiter_core::Answer> = serde_json::from_slice(&std::fs::read(file)?)?;
            c.post(&format!("/v1/threads/{thread}/answers"), json!({"id":id,"answers":answers,"more":more})).await?
        }
        Cmd::Models => {
            let v = c.get("/v1/models").await?;
            if !cli.json {
                println!("{}", serde_json::to_string_pretty(&v)?);
            }
            v
        }
        Cmd::Bench { model } => {
            let v = c.post(&format!("/v1/models/{model}/benchmark"), json!({})).await?;
            if !cli.json {
                println!("Benchmark started; see arb models --json for results.");
            }
            v
        }
        Cmd::Approve { thread, run, request, allow } => {
            c.post(
                &format!("/v1/threads/{thread}/approvals"),
                json!({"run_id":run,"request_id":request,"allowed":allow}),
            )
            .await?
        }
        Cmd::Status => {
            let v = c.get("/v1/health").await?;
            println!("arbiterd is up at {}", c.base);
            v
        }
        Cmd::Project(ProjectCmd::Add { path, name }) => {
            let v = c.post("/v1/projects", json!({ "path": path, "name": name })).await?;
            if !cli.json {
                println!("added {} ({})", v["name"].as_str().unwrap_or_default(), v["id"].as_str().unwrap_or_default());
            }
            v
        }
        Cmd::Project(ProjectCmd::Ls) => {
            let v = c.get("/v1/projects").await?;
            if !cli.json {
                for p in v.as_array().into_iter().flatten() {
                    println!("{}  {:<20} {}", s(&p["id"]), s(&p["name"]), s(&p["path"]));
                }
            }
            v
        }
        Cmd::New { title, project, harness, no_worktree, permission, model, no_intake, workflow } => {
            let project = match project {
                Some(p) => p,
                None => c.get("/v1/projects").await?[0]["id"]
                    .as_str()
                    .context("no projects yet; run `arb project add <path>`")?
                    .to_owned(),
            };
            let v = c
                .post(
                    "/v1/threads",
                    json!({
                        "project_id": project, "message": title, "harness": harness,
                        "worktree": !no_worktree, "permission": permission, "model": model,
                        "intake": !no_intake, "workflow": workflow,
                    }),
                )
                .await?;
            if !cli.json {
                println!("{}  {}", s(&v["id"]), v["worktree"].as_str().unwrap_or("(no worktree)"));
            }
            v
        }
        Cmd::Ls => {
            let v = c.get("/v1/threads").await?;
            if !cli.json {
                for t in v.as_array().into_iter().flatten() {
                    println!(
                        "{}  {:<14} {:<7} {:>8} tok  ${:.2}  {}",
                        s(&t["id"]),
                        s(&t["status"]),
                        s(&t["harness"]),
                        t["input_tokens"].as_u64().unwrap_or(0) + t["output_tokens"].as_u64().unwrap_or(0),
                        t["cost_usd"].as_f64().unwrap_or(0.0),
                        s(&t["title"]),
                    );
                }
            }
            v
        }
        Cmd::Send { thread, text, watch } => {
            let v = c.post(&format!("/v1/threads/{thread}/messages"), json!({ "text": text })).await?;
            if watch {
                return watch_thread(&c, &thread, v["seq"].as_i64().unwrap_or(0)).await;
            }
            v
        }
        Cmd::Watch { thread } => return watch_thread(&c, &thread, 0).await,
        Cmd::Interrupt { thread } => c.post(&format!("/v1/threads/{thread}/interrupt"), json!({})).await?,
        Cmd::Stop { thread } => c.post(&format!("/v1/threads/{thread}/stop"), json!({})).await?,
        Cmd::Log { thread, after } => {
            let v = c.get(&format!("/v1/threads/{thread}/events?after={after}")).await?;
            if !cli.json {
                for e in v.as_array().into_iter().flatten() {
                    println!("{:>4}  {}  {}", e["seq"], s(&e["kind"]["type"]), e["kind"]);
                }
            }
            v
        }
        Cmd::Diff { thread } => {
            let url = format!("{}/v1/threads/{thread}/diff", c.base);
            print!("{}", c.send(c.http.get(url)).await?.text().await?);
            return Ok(());
        }
        Cmd::Fork { thread, at_seq } => {
            let v = c.post(&format!("/v1/threads/{thread}/fork"), json!({ "at_seq": at_seq })).await?;
            if !cli.json {
                println!("forked -> {}", s(&v["id"]));
            }
            v
        }
    };
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    }
    Ok(())
}

/// Print new events as they arrive until the thread stops running. Polling keeps
/// the CLI simple; the desktop app uses the WebSocket instead.
async fn watch_thread(c: &Client, thread: &str, mut after: i64) -> Result<()> {
    loop {
        let events = c.get(&format!("/v1/threads/{thread}/events?after={after}")).await?;
        for e in events.as_array().into_iter().flatten() {
            after = e["seq"].as_i64().unwrap_or(after);
            if let Some(line) = describe(&e["kind"]) {
                println!("{line}");
            }
        }
        let t = c.get(&format!("/v1/threads/{thread}")).await?;
        if !matches!(s(&t["status"]), "running" | "healing") {
            println!(
                "-- {} · {} in / {} out tokens · ${:.4}",
                s(&t["status"]),
                t["input_tokens"],
                t["output_tokens"],
                t["cost_usd"].as_f64().unwrap_or(0.0)
            );
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
}

fn describe(k: &Value) -> Option<String> {
    let one_line = |s: &str, n: usize| {
        let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if flat.chars().count() > n { format!("{}…", flat.chars().take(n).collect::<String>()) } else { flat }
    };
    Some(match s(&k["type"]) {
        "user_message" => format!("> {}", s(&k["text"])),
        "agent" => {
            let e = &k["event"];
            match s(&e["type"]) {
                "message" => s(&e["text"]).to_owned(),
                "tool_call" => format!("  → {} {}", s(&e["name"]), one_line(&e["input"].to_string(), 100)),
                "tool_result" if e["is_error"] == true => format!("  ✗ {}", one_line(s(&e["output"]), 100)),
                "error" => format!("! {}", s(&e["message"])),
                _ => return None,
            }
        }
        "notice" => format!("! {}", s(&k["text"])),
        _ => return None,
    })
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
