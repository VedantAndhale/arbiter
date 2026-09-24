//! Tools the user needs, checked and installed from inside Arbiter. Every
//! install is an explicit, approved action and runs as a background job.
use crate::AppState;
use anyhow::{Context, Result, ensure};
use arbiter_core::NoWindow;
use arbiter_supervisor::prereq::{self, Tool};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Default, Clone)]
pub(crate) struct Job {
    running: bool,
    ok: Option<bool>,
    message: String,
}

pub(crate) type Jobs = HashMap<&'static str, Job>;

async fn git_config(key: &str) -> Option<String> {
    let out = tokio::process::Command::new("git")
        .no_window()
        .args(["config", "--global", "--get", key])
        .output()
        .await
        .ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !v.is_empty()).then_some(v)
}

impl AppState {
    pub(crate) async fn prerequisites(&self) -> Result<Value> {
        let (g, n, p, c, x) = tokio::join!(
            prereq::version(Tool::Git),
            prereq::version(Tool::Node),
            prereq::version(Tool::Python),
            prereq::version(Tool::ClaudeCode),
            prereq::version(Tool::Codex)
        );
        let versions = [g, n, p, c, x];
        let jobs = self.inner.prereq_jobs.lock().unwrap().clone();
        // Signed in means the account check saw a usable login for that CLI.
        let accounts = self.inner.accounts.lock().unwrap().clone();
        let signed_in = |id: &str| {
            accounts.iter().any(|a| a.harness == id && a.installed && matches!(a.auth.as_str(), "subscription" | "api"))
        };
        let prefs = self.inner.setup.lock().unwrap().read()?;
        let now = arbiter_adapters::account::now();
        // Plan, allowance and whether Arbiter may use it, for the agent rows.
        let account = |id: &str| -> Value {
            let Some(a) = accounts.iter().find(|a| a.harness == id) else { return Value::Null };
            let allowance: Vec<String> = a
                .windows
                .iter()
                .map(|w| {
                    let left = (100.0 - w.used_percent).clamp(0.0, 100.0);
                    let span = match w.duration_mins {
                        Some(m) if m >= 10_000 => "this week",
                        Some(m) if m >= 1_400 => "today",
                        Some(_) => "this window",
                        None => "",
                    };
                    let reset = w
                        .resets_at
                        .map(|r| format!(", resets {}", crate::prerequisites::when(r, now)))
                        .unwrap_or_default();
                    format!("{left:.0}% left {span}{reset}").replace("  ", " ")
                })
                .collect();
            json!({
                "checked": true,
                "plan": a.plan, "auth": a.auth,
                "allowance": allowance,
                "blocked": arbiter_setup::admission(&prefs, a, now).err(),
                "note": a.note,
            })
        };
        let tools: Vec<Value> = prereq::ALL
            .iter()
            .zip(versions)
            .map(|(t, version)| {
                let job = jobs.get(t.id());
                json!({
                    "id":t.id(),"name":t.name(),"why":t.why(),"required":t.required(),
                    "installed":version.is_some(),"version":version,
                    "can_install":t.install_command().is_some(),"manual_url":t.manual_url(),"sign_in":t.sign_in().is_some(),"signed_in":t.sign_in().is_some() && signed_in(t.id()),"account":if t.sign_in().is_some() { account(t.id()) } else { Value::Null },
                    "job":job.map(|j| json!({"running":j.running,"ok":j.ok,"message":j.message})),
                })
            })
            .collect();
        let identity = json!({"name":git_config("user.name").await,"email":git_config("user.email").await});
        Ok(json!({"tools":tools,"git_identity":identity}))
    }

    pub(crate) async fn install_prerequisite(&self, id: &str) -> Result<()> {
        let tool = Tool::parse(id).context("unknown tool")?;
        ensure!(
            self.inner.setup.lock().unwrap().read()?.network,
            "Installing needs network access; turn it on in Settings"
        );
        // Installed first: that answer is right on every system, including
        // ones where Arbiter has no installer for the tool.
        ensure!(prereq::version(tool).await.is_none(), "{} is already installed", tool.name());
        ensure!(
            tool.install_command().is_some(),
            "Arbiter cannot install {} here; use {}",
            tool.name(),
            tool.manual_url()
        );
        if matches!(tool, Tool::ClaudeCode | Tool::Codex) {
            ensure!(
                prereq::version(Tool::Node).await.is_some(),
                "Install Node.js first; {} is installed with it",
                tool.name()
            );
        }
        {
            let mut jobs = self.inner.prereq_jobs.lock().unwrap();
            ensure!(!jobs.values().any(|j| j.running), "Another install is running; wait for it to finish");
            jobs.insert(tool.id(), Job { running: true, ok: None, message: format!("Installing {}…", tool.name()) });
        }
        let state = self.clone();
        tokio::spawn(async move {
            let result = prereq::install(tool).await;
            let job = match result {
                Ok(_) => Job {
                    running: false,
                    ok: Some(true),
                    message: format!(
                        "{} is installed. You may need to restart Arbiter for it to be found.",
                        tool.name()
                    ),
                },
                Err(e) => Job { running: false, ok: Some(false), message: e.chars().take(600).collect() },
            };
            state.inner.prereq_jobs.lock().unwrap().insert(tool.id(), job);
        });
        Ok(())
    }

    pub(crate) fn sign_in(&self, id: &str) -> Result<()> {
        let tool = Tool::parse(id).context("unknown tool")?;
        let cmd = tool.sign_in().context("this tool has no sign-in")?;
        prereq::open_terminal(cmd).context("could not open a terminal window")?;
        Ok(())
    }

    pub(crate) async fn set_git_identity(&self, name: &str, email: &str) -> Result<Value> {
        let (name, email) = (name.trim(), email.trim());
        ensure!(!name.is_empty() && name.len() <= 100 && !name.contains(['\n', '\r']), "enter your name");
        ensure!(
            email.len() <= 200 && email.contains('@') && !email.contains([' ', '\n', '\r']),
            "enter a valid email address"
        );
        for (key, value) in [("user.name", name), ("user.email", email)] {
            let out = tokio::process::Command::new("git")
                .no_window()
                .args(["config", "--global", key, value])
                .output()
                .await?;
            ensure!(out.status.success(), "git could not save {key}");
        }
        Ok(json!({"name":name,"email":email}))
    }
}

/// "in 3 days", "in 5 hours" or "soon", for allowance resets.
pub(crate) fn when(at: i64, now: i64) -> String {
    let d = at - now;
    if d <= 0 {
        "now".into()
    } else if d >= 86_400 {
        format!("in {} day{}", d / 86_400, if d / 86_400 == 1 { "" } else { "s" })
    } else if d >= 3_600 {
        format!("in {} hour{}", d / 3_600, if d / 3_600 == 1 { "" } else { "s" })
    } else {
        "within the hour".into()
    }
}
