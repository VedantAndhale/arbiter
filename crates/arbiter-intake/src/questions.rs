use anyhow::{Result, ensure};
use arbiter_core::{Answer, Assessment, IntentSpec, Question, QuestionKind};

pub fn fallback(assessment: &Assessment, more: bool) -> Vec<Question> {
    if assessment.ambiguity < 0.6 && !more {
        return vec![];
    }
    let prompts = if more {
        [
            ("constraints", "Constraints", "Are there compatibility, design, or performance constraints?"),
            ("acceptance", "Acceptance", "How should we verify the result is correct?"),
        ]
    } else {
        [
            ("outcome", "Outcome", "What specific result should this task produce?"),
            ("scope", "Scope", "Which page, feature, or files should the agent focus on?"),
        ]
    };
    prompts
        .into_iter()
        .map(|(id, header, question)| Question {
            id: id.into(),
            header: header.into(),
            question: question.into(),
            kind: QuestionKind::Short,
            options: vec![],
            recommended: None,
        })
        .collect()
}

pub fn validate(questions: &[Question]) -> Result<()> {
    ensure!(!questions.is_empty() && questions.len() <= 4, "expected 1–4 questions");
    let mut ids = std::collections::HashSet::new();
    for q in questions {
        ensure!(!q.id.is_empty() && q.id.len() <= 64 && ids.insert(&q.id), "invalid or duplicate question id");
        ensure!(
            !q.question.trim().is_empty() && q.question.len() <= 500 && q.header.len() <= 48,
            "question exceeds bounds"
        );
        ensure!(
            q.options.len() <= 6 && (q.kind == QuestionKind::Short || q.options.len() >= 2),
            "invalid question options"
        );
        ensure!(q.recommended.is_none_or(|i| i < q.options.len()), "invalid recommendation");
        for o in &q.options {
            ensure!(
                !o.label.trim().is_empty() && o.label.len() <= 100 && o.description.len() <= 250,
                "option exceeds bounds"
            );
        }
    }
    Ok(())
}

pub fn validate_answers(questions: &[Question], answers: &[Answer]) -> Result<()> {
    ensure!(answers.len() == questions.len(), "answer every question before continuing");
    let mut ids = std::collections::HashSet::new();
    for a in answers {
        ensure!(
            ids.insert(&a.question_id) && questions.iter().any(|q| q.id == a.question_id),
            "unknown or duplicate answer"
        );
        ensure!(!a.text.trim().is_empty() && a.text.len() <= 1500, "answers must contain 1–1500 bytes");
    }
    Ok(())
}

pub fn compile(
    request: &str,
    answers: Vec<Answer>,
    context_paths: Vec<String>,
    assessment: &Assessment,
) -> Result<IntentSpec> {
    ensure!(!request.trim().is_empty() && request.len() <= 12000, "request must contain 1–12000 bytes");
    ensure!(
        answers.len() <= 8 && answers.iter().map(|a| a.text.len()).sum::<usize>() <= 6000,
        "clarification exceeds context budget"
    );
    ensure!(context_paths.len() <= 8 && context_paths.iter().all(|p| p.len() <= 300), "at most 8 context paths");
    Ok(IntentSpec {
        request: request.into(),
        answers,
        context_paths,
        task_type: assessment.task_type.clone(),
        size: assessment.size.clone(),
        risks: assessment.risks.clone(),
        needs_frontier: assessment.needs_frontier,
    })
}

pub fn brief(spec: &IntentSpec) -> String {
    let mut out = spec.request.clone();
    if !spec.answers.is_empty() {
        out.push_str("\n\nUser clarifications:\n");
    }
    for a in &spec.answers {
        out.push_str(&format!("- {}: {}\n", a.question_id, a.text));
    }
    if !spec.context_paths.is_empty() {
        out.push_str("\nReferenced project paths (read only as needed):\n");
    }
    for path in &spec.context_paths {
        out.push_str(&format!("- {path}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn answers_are_complete_unique_and_bounded() {
        let q = fallback(&crate::classifier::fallback("fix it"), false);
        assert!(validate(&q).is_ok());
        assert!(validate_answers(&q, &[]).is_err());
        let answers: Vec<_> =
            q.iter().map(|q| Answer { question_id: q.id.clone(), text: "Checkout submit".into() }).collect();
        assert!(validate_answers(&q, &answers).is_ok());
        let mut duplicate = answers.clone();
        duplicate[1] = duplicate[0].clone();
        assert!(validate_answers(&q, &duplicate).is_err());
    }
}
