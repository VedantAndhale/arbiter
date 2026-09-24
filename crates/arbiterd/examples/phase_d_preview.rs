//! Persistent fake-agent fixture for browser verification and process-kill recovery.
//! cargo run -p arbiterd --example phase_d_preview -- <home> <repo> [port] [delay-ms]
use arbiter_adapters::{Command, HarnessEvent, RunHandle, TurnOutcome};
use arbiter_core::AgentEvent;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let home = PathBuf::from(args.next().expect("home"));
    let repo = args.next().expect("fixture repo");
    let port = args.next().map(|s| s.parse()).transpose()?.unwrap_or(7441);
    let delay = args.next().map(|s| s.parse()).transpose()?.unwrap_or(8000);
    let mut cfg = arbiterd::Config::new(home.clone(), port);
    cfg.available = Arc::new(|_| true);
    cfg.heal = false;
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.launcher = Arc::new(move |_, opts| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (events, ev) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(command) = rx.recv().await {
                match command {
                    Command::Send(text) => {
                        let id = text
                            .lines()
                            .find_map(|line| line.strip_prefix("Step ").and_then(|s| s.split(':').next()))
                            .unwrap_or("a");
                        let _ = events.send(HarnessEvent::Agent(AgentEvent::Message {
                            text: format!("Working on step {id} in its isolated worktree (local test agent)."),
                        }));
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                        let (file, contents) = match id {
                            "a" => (
                                "catalog.ts",
                                "export const products = [{ id: 'desk', name: 'Oak desk', price: 240 }];\n",
                            ),
                            "b" => (
                                "cart.ts",
                                "export const cartTotal = (prices: number[]) => prices.reduce((sum, price) => sum + price, 0);\n",
                            ),
                            _ => (
                                "checkout.ts",
                                "import { products } from './catalog';\nimport { cartTotal } from './cart';\nexport const checkoutTotal = cartTotal(products.map(p => p.price));\n",
                            ),
                        };
                        std::fs::write(opts.cwd.join(file), contents).unwrap();
                        let _ = events.send(HarnessEvent::Agent(AgentEvent::Message { text: json!({"handoff":{"summary":format!("Implemented {file} and verified the fixture check."),"files":[file],"decisions":["Pure functions; no external services"],"interfaces":[format!("Public exports in {file}")],"open_issues":[]}}).to_string() }));
                        let _ = events.send(HarnessEvent::TurnDone(TurnOutcome::Completed));
                    }
                    Command::Shutdown => break,
                    Command::Interrupt => {
                        let _ = events.send(HarnessEvent::TurnDone(TurnOutcome::Interrupted));
                    }
                    Command::Approve { .. } => {}
                }
            }
        });
        Ok(RunHandle::from_channels(tx, ev))
    });
    let daemon = arbiterd::start(cfg).await?;
    let http = reqwest::Client::new();
    let base = format!("http://{}", daemon.addr);
    let threads: Vec<Value> =
        http.get(format!("{base}/v1/threads")).bearer_auth(&daemon.token).send().await?.json().await?;
    if threads.is_empty() {
        let project: Value = http
            .post(format!("{base}/v1/projects"))
            .bearer_auth(&daemon.token)
            .json(&json!({"path":repo}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let root: Value = http
            .post(format!("{base}/v1/threads"))
            .bearer_auth(&daemon.token)
            .json(&json!({"project_id":project["id"],"title":"Build a checkout flow","worktree":false}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let id = root["id"].as_str().unwrap();
        let plan = json!({"title":"Build a checkout flow","goal":"Create a product catalog and cart, then connect them in a checkout module. Review the combined changes before landing.","concurrency":3,"budget_usd":5.0,"nodes":[
            {"id":"a","title":"Product catalog","goal":"Define the typed product catalog.","scope":["catalog.ts"],"may_read":["**"],"non_goals":["No payment provider"],"checks":["git diff --check"],"harness":"claude","model_reason":"Small, isolated implementation"},
            {"id":"b","title":"Cart calculations","goal":"Add a pure cart total function.","scope":["cart.ts"],"may_read":["**"],"checks":["git diff --check"],"harness":"codex","model_reason":"Independent calculation module"},
            {"id":"c","title":"Connect checkout","goal":"Connect the catalog and cart functions after both integrate.","scope":["checkout.ts"],"may_read":["**"],"dependencies":["a","b"],"checks":["git diff --check"],"harness":"auto","model_reason":"Uses the verified dependency handoffs"}
        ]});
        http.patch(format!("{base}/v1/threads/{id}/plan"))
            .bearer_auth(&daemon.token)
            .json(&plan)
            .send()
            .await?
            .error_for_status()?;
    }
    println!("Phase D fixture ready on {}. Discovery: {}", daemon.addr, home.join("daemon.json").display());
    tokio::signal::ctrl_c().await?;
    Ok(())
}
