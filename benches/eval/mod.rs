//! Evaluation harness for only-cc-lite.
//!
//! Discovers session logs from Claude Code and Qwen Code,
//! replays them through the compressor, and produces a summary report.

pub mod discovery;
pub mod parser;
pub mod converter;
pub mod evaluator;
pub mod report;
