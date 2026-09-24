//! Local task assessment and bounded clarification. Model setup is explicit;
//! an unavailable model never masquerades as a neural classifier.
pub mod classifier;
pub mod models;
pub mod questions;
/// The embedded llama.cpp runtime (used by examples and benchmarks).
pub mod runtime;
