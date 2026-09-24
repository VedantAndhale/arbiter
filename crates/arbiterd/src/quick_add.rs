//! One step to add a project folder. Arbiter decides what the folder needs:
//! a Git repository with history is added as it is; an empty folder is set up
//! and saved as a first version; only a folder with files but no history needs
//! the user's review of what goes into that first version.
use crate::AppState;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::Path;

fn has_history(root: &Path) -> bool {
    root.join(".git").exists()
        && std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "--verify", "-q", "HEAD"])
            .output()
            .is_ok_and(|o| o.status.success())
}

impl AppState {
    pub(crate) async fn quick_add(&self, path: &str) -> Result<Value> {
        let root = std::fs::canonicalize(path).context("that folder does not exist")?;
        anyhow::ensure!(root.is_dir(), "choose a folder, not a file");
        let root_str = root.to_string_lossy().trim_start_matches(r"\\?\").to_owned();
        if has_history(&root) {
            return Ok(json!({"status":"added","project":self.register_project(&root_str)?}));
        }
        let inventory = arbiter_project::inspect(&root)?;
        if inventory.empty && !inventory.git {
            // Nothing here yet: set it up and save a first version, no review needed.
            let inspected = self.inspect_project(&root_str, "", "")?;
            let id = inspected["proposal"]["id"].as_str().context("proposal id")?.to_owned();
            self.apply_adoption(&id, false)?;
            let files = arbiter_project::inspect(&root)?.initial_files;
            self.initialize_project(&id, files).await?;
            return Ok(json!({"status":"added","project":self.register_project(&root_str)?}));
        }
        // Existing files without history: the user chooses what the first
        // version includes, so nothing sensitive is swept in.
        let inspected = self.inspect_project(&root_str, "", "")?;
        Ok(json!({"status":"review","proposal":inspected["proposal"],"checks":inspected["checks"]}))
    }
}
