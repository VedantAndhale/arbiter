//! Page-level operations. Each opens a fresh tab, does one thing, and returns
//! compact text-shaped data; the tab is always closed.

use crate::cdp::Browser;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PageIssue {
    /// `exception` | `console` | `http` | `request` | `navigation`
    pub kind: String,
    pub message: String,
    /// Script or resource URL, when known.
    pub source: Option<String>,
    pub line: Option<u32>,
    pub col: Option<u32>,
}

impl PageIssue {
    /// Source as a repo-ish path: strips the dev server origin, query strings
    /// and Vite's `/@fs` prefix, e.g. `http://localhost:5173/src/App.tsx?t=1` → `src/App.tsx`.
    pub fn source_path(&self, origin: &str) -> Option<String> {
        let s = self.source.as_deref()?;
        let rest = s.strip_prefix(origin)?;
        let rest = rest.split(['?', '#']).next().unwrap_or(rest);
        let rest = rest.strip_prefix("/@fs").unwrap_or(rest).trim_start_matches('/');
        (!rest.is_empty()).then(|| rest.to_owned())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageReport {
    pub url: String,
    pub title: String,
    pub load_ms: u64,
    pub issues: Vec<PageIssue>,
}

fn frame_loc(v: &Value) -> (Option<String>, Option<u32>, Option<u32>) {
    let f = &v["stackTrace"]["callFrames"][0];
    let url = f["url"].as_str().or(v["url"].as_str()).filter(|u| !u.is_empty()).map(str::to_owned);
    let line = f["lineNumber"].as_u64().or(v["lineNumber"].as_u64()).map(|l| l as u32 + 1);
    let col = f["columnNumber"].as_u64().or(v["columnNumber"].as_u64()).map(|c| c as u32 + 1);
    (url, line, col)
}

fn first_line(s: &str) -> String {
    let l = s.lines().next().unwrap_or("").trim();
    l.chars().take(300).collect()
}

struct Tab<'a> {
    browser: &'a Browser,
    target: String,
    session: String,
}

impl<'a> Tab<'a> {
    async fn open(browser: &'a Browser) -> Result<Self> {
        let (target, session) = browser.new_tab().await?;
        Ok(Self { browser, target, session })
    }
    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        self.browser.call(Some(&self.session), method, params).await
    }
    async fn close(self) {
        self.browser.close_tab(&self.target).await;
    }
    /// Navigate and wait for `load` (plus `settle` for late errors/XHRs).
    async fn goto(&self, url: &str, settle: Duration) -> Result<(Vec<PageIssue>, u64)> {
        let mut rx = self.browser.subscribe();
        for d in ["Runtime.enable", "Log.enable", "Network.enable", "Page.enable"] {
            self.call(d, json!({})).await?;
        }
        let start = Instant::now();
        let nav = self.call("Page.navigate", json!({ "url": url })).await?;
        let mut issues = Vec::new();
        if let Some(e) = nav["errorText"].as_str() {
            issues.push(PageIssue {
                kind: "navigation".into(),
                message: format!("{e} loading {url}"),
                source: None,
                line: None,
                col: None,
            });
            return Ok((issues, start.elapsed().as_millis() as u64));
        }
        let mut requests: HashMap<String, String> = HashMap::new();
        let mut loaded_at: Option<Instant> = None;
        let hard_deadline = start + Duration::from_secs(20);
        loop {
            if issues.len() >= 64 {
                break;
            }
            let deadline = loaded_at.map(|t| t + settle).unwrap_or(hard_deadline).min(hard_deadline);
            let Ok(Ok(ev)) = tokio::time::timeout_at(deadline.into(), rx.recv()).await else { break };
            if ev["sessionId"].as_str() != Some(self.session.as_str()) {
                continue;
            }
            let p = &ev["params"];
            match ev["method"].as_str().unwrap_or("") {
                "Page.loadEventFired" => loaded_at = loaded_at.or(Some(Instant::now())),
                "Runtime.exceptionThrown" => {
                    let d = &p["exceptionDetails"];
                    let (source, line, col) = frame_loc(d);
                    let msg =
                        d["exception"]["description"].as_str().or(d["text"].as_str()).unwrap_or("Uncaught exception");
                    issues.push(PageIssue { kind: "exception".into(), message: first_line(msg), source, line, col });
                }
                "Runtime.consoleAPICalled" if matches!(p["type"].as_str(), Some("error" | "assert")) => {
                    let text = p["args"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|a| {
                            a["value"]
                                .as_str()
                                .map(str::to_owned)
                                .or_else(|| a["description"].as_str().map(first_line))
                                .unwrap_or_else(|| a["value"].to_string())
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    let (source, line, col) = frame_loc(p);
                    issues.push(PageIssue { kind: "console".into(), message: first_line(&text), source, line, col });
                }
                // Network problems are reported via Network.*; Log would duplicate them.
                "Log.entryAdded" if p["entry"]["level"] == "error" && p["entry"]["source"] != "network" => {
                    let e = &p["entry"];
                    issues.push(PageIssue {
                        kind: "console".into(),
                        message: first_line(e["text"].as_str().unwrap_or("")),
                        source: e["url"].as_str().map(str::to_owned),
                        line: e["lineNumber"].as_u64().map(|l| l as u32 + 1),
                        col: None,
                    });
                }
                "Network.requestWillBeSent" => {
                    if let (Some(id), Some(u)) = (p["requestId"].as_str(), p["request"]["url"].as_str())
                        && requests.len() < 2048
                    {
                        requests.insert(id.to_owned(), u.chars().take(400).collect());
                    }
                }
                "Network.responseReceived" => {
                    let status = p["response"]["status"].as_u64().unwrap_or(0);
                    let u = p["response"]["url"].as_str().unwrap_or("");
                    if status >= 400 && !u.ends_with("/favicon.ico") {
                        issues.push(PageIssue {
                            kind: "http".into(),
                            message: format!("HTTP {status} for {}", p["type"].as_str().unwrap_or("request")),
                            source: Some(u.to_owned()),
                            line: None,
                            col: None,
                        });
                    }
                }
                "Network.loadingFailed" if !p["canceled"].as_bool().unwrap_or(false) => {
                    let u = p["requestId"].as_str().and_then(|id| requests.get(id)).cloned();
                    let err = p["errorText"].as_str().unwrap_or("failed");
                    if err != "net::ERR_ABORTED" {
                        issues.push(PageIssue {
                            kind: "request".into(),
                            message: err.to_owned(),
                            source: u,
                            line: None,
                            col: None,
                        });
                    }
                }
                _ => {}
            }
        }
        let mut seen = std::collections::HashSet::new();
        for issue in &mut issues {
            issue.source = issue.source.as_ref().map(|s| s.chars().take(400).collect());
        }
        issues.retain(|i| seen.insert(i.clone()));
        Ok((issues, start.elapsed().as_millis() as u64))
    }
}

/// Load `url` and report runtime problems.
pub async fn check(browser: &Browser, url: &str, settle: Duration) -> Result<PageReport> {
    let tab = Tab::open(browser).await?;
    let result = async {
        let (mut issues, load_ms) = tab.goto(url, settle).await?;
        let a11y =
            tab.call("Runtime.evaluate", json!({"expression":include_str!("a11y.js"),"returnByValue":true})).await?;
        for message in a11y["result"]["value"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            issues.push(PageIssue {
                kind: "a11y".into(),
                message: message.into(),
                source: None,
                line: None,
                col: None,
            });
        }
        let title = tab
            .call("Runtime.evaluate", json!({ "expression": "document.title", "returnByValue": true }))
            .await
            .ok()
            .and_then(|v| v["result"]["value"].as_str().map(str::to_owned))
            .unwrap_or_default();
        Ok(PageReport { url: url.to_owned(), title, load_ms, issues })
    }
    .await;
    tab.close().await;
    result
}

/// Up to `limit` elements matching `selector`, one compact line each.
pub async fn dom_query(browser: &Browser, url: &str, selector: &str, limit: usize) -> Result<Vec<String>> {
    const JS: &str = r#"(sel, limit) => Array.from(document.querySelectorAll(sel)).slice(0, limit).map(e => {
      const r = e.getBoundingClientRect(), s = getComputedStyle(e);
      const attrs = ['id','role','aria-label','data-testid','href','type','name'].map(a => e.getAttribute(a) ? `${a}=${JSON.stringify(e.getAttribute(a).slice(0,60))}` : '').filter(Boolean);
      const cls = e.classList.length ? '.' + Array.from(e.classList).slice(0,3).join('.') : '';
      const text = (e.innerText || e.value || '').replace(/\s+/g,' ').trim().slice(0,100);
      const vis = r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none';
      return `<${e.tagName.toLowerCase()}${cls}> ${attrs.join(' ')} ${text ? JSON.stringify(text) : ''} [${Math.round(r.x)},${Math.round(r.y)} ${Math.round(r.width)}x${Math.round(r.height)}${vis ? '' : ' hidden'}]`.replace(/\s+/g,' ');
    })"#;
    let tab = Tab::open(browser).await?;
    let result = async {
        tab.goto(url, Duration::from_millis(500)).await?;
        let expr = format!("({JS})({}, {limit})", serde_json::to_string(selector)?);
        let v = tab.call("Runtime.evaluate", json!({ "expression": expr, "returnByValue": true })).await?;
        if let Some(e) = v["exceptionDetails"]["exception"]["description"].as_str() {
            anyhow::bail!("{}", first_line(e));
        }
        Ok(v["result"]["value"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect())
    }
    .await;
    tab.close().await;
    result
}

/// Delete the profile's cookies for `host` and its subdomains (signing out).
pub async fn forget_cookies(browser: &Browser, host: &str) -> Result<usize> {
    let all = browser.call(None, "Storage.getCookies", json!({})).await?;
    let tab = Tab::open(browser).await?;
    let result = async {
        let mut n = 0;
        for c in all["cookies"].as_array().into_iter().flatten() {
            let domain = c["domain"].as_str().unwrap_or("").trim_start_matches('.');
            if domain == host || domain.ends_with(&format!(".{host}")) || host.ends_with(&format!(".{domain}")) {
                tab.call(
                    "Network.deleteCookies",
                    json!({ "name": c["name"], "domain": c["domain"], "path": c["path"] }),
                )
                .await?;
                n += 1;
            }
        }
        Ok(n)
    }
    .await;
    tab.close().await;
    result
}

/// Visible text of a rendered page, for pages that build their content with
/// JavaScript. Bounded; nothing else about the page is returned.
/// Returns `(final_url, text)` so callers can check where redirects led.
pub async fn read_text(browser: &Browser, url: &str, max_chars: usize) -> Result<(String, String)> {
    let tab = Tab::open(browser).await?;
    let result = async {
        tab.goto(url, Duration::from_millis(1500)).await?;
        let v = tab
            .call(
                "Runtime.evaluate",
                json!({ "expression": "[location.href, document.body ? document.body.innerText : '']", "returnByValue": true }),
            )
            .await?;
        let at = v["result"]["value"][0].as_str().unwrap_or("").to_owned();
        let text = v["result"]["value"][1].as_str().unwrap_or("");
        Ok((at, text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max_chars).collect()))
    }
    .await;
    tab.close().await;
    result
}

/// Indented `role "name"` lines for meaningful accessibility nodes, capped.
pub async fn a11y_snapshot(browser: &Browser, url: &str, max_lines: usize) -> Result<String> {
    let tab = Tab::open(browser).await?;
    let result = async {
        tab.goto(url, Duration::from_millis(500)).await?;
        let tree = tab.call("Accessibility.getFullAXTree", json!({})).await?;
        let nodes = tree["nodes"].as_array().context("no AX nodes")?;
        let by_id: HashMap<&str, &Value> =
            nodes.iter().filter_map(|n| n["nodeId"].as_str().map(|id| (id, n))).collect();
        let mut out = Vec::new();
        fn walk<'v>(n: &'v Value, by_id: &HashMap<&str, &'v Value>, depth: usize, out: &mut Vec<String>, max: usize) {
            if out.len() >= max {
                return;
            }
            let role = n["role"]["value"].as_str().unwrap_or("");
            let name = n["name"]["value"].as_str().unwrap_or("").trim();
            let ignored = n["ignored"].as_bool().unwrap_or(false);
            let boring = matches!(role, "generic" | "none" | "StaticText" | "InlineTextBox" | "LineBreak" | "");
            let shown = !ignored && (!boring || (!name.is_empty() && role != "InlineTextBox" && role != "StaticText"));
            let next = if shown {
                let name: String = name.chars().take(80).collect();
                out.push(format!(
                    "{}{role}{}",
                    "  ".repeat(depth),
                    if name.is_empty() { String::new() } else { format!(" {name:?}") }
                ));
                depth + 1
            } else {
                depth
            };
            for c in n["childIds"].as_array().into_iter().flatten() {
                if let Some(child) = c.as_str().and_then(|id| by_id.get(id)) {
                    walk(child, by_id, next, out, max);
                }
            }
        }
        if let Some(root) = nodes.first() {
            walk(root, &by_id, 0, &mut out, max_lines);
        }
        if out.len() >= max_lines {
            out.push(format!("… (truncated at {max_lines} lines)"));
        }
        Ok(out.join("\n"))
    }
    .await;
    tab.close().await;
    result
}

/// JPEG screenshot (viewport, or one element when `selector` is set). Opt-in:
/// pixels are expensive; prefer `dom_query`/`a11y_snapshot`.
pub async fn screenshot(browser: &Browser, url: &str, selector: Option<&str>, width: u32) -> Result<Vec<u8>> {
    let tab = Tab::open(browser).await?;
    let result = async {
        let height = width * 5 / 8;
        tab.call("Emulation.setDeviceMetricsOverride", json!({ "width": width, "height": height, "deviceScaleFactor": 1, "mobile": false })).await?;
        tab.goto(url, Duration::from_millis(500)).await?;
        let mut params = json!({ "format": "jpeg", "quality": 60 });
        if let Some(sel) = selector {
            let expr = format!(
                "(() => {{ const e = document.querySelector({}); if (!e) return null; const r = e.getBoundingClientRect(); return {{x: r.x, y: r.y, width: Math.max(1, r.width), height: Math.max(1, r.height)}}; }})()",
                serde_json::to_string(sel)?
            );
            let v = tab.call("Runtime.evaluate", json!({ "expression": expr, "returnByValue": true })).await?;
            let r = &v["result"]["value"];
            if r.is_null() {
                anyhow::bail!("no element matches {sel}");
            }
            params["clip"] = json!({ "x": r["x"], "y": r["y"], "width": r["width"], "height": r["height"], "scale": 1 });
        }
        let shot = tab.call("Page.captureScreenshot", params).await?;
        let b64 = shot["data"].as_str().context("no screenshot data")?;
        decode_b64(b64)
    }
    .await;
    tab.close().await;
    result
}

pub(crate) fn decode_b64(s: &str) -> Result<Vec<u8>> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut map = [255u8; 256];
    for (i, c) in T.iter().enumerate() {
        map[*c as usize] = i as u8;
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes().filter(|c| *c != b'=' && !c.is_ascii_whitespace()) {
        let v = map[c as usize];
        anyhow::ensure!(v != 255, "invalid base64");
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_path_strips_origin_query_and_fs() {
        let i = |s: &str| PageIssue {
            kind: "x".into(),
            message: String::new(),
            source: Some(s.into()),
            line: None,
            col: None,
        };
        let o = "http://localhost:5173";
        assert_eq!(i("http://localhost:5173/src/App.tsx?t=17").source_path(o).as_deref(), Some("src/App.tsx"));
        assert_eq!(i("http://localhost:5173/@fs/C:/w/lib.ts").source_path(o).as_deref(), Some("C:/w/lib.ts"));
        assert_eq!(i("https://cdn.example.com/x.js").source_path(o), None);
    }

    #[test]
    fn base64() {
        assert_eq!(decode_b64("aGVsbG8=").unwrap(), b"hello");
    }
}
