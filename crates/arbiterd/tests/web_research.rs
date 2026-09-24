//! Local web research with a fake web and a fake local model: off until the
//! user allows it, private questions refused, at most two pages read, and the
//! result is a short summary whose sources and quotes come from those pages.
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const RESULTS: &str = r#"
<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fvite.dev%2Fconfig%2Fserver-options&amp;rut=1">Server Options | Vite</a>
<a class="result__snippet" href="x">server.proxy configures custom proxy rules.</a>
<a class="result__a" href="https://blog.example.com/vite-proxy">A blog post</a>
<a class="result__a" href="https://127.0.0.1/internal">Internal</a>"#;
const PAGE: &str = "<html><script>steal()</script><h1>server.proxy</h1><p>Configure custom proxy rules for the dev server. Expects an object of { key: options } pairs.</p></html>";

#[tokio::test]
async fn research_is_local_bounded_and_cited() {
    let home = tempfile::tempdir().unwrap();
    let fetched = Arc::new(Mutex::new(Vec::<String>::new()));
    let prompts = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.launcher = Arc::new(|_, _| panic!("research must not start a frontier agent"));
    let log = fetched.clone();
    cfg.web_transport = Some(Arc::new(move |kind, arg| {
        log.lock().unwrap().push(format!("{kind}:{arg}"));
        Box::pin(async move { Ok(if kind == "search" { RESULTS.to_owned() } else { PAGE.to_owned() }) })
    }));
    let seen = prompts.clone();
    cfg.local_generator = Some(Arc::new(move |_, prompt, _, schema| {
        seen.lock().unwrap().push(prompt.clone());
        Box::pin(async move {
            Ok(if schema["properties"].get("urls").is_some() {
                json!({"urls":["https://vite.dev/config/server-options","https://evil.example/not-in-results"]})
            } else {
                json!({"summary":"Use server.proxy with { key: options } pairs.","sources":["https://vite.dev/config/server-options"],"limitations":"Check your Vite version.","citations":[{"source":"https://vite.dev/config/server-options","quote":"Configure custom proxy rules for the dev server."}]})
            }
            .to_string())
        })
    }));
    let daemon = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    let post = |p: &str| client.post(format!("{base}{p}")).bearer_auth(&daemon.token);
    let get = |p: &str| client.get(format!("{base}{p}")).bearer_auth(&daemon.token);
    let ask = async |q: &str| post("/v1/research").json(&json!({"question":q})).send().await.unwrap();

    // Off by default.
    let r = ask("How do I configure the Vite dev server proxy?").await;
    assert!(r.text().await.unwrap().contains("Turn it on in Settings"));
    let p: Value = get("/v1/setup").send().await.unwrap().json().await.unwrap();
    let mut p = p["preferences"].clone();
    p["web_research_enabled"] = json!(true);
    p["documentation_model"] = json!("fixture");
    post("/v1/setup").json(&p).send().await.unwrap().error_for_status().unwrap();

    // Private questions never leave the computer.
    let r = ask("why does C:\\Users\\me\\app\\server.js fail").await;
    assert!(r.text().await.unwrap().contains("looks private"));
    assert!(fetched.lock().unwrap().is_empty());

    let brief: Value = ask("How do I configure the Vite dev server proxy?").await.json().await.unwrap();
    assert_eq!(brief["summary"], "Use server.proxy with { key: options } pairs.", "{brief}");
    assert_eq!(brief["sources"], json!(["https://vite.dev/config/server-options"]));
    // Only the listed result was read; the invented URL and the local one were not.
    // (The short fixture page also gets a render attempt, which fails and
    // falls back to the fetched text.)
    let calls: Vec<String> = fetched.lock().unwrap().iter().filter(|c| !c.starts_with("render:")).cloned().collect();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[1], "fetch:https://vite.dev/config/server-options");
    // The local model saw page text without scripts; the answer is compact.
    let last = prompts.lock().unwrap().last().cloned().unwrap();
    assert!(last.contains("Configure custom proxy rules") && !last.contains("steal()"));
    assert!(brief.to_string().len() < 1500);
    daemon.handle.abort();
}
