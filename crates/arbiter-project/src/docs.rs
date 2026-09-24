//! Manifest-derived documentation profiles. Version ranges are never presented
//! as installed versions; live documentation is not an upgrade instruction.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub version: String,
    pub manifest: String,
    pub source: String,
    pub exact: bool,
    pub note: String,
}
pub fn profiles(manifests: &[(String, Vec<u8>)]) -> Vec<Profile> {
    let mut out = vec![];
    for (path, bytes) in manifests {
        let text = String::from_utf8_lossy(bytes);
        let mut add = |name: &str, version: String, source: String, exact: bool| {
            out.push(Profile {
            name: name.into(), version, manifest: path.clone(), source, exact,
            note: "Check source version applicability before using an API. Newer features may require a reviewed dependency upgrade.".into(),
        })
        };
        if path.ends_with("package.json") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                add(
                    "Node.js",
                    v["engines"]["node"].as_str().unwrap_or("unknown").into(),
                    "https://nodejs.org/api/".into(),
                    false,
                );
                for (name, source) in [
                    ("typescript", "https://www.typescriptlang.org/docs/"),
                    ("react", "https://react.dev/reference/react"),
                    ("next", "https://nextjs.org/docs"),
                ] {
                    if let Some(version) = v["dependencies"][name].as_str().or(v["devDependencies"][name].as_str()) {
                        let lock_path = path.trim_end_matches("package.json").to_owned() + "package-lock.json";
                        let locked = manifests
                            .iter()
                            .find(|(p, _)| p == &lock_path)
                            .and_then(|(_, b)| serde_json::from_slice::<serde_json::Value>(b).ok())
                            .and_then(|l| {
                                l["packages"][format!("node_modules/{name}")]["version"].as_str().map(str::to_owned)
                            });
                        let exact = locked.is_some();
                        let version = locked.unwrap_or_else(|| version.into());
                        let source = if name == "react" && version.trim_start_matches(['^', '~']).starts_with("18.") {
                            "https://18.react.dev/reference/react"
                        } else {
                            source
                        };
                        add(name, version, source.into(), exact);
                    }
                }
            }
        } else if path.ends_with("Cargo.toml") {
            if let Ok(v) = toml::from_str::<toml::Value>(&text) {
                add(
                    "Rust",
                    v.get("package")
                        .and_then(|p| p.get("rust-version"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .into(),
                    "https://doc.rust-lang.org/book/".into(),
                    false,
                );
            }
        } else if path.ends_with("pyproject.toml") {
            if let Ok(v) = toml::from_str::<toml::Value>(&text) {
                let version = v
                    .get("project")
                    .and_then(|p| p.get("requires-python"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let minimum = version.trim_start_matches(['>', '=', '~', ' ']).split([',', ' ']).next().unwrap_or("");
                let parts: Vec<_> = minimum.split('.').collect();
                let source = if parts.len() >= 2 && parts[0] == "3" && parts[1].parse::<u8>().is_ok() {
                    format!("https://docs.python.org/3.{}/", parts[1])
                } else {
                    "https://docs.python.org/3/".into()
                };
                add("Python", version.into(), source, false);
            }
        } else if path.ends_with("go.mod") {
            let version = text.lines().find_map(|l| l.strip_prefix("go ")).unwrap_or("unknown");
            add("Go", version.trim().into(), "https://go.dev/doc/".into(), false);
        }
    }
    out.truncate(32);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_stack_and_lockfile_distinguish_ranges() {
        let p = profiles(&[
            ("web/package.json".into(), br#"{"dependencies":{"react":"^18.0.0"}}"#.to_vec()),
            ("web/package-lock.json".into(), br#"{"packages":{"node_modules/react":{"version":"18.3.1"}}}"#.to_vec()),
            ("api/pyproject.toml".into(), b"[project]\nrequires-python = '>=3.11'".to_vec()),
            ("Cargo.toml".into(), b"[package]\nrust-version = '1.85'".to_vec()),
        ]);
        assert!(p.iter().any(|p| p.name == "react" && p.version == "18.3.1" && p.exact));
        assert!(p.iter().any(|p| p.name == "Python" && p.version == ">=3.11" && !p.exact));
        assert!(p.iter().any(|p| p.source == "https://docs.python.org/3.11/"));
        assert!(p.iter().any(|p| p.source == "https://18.react.dev/reference/react"));
        assert!(p.iter().any(|p| p.name == "Rust"));
    }
}
