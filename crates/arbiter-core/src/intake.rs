use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolProfile {
    Auto,
    Implementation,
    #[default]
    Research,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Assessment {
    pub task_type: String,
    pub size: String,
    pub ambiguity: f32,
    pub risks: Vec<String>,
    pub needs_frontier: bool,
    pub engine: String,
    pub elapsed_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionKind {
    Single,
    Multi,
    Short,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub header: String,
    pub question: String,
    pub kind: QuestionKind,
    pub options: Vec<QuestionOption>,
    pub recommended: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub question_id: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IntentSpec {
    pub request: String,
    pub answers: Vec<Answer>,
    pub context_paths: Vec<String>,
    pub task_type: String,
    pub size: String,
    pub risks: Vec<String>,
    pub needs_frontier: bool,
}
