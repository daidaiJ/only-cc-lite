//! Hybrid scorer: delegates to BM25 in lite build (embeddings unavailable).

use super::base::{RelevanceScore, RelevanceScorer};
use super::bm25::BM25Scorer;

#[derive(Debug)]
pub struct HybridScorer {
    bm25: BM25Scorer,
}

impl Default for HybridScorer {
    fn default() -> Self {
        Self {
            bm25: BM25Scorer::default(),
        }
    }
}

impl RelevanceScorer for HybridScorer {
    fn score(&self, item: &str, context: &str) -> RelevanceScore {
        self.bm25.score(item, context)
    }

    fn score_batch(&self, items: &[&str], context: &str) -> Vec<RelevanceScore> {
        self.bm25.score_batch(items, context)
    }
}
