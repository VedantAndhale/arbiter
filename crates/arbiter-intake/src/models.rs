//! Model downloads and inference lifecycle are owned by the daemon.
use crate::{
    classifier::{self, Classifier},
    questions, runtime,
};
use anyhow::{Context, Result, ensure};
use arbiter_core::{Assessment, Question};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelFile {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub repository: String,
    pub revision: String,
    pub license: String,
    pub files: Vec<ModelFile>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Benchmark {
    pub first_token_ms: f64,
    pub complete_ms: f64,
    pub target_ms: f64,
    pub meets_target: bool,
    pub samples: usize,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Progress {
    pub phase: String,
    pub downloaded: u64,
    pub total: u64,
    pub error: Option<String>,
}

pub fn catalog() -> Vec<Model> {
    serde_json::from_str(include_str!("catalog.json")).expect("embedded model catalog")
}

#[derive(Clone)]
pub struct Manager {
    root: PathBuf,
    progress: Arc<Mutex<HashMap<String, Progress>>>,
    classifier: Arc<Mutex<Option<(Classifier, Instant)>>>,
    last_error: Arc<Mutex<Option<String>>>,
    cancellations: Arc<Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>>,
}

impl Manager {
    /// Explicit model choice for coding/review. Downloaded question models are
    /// never silently advertised as generally capable coding agents.
    pub async fn structured(
        &self,
        id: &str,
        prompt: String,
        system: String,
        schema: serde_json::Value,
    ) -> Result<String> {
        let model = catalog()
            .into_iter()
            .find(|m| m.id == id && m.id != "potion" && self.ready(m))
            .context("download a supported local generation model first")?;
        let result =
            runtime::structured(self.root.join(&model.id).join(&model.files[0].name), prompt, system, schema).await?;
        Ok(result.text)
    }
    pub fn learn(&self, example: classifier::Example) -> Result<()> {
        ensure!(example.text.len() <= 12000, "training example exceeds bounds");
        ensure!(
            ["bugfix", "feature", "refactor", "test", "docs", "research", "ops"].contains(&example.task_type.as_str())
                && ["S", "M", "L"].contains(&example.size.as_str()),
            "unknown intake labels"
        );
        let dir = self.root.join("potion");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("examples.json");
        let mut examples: Vec<classifier::Example> =
            std::fs::read(&path).ok().and_then(|v| serde_json::from_slice(&v).ok()).unwrap_or_default();
        examples.retain(|e| e.text != example.text);
        examples.push(example);
        if examples.len() > 256 {
            examples.drain(..examples.len() - 256);
        }
        std::fs::write(path, serde_json::to_vec(&examples)?)?;
        *self.classifier.lock().unwrap() = None;
        Ok(())
    }
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            progress: Default::default(),
            classifier: Default::default(),
            last_error: Default::default(),
            cancellations: Default::default(),
        }
    }

    fn ready(&self, model: &Model) -> bool {
        let dir = self.root.join(&model.id);
        std::fs::read_to_string(dir.join("verified")).ok().as_deref() == Some(&model.revision)
            && model.files.iter().all(|f| std::fs::metadata(dir.join(&f.name)).is_ok_and(|m| m.len() == f.bytes))
    }

    pub fn status(&self) -> serde_json::Value {
        let progress = self.progress.lock().unwrap();
        let models: Vec<_> = catalog().into_iter().map(|m| {
            let benchmark = std::fs::read(self.root.join(&m.id).join("benchmark.json")).ok()
                .and_then(|s| serde_json::from_slice::<Benchmark>(&s).ok());
            serde_json::json!({"ready":self.ready(&m),"progress":progress.get(&m.id),"benchmark":benchmark,"model":m})
        }).collect();
        serde_json::json!({"models":models,"selected":self.selected().map(|m| m.id),"last_error":*self.last_error.lock().unwrap()})
    }

    fn selected(&self) -> Option<Model> {
        let preferred = std::fs::read_to_string(self.root.join("selected")).ok();
        let available: Vec<_> = catalog().into_iter().filter(|m| m.id != "potion" && self.ready(m)).collect();
        if let Some(model) = available.iter().find(|m| preferred.as_deref() == Some(&m.id)) {
            return Some(model.clone());
        }
        // Downloaded models do not become the default until their measured latency qualifies.
        // Users may explicitly select a slower model after seeing its benchmark.
        available
            .into_iter()
            .filter_map(|model| {
                let benchmark: Benchmark =
                    serde_json::from_slice(&std::fs::read(self.root.join(&model.id).join("benchmark.json")).ok()?)
                        .ok()?;
                benchmark.meets_target.then_some((model, benchmark.complete_ms))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(model, _)| model)
    }

    pub fn select(&self, id: &str) -> Result<()> {
        ensure!(
            catalog().iter().any(|m| m.id == id && m.id != "potion" && self.ready(m)),
            "download this question model first"
        );
        let measured = std::fs::read(self.root.join(id).join("benchmark.json"))
            .ok()
            .and_then(|v| serde_json::from_slice::<Benchmark>(&v).ok());
        ensure!(measured.is_some(), "Benchmark this model before enabling it");
        std::fs::create_dir_all(&self.root)?;
        std::fs::write(self.root.join("selected"), id)?;
        Ok(())
    }

    pub fn install(&self, id: &str, accept_license: bool) -> Result<()> {
        let model = catalog().into_iter().find(|m| m.id == id).context("unknown model")?;
        ensure!(model.license != "lfm1.0" || accept_license, "review and accept the LFM license before download");
        let mut progress = self.progress.lock().unwrap();
        ensure!(
            !progress
                .get(id)
                .is_some_and(|p| matches!(p.phase.as_str(), "downloading" | "cancelling" | "benchmarking")),
            "model setup already running"
        );
        progress.insert(
            id.into(),
            Progress {
                phase: "downloading".into(),
                total: model.files.iter().map(|f| f.bytes).sum(),
                ..Default::default()
            },
        );
        drop(progress);
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.cancellations.lock().unwrap().insert(id.into(), cancelled.clone());
        let manager = self.clone();
        tokio::spawn(async move {
            let result = tokio::select! {
                result = manager.download(&model, &cancelled) => result,
                _ = async { while !cancelled.load(std::sync::atomic::Ordering::SeqCst) { tokio::time::sleep(Duration::from_millis(100)).await; } } => Err(anyhow::anyhow!("Download cancelled; retry to resume")),
            };
            manager.cancellations.lock().unwrap().remove(&model.id);
            if let Err(e) = result {
                manager.progress.lock().unwrap().entry(model.id.clone()).or_default().error = Some(format!("{e:#}"));
                manager.progress.lock().unwrap().entry(model.id.clone()).or_default().phase =
                    if cancelled.load(std::sync::atomic::Ordering::SeqCst) { "cancelled" } else { "failed" }.into();
                return;
            }
            manager.progress.lock().unwrap().entry(model.id.clone()).or_default().phase = "ready".into();
            let _ = manager.start_benchmark(&model.id);
        });
        Ok(())
    }

    pub fn cancel(&self, id: &str) -> Result<()> {
        let cancellations = self.cancellations.lock().unwrap();
        let flag = cancellations.get(id).context("No active download to cancel")?;
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
        self.progress.lock().unwrap().entry(id.into()).or_default().phase = "cancelling".into();
        Ok(())
    }

    pub fn cancel_downloads(&self) {
        for flag in self.cancellations.lock().unwrap().values() {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    async fn download(&self, model: &Model, cancelled: &std::sync::atomic::AtomicBool) -> Result<()> {
        let dir = self.root.join(&model.id);
        tokio::fs::create_dir_all(&dir).await?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(1800))
            .build()?;
        let mut completed = 0;
        for item in &model.files {
            ensure!(!cancelled.load(std::sync::atomic::Ordering::SeqCst), "Download cancelled; retry to resume");
            let path = dir.join(&item.name);
            let expected = item.sha256.clone();
            let existing = path.clone();
            let valid = tokio::task::spawn_blocking(move || verify_file(&existing, &expected)).await??;
            if valid {
                completed += item.bytes;
                continue;
            }
            let part = path.with_extension("part");
            let offset =
                tokio::fs::metadata(&part).await.ok().map(|m| m.len()).filter(|n| *n < item.bytes).unwrap_or(0);
            let mut request = client.get(&item.url);
            if offset > 0 {
                request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
            }
            let mut response = request.send().await?.error_for_status()?;
            let resume = offset > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
            let mut file = if resume {
                tokio::fs::OpenOptions::new().append(true).open(&part).await?
            } else {
                tokio::fs::File::create(&part).await?
            };
            let mut hash = Sha256::new();
            let mut bytes = if resume { offset } else { 0 };
            if resume {
                let mut previous = tokio::fs::File::open(&part).await?;
                let mut buffer = vec![0u8; 65536];
                loop {
                    let n = previous.read(&mut buffer).await?;
                    if n == 0 {
                        break;
                    }
                    hash.update(&buffer[..n]);
                }
            }
            while let Some(chunk) = response.chunk().await? {
                ensure!(!cancelled.load(std::sync::atomic::Ordering::SeqCst), "Download cancelled; retry to resume");
                bytes += chunk.len() as u64;
                ensure!(bytes <= item.bytes, "download larger than pinned model file");
                hash.update(&chunk);
                file.write_all(&chunk).await?;
                self.progress.lock().unwrap().entry(model.id.clone()).or_default().downloaded = completed + bytes;
            }
            file.flush().await?;
            drop(file);
            ensure!(!cancelled.load(std::sync::atomic::Ordering::SeqCst), "Download cancelled; retry to resume");
            ensure!(
                bytes == item.bytes && format!("{:x}", hash.finalize()) == item.sha256,
                "model checksum mismatch; retry the download"
            );
            if path.exists() {
                tokio::fs::remove_file(&path).await?;
            }
            tokio::fs::rename(part, path).await?;
            completed += bytes;
        }
        tokio::fs::write(dir.join("verified"), &model.revision).await?;
        self.progress.lock().unwrap().entry(model.id.clone()).or_default().downloaded = completed;
        Ok(())
    }

    pub async fn assess(&self, text: String) -> Assessment {
        let model = catalog().into_iter().find(|m| m.id == "potion").unwrap();
        if !self.ready(&model) {
            return classifier::fallback(&text);
        }
        let fallback = classifier::fallback(&text);
        let on_failure = fallback.clone();
        let dir = self.root.join("potion");
        let cache = self.classifier.clone();
        let errors = self.last_error.clone();
        tokio::task::spawn_blocking(move || {
            let mut slot = cache.lock().unwrap();
            if slot.is_none() {
                match Classifier::load(&dir) {
                    Ok(c) => *slot = Some((c, Instant::now())),
                    Err(e) => {
                        *errors.lock().unwrap() = Some(format!("Classifier unavailable: {e:#}"));
                        return fallback;
                    }
                }
            }
            let (c, used) = slot.as_mut().unwrap();
            *used = Instant::now();
            c.assess(&text)
        })
        .await
        .unwrap_or(on_failure)
    }

    pub fn unload_idle(&self) {
        if let Ok(mut slot) = self.classifier.try_lock()
            && slot.as_ref().is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(300))
        {
            *slot = None;
        }
    }

    pub async fn questions(&self, text: &str, assessment: &Assessment, more: bool) -> Vec<Question> {
        if assessment.ambiguity < 0.6 && !more {
            return vec![];
        }
        if let Some(model) = self.selected() {
            let prompt = format!(
                "Request: {text}\n{}",
                if more {
                    "Ask about constraints and acceptance criteria only."
                } else {
                    "Ask only for the missing outcome and scope."
                }
            );
            match runtime::generate(self.root.join(&model.id).join(&model.files[0].name), prompt).await {
                Ok(g) => {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&g.text)
                        && let Ok(cards) = serde_json::from_value::<Vec<Question>>(value["questions"].clone())
                        && questions::validate(&cards).is_ok()
                    {
                        *self.last_error.lock().unwrap() = None;
                        return cards;
                    }
                    *self.last_error.lock().unwrap() =
                        Some("Question output failed validation; using standard clarification cards.".into());
                }
                Err(e) => *self.last_error.lock().unwrap() = Some(format!("Question model unavailable: {e:#}")),
            }
        }
        questions::fallback(assessment, more)
    }

    pub async fn benchmark(&self, id: &str) -> Result<Benchmark> {
        let model = catalog().into_iter().find(|m| m.id == id).context("unknown model")?;
        ensure!(self.ready(&model), "download the model before benchmarking");
        let prompts = ["Improve my app", "Build a dashboard", "Fix the problem"];
        let mut first: f64 = 0.0;
        let mut total: f64 = 0.0;
        if id == "potion" {
            self.assess(prompts[0].into()).await;
            for prompt in prompts {
                let a = self.assess(prompt.into()).await;
                ensure!(a.engine.starts_with("model2vec"), "classifier could not load");
                total = total.max(a.elapsed_ms);
            }
            first = total;
        } else {
            let path = self.root.join(id).join(&model.files[0].name);
            runtime::generate(path.clone(), prompts[0].into()).await?;
            for prompt in prompts {
                let g = runtime::generate(path.clone(), prompt.into()).await?;
                let value: serde_json::Value = serde_json::from_str(&g.text)?;
                questions::validate(&serde_json::from_value::<Vec<Question>>(value["questions"].clone())?)?;
                first = first.max(g.first_token_ms);
                total = total.max(g.total_ms);
            }
        }
        // Questions appear before a task starts, so a few seconds is the
        // most anyone should wait; the classifier must be near-instant.
        let target = if id == "potion" { 5.0 } else { 5000.0 };
        let result = Benchmark {
            first_token_ms: first,
            complete_ms: total,
            target_ms: target,
            meets_target: total <= target,
            samples: 3,
        };
        tokio::fs::write(self.root.join(id).join("benchmark.json"), serde_json::to_vec_pretty(&result)?).await?;
        Ok(result)
    }

    pub fn start_benchmark(&self, id: &str) -> Result<()> {
        let model = catalog().into_iter().find(|m| m.id == id).context("unknown model")?;
        ensure!(self.ready(&model), "download the model first");
        let mut state = self.progress.lock().unwrap();
        ensure!(!state.values().any(|p| p.phase == "benchmarking"), "another benchmark is running");
        state.entry(id.into()).or_default().phase = "benchmarking".into();
        drop(state);
        let manager = self.clone();
        let id = id.to_owned();
        tokio::spawn(async move {
            let result = manager.benchmark(&id).await;
            let mut state = manager.progress.lock().unwrap();
            let item = state.entry(id.clone()).or_default();
            item.phase = "ready".into();
            item.error = result.as_ref().err().map(|e| format!("Benchmark failed: {e:#}"));
            if let Ok(b) = result
                && id != "potion"
                && b.meets_target
            {
                let current = manager
                    .selected()
                    .and_then(|m| std::fs::read(manager.root.join(m.id).join("benchmark.json")).ok())
                    .and_then(|v| serde_json::from_slice::<Benchmark>(&v).ok());
                if current.is_none_or(|old| b.complete_ms <= old.complete_ms) {
                    let _ = manager.select(&id);
                }
            }
        });
        Ok(())
    }
}

fn verify_file(path: &Path, expected: &str) -> Result<bool> {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()) == expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn resumes_partial_model_and_cancellation_preserves_it() {
        let root = tempfile::tempdir().unwrap();
        let manager = Manager::new(root.path().into());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let n = socket.read(&mut buf).await.unwrap();
            assert!(String::from_utf8_lossy(&buf[..n]).to_lowercase().contains("range: bytes=3-"));
            socket.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nConnection: close\r\n\r\ndef").await.unwrap();
        });
        let model = Model {
            id: "fixture".into(),
            name: "fixture".into(),
            repository: "fixture".into(),
            revision: "revision".into(),
            license: "test".into(),
            files: vec![ModelFile {
                name: "weights.bin".into(),
                url: format!("http://{addr}/weights"),
                sha256: format!("{:x}", Sha256::digest(b"abcdef")),
                bytes: 6,
            }],
        };
        std::fs::create_dir_all(root.path().join("fixture")).unwrap();
        std::fs::write(root.path().join("fixture/weights.part"), b"abc").unwrap();
        let cancelled = std::sync::atomic::AtomicBool::new(true);
        assert!(manager.download(&model, &cancelled).await.is_err());
        assert_eq!(std::fs::read(root.path().join("fixture/weights.part")).unwrap(), b"abc");
        cancelled.store(false, std::sync::atomic::Ordering::SeqCst);
        manager.download(&model, &cancelled).await.unwrap();
        server.await.unwrap();
        assert_eq!(std::fs::read(root.path().join("fixture/weights.bin")).unwrap(), b"abcdef");
        assert!(manager.ready(&model));
    }
    #[test]
    fn user_training_examples_are_deduplicated_and_bounded() {
        let root = tempfile::tempdir().unwrap();
        let manager = Manager::new(root.path().into());
        for i in 0..260 {
            manager
                .learn(classifier::Example {
                    text: format!("request {i}"),
                    task_type: "feature".into(),
                    size: "S".into(),
                    ambiguous: false,
                })
                .unwrap();
        }
        manager
            .learn(classifier::Example {
                text: "request 259".into(),
                task_type: "research".into(),
                size: "M".into(),
                ambiguous: true,
            })
            .unwrap();
        let examples: Vec<classifier::Example> =
            serde_json::from_slice(&std::fs::read(root.path().join("potion/examples.json")).unwrap()).unwrap();
        assert_eq!(examples.len(), 256);
        assert_eq!(examples.last().unwrap().task_type, "research");
        assert!(
            manager
                .learn(classifier::Example {
                    text: "x".into(),
                    task_type: "invalid".into(),
                    size: "S".into(),
                    ambiguous: false
                })
                .is_err()
        );
    }
    #[test]
    fn pinned_catalog_is_bounded_and_checksums_detect_corruption() {
        for m in catalog() {
            assert!(m.revision.len() == 40);
            for f in m.files {
                assert!(f.sha256.len() == 64 && f.bytes < 3_000_000_000);
                assert!(f.url.starts_with("https://huggingface.co/"));
                assert!(!f.name.contains(['/', '\\']));
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model");
        std::fs::write(&path, b"abc").unwrap();
        let expected = format!("{:x}", Sha256::digest(b"abc"));
        assert!(verify_file(&path, &expected).unwrap());
        std::fs::write(&path, b"bad").unwrap();
        assert!(!verify_file(&path, &expected).unwrap());
    }
}
