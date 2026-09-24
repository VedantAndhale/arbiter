use crate::AppState;
use anyhow::{Context, Result, ensure};
use arbiter_core::{Answer, Assessment, EventKind, Question, ThreadId, ThreadStatus};
use arbiter_intake::questions;
use std::path::{Component, Path};

pub(crate) fn validate_paths(root: &Path, paths: &[String]) -> Result<Vec<String>> {
    ensure!(paths.len() <= 8, "at most 8 context paths");
    let root = root.canonicalize()?;
    let mut result = Vec::new();
    for path in paths {
        ensure!(path.len() <= 300 && !path.contains(['\n', '\r']) && !path.is_empty(), "invalid context path");
        ensure!(
            Path::new(path).components().all(|c| matches!(c, Component::Normal(_))),
            "use project-relative context paths"
        );
        let resolved = root.join(path).canonicalize().context("context path does not exist")?;
        ensure!(
            resolved.starts_with(&root) && (resolved.is_file() || resolved.is_dir()),
            "context path is outside the project or not a file or folder"
        );
        let normalized = path.replace('\\', "/").trim_end_matches('/').to_owned();
        ensure!(!sensitive(&normalized), "secret/configuration files cannot be mentioned");
        if !result.contains(&normalized) {
            result.push(normalized);
        }
    }
    Ok(result)
}

const APPROACH: &str = "approach";

/// Asked when a request is neither clearly small nor clearly large.
fn approach_card() -> Question {
    Question {
        id: APPROACH.into(),
        header: "Approach".into(),
        question: "This is a medium-sized change. How should Arbiter start?".into(),
        kind: arbiter_core::QuestionKind::Single,
        options: vec![
            arbiter_core::QuestionOption {
                label: "Plan it first".into(),
                description: "You approve a short plan; then agents work in isolated branches.".into(),
            },
            arbiter_core::QuestionOption {
                label: "Just start".into(),
                description: "One agent starts now in its own branch. You can steer it.".into(),
            },
        ],
        recommended: Some(0),
    }
}

pub(crate) fn sensitive(path: &str) -> bool {
    path.split('/').any(|part| {
        let p = part.to_lowercase();
        p == ".git"
            || p.starts_with(".env")
            || p.ends_with(".pem")
            || p.ends_with(".key")
            || p.contains("credentials")
            || p == "id_rsa"
    })
}

struct Pending {
    id: String,
    questions: Vec<Question>,
    request: String,
    paths: Vec<String>,
    attachments: Vec<String>,
    assessment: Assessment,
    previous: Vec<Answer>,
}

impl AppState {
    pub(crate) async fn correct_intake(
        &self,
        thread: ThreadId,
        task_type: String,
        size: String,
        ambiguous: bool,
    ) -> Result<()> {
        let _guard = self.inner.intake_lock.lock().await;
        let mut p = self.pending_intake(thread)?;
        ensure!(
            ["bugfix", "feature", "refactor", "test", "docs", "research", "ops"].contains(&task_type.as_str())
                && ["S", "M", "L"].contains(&size.as_str()),
            "unknown intake labels"
        );
        p.assessment.task_type = task_type.clone();
        p.assessment.size = size.clone();
        p.assessment.ambiguity = if ambiguous { 0.85 } else { 0.2 };
        p.assessment.needs_frontier = size != "S" || !p.assessment.risks.is_empty();
        p.assessment.engine = "user correction".into();
        self.append(thread, EventKind::IntakeAssessed { assessment: p.assessment })?;
        let manager = self.inner.intake.clone();
        tokio::task::spawn_blocking(move || {
            manager.learn(arbiter_intake::classifier::Example { text: p.request, task_type, size, ambiguous })
        })
        .await??;
        Ok(())
    }
    pub(crate) async fn begin_intake(
        &self,
        thread: ThreadId,
        request: String,
        paths: Vec<String>,
        attachments: Vec<String>,
    ) -> Result<()> {
        let assessment = self.inner.intake.assess(request.clone()).await;
        self.append(thread, EventKind::IntakeAssessed { assessment: assessment.clone() })?;
        let mut cards = self.inner.intake.questions(&request, &assessment, false).await;
        if self.store(|st| st.events(thread, 0))?.iter().any(|e| matches!(e.kind, EventKind::ApproachAsked)) {
            cards.truncate(3);
            cards.insert(0, approach_card());
        }
        if cards.is_empty() {
            self.finish_intake(thread, &request, vec![], paths, attachments, &assessment).await
        } else {
            self.append(
                thread,
                EventKind::QuestionsAsked {
                    id: format!("intake-{}", ThreadId::new()),
                    questions: cards,
                    request,
                    context_paths: paths,
                    attachments,
                },
            )?;
            self.append(thread, EventKind::StatusChanged { status: ThreadStatus::NeedsApproval })?;
            Ok(())
        }
    }

    fn pending_intake(&self, thread: ThreadId) -> Result<Pending> {
        self.store(|st| st.thread(thread))?.context("thread not found")?;
        let events = self.store(|st| st.events(thread, 0))?;
        let mut pending = None;
        let mut assessment = None;
        let mut previous = vec![];
        for e in events {
            match e.kind {
                EventKind::IntakeAssessed { assessment: a } => assessment = Some(a),
                EventKind::QuestionsAsked { id, questions, request, context_paths, attachments } => {
                    pending = Some((id, questions, request, context_paths, attachments))
                }
                EventKind::QuestionsAnswered { id, answers } => {
                    if pending.as_ref().is_some_and(|p| p.0 == id) {
                        pending = None;
                    }
                    previous.extend(answers);
                }
                EventKind::IntentReady { .. } => {
                    pending = None;
                    previous.clear();
                }
                _ => {}
            }
        }
        let (id, questions, request, paths, attachments) = pending.context("no pending clarification")?;
        Ok(Pending {
            id,
            questions,
            request,
            paths,
            attachments,
            assessment: assessment.context("missing assessment")?,
            previous,
        })
    }

    pub(crate) fn has_pending_intake(&self, thread: ThreadId) -> bool {
        self.pending_intake(thread).is_ok()
    }

    pub(crate) async fn answer_intake(
        &self,
        thread: ThreadId,
        id: &str,
        answers: Vec<Answer>,
        more: bool,
    ) -> Result<()> {
        // Serializes answer submission so double-clicks cannot start two runs.
        let _guard = self.inner.intake_lock.lock().await;
        let p = self.pending_intake(thread)?;
        ensure!(p.id == id, "this clarification has already changed");
        questions::validate_answers(&p.questions, &answers)?;
        ensure!(!more || p.previous.is_empty(), "only one additional clarification round is available");
        let mut all = p.previous;
        all.extend(answers.clone());
        ensure!(all.len() <= 8, "clarification limit reached");
        questions::compile(&p.request, all.clone(), p.paths.clone(), &p.assessment)?;
        let extra = if more {
            ensure!(all.len() <= 4, "only one additional clarification round is available");
            let context = format!("{}\nAnswers already supplied: {}", p.request, serde_json::to_string(&all)?);
            self.inner.intake.questions(&context, &p.assessment, true).await
        } else {
            vec![]
        };
        self.append(thread, EventKind::QuestionsAnswered { id: p.id, answers })?;
        if !extra.is_empty() {
            self.append(
                thread,
                EventKind::QuestionsAsked {
                    id: format!("intake-{}", ThreadId::new()),
                    questions: extra,
                    request: p.request,
                    context_paths: p.paths,
                    attachments: p.attachments,
                },
            )?;
            return Ok(());
        }
        self.finish_intake(thread, &p.request, all, p.paths, p.attachments, &p.assessment).await
    }

    async fn finish_intake(
        &self,
        thread: ThreadId,
        request: &str,
        answers: Vec<Answer>,
        paths: Vec<String>,
        attachments: Vec<String>,
        assessment: &Assessment,
    ) -> Result<()> {
        // The approach answer steers Arbiter; it is not part of the brief.
        let plan = answers.iter().any(|a| a.question_id == APPROACH && a.text.starts_with("Plan"));
        let answers: Vec<Answer> = answers.into_iter().filter(|a| a.question_id != APPROACH).collect();
        if plan
            && !self.store(|st| st.events(thread, 0))?.iter().any(|e| matches!(e.kind, EventKind::WorkflowRequested))
        {
            self.append(thread, EventKind::WorkflowRequested)?;
        }
        let spec = questions::compile(request, answers, paths, assessment)?;
        let text = questions::brief(&spec);
        if self.store(|st| st.thread(thread))?.is_some_and(|t| t.tool_profile == arbiter_core::ToolProfile::Auto) {
            self.append(
                thread,
                EventKind::ToolProfileChanged {
                    profile: if assessment.task_type == "research" {
                        arbiter_core::ToolProfile::Research
                    } else {
                        arbiter_core::ToolProfile::Implementation
                    },
                },
            )?;
        }
        self.append(thread, EventKind::IntentReady { spec: spec.clone() })?;
        if self.store(|st| st.events(thread, 0))?.iter().any(|e| matches!(e.kind, EventKind::WorkflowRequested)) {
            return self.draft_plan(thread, &spec, &attachments).await;
        }
        // Failure is already persisted by send_message, leaving the intent inspectable.
        let _ = self.send_message(thread, text, &attachments).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn context_paths_reject_parent_traversal_and_secrets() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("code.rs"), "").unwrap();
        std::fs::write(dir.path().join(".env"), "TOKEN=example").unwrap();
        assert_eq!(validate_paths(dir.path(), &["code.rs".into(), "code.rs".into()]).unwrap(), ["code.rs"]);
        assert!(validate_paths(dir.path(), &["../outside".into()]).is_err());
        assert!(validate_paths(dir.path(), &[".env".into()]).is_err());
        // Folders can be mentioned too; a trailing slash is normalized away.
        std::fs::create_dir(dir.path().join("src")).unwrap();
        assert_eq!(validate_paths(dir.path(), &["src/".into()]).unwrap(), ["src"]);
    }
}
