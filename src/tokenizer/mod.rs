//! Token counting — lite build (estimation only, no tiktoken/HF deps).

mod estimator;
mod registry;

pub use estimator::EstimatingCounter;
pub use registry::{detect_backend, get_tokenizer, Backend};

/// Counts tokens. Implementations must be thread-safe (`Send + Sync`).
pub trait Tokenizer: Send + Sync + std::fmt::Debug {
    /// Number of tokens that this tokenizer assigns to `text`.
    fn count_text(&self, text: &str) -> usize;

    /// Which backend produced the count.
    fn backend(&self) -> Backend;
}
