use arbiter_core::Assessment;
use model2vec_rs::model::StaticModel;
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Instant};

const SEEDS: &[(&str, &[&str])] = &[
    (
        "bugfix",
        &[
            "Fix the crash when submitting an empty login form",
            "Resolve incorrect totals in the shopping cart",
            "Repair the failing test and reproduce the error",
        ],
    ),
    (
        "feature",
        &[
            "Add a new user settings page",
            "Implement file uploads with progress",
            "Build a dashboard showing weekly sales",
        ],
    ),
    (
        "refactor",
        &[
            "Refactor the authentication module without changing behavior",
            "Extract duplicated helpers into a shared library",
            "Simplify complex functions and rename confusing variables",
        ],
    ),
    (
        "test",
        &[
            "Write unit tests for the parser",
            "Increase integration test coverage",
            "Add regression tests for input validation",
        ],
    ),
    (
        "docs",
        &[
            "Document the API endpoints",
            "Update the README installation instructions",
            "Write a tutorial for new contributors",
        ],
    ),
    (
        "research",
        &[
            "Research and compare database options",
            "Look up the latest framework documentation",
            "Investigate alternatives and recommend an approach",
        ],
    ),
    (
        "ops",
        &[
            "Configure continuous integration and deployment",
            "Set up monitoring for production",
            "Update the Docker build and release pipeline",
        ],
    ),
];

pub struct Classifier {
    model: StaticModel,
    centers: Vec<Vec<f32>>,
    size: Vec<Vec<f32>>,
    ambiguity: Vec<Vec<f32>>,
    risk: Vec<Vec<f32>>,
}

const SIZE: &[(&str, &[&str])] = &[
    (
        "S",
        &[
            "Change the button label to Save in Form.tsx",
            "Fix an off by one error in the parser",
            "Add a unit test for empty input",
        ],
    ),
    (
        "M",
        &[
            "Implement a settings page with validation and API integration",
            "Refactor the checkout module and its integration tests",
            "Add authentication to the existing application",
        ],
    ),
    (
        "L",
        &[
            "Build a complete multi tenant platform with billing and administration",
            "Rewrite the entire application architecture",
            "Migrate all services and databases to a new platform",
        ],
    ),
];
const AMBIGUITY: &[(&str, &[&str])] = &[
    (
        "clear",
        &[
            "Change the Save button label to Continue in src/Form.tsx",
            "Fix parser.rs so an empty string returns None and add a regression test",
            "Add a dark mode switch to Settings using the existing CSS theme tokens",
        ],
    ),
    ("unclear", &["Make it better", "Fix the issue", "Build something nice", "Improve the app", "Add a dashboard"]),
];
const RISK: &[(&str, &[&str])] = &[
    ("none", &["Change a button label", "Add a documentation example", "Write unit tests for formatting"]),
    (
        "authentication",
        &["Change login permissions and session handling", "Add OAuth authentication and access control"],
    ),
    (
        "database migration",
        &["Migrate the users table schema", "Drop a database column and backfill production records"],
    ),
    ("secrets", &["Rotate API keys and credentials", "Configure secret tokens in the environment"]),
    ("deployment", &["Deploy a new version to production", "Change the continuous integration release workflow"]),
];

#[derive(Clone, Serialize, Deserialize)]
pub struct Example {
    pub text: String,
    pub task_type: String,
    pub size: String,
    pub ambiguous: bool,
}

fn centers(
    model: &StaticModel,
    seeds: &[(&str, &[&str])],
    examples: &[Example],
    field: fn(&Example) -> &str,
) -> Vec<Vec<f32>> {
    seeds
        .iter()
        .map(|(label, samples)| {
            let mut texts: Vec<_> = samples.iter().map(|s| (*s).to_owned()).collect();
            texts.extend(examples.iter().filter(|e| field(e) == *label).map(|e| e.text.clone()));
            let vectors = model.encode(&texts);
            let mut mean = vec![0.0; vectors[0].len()];
            for v in &vectors {
                for (a, b) in mean.iter_mut().zip(v) {
                    *a += b;
                }
            }
            normalize(&mut mean);
            mean
        })
        .collect()
}
fn nearest(vector: &[f32], centers: &[Vec<f32>]) -> usize {
    centers
        .iter()
        .enumerate()
        .map(|(i, c)| (i, dot(vector, c)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(0, |(i, _)| i)
}

impl Classifier {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let model = StaticModel::from_pretrained(dir, None, Some(true), None)?;
        let examples: Vec<Example> = std::fs::read(dir.join("examples.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let examples = &examples[..examples.len().min(256)];
        let task = centers(&model, SEEDS, examples, |e| &e.task_type);
        let size = centers(&model, SIZE, examples, |e| &e.size);
        let ambiguity = centers(&model, AMBIGUITY, examples, |e| if e.ambiguous { "unclear" } else { "clear" });
        let risk = centers(&model, RISK, &[], |_| "");
        Ok(Self { model, centers: task, size, ambiguity, risk })
    }

    pub fn assess(&self, text: &str) -> Assessment {
        let start = Instant::now();
        let mut result = fallback(text);
        let bounded: String = text.chars().take(4000).collect();
        let vectors = self.model.encode(&[bounded]);
        if let Some(vector) = vectors.first() {
            result.task_type = SEEDS[nearest(vector, &self.centers)].0.into();
            result.size = SIZE[nearest(vector, &self.size)].0.into();
            result.ambiguity = if nearest(vector, &self.ambiguity) == 1 { 0.85 } else { result.ambiguity.min(0.4) };
            let risk = RISK[nearest(vector, &self.risk)].0;
            if risk != "none" && !result.risks.iter().any(|r| r == risk) {
                result.risks.push(risk.into());
            }
            result.needs_frontier = result.size != "S" || !result.risks.is_empty();
        }
        result.engine = "model2vec/potion-base-32M + seed-trained centroid heads".into();
        result.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        result
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn normalize(v: &mut [f32]) {
    let n = dot(v, v).sqrt();
    if n > 0.0 {
        for x in v {
            *x /= n;
        }
    }
}

/// Conservative, explicit fallback while local model files are unavailable.
pub fn fallback(text: &str) -> Assessment {
    let start = Instant::now();
    let lower = text.to_lowercase();
    let words: Vec<_> = lower.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()).collect();
    let has = |terms: &[&str]| terms.iter().any(|term| words.contains(term));
    let task_type = if has(&["research", "compare", "lookup", "investigate"]) {
        "research"
    } else if has(&["fix", "bug", "crash", "broken", "repair"]) {
        "bugfix"
    } else if has(&["refactor", "simplify", "reorganize"]) {
        "refactor"
    } else if has(&["test", "tests", "coverage"]) {
        "test"
    } else if has(&["docs", "document", "readme", "tutorial"]) {
        "docs"
    } else if has(&["deploy", "docker", "ci", "pipeline"]) {
        "ops"
    } else {
        "feature"
    };
    let risks = [
        ("database migration", &["migration", "migrations", "schema"][..]),
        ("authentication", &["auth", "authentication", "login", "oauth"][..]),
        ("secrets", &["secret", "secrets", "credentials", "token"][..]),
        ("deployment", &["production", "deploy", "ci"][..]),
    ]
    .into_iter()
    .filter(|(_, terms)| has(terms))
    .map(|(label, _)| label.to_owned())
    .collect::<Vec<_>>();
    let size = if has(&["platform", "entire", "rewrite", "fullstack"]) || words.len() > 160 {
        "L"
    } else if words.len() > 45 || !risks.is_empty() {
        "M"
    } else {
        "S"
    };
    let ambiguous = words.len() < 7 || has(&["something", "somehow", "better", "improve"]) && words.len() < 25;
    Assessment {
        task_type: task_type.into(),
        size: size.into(),
        ambiguity: if ambiguous { 0.85 } else { 0.2 },
        needs_frontier: size != "S" || !risks.is_empty(),
        risks,
        engine: "rules (local model not loaded)".into(),
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinguishes_ambiguous_requests_and_flags_sensitive_scope() {
        assert!(fallback("make it better").ambiguity > 0.5);
        assert!(fallback("Change the Save label to Continue in src/Form.tsx").ambiguity < 0.5);
        assert!(fallback("Add authentication and database migrations for users").needs_frontier);
        assert_eq!(fallback("Research and compare database engines").task_type, "research");
    }
}
