//! Read-only account probes. Never starts a thread, sends a prompt or buys credits.
use crate::{Harness, catalog, resolve};
use arbiter_setup::{Account, Window};
use serde_json::{Value, json};
use std::{
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

pub fn environment_conflict(h: Harness) -> bool {
    let names: &[&str] = match h {
        Harness::Claude => &[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
        ],
        Harness::Codex => &["OPENAI_API_KEY", "CODEX_API_KEY", "OPENAI_BASE_URL"],
    };
    names.iter().any(|n| std::env::var_os(n).is_some_and(|v| !v.is_empty()))
}

pub async fn probe(h: Harness) -> Account {
    let id = match h {
        Harness::Claude => "claude",
        Harness::Codex => "codex",
    };
    let mut account = Account {
        harness: id.into(),
        installed: catalog::installed(h),
        auth: "unknown".into(),
        source: format!("{id} CLI"),
        observed_at: now(),
        conflict: environment_conflict(h),
        ..Default::default()
    };
    if !account.installed {
        account.note = "Harness not installed".into();
        return account;
    }
    account.version = tokio::time::timeout(Duration::from_secs(3), version(id)).await.ok().and_then(Result::ok);
    let result = tokio::time::timeout(Duration::from_secs(12), async {
        match h {
            Harness::Claude => claude().await,
            Harness::Codex => codex().await,
        }
    })
    .await;
    match result {
        Ok(Ok(mut value)) => {
            value.version = account.version;
            value.conflict = account.conflict;
            value.observed_at = now();
            value
        }
        _ => {
            account.note =
                "Status unavailable or unsupported. Update/sign in through the official CLI and refresh.".into();
            account
        }
    }
}

async fn version(id: &str) -> anyhow::Result<String> {
    let mut command = resolve(id)?;
    command.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    let mut owned = arbiter_supervisor::SupervisedChild::spawn(command)?;
    let child = &mut owned.child;
    let mut bytes = Vec::new();
    child.stdout.take().unwrap().take(1024).read_to_end(&mut bytes).await?;
    let _ = child.kill().await;
    let _ = child.wait().await;
    let text = String::from_utf8_lossy(&bytes);
    text.split_whitespace()
        .find(|s| {
            s.len() < 64
                && s.starts_with(|c: char| c.is_ascii_digit())
                && s.contains('.')
                && s.chars().all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c))
        })
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("version unavailable"))
}

async fn claude() -> anyhow::Result<Account> {
    let mut cmd = resolve("claude")?;
    cmd.args(["auth", "status"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    let mut owned = arbiter_supervisor::SupervisedChild::spawn(cmd)?;
    let child = &mut owned.child;
    let mut bytes = Vec::new();
    child.stdout.take().unwrap().take(65537).read_to_end(&mut bytes).await?;
    anyhow::ensure!(bytes.len() <= 65536, "oversized status");
    let _ = child.wait().await?;
    let v: Value = serde_json::from_slice(&bytes)?;
    Ok(parse_claude(&v))
}

pub fn parse_claude(v: &Value) -> Account {
    let logged = v["loggedIn"].as_bool();
    let method = v["authMethod"].as_str().unwrap_or("");
    let auth = if logged == Some(false) {
        "signed_out"
    } else if logged == Some(true) && method == "claude.ai" {
        "subscription"
    } else if logged == Some(true) && matches!(method, "api_key" | "apiKey" | "console") {
        "api"
    } else {
        "unknown"
    };
    Account {
        harness: "claude".into(),
        installed: true,
        auth: auth.into(),
        plan: safe_plan(v["subscriptionType"].as_str()),
        source: "claude auth status".into(),
        note: "Claude's CLI does not report remaining allowance; Claude enforces your plan's limits.".into(),
        ..Default::default()
    }
}

async fn codex() -> anyhow::Result<Account> {
    let mut cmd = resolve("codex")?;
    cmd.arg("app-server").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    let mut owned = arbiter_supervisor::SupervisedChild::spawn(cmd)?;
    let child = &mut owned.child;
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap().take(262144)).lines();
    stdin.write_all(format!("{}\n",json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"arbiter-setup","version":env!("CARGO_PKG_VERSION")}}})).as_bytes()).await?;
    let mut auth = None;
    let mut limits = None;
    while let Some(line) = lines.next_line().await? {
        let v: Value = serde_json::from_str(&line)?;
        match v["id"].as_u64() {
            Some(1) => {
                anyhow::ensure!(v.get("error").is_none(), "initialize unavailable");
                for request in [
                    json!({"method":"initialized"}),
                    json!({"id":2,"method":"account/read","params":{"refreshToken":false}}),
                    json!({"id":3,"method":"account/rateLimits/read"}),
                ] {
                    stdin.write_all(format!("{request}\n").as_bytes()).await?;
                }
            }
            Some(2) => auth = Some(v["result"].clone()),
            Some(3) => limits = Some(v["result"].clone()),
            _ => {}
        }
        if let (Some(a), Some(l)) = (&auth, &limits) {
            let result = parse_codex(a, l);
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Ok(result);
        }
    }
    anyhow::bail!("status unavailable")
}
fn safe_plan(s: Option<&str>) -> Option<String> {
    s.filter(|s| s.len() <= 32 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .map(str::to_owned)
}
pub fn parse_codex(a: &Value, l: &Value) -> Account {
    let auth = match a["account"]["type"].as_str() {
        Some("chatgpt") => "subscription",
        Some("apiKey") => "api",
        _ => "unknown",
    };
    // Never substitute a different model bucket for the general Codex allowance.
    let bucket =
        if l["rateLimitsByLimitId"].is_object() { &l["rateLimitsByLimitId"]["codex"] } else { &l["rateLimits"] };
    let windows = ["primary", "secondary"]
        .iter()
        .filter_map(|key| {
            let w = &bucket[key];
            let used = w["usedPercent"].as_f64()?;
            if !used.is_finite() || !(0.0..=100.0).contains(&used) {
                return None;
            }
            Some(Window {
                used_percent: used,
                duration_mins: w["windowDurationMins"].as_u64(),
                resets_at: w["resetsAt"].as_i64(),
            })
        })
        .collect();
    Account {
        harness: "codex".into(),
        installed: true,
        auth: auth.into(),
        plan: safe_plan(a["account"]["planType"].as_str()),
        windows,
        credits_enabled: match (bucket["credits"]["hasCredits"].as_bool(), bucket["credits"]["unlimited"].as_bool()) {
            (Some(false), Some(false)) => Some(false),
            (Some(true), _) | (_, Some(true)) => Some(true),
            _ => None,
        },
        source: "codex account/read + account/rateLimits/read".into(),
        note: "Shared allowance: other apps on this account use it too.".into(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strips_identity_and_uses_general_bucket() {
        let a = json!({"account":{"type":"chatgpt","email":"secret@example.com","planType":"plus"}});
        let l = json!({"rateLimits":{"primary":{"usedPercent":0}},"rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":42,"resetsAt":999},"credits":{"hasCredits":false,"unlimited":false}},"other":{"primary":{"usedPercent":1}}}});
        let parsed = parse_codex(&a, &l);
        assert_eq!(parsed.windows[0].used_percent, 42.0);
        assert_eq!(parsed.credits_enabled, Some(false));
        assert!(!serde_json::to_string(&parsed).unwrap().contains("secret"));
        assert!(parse_codex(&a, &json!({})).windows.is_empty());
        assert_eq!(parse_codex(&a, &json!({})).credits_enabled, None);
    }
    #[test]
    fn claude_auth_is_not_quota_evidence() {
        let p = parse_claude(&json!({"loggedIn":true,"authMethod":"claude.ai","subscriptionType":"max"}));
        assert_eq!(p.auth, "subscription");
        assert!(p.windows.is_empty());
        assert_eq!(p.credits_enabled, None);
    }
}
