//! Download/benchmark public local models without starting a coding harness.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("model directory");
    let id = args.next().expect("potion or granite (lfm requires license acceptance in UI)");
    let manager = arbiter_intake::models::Manager::new(root.into());
    manager.install(&id, false)?;
    let mut last = String::new();
    loop {
        let status = manager.status();
        let row = status["models"].as_array().unwrap().iter().find(|m| m["model"]["id"] == id).unwrap();
        let progress = &row["progress"];
        let label = format!(
            "{} {}MB",
            progress["phase"].as_str().unwrap_or("pending"),
            progress["downloaded"].as_u64().unwrap_or(0) / 1_000_000
        );
        if label != last {
            println!("{label}");
            last = label;
        }
        if progress["phase"] == "failed" || (progress["phase"] == "ready" && !progress["error"].is_null()) {
            anyhow::bail!("{}", progress["error"]);
        }
        if progress["phase"] == "ready" && !row["benchmark"].is_null() {
            println!("{}", row["benchmark"]);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    Ok(())
}
