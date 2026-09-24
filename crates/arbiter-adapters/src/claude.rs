//! Claude Code over `claude -p --input-format stream-json --output-format stream-json`.
//!
//! One process serves many turns: each user message is a JSON line on stdin,
//! and each turn ends with a `result` line. `total_cost_usd` in `result` is
//! cumulative for the process, so we emit deltas; `usage` is per turn.

use crate::{Codec, Command, HarnessEvent, StartOpts, TurnOutcome, clip, resolve};
use arbiter_core::{AgentEvent, PermissionMode, Usage};
use serde_json::{Value, json};

pub(crate) fn command(opts: &StartOpts) -> anyhow::Result<tokio::process::Command> {
    let mut c = resolve("claude")?;
    c.env_remove("CONTEXT7_API_KEY");
    c.current_dir(&opts.cwd)
        .args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"])
        .args(["--permission-mode", permission_mode(opts.permission)])
        .args(["--permission-prompt-tool", "stdio"]);
    if let Some(m) = &opts.model {
        c.args(["--model", m]);
    }
    if let Some(e) = &opts.effort {
        c.args(["--effort", e]);
    }
    if let Some(r) = &opts.resume {
        c.args(["--resume", &r.session_id]);
        if r.fork {
            c.arg("--fork-session");
        }
    }
    c.args(lean_args(opts.lean, opts.tool_profile));
    for d in &opts.read_dirs {
        c.arg("--add-dir").arg(d);
    }
    if let Some(m) = &opts.mcp {
        let env: serde_json::Map<String, Value> = m.env.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
        let cfg = json!({ "mcpServers": { "arbiter": { "command": m.command, "args": [], "env": env } } });
        c.args(["--mcp-config", &cfg.to_string()]);
        // Arbiter's bounded local tools are pre-approved in supervised mode.
        c.args(["--allowedTools", "mcp__arbiter"]);
    }
    Ok(c)
}

/// Lean mode: measured on a trivial turn, a default session carries ~34k
/// context tokens (36 tools, user plugins/skills/MCP); this profile carries
/// ~13k. Project settings and CLAUDE.md still load; user-level extras don't.
/// Web tools stay because research needs them.
pub(crate) fn lean_args(lean: bool, profile: arbiter_core::ToolProfile) -> Vec<&'static str> {
    if !lean {
        return Vec::new();
    }
    vec![
        "--setting-sources",
        "project",
        "--strict-mcp-config",
        "--disable-slash-commands",
        "--tools",
        if profile == arbiter_core::ToolProfile::Implementation {
            "Read,Edit,Write,Glob,Grep,Bash"
        } else {
            "Read,Edit,Write,Glob,Grep,Bash,WebSearch,WebFetch"
        },
    ]
}

fn permission_mode(p: PermissionMode) -> &'static str {
    match p {
        PermissionMode::Safe => "acceptEdits",
        PermissionMode::Auto => "bypassPermissions",
        PermissionMode::Plan => "plan",
    }
}

pub struct ClaudeCodec {
    permission: PermissionMode,
    last_total_cost: f64,
    interrupting: bool,
    next_request: u64,
    approvals: std::collections::HashMap<String, Value>,
}

impl ClaudeCodec {
    pub fn new(permission: PermissionMode) -> Self {
        Self { permission, last_total_cost: 0.0, interrupting: false, next_request: 1, approvals: Default::default() }
    }
}

impl Codec for ClaudeCodec {
    fn on_command(&mut self, cmd: &Command) -> Vec<String> {
        match cmd {
            Command::Approve { request_id, allowed } => {
                let Some(input) = self.approvals.remove(request_id) else {
                    return vec![];
                };
                let response = if *allowed {
                    json!({"behavior":"allow","updatedInput":input})
                } else {
                    json!({"behavior":"deny","message":"Denied by the user in Arbiter."})
                };
                vec![json!({"type":"control_response","response":{"subtype":"success","request_id":request_id,"response":response}}).to_string()]
            }
            Command::Send(text) => {
                vec![json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "text", "text": text }] } }).to_string()]
            }
            Command::Interrupt => {
                self.interrupting = true;
                let id = format!("arb-{}", self.next_request);
                self.next_request += 1;
                vec![
                    json!({ "type": "control_request", "request_id": id, "request": { "subtype": "interrupt" } })
                        .to_string(),
                ]
            }
            Command::Shutdown => Vec::new(),
        }
    }

    fn on_line(&mut self, line: &str) -> (Vec<HarnessEvent>, Vec<String>) {
        let Ok(m) = serde_json::from_str::<Value>(line) else {
            return (Vec::new(), Vec::new());
        };
        let mut out = Vec::new();
        match m["type"].as_str() {
            Some("control_request") => {
                let Some(id) = m["request_id"].as_str().filter(|s| !s.is_empty() && s.len() <= 160) else {
                    return (vec![], vec![]);
                };
                let req = &m["request"];
                if req["subtype"] == "can_use_tool"
                    && req["input"].is_object()
                    && req["input"].to_string().len() <= 16000
                    && self.approvals.len() < 8
                    && !self.approvals.contains_key(id)
                {
                    self.approvals.insert(id.into(), req["input"].clone());
                    out.push(HarnessEvent::Approval {
                        request_id: id.into(),
                        tool: req["tool_name"].as_str().unwrap_or("tool").chars().take(100).collect(),
                        input: req["input"].clone(),
                    });
                } else {
                    return (vec![], vec![json!({"type":"control_response","response":{"subtype":"error","request_id":id,"error":"Unsupported or oversized control request"}}).to_string()]);
                }
            }
            Some("system") if m["subtype"] == "init" => {
                if let Some(s) = m["session_id"].as_str() {
                    out.push(HarnessEvent::Session(s.to_owned()));
                }
            }
            Some("assistant") => {
                for block in m["message"]["content"].as_array().into_iter().flatten() {
                    match block["type"].as_str() {
                        Some("text") => {
                            let text = block["text"].as_str().unwrap_or_default();
                            if !text.trim().is_empty() {
                                out.push(HarnessEvent::Agent(AgentEvent::Message { text: text.to_owned() }));
                            }
                        }
                        Some("tool_use") => out.push(HarnessEvent::Agent(AgentEvent::ToolCall {
                            id: block["id"].as_str().unwrap_or_default().to_owned(),
                            name: block["name"].as_str().unwrap_or("tool").to_owned(),
                            input: block["input"].clone(),
                        })),
                        _ => {} // thinking, images: not surfaced
                    }
                }
            }
            Some("user") => {
                for block in m["message"]["content"].as_array().into_iter().flatten() {
                    if block["type"] == "tool_result" {
                        out.push(HarnessEvent::Agent(AgentEvent::ToolResult {
                            id: block["tool_use_id"].as_str().unwrap_or_default().to_owned(),
                            output: clip(&tool_result_text(&block["content"])),
                            is_error: block["is_error"].as_bool().unwrap_or(false),
                        }));
                    }
                }
            }
            Some("result") => {
                self.approvals.clear();
                let u = &m["usage"];
                let total = m["total_cost_usd"].as_f64().unwrap_or(self.last_total_cost);
                let usage = Usage {
                    input_tokens: u["input_tokens"].as_u64().unwrap_or(0)
                        + u["cache_creation_input_tokens"].as_u64().unwrap_or(0),
                    output_tokens: u["output_tokens"].as_u64().unwrap_or(0),
                    cache_read_tokens: u["cache_read_input_tokens"].as_u64().unwrap_or(0),
                    cost_usd: (total - self.last_total_cost).max(0.0),
                };
                self.last_total_cost = total;
                if usage != Usage::default() {
                    out.push(HarnessEvent::Agent(AgentEvent::Usage(usage)));
                }
                for d in m["permission_denials"].as_array().into_iter().flatten() {
                    let tool = d["tool_name"].as_str().unwrap_or("tool");
                    let hint = match self.permission {
                        PermissionMode::Auto => "",
                        _ => " (thread is in safe mode; switch to auto to allow)",
                    };
                    out.push(HarnessEvent::Agent(AgentEvent::Error {
                        message: format!("Permission denied: {tool}{hint}"),
                    }));
                }
                let outcome = if std::mem::take(&mut self.interrupting) {
                    TurnOutcome::Interrupted
                } else if m["subtype"] == "success" && !m["is_error"].as_bool().unwrap_or(false) {
                    TurnOutcome::Completed
                } else {
                    let msg = m["result"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| m["subtype"].as_str().unwrap_or("error").to_owned());
                    TurnOutcome::Failed(msg)
                };
                out.push(HarnessEvent::TurnDone(outcome));
            }
            Some("rate_limit_event") => {
                let info = &m["rate_limit_info"];
                if info["status"] == "rejected" {
                    out.push(HarnessEvent::RateLimited { resets_at: info["resetsAt"].as_i64() });
                }
            }
            _ => {} // summaries, control responses
        }
        (out, Vec::new())
    }
}

fn tool_result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replay(fixture: &str, codec: &mut ClaudeCodec) -> Vec<HarnessEvent> {
        fixture.lines().flat_map(|l| codec.on_line(l).0).collect()
    }

    #[test]
    fn tool_turn_maps_to_normalized_events() {
        let mut c = ClaudeCodec::new(PermissionMode::Auto);
        let ev = replay(include_str!("../tests/fixtures/claude_tool_turn.jsonl"), &mut c);

        assert!(matches!(&ev[0], HarnessEvent::Session(s) if s.len() == 36));
        let call = ev.iter().find_map(|e| match e {
            HarnessEvent::Agent(AgentEvent::ToolCall { name, input, id }) => Some((name, input, id)),
            _ => None,
        });
        let (name, input, call_id) = call.expect("tool call");
        assert_eq!(name, "Bash");
        assert_eq!(input["command"], "echo hi");
        assert!(ev.iter().any(|e| matches!(e,
            HarnessEvent::Agent(AgentEvent::ToolResult { id, output, is_error: false }) if id == call_id && output == "hi")));
        assert!(ev.iter().any(|e| matches!(e, HarnessEvent::Agent(AgentEvent::Message { text }) if text == "ok")));

        let usage = ev.iter().find_map(|e| match e {
            HarnessEvent::Agent(AgentEvent::Usage(u)) => Some(*u),
            _ => None,
        });
        let u = usage.expect("usage");
        assert!(u.cost_usd > 0.0 && u.output_tokens > 0 && u.cache_read_tokens > 0);
        assert_eq!(ev.last(), Some(&HarnessEvent::TurnDone(TurnOutcome::Completed)));
    }

    #[test]
    fn cumulative_cost_becomes_per_turn_deltas_and_interrupt_is_recognized() {
        let mut c = ClaudeCodec::new(PermissionMode::Safe);
        let fixture = include_str!("../tests/fixtures/claude_multi_turn_interrupt.jsonl");
        let mut outcomes = Vec::new();
        let mut costs = Vec::new();
        for line in fixture.lines() {
            for e in c.on_line(line).0 {
                match e {
                    HarnessEvent::TurnDone(o) => {
                        outcomes.push(o);
                        // The recording interrupted the second turn right after it began.
                        if outcomes.len() == 1 {
                            c.on_command(&Command::Interrupt);
                        }
                    }
                    HarnessEvent::Agent(AgentEvent::Usage(u)) => costs.push(u.cost_usd),
                    _ => {}
                }
            }
        }
        assert_eq!(outcomes, [TurnOutcome::Completed, TurnOutcome::Interrupted, TurnOutcome::Completed]);
        // Turn 2 was interrupted before spending anything, so only two usage events.
        assert_eq!(costs.len(), 2);
        assert!((costs.iter().sum::<f64>() - 0.0161211).abs() < 1e-9, "deltas sum to the final total: {costs:?}");
    }

    #[test]
    fn encodes_user_messages_and_interrupts() {
        let mut c = ClaudeCodec::new(PermissionMode::Safe);
        let msg: Value = serde_json::from_str(&c.on_command(&Command::Send("hi".into()))[0]).unwrap();
        assert_eq!(msg["message"]["content"][0]["text"], "hi");
        let int: Value = serde_json::from_str(&c.on_command(&Command::Interrupt)[0]).unwrap();
        assert_eq!(int["request"]["subtype"], "interrupt");
    }

    #[test]
    fn approval_waits_for_a_matching_explicit_decision() {
        let mut c = ClaudeCodec::new(PermissionMode::Safe);
        let (events, replies) = c.on_line(include_str!("../tests/fixtures/claude_approval.jsonl"));
        assert!(replies.is_empty());
        assert!(
            matches!(&events[0], HarnessEvent::Approval { request_id, tool, .. } if request_id == "approval-1" && tool == "Bash")
        );
        assert!(c.on_command(&Command::Approve { request_id: "other".into(), allowed: true }).is_empty());
        let reply = c.on_command(&Command::Approve { request_id: "approval-1".into(), allowed: true });
        let value: Value = serde_json::from_str(&reply[0]).unwrap();
        assert_eq!(value["response"]["response"]["updatedInput"]["command"], "echo approved");
        assert!(c.on_command(&Command::Approve { request_id: "approval-1".into(), allowed: true }).is_empty());
        c.on_line(include_str!("../tests/fixtures/claude_approval.jsonl"));
        let reply = c.on_command(&Command::Approve { request_id: "approval-1".into(), allowed: false });
        assert!(reply[0].contains("deny"));
    }

    #[test]
    fn permission_denials_surface_as_errors() {
        let mut c = ClaudeCodec::new(PermissionMode::Safe);
        let line = r#"{"type":"result","subtype":"success","is_error":false,"total_cost_usd":0.01,"usage":{"input_tokens":1,"output_tokens":1},"permission_denials":[{"tool_name":"Bash","tool_use_id":"t1","tool_input":{}}]}"#;
        let (ev, _) = c.on_line(line);
        assert!(ev.iter().any(|e| matches!(e, HarnessEvent::Agent(AgentEvent::Error { message }) if message.contains("Bash") && message.contains("safe mode"))));
    }
}

#[cfg(test)]
mod lean_tests {
    use super::*;

    #[test]
    fn lean_keeps_core_and_web_tools_only() {
        assert!(lean_args(false, arbiter_core::ToolProfile::Research).is_empty());
        let a = lean_args(true, arbiter_core::ToolProfile::Research);
        let tools = a[a.iter().position(|x| *x == "--tools").unwrap() + 1];
        assert_eq!(tools, "Read,Edit,Write,Glob,Grep,Bash,WebSearch,WebFetch");
        assert!(a.contains(&"--strict-mcp-config") && a.contains(&"--disable-slash-commands"));
        assert!(!lean_args(true, arbiter_core::ToolProfile::Implementation).join(" ").contains("WebSearch"));
    }
}

#[cfg(test)]
mod rate_limit_tests {
    use super::*;

    #[test]
    fn rejected_rate_limit_is_surfaced() {
        let mut c = ClaudeCodec::new(PermissionMode::Safe);
        let ok = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1790157000}}"#;
        assert!(c.on_line(ok).0.is_empty());
        let rejected = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1790157000}}"#;
        assert_eq!(c.on_line(rejected).0, [HarnessEvent::RateLimited { resets_at: Some(1790157000) }]);
    }
}
