//! What can run here: installed harnesses and their models, for the picker and
//! the router. Codex publishes a live model list over app-server; Claude Code
//! has no list API, so it gets its documented aliases.

use crate::{Harness, resolve};
use serde::Serialize;
use serde_json::{Value, json};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Clone, Debug, Serialize)]
pub struct HarnessInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub installed: bool,
    pub models: Vec<ModelInfo>,
    /// Why the model list may be incomplete (e.g. not logged in).
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelInfo {
    /// Value passed to the harness (`--model` / `model`); `None` = harness default.
    pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
}

const CLAUDE_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

pub fn installed(h: Harness) -> bool {
    resolve(match h {
        Harness::Claude => "claude",
        Harness::Codex => "codex",
    })
    .is_ok()
}

pub async fn catalog() -> Vec<HarnessInfo> {
    let (claude, codex) = tokio::join!(claude_info(), codex_info());
    vec![claude, codex]
}

async fn claude_info() -> HarnessInfo {
    let efforts: Vec<String> = CLAUDE_EFFORTS.iter().map(|s| s.to_string()).collect();
    let model = |id: Option<&str>, name: &str, description: &str| ModelInfo {
        id: id.map(str::to_owned),
        name: name.to_owned(),
        description: description.to_owned(),
        efforts: efforts.clone(),
        default_effort: None,
    };
    HarnessInfo {
        id: "claude",
        name: "Claude Code",
        installed: installed(Harness::Claude),
        models: vec![
            model(None, "Default", "Whatever your Claude Code settings choose"),
            model(Some("fable"), "Fable", "Most capable"),
            model(Some("opus"), "Opus", "Deep reasoning for hard problems"),
            model(Some("sonnet"), "Sonnet", "Fast and strong for everyday coding"),
            model(Some("haiku"), "Haiku", "Fastest and cheapest"),
        ],
        note: None,
    }
}

async fn codex_info() -> HarnessInfo {
    let mut info = HarnessInfo {
        id: "codex",
        name: "Codex",
        installed: installed(Harness::Codex),
        models: Vec::new(),
        note: None,
    };
    if !info.installed {
        return info;
    }
    let default = ModelInfo {
        id: None,
        name: "Default".into(),
        description: "Whatever your Codex config chooses".into(),
        efforts: Vec::new(),
        default_effort: None,
    };
    match tokio::time::timeout(Duration::from_secs(20), codex_models()).await {
        Ok(Ok(models)) => info.models = std::iter::once(default).chain(models).collect(),
        Ok(Err(e)) => {
            info.models = vec![default];
            info.note = Some(format!("couldn't list models: {e:#}"));
        }
        Err(_) => {
            info.models = vec![default];
            info.note = Some("timed out listing models".into());
        }
    }
    info
}

/// One short-lived app-server session: initialize, `model/list`, exit.
async fn codex_models() -> anyhow::Result<Vec<ModelInfo>> {
    let mut cmd = resolve("codex")?;
    cmd.arg("app-server").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().expect("piped");
    let mut lines = BufReader::new(child.stdout.take().expect("piped")).lines();
    let send = |v: Value| format!("{v}\n");
    stdin
        .write_all(
            send(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": { "clientInfo": { "name": "arbiter", "version": env!("CARGO_PKG_VERSION") }, "capabilities": null } }))
            .as_bytes(),
        )
        .await?;
    while let Some(line) = lines.next_line().await? {
        let Ok(m) = serde_json::from_str::<Value>(&line) else { continue };
        if m["id"] == 1 && m.get("method").is_none() {
            let msgs = send(json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }))
                + &send(json!({ "jsonrpc": "2.0", "id": 2, "method": "model/list", "params": {} }));
            stdin.write_all(msgs.as_bytes()).await?;
        } else if m["id"] == 2 && m.get("method").is_none() {
            let _ = child.kill().await;
            if let Some(e) = m.get("error") {
                anyhow::bail!("{}", e["message"].as_str().unwrap_or("model/list failed"));
            }
            return Ok(parse_models(&m["result"]));
        }
    }
    anyhow::bail!("codex app-server exited before answering")
}

fn parse_models(result: &Value) -> Vec<ModelInfo> {
    result["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| !m["hidden"].as_bool().unwrap_or(false))
        .filter_map(|m| {
            let id = m["model"].as_str().or(m["id"].as_str())?.to_owned();
            Some(ModelInfo {
                name: m["displayName"].as_str().unwrap_or(&id).to_owned(),
                description: m["description"].as_str().unwrap_or_default().to_owned(),
                efforts: m["supportedReasoningEfforts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e["reasoningEffort"].as_str().map(str::to_owned))
                    .collect(),
                default_effort: m["defaultReasoningEffort"].as_str().map(str::to_owned),
                id: Some(id),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codex_model_list() {
        let r = json!({ "data": [
            { "id": "a", "model": "gpt-x", "displayName": "GPT-X", "description": "d", "hidden": false,
              "supportedReasoningEfforts": [{ "reasoningEffort": "low", "description": "" }, { "reasoningEffort": "high", "description": "" }],
              "defaultReasoningEffort": "high", "isDefault": true },
            { "id": "b", "model": "secret", "displayName": "S", "hidden": true, "supportedReasoningEfforts": [] }
        ], "nextCursor": null });
        let m = parse_models(&r);
        assert_eq!(m.len(), 1);
        assert_eq!(
            (m[0].id.as_deref(), m[0].efforts.len(), m[0].default_effort.as_deref()),
            (Some("gpt-x"), 2, Some("high"))
        );
    }
}
