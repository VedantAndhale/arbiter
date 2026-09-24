//! Codex over `codex app-server` (JSON-RPC 2.0, one message per line).
//!
//! Handshake: `initialize` → `initialized` → `thread/start` (or `thread/resume`
//! / `thread/fork`). Messages sent before the thread exists are queued. A
//! message during an active turn becomes `turn/steer`.
//! `thread/tokenUsage/updated` reports cumulative totals, so we emit deltas.

use crate::{Codec, Command, HarnessEvent, StartOpts, TurnOutcome, clip, resolve};
use arbiter_core::{AgentEvent, PermissionMode, Usage};
use serde_json::{Value, json};
use std::collections::HashMap;

const LEAN_DISABLE: [&str; 12] = [
    "apps",
    "browser_use",
    "browser_use_external",
    "computer_use",
    "image_generation",
    "plugins",
    "remote_plugin",
    "tool_suggest",
    "multi_agent",
    "goals",
    "in_app_browser",
    "skill_search",
];

pub(crate) fn command(opts: &StartOpts) -> anyhow::Result<tokio::process::Command> {
    let mut c = resolve("codex")?;
    c.env_remove("CONTEXT7_API_KEY");
    c.current_dir(&opts.cwd).arg("app-server");
    if opts.subscription_only {
        c.args(["-c", "forced_login_method=\"chatgpt\"", "-c", "model_provider=\"openai\""]);
    }
    if opts.lean {
        // Measured: 16.7k → 13.3k context tokens on a trivial turn. Arbiter does
        // the orchestrating, so Codex's own multi-agent and app tooling go.
        for f in LEAN_DISABLE {
            c.args(["--disable", f]);
        }
        c.args([
            "-c",
            if opts.tool_profile == arbiter_core::ToolProfile::Implementation {
                "web_search=\"disabled\""
            } else {
                "web_search=\"live\""
            },
        ]);
    }
    if let Some(m) = &opts.mcp {
        // TOML literal strings: no escaping needed for Windows paths.
        c.args(["-c", &format!("mcp_servers.arbiter.command='{}'", m.command.display())]);
        let env: Vec<String> = m.env.iter().map(|(k, v)| format!("{k} = '{v}'")).collect();
        c.args(["-c", &format!("mcp_servers.arbiter.env={{ {} }}", env.join(", "))]);
    }
    Ok(c)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Pending {
    Initialize,
    Thread,
    TurnStart,
    Other,
}

pub struct CodexCodec {
    cwd: String,
    permission: PermissionMode,
    model: Option<String>,
    effort: Option<String>,
    resume: Option<crate::Resume>,
    next_id: u64,
    pending: HashMap<u64, Pending>,
    thread_id: Option<String>,
    turn_id: Option<String>,
    queued: Vec<String>,
    interrupting: bool,
    last_total: Usage,
}

impl CodexCodec {
    pub fn new(opts: &StartOpts) -> Self {
        Self {
            cwd: opts.cwd.to_string_lossy().into_owned(),
            permission: opts.permission,
            model: opts.model.clone(),
            effort: opts.effort.clone(),
            resume: opts.resume.clone(),
            next_id: 1,
            pending: HashMap::new(),
            thread_id: None,
            turn_id: None,
            queued: Vec::new(),
            interrupting: false,
            last_total: Usage::default(),
        }
    }

    fn request(&mut self, kind: Pending, method: &str, params: Value) -> String {
        let id = self.next_id;
        self.next_id += 1;
        self.pending.insert(id, kind);
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
    }

    fn sandbox(&self) -> &'static str {
        match self.permission {
            PermissionMode::Safe => "workspace-write",
            PermissionMode::Auto => "danger-full-access",
            PermissionMode::Plan => "read-only",
        }
    }

    fn thread_request(&mut self) -> String {
        // Approvals are never requested: the sandbox is the policy. The
        // approvals inbox (with the risk decider) will switch this to on-request.
        let mut params = json!({
            "cwd": self.cwd,
            "approvalPolicy": "never",
            "sandbox": self.sandbox(),
        });
        if let Some(m) = &self.model {
            params["model"] = json!(m);
        }
        match self.resume.clone() {
            Some(r) if r.fork => {
                params["threadId"] = json!(r.session_id);
                self.request(Pending::Thread, "thread/fork", params)
            }
            Some(r) => {
                params["threadId"] = json!(r.session_id);
                params["excludeTurns"] = json!(true);
                self.request(Pending::Thread, "thread/resume", params)
            }
            None => self.request(Pending::Thread, "thread/start", params),
        }
    }

    fn user_turn(&mut self, text: &str) -> String {
        let thread_id = self.thread_id.clone().expect("thread ready");
        let input = json!([{ "type": "text", "text": text, "text_elements": [] }]);
        match self.turn_id.clone() {
            Some(turn) => self.request(
                Pending::Other,
                "turn/steer",
                json!({ "threadId": thread_id, "input": input, "expectedTurnId": turn }),
            ),
            None => {
                let mut params = json!({ "threadId": thread_id, "input": input });
                if let Some(e) = &self.effort {
                    params["effort"] = json!(e);
                }
                self.request(Pending::TurnStart, "turn/start", params)
            }
        }
    }

    fn on_response(&mut self, kind: Pending, m: &Value, out: &mut Vec<HarnessEvent>, replies: &mut Vec<String>) {
        if let Some(err) = m.get("error") {
            let message = err["message"].as_str().unwrap_or("codex request failed").to_owned();
            out.push(HarnessEvent::Agent(AgentEvent::Error { message: message.clone() }));
            if matches!(kind, Pending::Thread | Pending::TurnStart | Pending::Initialize) {
                out.push(HarnessEvent::TurnDone(TurnOutcome::Failed(message)));
            }
            return;
        }
        match kind {
            Pending::Initialize => {
                replies.push(json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }).to_string());
                replies.push(self.thread_request());
            }
            Pending::Thread => {
                if let Some(id) = m["result"]["thread"]["id"].as_str() {
                    self.thread_id = Some(id.to_owned());
                    out.push(HarnessEvent::Session(id.to_owned()));
                    for text in std::mem::take(&mut self.queued) {
                        replies.push(self.user_turn(&text));
                    }
                }
            }
            Pending::TurnStart => {
                if let Some(id) = m["result"]["turn"]["id"].as_str() {
                    self.turn_id = Some(id.to_owned());
                }
            }
            Pending::Other => {}
        }
    }

    fn on_notification(&mut self, method: &str, p: &Value, out: &mut Vec<HarnessEvent>) {
        match method {
            "turn/started" => {
                if let Some(id) = p["turn"]["id"].as_str() {
                    self.turn_id = Some(id.to_owned());
                }
            }
            "item/started" => {
                let item = &p["item"];
                let id = item["id"].as_str().unwrap_or_default().to_owned();
                let call = match item["type"].as_str() {
                    Some("commandExecution") => {
                        let cmd = item["commandActions"][0]["command"].as_str().or(item["command"].as_str());
                        Some(("shell".to_owned(), json!({ "command": cmd })))
                    }
                    Some("fileChange") => {
                        let files: Vec<&str> = item["changes"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|c| c["path"].as_str())
                            .collect();
                        Some(("edit".to_owned(), json!({ "files": files })))
                    }
                    Some("mcpToolCall") => Some((
                        format!(
                            "{}.{}",
                            item["server"].as_str().unwrap_or("mcp"),
                            item["tool"].as_str().unwrap_or("tool")
                        ),
                        item["arguments"].clone(),
                    )),
                    Some("webSearch") => Some(("web_search".to_owned(), json!({ "query": item["query"] }))),
                    _ => None,
                };
                if let Some((name, input)) = call {
                    out.push(HarnessEvent::Agent(AgentEvent::ToolCall { id, name, input }));
                }
            }
            "item/completed" => {
                let item = &p["item"];
                let id = item["id"].as_str().unwrap_or_default().to_owned();
                let result = match item["type"].as_str() {
                    Some("agentMessage") => {
                        let text = item["text"].as_str().unwrap_or_default().trim_end();
                        if !text.is_empty() {
                            out.push(HarnessEvent::Agent(AgentEvent::Message { text: text.to_owned() }));
                        }
                        None
                    }
                    Some("commandExecution") => Some((
                        item["aggregatedOutput"].as_str().unwrap_or_default().to_owned(),
                        item["exitCode"].as_i64() != Some(0),
                    )),
                    Some("fileChange") => {
                        let status = item["status"].as_str().unwrap_or("completed");
                        Some((status.to_owned(), status != "completed"))
                    }
                    Some("mcpToolCall") => match item["error"]["message"].as_str() {
                        Some(e) => Some((e.to_owned(), true)),
                        None => Some((item["result"]["content"].to_string(), false)),
                    },
                    _ => None,
                };
                if let Some((output, is_error)) = result {
                    out.push(HarnessEvent::Agent(AgentEvent::ToolResult { id, output: clip(&output), is_error }));
                }
            }
            "thread/tokenUsage/updated" => {
                let t = &p["tokenUsage"]["total"];
                let cached = t["cachedInputTokens"].as_u64().unwrap_or(0);
                let total = Usage {
                    input_tokens: t["inputTokens"].as_u64().unwrap_or(0).saturating_sub(cached),
                    output_tokens: t["outputTokens"].as_u64().unwrap_or(0),
                    cache_read_tokens: cached,
                    cost_usd: 0.0, // subscription usage; Codex reports no price
                };
                let delta = Usage {
                    input_tokens: total.input_tokens.saturating_sub(self.last_total.input_tokens),
                    output_tokens: total.output_tokens.saturating_sub(self.last_total.output_tokens),
                    cache_read_tokens: total.cache_read_tokens.saturating_sub(self.last_total.cache_read_tokens),
                    cost_usd: 0.0,
                };
                self.last_total = total;
                if delta != Usage::default() {
                    out.push(HarnessEvent::Agent(AgentEvent::Usage(delta)));
                }
            }
            "error" => {
                if !p["willRetry"].as_bool().unwrap_or(false) {
                    let message = p["error"]["message"].as_str().unwrap_or("codex error").to_owned();
                    out.push(HarnessEvent::Agent(AgentEvent::Error { message }));
                }
            }
            "turn/completed" => {
                self.turn_id = None;
                let turn = &p["turn"];
                let outcome = match turn["status"].as_str() {
                    _ if std::mem::take(&mut self.interrupting) => TurnOutcome::Interrupted,
                    Some("completed") => TurnOutcome::Completed,
                    Some("interrupted") => TurnOutcome::Interrupted,
                    _ => TurnOutcome::Failed(turn["error"]["message"].as_str().unwrap_or("turn failed").to_owned()),
                };
                out.push(HarnessEvent::TurnDone(outcome));
            }
            _ => {}
        }
    }
}

impl Codec for CodexCodec {
    fn on_start(&mut self) -> Vec<String> {
        vec![self.request(
            Pending::Initialize,
            "initialize",
            json!({ "clientInfo": { "name": "arbiter", "title": "Arbiter", "version": env!("CARGO_PKG_VERSION") }, "capabilities": null }),
        )]
    }

    fn on_command(&mut self, cmd: &Command) -> Vec<String> {
        match cmd {
            Command::Approve { .. } => Vec::new(),
            Command::Send(text) if self.thread_id.is_none() => {
                self.queued.push(text.clone());
                Vec::new()
            }
            Command::Send(text) => vec![self.user_turn(text)],
            Command::Interrupt => match (self.thread_id.clone(), self.turn_id.clone()) {
                (Some(thread), Some(turn)) => {
                    self.interrupting = true;
                    vec![self.request(Pending::Other, "turn/interrupt", json!({ "threadId": thread, "turnId": turn }))]
                }
                _ => Vec::new(),
            },
            Command::Shutdown => Vec::new(),
        }
    }

    fn on_line(&mut self, line: &str) -> (Vec<HarnessEvent>, Vec<String>) {
        let Ok(m) = serde_json::from_str::<Value>(line) else {
            return (Vec::new(), Vec::new());
        };
        let (mut out, mut replies) = (Vec::new(), Vec::new());
        // Ids may be numbers or strings; ours are always numbers.
        match (m.get("id").filter(|v| !v.is_null()), m["method"].as_str()) {
            // Response to one of our requests.
            (Some(id), None) => {
                if let Some(kind) = id.as_u64().and_then(|id| self.pending.remove(&id)) {
                    self.on_response(kind, &m, &mut out, &mut replies);
                }
            }
            // Server-initiated request (approvals etc.). With approvalPolicy=never
            // these should not arrive; refuse rather than hang the turn.
            (Some(_), Some(method)) => {
                out.push(HarnessEvent::Agent(AgentEvent::Error {
                    message: format!("declined codex request {method}"),
                }));
                replies.push(
                    json!({ "jsonrpc": "2.0", "id": m["id"], "error": { "code": -32601, "message": "not supported by Arbiter yet" } })
                        .to_string(),
                );
            }
            (None, Some(method)) => self.on_notification(method, &m["params"], &mut out),
            (None, None) => {}
        }
        (out, replies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn opts() -> StartOpts {
        StartOpts {
            subscription_only: false,
            cwd: PathBuf::from("C:\\work\\repo"),
            permission: PermissionMode::Safe,
            model: None,
            effort: Some("high".into()),
            lean: true,
            tool_profile: arbiter_core::ToolProfile::Research,
            resume: None,
            mcp: None,
            read_dirs: vec![],
        }
    }

    fn parse(line: &str) -> Value {
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn handshake_queues_messages_until_thread_exists() {
        let mut c = CodexCodec::new(&opts());
        let init = parse(&c.on_start()[0]);
        assert_eq!(init["method"], "initialize");
        assert!(c.on_command(&Command::Send("hello".into())).is_empty(), "queued before thread exists");

        let (_, replies) = c.on_line(r#"{"id":1,"result":{"userAgent":"x"}}"#);
        assert_eq!(parse(&replies[0])["method"], "initialized");
        let start = parse(&replies[1]);
        assert_eq!(start["method"], "thread/start");
        assert_eq!(start["params"]["sandbox"], "workspace-write");

        let (ev, replies) = c.on_line(r#"{"id":2,"result":{"thread":{"id":"th-1"}}}"#);
        assert_eq!(ev, [HarnessEvent::Session("th-1".into())]);
        let turn = parse(&replies[0]);
        assert_eq!(turn["method"], "turn/start");
        assert_eq!(turn["params"]["input"][0]["text"], "hello");

        // While a turn is active, a new message steers it.
        c.on_line(r#"{"id":3,"result":{"turn":{"id":"turn-1","status":"inProgress"}}}"#);
        let steer = parse(&c.on_command(&Command::Send("also this".into()))[0]);
        assert_eq!(steer["method"], "turn/steer");
        assert_eq!(steer["params"]["expectedTurnId"], "turn-1");
        let int = parse(&c.on_command(&Command::Interrupt)[0]);
        assert_eq!(int["method"], "turn/interrupt");
    }

    #[test]
    fn resume_and_fork_use_the_right_methods() {
        for (fork, method) in [(false, "thread/resume"), (true, "thread/fork")] {
            let mut o = opts();
            o.resume = Some(crate::Resume { session_id: "th-9".into(), fork });
            let mut c = CodexCodec::new(&o);
            c.on_start();
            let (_, replies) = c.on_line(r#"{"id":1,"result":{}}"#);
            let req = parse(&replies[1]);
            assert_eq!(req["method"], method);
            assert_eq!(req["params"]["threadId"], "th-9");
        }
    }

    #[test]
    fn recorded_session_maps_to_normalized_events() {
        let mut c = CodexCodec::new(&opts());
        c.on_start();
        c.on_command(&Command::Send("first".into()));
        let mut ev = Vec::new();
        for line in include_str!("../tests/fixtures/codex_two_turns.jsonl").lines() {
            ev.extend(c.on_line(line).0);
        }
        // The recording ends each turn after `ok` / `done`.
        let messages: Vec<&str> = ev
            .iter()
            .filter_map(|e| match e {
                HarnessEvent::Agent(AgentEvent::Message { text }) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(messages.contains(&"ok") && messages.contains(&"done"), "{messages:?}");

        assert!(ev.iter().any(|e| matches!(e,
            HarnessEvent::Agent(AgentEvent::ToolCall { name, input, .. }) if name == "shell" && input["command"] == "echo hi")));
        assert!(ev.iter().any(|e| matches!(e,
            HarnessEvent::Agent(AgentEvent::ToolResult { output, is_error: false, .. }) if output.trim() == "hi")));

        let turns = ev.iter().filter(|e| matches!(e, HarnessEvent::TurnDone(TurnOutcome::Completed))).count();
        assert_eq!(turns, 2);

        // Usage deltas add back up to the final cumulative total (59359 in, 39168 cached, 52 out).
        let sum = ev.iter().fold(Usage::default(), |mut acc, e| {
            if let HarnessEvent::Agent(AgentEvent::Usage(u)) = e {
                acc += *u;
            }
            acc
        });
        assert_eq!(
            (sum.input_tokens + sum.cache_read_tokens, sum.cache_read_tokens, sum.output_tokens),
            (59359, 39168, 52)
        );
    }

    #[test]
    fn server_requests_are_refused_not_ignored() {
        let mut c = CodexCodec::new(&opts());
        let (ev, replies) = c.on_line(r#"{"id":"srv-1","method":"item/commandExecution/requestApproval","params":{}}"#);
        assert!(matches!(&ev[0], HarnessEvent::Agent(AgentEvent::Error { .. })));
        assert_eq!(parse(&replies[0])["id"], "srv-1");
    }
}
