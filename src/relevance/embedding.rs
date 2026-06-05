//! Stub: embedding scorer disabled in lite build (no ONNX/fastembed dep).

use super::base::{RelevanceScore, RelevanceScorer};

#[derive(Debug, Default)]
pub struct EmbeddingScorer;

impl EmbeddingScorer {
    pub fn is_available(&self) -> bool {
        false
    }
}

impl RelevanceScorer for EmbeddingScorer {
    fn score(&self, _item: &str, _context: &str) -> RelevanceScore {
        RelevanceScore::empty("embedding scorer unavailable in lite build")
    }

    fn is_available(&self) -> bool {
        false
    }
}
