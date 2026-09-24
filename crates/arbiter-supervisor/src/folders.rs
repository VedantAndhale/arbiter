//! Folder browsing for the project picker: directory names only, never file
//! contents. Hidden and system folders are left out; listings are bounded.
use std::path::{Path, PathBuf};

pub const MAX_ENTRIES: usize = 500;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Listing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<Entry>,
    pub truncated: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(&'static str),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn display(p: &Path) -> String {
    p.to_string_lossy().trim_start_matches(r"\\?\").to_owned()
}

fn hidden(name: &str, m: &std::fs::Metadata) -> bool {
    // Profile internals that are never a project home, even when the OS does
    // not flag them hidden.
    const INTERNAL: [&str; 7] = [
        "System Volume Information",
        "AppData",
        "Application Data",
        "Local Settings",
        "node_modules",
        "__pycache__",
        "Recovery",
    ];
    if name.starts_with('.') || name.starts_with('$') || INTERNAL.iter().any(|n| n.eq_ignore_ascii_case(name)) {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM
        m.file_attributes() & 0x6 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = m;
        false
    }
}

/// Subfolders of `dir`, sorted case-insensitively.
pub fn list(dir: &Path) -> Result<Listing, Error> {
    if !dir.is_absolute() {
        return Err(Error::Invalid("choose an absolute folder"));
    }
    let dir = std::fs::canonicalize(dir)?;
    if !dir.is_dir() {
        return Err(Error::Invalid("not a folder"));
    }
    let mut entries = vec![];
    let mut truncated = false;
    for e in std::fs::read_dir(&dir)?.flatten() {
        let Ok(m) = std::fs::symlink_metadata(e.path()) else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        if !m.is_dir() || crate::hardware::is_link(&m) || hidden(&name, &m) {
            continue;
        }
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        entries.push(Entry { path: display(&e.path()), name });
    }
    entries.sort_by_key(|e| e.name.to_lowercase());
    Ok(Listing { path: display(&dir), parent: dir.parent().map(display), entries, truncated })
}

/// Create one new folder `name` inside `parent`.
pub fn create(parent: &Path, name: &str) -> Result<PathBuf, Error> {
    let name = name.trim();
    let reserved = ["con", "prn", "aux", "nul", "com1", "com2", "com3", "lpt1", "lpt2", "lpt3"];
    if name.is_empty()
        || name.len() > 100
        || name == "."
        || name == ".."
        || name.ends_with('.')
        || name.chars().any(|c| c.is_control() || r#"<>:"/\|?*"#.contains(c))
        || reserved.contains(&name.to_lowercase().as_str())
    {
        return Err(Error::Invalid("use a folder name without / \\ : * ? \" < > |"));
    }
    if !parent.is_absolute() || !parent.is_dir() {
        return Err(Error::Invalid("the parent folder does not exist"));
    }
    let target = std::fs::canonicalize(parent)?.join(name);
    if target.exists() {
        return Err(Error::Invalid("a folder with that name already exists"));
    }
    std::fs::create_dir(&target)?;
    Ok(PathBuf::from(display(&target)))
}

/// Starting points: home, common user folders, and drives on Windows.
pub fn places() -> Vec<Entry> {
    let mut out = vec![];
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    if let Some(home) = &home {
        out.push(Entry { name: "Home".into(), path: display(home) });
        for sub in ["Desktop", "Documents", "Projects", "code", "dev", "source/repos"] {
            let p = home.join(sub);
            if p.is_dir() {
                out.push(Entry { name: sub.rsplit('/').next().unwrap_or(sub).into(), path: display(&p) });
            }
        }
    }
    #[cfg(windows)]
    for letter in b'A'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        if Path::new(&root).is_dir() {
            out.push(Entry { name: format!("{}:", letter as char), path: root });
        }
    }
    #[cfg(not(windows))]
    out.push(Entry { name: "Computer".into(), path: "/".into() });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lists_folders_only_and_creates_safely() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("beta")).unwrap();
        std::fs::create_dir(d.path().join("Alpha")).unwrap();
        std::fs::create_dir(d.path().join(".git")).unwrap();
        std::fs::create_dir(d.path().join("AppData")).unwrap();
        std::fs::write(d.path().join("file.txt"), "x").unwrap();
        let l = list(d.path()).unwrap();
        assert_eq!(l.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Alpha", "beta"]);
        assert!(l.parent.is_some());
        let made = create(d.path(), "new-app").unwrap();
        assert!(made.is_dir());
        assert!(create(d.path(), "new-app").is_err(), "exists");
        for bad in ["", "..", "a/b", "a\\b", "con", "x:"] {
            assert!(create(d.path(), bad).is_err(), "{bad:?}");
        }
        assert!(list(Path::new("relative")).is_err());
        assert!(!places().is_empty());
    }
}
