//! Needs an installed Chromium; skipped (with a note) when none is found.

use arbiter_browser::{Browser, find_browser, page};
use axum::Router;
use axum::response::Html;
use axum::routing::get;
use std::sync::Arc;
use std::time::Duration;

const PAGE: &str = r#"<!doctype html><html><head><title>Demo</title></head><body>
<main><h1>Checkout</h1><button id="pay" class="btn primary" aria-label="Pay now">Pay</button>
<img src="/missing.png" alt="logo"></main>
<script src="/app.js"></script></body></html>"#;
const APP: &str = "console.error('cart total is NaN');\nfunction boom() { throw new TypeError('price.toFixed is not a function'); }\nboom();\n";

async fn serve() -> String {
    let app = Router::new()
        .route("/", get(|| async { Html(PAGE) }))
        .route("/app.js", get(|| async { ([("content-type", "application/javascript")], APP) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://127.0.0.1:{}", addr.port())
}

#[tokio::test]
async fn persistent_preview_inspects_types_clicks_and_bounds_output() {
    let Some(exe) = find_browser() else {
        eprintln!("no Chromium browser found; skipping");
        return;
    };
    let html = r#"<!doctype html><html lang="en"><head><title>Form</title></head><body>
      <label for="name">Name</label><input id="name"><button id="save" data-source="src/Form.tsx:12" onclick="document.querySelector('#result').textContent=document.querySelector('#name').value">Save</button><p id="result"></p>
      <img src="data:,x"><button id="unnamed"></button><div style="height:1000px"></div><button id="bottom">Bottom</button></body></html>"#;
    let app = Router::new().route("/", get(move || async move { Html(html) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let browser = Arc::new(Browser::launch(&exe).await.unwrap());
    let mut page = arbiter_browser::session::PageSession::open(browser.clone(), &url).await.unwrap();
    let d = page.inspect(Some("#save"), None).await.unwrap();
    assert_eq!(d["source"], "src/Form.tsx:12");
    assert!(d["descriptor"].as_str().unwrap().len() <= 1100);
    assert_eq!(d["styles"].as_object().unwrap().len(), 10);
    let x = d["box"]["x"].as_f64().unwrap() + 2.0;
    let y = d["box"]["y"].as_f64().unwrap() + 2.0;
    assert_eq!(page.inspect(None, Some((x, y))).await.unwrap()["selector"], "#save");
    page.type_text("#name", "Ada").await.unwrap();
    page.click(Some("#save"), None).await.unwrap();
    assert!(page.query("#result", 1).await.unwrap()[0].as_str().unwrap().contains("Ada"));
    assert!(page.a11y().await.unwrap().contains("Save"));
    assert_eq!(&page.screenshot(None, 768).await.unwrap()[..2], &[0xff, 0xd8]);
    let before = page.frame(640).await.unwrap();
    assert!(page.scroll(700.0).await.unwrap()["scrollY"].as_f64().unwrap() > 0.0);
    assert_ne!(before, page.frame(640).await.unwrap(), "preview follows the scrolled viewport");
    assert_eq!(&page.screenshot(Some("#bottom"), 768).await.unwrap()[..2], &[0xff, 0xd8]);
    assert!(page.goto("//example.com").await.is_err());
    assert!(page.inspect(Some("["), None).await.is_err());
    let report = page::check(&browser, &url, Duration::from_millis(100)).await.unwrap();
    assert!(report.issues.iter().any(|i| i.kind == "a11y" && i.message.contains("alternative text")));
    assert!(report.issues.iter().any(|i| i.kind == "a11y" && i.message.contains("accessible name")));
    for (width, height) in [(1280, 800), (768, 1024), (390, 844)] {
        page.resize(arbiter_browser::session::Viewport { width, height }).await.unwrap();
        page.scroll(-700.0).await.unwrap();
        let jpeg = page.frame(width).await.unwrap();
        let image = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((image.width(), image.height()), (width, height));
        let d = page.inspect(Some("#save"), None).await.unwrap();
        let x = d["box"]["x"].as_f64().unwrap() + 2.0;
        let y = d["box"]["y"].as_f64().unwrap() + 2.0;
        page.type_text("#name", &format!("{width}x{height}")).await.unwrap();
        page.click(None, Some((x, y))).await.unwrap();
        assert!(page.query("#result", 1).await.unwrap()[0].as_str().unwrap().contains(&format!("{width}x{height}")));
        assert!(page.inspect(None, Some((width as f64 + 1.0, 20.0))).await.is_err());
    }
    assert!(page.resize(arbiter_browser::session::Viewport { width: 0, height: 800 }).await.is_err());
    page.close().await;
    server.abort();
}

#[tokio::test]
async fn reports_errors_queries_dom_and_a11y() {
    let Some(exe) = find_browser() else {
        eprintln!("no Chromium browser found; skipping");
        return;
    };
    let origin = serve().await;
    let b = Browser::launch(&exe).await.unwrap();

    let r = page::check(&b, &format!("{origin}/"), Duration::from_millis(500)).await.unwrap();
    assert_eq!(r.title, "Demo");
    let kinds: Vec<&str> = r.issues.iter().map(|i| i.kind.as_str()).collect();
    assert!(kinds.contains(&"exception"), "{:#?}", r.issues);
    assert!(kinds.contains(&"console"), "{:#?}", r.issues);
    assert!(kinds.contains(&"http"), "404 image: {:#?}", r.issues);
    let ex = r.issues.iter().find(|i| i.kind == "exception").unwrap();
    assert!(ex.message.contains("price.toFixed"), "{ex:?}");
    assert_eq!((ex.source_path(&origin).as_deref(), ex.line), (Some("app.js"), Some(2)));

    let els = page::dom_query(&b, &format!("{origin}/"), "button", 5).await.unwrap();
    assert_eq!(els.len(), 1);
    assert!(
        els[0].contains("<button.btn.primary>")
            && els[0].contains("aria-label=\"Pay now\"")
            && els[0].contains("\"Pay\""),
        "{els:?}"
    );

    let tree = page::a11y_snapshot(&b, &format!("{origin}/"), 50).await.unwrap();
    assert!(tree.contains("heading \"Checkout\"") && tree.contains("button \"Pay now\""), "{tree}");
    assert!(tree.len() < 1500, "compact: {} chars", tree.len());

    let shot = page::screenshot(&b, &format!("{origin}/"), Some("#pay"), 768).await.unwrap();
    assert_eq!(&shot[..2], &[0xFF, 0xD8], "JPEG");
}

#[tokio::test]
async fn renders_script_text_on_a_kept_profile_and_forgets_cookies() {
    let Some(exe) = find_browser() else {
        eprintln!("no Chromium browser found; skipping");
        return;
    };
    let html = r#"<!doctype html><html><body><div id="root"></div><script>
      document.cookie = "session=abc; max-age=3600";
      document.getElementById("root").textContent = "Built by script: " + "hello";
    </script></body></html>"#;
    let app = Router::new().route("/", get(move || async move { Html(html) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let profile = tempfile::tempdir().unwrap();
    let browser = Browser::launch_in(&exe, Some(profile.path())).await.unwrap();
    let (at, text) = page::read_text(&browser, &url, 100).await.unwrap();
    assert_eq!(at, url);
    assert_eq!(text, "Built by script: hello");
    assert!(std::fs::read_dir(profile.path()).unwrap().next().is_some(), "profile kept on disk");
    let cookies = || async {
        browser.call(None, "Storage.getCookies", serde_json::json!({})).await.unwrap()["cookies"]
            .as_array()
            .unwrap()
            .len()
    };
    assert_eq!(cookies().await, 1);
    assert_eq!(page::forget_cookies(&browser, "127.0.0.1").await.unwrap(), 1);
    assert_eq!(cookies().await, 0);
    server.abort();
}
