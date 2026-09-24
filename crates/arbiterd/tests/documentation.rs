use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn local_retrieval_never_launches_cloud_or_persists_responses() {
    let home = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut cfg = arbiterd::Config::new(home.path().into(), 0);
    cfg.fixture_accounts = Some(arbiterd::fixture_accounts());
    cfg.launcher = Arc::new(|_, _| panic!("documentation must not launch a frontier harness"));
    let recorded = calls.clone();
    cfg.documentation_transport = Some(Arc::new(move |request| {
        recorded.lock().unwrap().push(request.clone());
        Box::pin(async move {
            Ok(match request["method"].as_str().unwrap() {
                "initialize" => json!({"protocolVersion":"2025-03-26"}),
                "notifications/initialized" => Value::Null,
                "tools/list" => json!({"tools":[{"name":"resolve-library-id"},{"name":"query-docs"}]}),
                "tools/call" if request["params"]["name"] == "resolve-library-id" => {
                    json!({"content":[{"type":"text","text":"Library: /facebook/react"}]})
                }
                "tools/call" => {
                    json!({"content":[{"type":"text","text":"Effect cleanup runs before reruns. https://react.dev/reference/react/useEffect"}]})
                }
                _ => panic!("unexpected MCP method"),
            })
        })
    }));
    cfg.local_generator = Some(Arc::new(|_, _, _, schema| {
        Box::pin(async move {
            Ok(if schema["properties"].get("library_id").is_some() {json!({"library_id":"/facebook/react"})}
        else if schema["properties"].get("summary").is_some() {json!({"summary":"Cleanup runs before reruns.","limitations":"Confirm your installed version.","sources":["https://react.dev/reference/react/useEffect"]})}
        else {json!({"message":"Save your connection choices, then test Context7."})}.to_string())
        })
    }));
    let daemon = arbiterd::start(cfg).await.unwrap();
    let base = format!("http://{}", daemon.addr);
    let client = reqwest::Client::new();
    let post = |p: &str| client.post(format!("{base}{p}")).bearer_auth(&daemon.token);
    let state: Value =
        client.get(format!("{base}/v1/setup")).bearer_auth(&daemon.token).send().await.unwrap().json().await.unwrap();
    let mut prefs = state["preferences"].clone();
    prefs["context7_enabled"] = json!(true);
    prefs["documentation_model"] = json!("fixture");
    let saved: Value = post("/v1/setup").json(&prefs).send().await.unwrap().json().await.unwrap();
    let guidance = post("/v1/documentation/setup").json(&json!({"test":false})).send().await.unwrap();
    assert!(guidance.status().is_success());
    assert!(calls.lock().unwrap().is_empty());
    assert!(post("/v1/documentation/setup").json(&json!({"test":true})).send().await.unwrap().status().is_success());
    let query = json!({"library":"react","topic":"Effect cleanup"});
    let response = post("/v1/documentation/query").json(&query).send().await.unwrap();
    assert!(response.status().is_success(), "{}", response.text().await.unwrap());
    let brief: Value = post("/v1/documentation/query").json(&query).send().await.unwrap().json().await.unwrap();
    assert_eq!(brief["persistent_cache"], false);
    assert_eq!(brief["library_id"], "/facebook/react");
    assert!(!home.path().join("docs-cache").exists());
    assert_eq!(
        calls.lock().unwrap().iter().filter(|c| c["params"]["name"] == "query-docs").count(),
        2,
        "each request must fetch live"
    );
    let count = calls.lock().unwrap().len();
    assert!(
        !post("/v1/documentation/query")
            .json(&json!({"library":"react","topic":"token=private"}))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    prefs = saved["preferences"].clone();
    prefs["network"] = json!(false);
    assert!(post("/v1/setup").json(&prefs).send().await.unwrap().status().is_success());
    assert!(!post("/v1/documentation/query").json(&query).send().await.unwrap().status().is_success());
    assert_eq!(count, calls.lock().unwrap().len());
    assert_eq!(client.post(format!("{base}/v1/documentation/query")).json(&query).send().await.unwrap().status(), 401);
    daemon.handle.abort();
}
