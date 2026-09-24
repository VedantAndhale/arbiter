//! A persistent page for preview and agent interactions. The daemon serializes
//! access per thread; state survives calls (typing then clicking, for example).
use crate::Browser;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}
impl Default for Viewport {
    fn default() -> Self {
        Self { width: 1280, height: 800 }
    }
}
impl Viewport {
    pub fn validate(self) -> Result<Self> {
        ensure!(
            (320..=2560).contains(&self.width) && (240..=1600).contains(&self.height),
            "viewport must be 320-2560 pixels wide and 240-1600 pixels high"
        );
        Ok(self)
    }
}
pub struct PageSession {
    viewport: Viewport,
    browser: Arc<Browser>,
    target: String,
    session: String,
    origin: String,
}

impl PageSession {
    pub fn belongs_to(&self, browser: &Arc<Browser>, origin: &str) -> bool {
        Arc::ptr_eq(&self.browser, browser) && self.origin == origin
    }
    pub async fn open(browser: Arc<Browser>, origin: &str) -> Result<Self> {
        let (target, session) = browser.new_tab().await?;
        let page = Self { viewport: Viewport::default(), browser, target, session, origin: origin.to_owned() };
        if let Err(e) = page.initialize().await {
            page.close().await;
            return Err(e);
        }
        Ok(page)
    }

    async fn initialize(&self) -> Result<()> {
        self.call("Page.enable", json!({})).await?;
        self.call("Runtime.enable", json!({})).await?;
        self.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":self.viewport.width,"height":self.viewport.height,"deviceScaleFactor":1,"mobile":false}),
        )
        .await?;
        self.goto("/").await?;
        Ok(())
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }
    pub async fn resize(&mut self, viewport: Viewport) -> Result<()> {
        viewport.validate()?;
        self.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":viewport.width,"height":viewport.height,"deviceScaleFactor":1,"mobile":false}),
        )
        .await?;
        self.viewport = viewport;
        // Wait for layout/paint before mapping coordinates or capturing pixels.
        self.eval(
            "new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true))))".into(),
        )
        .await?;
        Ok(())
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        self.browser.call(Some(&self.session), method, params).await
    }

    async fn eval(&self, expression: String) -> Result<Value> {
        let v = self
            .call("Runtime.evaluate", json!({"expression":expression,"returnByValue":true,"awaitPromise":true}))
            .await?;
        if v.get("exceptionDetails").is_some() {
            anyhow::bail!(
                "page operation failed: {}",
                v["exceptionDetails"]["exception"]["description"]
                    .as_str()
                    .unwrap_or("JavaScript error")
                    .chars()
                    .take(250)
                    .collect::<String>()
            );
        }
        Ok(v["result"]["value"].clone())
    }

    pub async fn goto(&self, route: &str) -> Result<Value> {
        ensure!(
            route.starts_with('/') && !route.starts_with("//") && !route.contains('\\'),
            "use a local route beginning with /"
        );
        let r = self.call("Page.navigate", json!({"url":format!("{}{route}",self.origin)})).await?;
        ensure!(r.get("errorText").is_none(), "navigation failed: {}", r["errorText"]);
        // Bounded readiness wait; a slow page reports an error instead of hanging.
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if self.eval("document.readyState === 'complete'".into()).await.unwrap_or(Value::Bool(false)) == true {
                return self.location().await;
            }
        }
        anyhow::bail!("page did not finish loading within 5 seconds")
    }

    async fn local(&self) -> Result<()> {
        let origin = self.eval("location.origin".into()).await?;
        ensure!(
            origin.as_str() == Some(&self.origin),
            "page left the local dev server; navigate to a local route to continue"
        );
        Ok(())
    }

    pub async fn location(&self) -> Result<Value> {
        self.local().await?;
        self.eval("({route:location.pathname+location.search+location.hash,title:document.title.slice(0,120)})".into())
            .await
    }

    pub async fn frame(&self, width: u32) -> Result<Vec<u8>> {
        self.local().await?;
        let offset = self.eval("({x:scrollX,y:scrollY})".into()).await?;
        let shot = self.call("Page.captureScreenshot", json!({"format":"jpeg","quality":65,"clip":{"x":offset["x"],"y":offset["y"],"width":self.viewport.width,"height":self.viewport.height,"scale":(width.clamp(320,2560) as f64/self.viewport.width as f64).min(1.0)}})).await?;
        crate::page::decode_b64(shot["data"].as_str().context("no screenshot data")?)
    }

    pub async fn inspect(&self, selector: Option<&str>, point: Option<(f64, f64)>) -> Result<Value> {
        self.local().await?;
        let expression = match (selector, point) {
            (Some(s), _) => format!("document.querySelector({})", serde_json::to_string(s)?),
            (_, Some((x, y))) => {
                ensure!(
                    x.is_finite()
                        && y.is_finite()
                        && (0.0..self.viewport.width as f64).contains(&x)
                        && (0.0..self.viewport.height as f64).contains(&y),
                    "point outside preview"
                );
                format!("document.elementFromPoint({x},{y})")
            }
            _ => anyhow::bail!("provide a selector or preview coordinates"),
        };
        self.eval(format!("({})({expression})", include_str!("inspect.js"))).await
    }

    pub async fn query(&self, selector: &str, limit: usize) -> Result<Value> {
        self.local().await?;
        self.eval(format!(
            "Array.from(document.querySelectorAll({})).slice(0,{}).map(e=>({})(e).descriptor)",
            serde_json::to_string(selector)?,
            limit.min(5),
            include_str!("inspect.js")
        ))
        .await
    }

    pub async fn a11y(&self) -> Result<String> {
        self.local().await?;
        let v = self.call("Accessibility.getFullAXTree", json!({})).await?;
        let mut out = String::new();
        for n in v["nodes"].as_array().into_iter().flatten() {
            let role = n["role"]["value"].as_str().unwrap_or("");
            if n["ignored"] == true || matches!(role, "generic" | "none" | "StaticText" | "InlineTextBox" | "") {
                continue;
            }
            let name: String = n["name"]["value"].as_str().unwrap_or("").chars().take(80).collect();
            let line = format!("{role} {name:?}\n");
            if out.len() + line.len() > 5000 {
                out.push_str("… truncated");
                break;
            }
            out.push_str(&line);
        }
        Ok(out)
    }

    pub async fn screenshot(&self, selector: Option<&str>, edge: u32) -> Result<Vec<u8>> {
        self.local().await?;
        let (x, y, w, h) = if let Some(s) = selector {
            let d = self.inspect(Some(s), None).await?;
            let b = &d["box"];
            (
                b["x"].as_f64().unwrap_or(0.0),
                b["y"].as_f64().unwrap_or(0.0),
                b["width"].as_f64().unwrap_or(0.0),
                b["height"].as_f64().unwrap_or(0.0),
            )
        } else {
            (0.0, 0.0, self.viewport.width as f64, self.viewport.height as f64)
        };
        ensure!(w > 0.0 && h > 0.0, "element has no visible size");
        let offset = self.eval("({x:scrollX,y:scrollY})".into()).await?;
        let x = x + offset["x"].as_f64().unwrap_or(0.0);
        let y = y + offset["y"].as_f64().unwrap_or(0.0);
        let scale = (edge.clamp(320, 768) as f64 / w.max(h)).min(1.0);
        let shot = self
            .call(
                "Page.captureScreenshot",
                json!({"format":"jpeg","quality":60,"clip":{"x":x,"y":y,"width":w,"height":h,"scale":scale}}),
            )
            .await?;
        crate::page::decode_b64(shot["data"].as_str().context("no screenshot data")?)
    }

    pub async fn click(&self, selector: Option<&str>, point: Option<(f64, f64)>) -> Result<Value> {
        let d = self.inspect(selector, point).await?;
        let r = &d["box"];
        let (x, y) = point.unwrap_or((
            r["x"].as_f64().unwrap_or(0.0) + r["width"].as_f64().unwrap_or(0.0) / 2.0,
            r["y"].as_f64().unwrap_or(0.0) + r["height"].as_f64().unwrap_or(0.0) / 2.0,
        ));
        ensure!(
            (0.0..self.viewport.width as f64).contains(&x) && (0.0..self.viewport.height as f64).contains(&y),
            "element is outside viewport; scroll first"
        );
        for kind in ["mousePressed", "mouseReleased"] {
            self.call("Input.dispatchMouseEvent", json!({"type":kind,"x":x,"y":y,"button":"left","clickCount":1}))
                .await?;
        }
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        self.location().await
    }

    pub async fn type_text(&self, selector: &str, text: &str) -> Result<Value> {
        self.local().await?;
        ensure!(text.len() <= 8192, "text exceeds 8192 bytes");
        let s = serde_json::to_string(selector)?;
        self.eval(format!("(() => {{const e=document.querySelector({s}); if(!e || e.disabled || e.readOnly || !(e.matches('input,textarea') || e.isContentEditable)) throw Error('Choose an editable field'); e.focus(); if(e.select) e.select(); else {{ const r=document.createRange();r.selectNodeContents(e);const s=getSelection();s.removeAllRanges();s.addRange(r); }} return true;}})()")).await?;
        self.call("Input.insertText", json!({"text":text})).await?;
        Ok(json!({"typed":true}))
    }

    pub async fn scroll(&self, dy: f64) -> Result<Value> {
        self.local().await?;
        ensure!(dy.is_finite() && dy.abs() <= 800.0, "scroll must be within 800 pixels");
        self.eval(format!("(() => {{window.scrollBy(0,{dy}); return {{scrollY:window.scrollY}};}})()")).await
    }

    pub async fn close(&self) {
        self.browser.close_tab(&self.target).await;
    }
}
