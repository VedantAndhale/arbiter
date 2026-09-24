//! Isolated manual UI fixture. No real harness is launched or tokens spent.
//! cargo run -p arbiterd --example b2_preview -- <home> <repo>
use arbiter_adapters::{Command, HarnessEvent, RunHandle, TurnOutcome};
use arbiter_core::AgentEvent;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let home = std::path::PathBuf::from(args.next().expect("home directory"));
    let repo = args.next().expect("fixture repository");
    let port = args.next().map(|s| s.parse()).transpose()?.unwrap_or(7439);
    let mut cfg = arbiterd::Config::new(home.clone(), port);
    cfg.available = Arc::new(|_| true);
    cfg.heal = false;
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.launcher = Arc::new(|_, _| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (events, ev) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::Approve { allowed, .. } => {
                        let _ = events.send(HarnessEvent::Agent(AgentEvent::Message {
                            text: format!("Fixture approval: {allowed}"),
                        }));
                        let _ = events.send(HarnessEvent::TurnDone(TurnOutcome::Completed));
                    }
                    Command::Send(text) => {
                        let _ = events.send(HarnessEvent::Session("fixture".into()));
                        if text == "approval" {
                            let _ = events.send(HarnessEvent::Approval {
                                request_id: "fixture-approval".into(),
                                tool: "Bash".into(),
                                input: serde_json::json!({"command":"echo approved"}),
                            });
                            continue;
                        }
                        let _ = events.send(HarnessEvent::Agent(AgentEvent::Message {
                            text: format!("Fixture received:\n{text}"),
                        }));
                        let _ = events.send(HarnessEvent::TurnDone(TurnOutcome::Completed));
                    }
                    Command::Shutdown => break,
                    Command::Interrupt => {
                        let _ = events.send(HarnessEvent::TurnDone(TurnOutcome::Interrupted));
                    }
                }
            }
        });
        Ok(RunHandle::from_channels(tx, ev))
    });
    let daemon = arbiterd::start(cfg).await?;
    let http = reqwest::Client::new();
    let base = format!("http://{}", daemon.addr);
    let p: serde_json::Value = http
        .post(format!("{base}/v1/projects"))
        .bearer_auth(&daemon.token)
        .json(&serde_json::json!({"path":repo}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    http.post(format!("{base}/v1/threads")).bearer_auth(&daemon.token).json(&serde_json::json!({"project_id":p["id"],"title":"Checkout preview fixture","harness":"claude","worktree":false})).send().await?.error_for_status()?;
    println!("Fixture daemon ready on {}; discovery: {}", daemon.addr, home.join("daemon.json").display());
    tokio::signal::ctrl_c().await?;
    Ok(())
}
