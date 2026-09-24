//! Which checks a project has. `.arbiter/healing.toml` wins; otherwise we look
//! at the usual manifests. Detection is cheap (file reads only).

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    Typecheck,
    Lint,
    Test,
    Build,
    Custom,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub kind: CheckKind,
    /// Shell command, run from the worktree root.
    pub cmd: String,
    /// Deterministic fixer to try before involving the agent (e.g. `eslint --fix`).
    #[serde(default)]
    pub fix: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    300
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HealConfig {
    /// Environment setup to run in a fresh worktree (e.g. dependency install).
    #[serde(default)]
    pub setup: Option<String>,
    #[serde(default, rename = "check")]
    pub checks: Vec<Check>,
    /// Set `enabled = false` to turn healing off for the project.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Load the app in a headless browser after front-end changes.
    #[serde(default)]
    pub browser: Option<BrowserConfig>,
}

/// `[browser]` in healing.toml. `dev` may use `{port}`; `PORT` is also set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrowserConfig {
    pub dev: String,
    #[serde(default = "root_route")]
    pub routes: Vec<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "ready_timeout")]
    pub ready_timeout_secs: u64,
}

fn root_route() -> Vec<String> {
    vec!["/".into()]
}

fn ready_timeout() -> u64 {
    90
}

/// Files whose changes can break what the browser shows.
pub fn is_frontend_file(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    [
        ".tsx", ".jsx", ".ts", ".js", ".mjs", ".vue", ".svelte", ".astro", ".css", ".scss", ".sass", ".less", ".html",
        ".mdx",
    ]
    .iter()
    .any(|e| p.ends_with(e))
        && !p.contains("test.")
        && !p.contains("spec.")
}

fn yes() -> bool {
    true
}

/// `.arbiter/healing.toml`:
/// ```toml
/// setup = "pnpm install --frozen-lockfile"
/// [[check]]
/// name = "typecheck"
/// kind = "typecheck"
/// cmd = "pnpm tsc --noEmit"
/// ```
pub fn detect(root: &Path) -> anyhow::Result<HealConfig> {
    detect_with(root, None)
}

/// Like [`detect`], preferring `personal` (Arbiter's own copy, kept outside
/// the project) over a `.arbiter/healing.toml` a team chose to commit.
pub fn detect_with(root: &Path, personal: Option<&Path>) -> anyhow::Result<HealConfig> {
    for path in personal.into_iter().map(Path::to_path_buf).chain([root.join(".arbiter").join("healing.toml")]) {
        if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            return Ok(toml::from_str(&text)?);
        }
    }
    let mut cfg = HealConfig { enabled: true, ..Default::default() };
    detect_node(root, &mut cfg);
    detect_rust(root, &mut cfg);
    detect_python(root, &mut cfg);
    detect_go(root, &mut cfg);
    if cfg.browser.is_none() {
        cfg.browser = detect_python_web(root).or_else(|| detect_static(root));
    }
    Ok(cfg)
}

/// Served by arbiterd itself: a plain HTML site needs no toolchain.
pub const STATIC_SERVER: &str = "builtin:static";

fn web_config(dev: impl Into<String>) -> BrowserConfig {
    BrowserConfig { dev: dev.into(), routes: root_route(), enabled: true, ready_timeout_secs: ready_timeout() }
}

/// Flask, FastAPI and Django apps, started on the chosen port.
fn detect_python_web(root: &Path) -> Option<BrowserConfig> {
    let manifest = ["requirements.txt", "pyproject.toml", "Pipfile"]
        .iter()
        .filter_map(|f| std::fs::read_to_string(root.join(f)).ok())
        .collect::<String>()
        .to_lowercase();
    if root.join("manage.py").exists() {
        return Some(web_config("python manage.py runserver 127.0.0.1:{port}"));
    }
    let entry = ["main.py", "app.py"].into_iter().find(|f| root.join(f).exists())?;
    let module = entry.trim_end_matches(".py");
    if manifest.contains("fastapi") || manifest.contains("uvicorn") {
        return Some(web_config(format!("python -m uvicorn {module}:app --host 127.0.0.1 --port {{port}}")));
    }
    if manifest.contains("flask") {
        return Some(web_config(format!("python -m flask --app {module} run --host 127.0.0.1 --port {{port}}")));
    }
    None
}

/// A folder whose root holds `index.html` and no app toolchain.
fn detect_static(root: &Path) -> Option<BrowserConfig> {
    (root.join("index.html").is_file() && !root.join("package.json").exists()).then(|| web_config(STATIC_SERVER))
}

fn check(name: &str, kind: CheckKind, cmd: impl Into<String>, fix: Option<String>) -> Check {
    Check { name: name.into(), kind, cmd: cmd.into(), fix, timeout_secs: default_timeout() }
}

fn detect_node(root: &Path, cfg: &mut HealConfig) {
    let Ok(text) = std::fs::read_to_string(root.join("package.json")) else { return };
    let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&text) else { return };
    let pm = if root.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if root.join("yarn.lock").exists() {
        "yarn"
    } else if root.join("bun.lockb").exists() || root.join("bun.lock").exists() {
        "bun"
    } else {
        "npm"
    };
    // Worktrees don't carry node_modules; install before checking.
    if !root.join("node_modules").exists() {
        cfg.setup = Some(match pm {
            "pnpm" => "pnpm install --frozen-lockfile --prefer-offline".into(),
            "yarn" => "yarn install --frozen-lockfile".into(),
            "bun" => "bun install --frozen-lockfile".into(),
            _ if root.join("package-lock.json").exists() => "npm ci --prefer-offline --no-audit".into(),
            _ => "npm install --no-audit".into(),
        });
    }
    let scripts = pkg["scripts"].as_object().cloned().unwrap_or_default();
    let has = |s: &str| scripts.contains_key(s);
    let run = |s: &str| format!("{pm} run {s}");
    let deps = |name: &str| pkg["devDependencies"][name].is_string() || pkg["dependencies"][name].is_string();

    if let Some(s) = ["typecheck", "type-check", "check-types", "tsc"].into_iter().find(|s| has(s)) {
        cfg.checks.push(check("typecheck", CheckKind::Typecheck, run(s), None));
    } else if root.join("tsconfig.json").exists() && deps("typescript") {
        cfg.checks.push(check("typecheck", CheckKind::Typecheck, format!("{pm} exec tsc --noEmit"), None));
    }
    if has("lint") {
        let fix = has("lint:fix").then(|| run("lint:fix"));
        cfg.checks.push(check("lint", CheckKind::Lint, run("lint"), fix));
    }
    if let Some(t) = scripts.get("test").and_then(|v| v.as_str())
        && !t.contains("no test specified")
    {
        cfg.checks.push(check("test", CheckKind::Test, run("test"), None));
    }
    cfg.browser = detect_dev_server(pm, &scripts, &deps);
}

/// How to start the app's dev server on a chosen port, for known frameworks.
fn detect_dev_server(
    pm: &str,
    scripts: &serde_json::Map<String, serde_json::Value>,
    deps: &dyn Fn(&str) -> bool,
) -> Option<BrowserConfig> {
    let (script, body) = ["dev", "start", "serve"]
        .into_iter()
        .find_map(|s| scripts.get(s).and_then(|v| v.as_str()).map(|b| (s, b.to_owned())))?;
    // npm needs "--" to forward flags to the script; pnpm/yarn/bun forward them.
    let run = |args: &str| match (pm, args.is_empty()) {
        (_, true) => format!("{pm} run {script}"),
        ("npm", false) => format!("npm run {script} -- {args}"),
        _ => format!("{pm} run {script} {args}"),
    };
    let web = [
        "vite",
        "next",
        "@sveltejs/kit",
        "astro",
        "nuxt",
        "react-scripts",
        "@angular/core",
        "@remix-run/dev",
        "webpack-dev-server",
        "parcel",
    ];
    if !web.iter().any(|d| deps(d))
        && !["vite", "next", "astro", "nuxt", "ng serve", "webpack", "parcel"].iter().any(|k| body.contains(k))
    {
        return None;
    }
    let dev = if body.contains("react-scripts") || deps("react-scripts") && !body.contains("vite") {
        run("") // CRA reads PORT
    } else if body.contains("parcel") {
        run("--port {port}")
    } else {
        // vite, next, astro, nuxt, ng serve and webpack-dev-server all accept --port.
        run("--port {port}")
    };
    Some(BrowserConfig { dev, routes: root_route(), enabled: true, ready_timeout_secs: ready_timeout() })
}

fn detect_rust(root: &Path, cfg: &mut HealConfig) {
    if !root.join("Cargo.toml").exists() {
        return;
    }
    cfg.checks.push(check(
        "cargo check",
        CheckKind::Typecheck,
        "cargo check --all-targets --message-format short",
        Some("cargo fmt --all".into()),
    ));
    cfg.checks.push(check("cargo test", CheckKind::Test, "cargo test --no-fail-fast", None));
}

fn detect_python(root: &Path, cfg: &mut HealConfig) {
    let pyproject = std::fs::read_to_string(root.join("pyproject.toml")).unwrap_or_default();
    let has_pytest = pyproject.contains("pytest") || root.join("pytest.ini").exists() || root.join("tests").is_dir();
    let is_python = !pyproject.is_empty() || root.join("setup.py").exists() || root.join("requirements.txt").exists();
    if !is_python {
        return;
    }
    if pyproject.contains("[tool.ruff") || root.join("ruff.toml").exists() {
        cfg.checks.push(check("ruff", CheckKind::Lint, "ruff check .", Some("ruff check --fix .".into())));
    }
    if pyproject.contains("[tool.mypy") || root.join("mypy.ini").exists() {
        cfg.checks.push(check("mypy", CheckKind::Typecheck, "mypy .", None));
    }
    if has_pytest {
        cfg.checks.push(check("pytest", CheckKind::Test, "python -m pytest -q -rf --no-header", None));
    }
}

fn detect_go(root: &Path, cfg: &mut HealConfig) {
    if root.join("go.mod").exists() {
        cfg.checks.push(check("go vet", CheckKind::Typecheck, "go vet ./...", Some("gofmt -w .".into())));
        cfg.checks.push(check("go test", CheckKind::Test, "go test ./...", None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_pnpm_scripts_and_setup() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("package.json"),
            r#"{"scripts":{"typecheck":"tsc --noEmit","lint":"eslint .","lint:fix":"eslint . --fix","test":"vitest run"}}"#,
        )
        .unwrap();
        std::fs::write(d.path().join("pnpm-lock.yaml"), "").unwrap();
        let c = detect(d.path()).unwrap();
        assert_eq!(c.setup.as_deref(), Some("pnpm install --frozen-lockfile --prefer-offline"));
        let names: Vec<_> = c.checks.iter().map(|c| (c.name.as_str(), c.cmd.as_str())).collect();
        assert_eq!(names, [("typecheck", "pnpm run typecheck"), ("lint", "pnpm run lint"), ("test", "pnpm run test")]);
        assert_eq!(c.checks[1].fix.as_deref(), Some("pnpm run lint:fix"));
    }

    #[test]
    fn detects_static_and_python_web_servers() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("index.html"), "<h1>Hi</h1>").unwrap();
        assert_eq!(detect(d.path()).unwrap().browser.unwrap().dev, STATIC_SERVER);
        let f = tempfile::tempdir().unwrap();
        std::fs::write(f.path().join("requirements.txt"), "Flask==3.0\n").unwrap();
        std::fs::write(f.path().join("app.py"), "").unwrap();
        assert_eq!(
            detect(f.path()).unwrap().browser.unwrap().dev,
            "python -m flask --app app run --host 127.0.0.1 --port {port}"
        );
        let a = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join("pyproject.toml"), "[project]\ndependencies=[\"fastapi\"]\n").unwrap();
        std::fs::write(a.path().join("main.py"), "").unwrap();
        assert_eq!(
            detect(a.path()).unwrap().browser.unwrap().dev,
            "python -m uvicorn main:app --host 127.0.0.1 --port {port}"
        );
        let j = tempfile::tempdir().unwrap();
        std::fs::write(j.path().join("manage.py"), "").unwrap();
        assert_eq!(detect(j.path()).unwrap().browser.unwrap().dev, "python manage.py runserver 127.0.0.1:{port}");
    }

    #[test]
    fn detects_dev_servers() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("node_modules")).unwrap();
        std::fs::write(d.path().join("package.json"), r#"{"scripts":{"dev":"vite"},"devDependencies":{"vite":"^7"}}"#)
            .unwrap();
        let b = detect(d.path()).unwrap().browser.unwrap();
        assert_eq!((b.dev.as_str(), b.routes.as_slice()), ("npm run dev -- --port {port}", &["/".to_owned()][..]));

        std::fs::write(d.path().join("pnpm-lock.yaml"), "").unwrap();
        std::fs::write(d.path().join("package.json"), r#"{"scripts":{"dev":"next dev"},"dependencies":{"next":"16"}}"#)
            .unwrap();
        assert_eq!(detect(d.path()).unwrap().browser.unwrap().dev, "pnpm run dev --port {port}");

        std::fs::write(d.path().join("package.json"), r#"{"scripts":{"start":"node server.js"}}"#).unwrap();
        assert!(detect(d.path()).unwrap().browser.is_none(), "plain node servers are not assumed to be web UIs");
        assert!(
            is_frontend_file("src/App.tsx") && !is_frontend_file("src/App.test.tsx") && !is_frontend_file("README.md")
        );
    }

    #[test]
    fn npm_placeholder_test_script_is_ignored() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("package.json"),
            r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#,
        )
        .unwrap();
        std::fs::create_dir(d.path().join("node_modules")).unwrap();
        let c = detect(d.path()).unwrap();
        assert!(c.checks.is_empty() && c.setup.is_none());
    }

    #[test]
    fn override_file_wins() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("Cargo.toml"), "[package]").unwrap();
        std::fs::create_dir(d.path().join(".arbiter")).unwrap();
        std::fs::write(
            d.path().join(".arbiter/healing.toml"),
            "setup = \"make deps\"\n[[check]]\nname = \"unit\"\nkind = \"test\"\ncmd = \"make test\"\ntimeout_secs = 60\n",
        )
        .unwrap();
        let c = detect(d.path()).unwrap();
        assert_eq!(c.setup.as_deref(), Some("make deps"));
        assert_eq!(c.checks.len(), 1);
        assert_eq!((c.checks[0].kind, c.checks[0].timeout_secs), (CheckKind::Test, 60));
    }

    #[test]
    fn rust_and_python() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("Cargo.toml"), "[package]").unwrap();
        std::fs::write(d.path().join("pyproject.toml"), "[tool.ruff]\n[tool.pytest.ini_options]\n").unwrap();
        let kinds: Vec<_> = detect(d.path()).unwrap().checks.into_iter().map(|c| c.name).collect();
        assert_eq!(kinds, ["cargo check", "cargo test", "ruff", "pytest"]);
    }
}
