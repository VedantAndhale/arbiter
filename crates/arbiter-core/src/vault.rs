use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    /// Stable category/slug, without extension.
    pub id: String,
    pub title: String,
    pub body: String,
    pub revision: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub task_type: String,
    pub harness: String,
    pub model: Option<String>,
    pub passed: bool,
    #[serde(default)]
    pub checks_run: usize,
    pub first_try: bool,
    pub heal_attempts: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub user_rework: bool,
}
