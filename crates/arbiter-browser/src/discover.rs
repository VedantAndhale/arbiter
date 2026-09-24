//! Find a Chromium-family browser without downloading one.

use std::path::PathBuf;

/// `$ARBITER_BROWSER`, then registered/standard Chrome, Edge and Brave
/// installs, then Playwright's lightweight headless shell.
pub fn find_browser() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("ARBITER_BROWSER").map(PathBuf::from).filter(|p| p.exists()) {
        return Some(p);
    }
    candidates().into_iter().find(|p| p.exists())
}

#[cfg(windows)]
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for exe in ["chrome.exe", "msedge.exe", "brave.exe"] {
        out.extend(registry_app_path(exe));
    }
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    for base in [env("ProgramFiles"), env("ProgramFiles(x86)"), env("LOCALAPPDATA")].into_iter().flatten() {
        out.push(base.join(r"Google\Chrome\Application\chrome.exe"));
        out.push(base.join(r"Microsoft\Edge\Application\msedge.exe"));
        out.push(base.join(r"BraveSoftware\Brave-Browser\Application\brave.exe"));
    }
    if let Some(local) = env("LOCALAPPDATA") {
        out.extend(playwright(
            &local.join("ms-playwright"),
            "chrome-headless-shell-win64",
            "chrome-headless-shell.exe",
        ));
        out.extend(playwright(&local.join("ms-playwright"), "chrome-win", "chrome.exe"));
    }
    out
}

#[cfg(not(windows))]
fn candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> =
        ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "microsoft-edge", "brave-browser"]
            .into_iter()
            .filter_map(|n| which::which(n).ok())
            .collect();
    out.push("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into());
    out.push("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge".into());
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for cache in [home.join(".cache/ms-playwright"), home.join("Library/Caches/ms-playwright")] {
            out.extend(playwright(&cache, "chrome-headless-shell-linux64", "chrome-headless-shell"));
            out.extend(playwright(&cache, "chrome-headless-shell-mac-arm64", "chrome-headless-shell"));
        }
    }
    out
}

/// Newest `chromium*` install under a Playwright cache.
fn playwright(cache: &std::path::Path, dir: &str, exe: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(cache) else { return Vec::new() };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("chromium")))
        .map(|p| p.join(dir).join(exe))
        .collect();
    found.sort();
    found.reverse();
    found
}

#[cfg(windows)]
fn registry_app_path(exe: &str) -> Vec<PathBuf> {
    use arbiter_core::NoWindow;
    let mut out = Vec::new();
    for hive in ["HKLM", "HKCU"] {
        let key = format!(r"{hive}\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
        let Ok(o) = std::process::Command::new("reg").no_window().args(["query", &key, "/ve"]).output() else {
            continue;
        };
        let text = String::from_utf8_lossy(&o.stdout);
        // "    (Default)    REG_SZ    C:\...\chrome.exe"
        if let Some(path) = text.lines().find_map(|l| l.split("REG_SZ").nth(1)).map(|p| p.trim().trim_matches('"')) {
            out.push(PathBuf::from(path));
        }
    }
    out
}
