//! Measures whether a local model can actually code before routing trusts it.
//! Fixed, small tasks run through the same bounded tool loop as real work, in
//! a disposable folder. An intake benchmark says nothing about coding; this does.
use crate::AppState;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{path::Path, time::Instant};

/// Bump when the tasks change, so old results stop counting.
const PROBE_VERSION: u64 = 1;
const PROBE_STEPS: usize = 12;
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

struct Probe {
    name: &'static str,
    request: &'static str,
    files: &'static [(&'static str, &'static str)],
    verify: fn(&Path) -> Result<(), String>,
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_default().replace("\r\n", "\n")
}

const PROBES: [Probe; 3] = [
    Probe {
        name: "Fix a bug in one file",
        request: "calc.py: add returns the difference instead of the sum. Fix add so it returns the sum. Do not change mul.",
        files: &[("calc.py", "def add(a, b):\n    return a - b\n\n\ndef mul(a, b):\n    return a * b\n")],
        verify: |root| {
            let text = read(root, "calc.py");
            let add = text.split("def mul").next().unwrap_or("");
            if !(add.contains("a + b") || add.contains("a+b") || add.contains("b + a")) {
                return Err("add still does not return the sum".into());
            }
            if !text.contains("def mul(a, b):\n    return a * b") {
                return Err("mul was changed".into());
            }
            Ok(())
        },
    },
    Probe {
        name: "Find and fix across files",
        request: "The shout helper returns lower case, but its docstring says it returns upper case. Find it and fix it. Do not change its callers.",
        files: &[
            ("src/app.py", "from text.shout import shout\n\nprint(shout(\"hi\"))\n"),
            ("src/text/__init__.py", ""),
            ("src/text/shout.py", "def shout(s):\n    \"\"\"Return s in upper case.\"\"\"\n    return s.lower()\n"),
        ],
        verify: |root| {
            let text = read(root, "src/text/shout.py");
            if !text.contains(".upper()") || text.contains(".lower()") {
                return Err("shout still returns lower case".into());
            }
            if read(root, "src/app.py") != "from text.shout import shout\n\nprint(shout(\"hi\"))\n" {
                return Err("the caller was changed".into());
            }
            Ok(())
        },
    },
    Probe {
        name: "Create a new file",
        request: "Add src/text/greet.py with a function greet(name) that returns \"Hello, \" followed by the name.",
        files: &[("src/text/__init__.py", "")],
        verify: |root| {
            let text = read(root, "src/text/greet.py");
            if !text.contains("def greet(") || !text.contains("Hello, ") {
                return Err("greet.py is missing or incomplete".into());
            }
            Ok(())
        },
    },
];

impl AppState {
    fn capability_path(&self) -> std::path::PathBuf {
        self.inner.home.join("local-capability.json")
    }
    fn capability_results(&self) -> serde_json::Map<String, Value> {
        std::fs::read(self.capability_path())
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
    }
    pub(crate) fn local_model_installed(&self, model: &str) -> bool {
        self.inner.local_generator.is_some()
            || self.inner.intake.status()["models"].as_array().is_some_and(|ms| {
                ms.iter().any(|m| m["model"]["id"] == model && m["ready"] == true && model != "potion")
            })
    }
    pub(crate) fn capability_report(&self) -> Value {
        let results: Vec<Value> = self
            .capability_results()
            .into_values()
            .map(|mut r| {
                let current = r["probe_version"] == PROBE_VERSION;
                r["current"] = json!(current);
                r
            })
            .collect();
        json!({"probe_version":PROBE_VERSION,"tasks":PROBES.iter().map(|p| p.name).collect::<Vec<_>>(),"results":results})
    }
    /// The installed local model that passed every current probe, fastest first.
    pub(crate) fn capable_local_model(&self) -> Option<String> {
        let mut passed: Vec<(u64, String)> = self
            .capability_results()
            .into_iter()
            .filter(|(model, r)| {
                r["passed"] == true && r["probe_version"] == PROBE_VERSION && self.local_model_installed(model)
            })
            .map(|(model, r)| (r["duration_ms"].as_u64().unwrap_or(u64::MAX), model))
            .collect();
        passed.sort();
        passed.into_iter().next().map(|(_, m)| m)
    }
    pub(crate) async fn run_capability_probe(&self, model: &str) -> Result<Value> {
        ensure!(!model.is_empty() && model.len() <= 80, "choose a local model");
        ensure!(self.local_model_installed(model), "Install the local model before testing it");
        ensure!(
            self.inner.local_runs.lock().unwrap().is_empty(),
            crate::runs::Busy("The local model is coding right now; test it when that finishes".into())
        );
        let _slot = self.local_slot().await?;
        let started = Instant::now();
        let mut tasks = vec![];
        for probe in &PROBES {
            let dir = self.inner.home.join("probes").join(arbiter_core::RunId::new().to_string());
            let outcome = async {
                for (rel, body) in probe.files {
                    let path = dir.join(rel);
                    std::fs::create_dir_all(path.parent().context("probe path")?)?;
                    std::fs::write(path, body)?;
                }
                let mut session = arbiter_local::Session::with_limit(&dir, vec!["**".into()], vec![], false, PROBE_STEPS)?;
                let goal = format!("Request:\n{}\nApproved writable scope: **\n", probe.request);
                let t = Instant::now();
                let result =
                    tokio::time::timeout(PROBE_TIMEOUT, self.local_loop(None, &dir, model, &goal, &mut session, false))
                        .await
                        .map_err(|_| anyhow::anyhow!("timed out after {}s", PROBE_TIMEOUT.as_secs()))
                        .and_then(|r| r);
                let ms = t.elapsed().as_millis() as u64;
                anyhow::Ok(match result {
                    Ok(o) => match (probe.verify)(&dir) {
                        Ok(()) => json!({"name":probe.name,"passed":true,"steps":o.steps,"duration_ms":ms}),
                        Err(why) => {
                            json!({"name":probe.name,"passed":false,"steps":o.steps,"duration_ms":ms,"reason":why})
                        }
                    },
                    Err(e) => {
                        json!({"name":probe.name,"passed":false,"duration_ms":ms,"reason":format!("{e:#}").chars().take(300).collect::<String>()})
                    }
                })
            }
            .await;
            let _ = std::fs::remove_dir_all(&dir);
            tasks.push(outcome?);
        }
        let passed = tasks.iter().all(|t| t["passed"] == true);
        let result = json!({
            "model":model,"probe_version":PROBE_VERSION,"passed":passed,
            "score":tasks.iter().filter(|t| t["passed"] == true).count(),"of":tasks.len(),
            "duration_ms":started.elapsed().as_millis() as u64,"at":crate::projects::now(),"tasks":tasks
        });
        let mut all = self.capability_results();
        all.insert(model.to_owned(), result.clone());
        let target = self.capability_path();
        std::fs::write(&target, serde_json::to_vec(&Value::Object(all))?)?;
        Ok(result)
    }
}
