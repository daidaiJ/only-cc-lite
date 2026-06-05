//! # only-cc-lite
//!
//! Lightweight context compression extracted from
//! [Headroom](https://github.com/chopratejas/headroom).
//!
//! Zero ML dependencies (no ONNX, no fastembed, no HuggingFace tokenizers).
//! Designed for embedding into proxy servers and Tauri desktop apps.
//!
//! ## Quick start
//!
//! ```rust
//! use only_cc_lite::{compress_request, Provider, CcrBackendConfig};
//!
//! // Initialize CCR store (optional, for reversible compression)
//! let ccr = only_cc_lite::ccr::backends::from_config(
//!     &CcrBackendConfig::in_memory_default()
//! ).unwrap();
//!
//! // Compress an Anthropic request body
//! let body = br#"{"model":"claude-sonnet-4-5-20250929","messages":[{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":[{"type":"text","text":"[{\"id\":1},{\"id\":2},{\"id\":3}]"}]}]}]}"#;
//! let outcome = compress_request(body, Provider::Anthropic, "claude-sonnet-4-5-20250929", Some(ccr.as_ref())).unwrap();
//!
//! if let Some(compressed) = &outcome.body {
//!     println!("Saved {} tokens ({} bytes)", outcome.tokens_saved, outcome.bytes_saved);
//!     println!("Strategies: {:?}", outcome.strategies);
//! }
//! ```

pub mod auth_mode;
pub mod ccr;
pub mod relevance;
pub mod signals;
pub mod tokenizer;
pub mod transforms;

use transforms::live_zone;

// ─── Re-exports for convenience ────────────────────────────────────────

pub use ccr::{CcrStore, DEFAULT_CAPACITY, DEFAULT_TTL};
pub use ccr::backends::CcrBackendConfig;
pub use tokenizer::{get_tokenizer, EstimatingCounter, Tokenizer};
pub use transforms::{
    ContentType, DiffCompressor, LogCompressor, SearchCompressor, SmartCrusher,
};

// ─── Proxy-friendly API ────────────────────────────────────────────────

/// Which LLM provider's request format to compress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// Anthropic Messages API (`POST /v1/messages`).
    Anthropic,
    /// OpenAI Chat Completions (`POST /v1/chat/completions`).
    OpenAiChat,
    /// OpenAI Responses API (`POST /v1/responses`).
    OpenAiResponses,
}

/// Per-block compression report.
#[derive(Debug, Clone)]
pub struct BlockReport {
    pub message_index: usize,
    pub block_index: Option<usize>,
    pub block_type: String,
    pub strategy: Option<&'static str>,
    pub original_bytes: usize,
    pub compressed_bytes: usize,
    pub original_tokens: usize,
    pub compressed_tokens: usize,
}

/// Outcome of compressing a single request body.
#[derive(Debug)]
pub struct CompressOutcome {
    /// The compressed body bytes. `None` when no compression was applied
    /// (forward the original body unchanged).
    pub body: Option<Vec<u8>>,
    /// Total tokens saved across all compressed blocks.
    pub tokens_saved: usize,
    /// Total bytes saved across all compressed blocks.
    pub bytes_saved: usize,
    /// Compressor strategies that produced output (e.g. `["smart_crusher"]`).
    pub strategies: Vec<&'static str>,
    /// Per-block detail for observability.
    pub per_block: Vec<BlockReport>,
}

impl CompressOutcome {
    /// Compression ratio: `compressed_bytes / original_bytes`.
    /// Returns 1.0 when no compression was applied.
    /// Lower = better (0.3 means kept 30% of original).
    pub fn compression_ratio(&self) -> f64 {
        if self.bytes_saved == 0 {
            return 1.0;
        }
        let original = self.bytes_saved + self.body.as_ref().map(|b| b.len()).unwrap_or(0);
        if original == 0 {
            return 1.0;
        }
        (original - self.bytes_saved) as f64 / original as f64
    }

    /// Empty passthrough outcome — used for error degradation.
    pub fn passthrough() -> Self {
        Self {
            body: None,
            tokens_saved: 0,
            bytes_saved: 0,
            strategies: Vec::new(),
            per_block: Vec::new(),
        }
    }
}

/// Compress a buffered LLM request body.
///
/// This is the single entry point for proxy middleware. It:
/// 1. Parses the JSON body
/// 2. Identifies the live zone (latest user/tool messages)
/// 3. Detects content types via regex (JSON arrays, diffs, logs, search results)
/// 4. Dispatches to the appropriate compressor (SmartCrusher, LogCompressor, etc.)
/// 5. Validates compression actually reduced tokens (estimation-based)
/// 6. Optionally injects CCR markers for reversible compression
/// 7. Returns the compressed body + detailed metrics
///
/// Returns `Ok(CompressOutcome)` even when no compression was applied
/// (outcome.body will be None). Errors only on invalid JSON input.
pub fn compress_request(
    body: &[u8],
    provider: Provider,
    model: &str,
    ccr_store: Option<&dyn CcrStore>,
) -> Result<CompressOutcome, only_cc_error::CompressError> {
    let auth_mode = auth_mode::AuthMode::Payg;

    let outcome = match provider {
        Provider::Anthropic => {
            let frozen = compute_frozen_count_from_body(body);
            if let Some(store) = ccr_store {
                live_zone::compress_anthropic_live_zone_with_ccr(body, frozen, auth_mode.into(), model, Some(store))
            } else {
                live_zone::compress_anthropic_live_zone(body, frozen, auth_mode.into(), model)
            }
        }
        Provider::OpenAiChat => {
            live_zone::compress_openai_chat_live_zone(body, auth_mode.into(), model)
        }
        Provider::OpenAiResponses => {
            live_zone::compress_openai_responses_live_zone(body, auth_mode.into(), model)
        }
    };

    match outcome {
        Ok(live_zone::LiveZoneOutcome::Modified { new_body, manifest }) => {
            let original_len = body.len();
            let compressed_bytes = new_body.get().as_bytes();
            let compressed_len = compressed_bytes.len();
            let tokens_saved = manifest.tokens_saved();
            let strategies = manifest.transforms_applied();
            let per_block = manifest
                .block_outcomes
                .iter()
                .map(|b| BlockReport {
                    message_index: b.message_index,
                    block_index: b.block_index,
                    block_type: b.block_type.clone(),
                    strategy: match &b.action {
                        live_zone::BlockAction::Compressed { strategy, .. } => Some(*strategy),
                        _ => None,
                    },
                    original_bytes: match &b.action {
                        live_zone::BlockAction::Compressed { original_bytes, .. } => *original_bytes,
                        _ => 0,
                    },
                    compressed_bytes: match &b.action {
                        live_zone::BlockAction::Compressed { compressed_bytes, .. } => *compressed_bytes,
                        _ => 0,
                    },
                    original_tokens: match &b.action {
                        live_zone::BlockAction::Compressed { original_tokens, .. } => *original_tokens,
                        _ => 0,
                    },
                    compressed_tokens: match &b.action {
                        live_zone::BlockAction::Compressed { compressed_tokens, .. } => *compressed_tokens,
                        _ => 0,
                    },
                })
                .collect();

            Ok(CompressOutcome {
                body: Some(compressed_bytes.to_vec()),
                tokens_saved,
                bytes_saved: original_len.saturating_sub(compressed_len),
                strategies,
                per_block,
            })
        }
        Ok(live_zone::LiveZoneOutcome::NoChange { manifest }) => {
            Ok(CompressOutcome {
                body: None,
                tokens_saved: 0,
                bytes_saved: 0,
                strategies: Vec::new(),
                per_block: manifest
                    .block_outcomes
                    .iter()
                    .map(|b| BlockReport {
                        message_index: b.message_index,
                        block_index: b.block_index,
                        block_type: b.block_type.clone(),
                        strategy: None,
                        original_bytes: 0,
                        compressed_bytes: 0,
                        original_tokens: 0,
                        compressed_tokens: 0,
                    })
                    .collect(),
            })
        }
        Err(e) => Err(only_cc_error::CompressError::LiveZone(e)),
    }
}

/// Errors from the compression API.
pub mod only_cc_error {
    use crate::transforms::live_zone::LiveZoneError;

    #[derive(Debug, thiserror::Error)]
    pub enum CompressError {
        #[error("live zone compression error: {0}")]
        LiveZone(#[from] LiveZoneError),
    }
}

// ─── Helper: frozen count from body ────────────────────────────────────

impl ccr::CcrStore for () {
    fn put(&self, _hash: &str, _payload: &str) {}
    fn get(&self, _hash: &str) -> Option<String> { None }
    fn len(&self) -> usize { 0 }
}

/// Compute frozen message count from a request body.
///
/// Scans `messages[*].content[*].cache_control` markers to find the
/// highest marked message index. Returns 0 when no markers are found
/// (the common case for OpenAI and Anthropic without explicit markers).
fn compute_frozen_count_from_body(body: &[u8]) -> usize {
    let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(body) else {
        return 0;
    };
    let Some(messages) = parsed.get("messages").and_then(|v| v.as_array()) else {
        return 0;
    };
    let mut highest: Option<usize> = None;
    for (i, message) in messages.iter().enumerate() {
        let Some(content) = message.get("content") else { continue };
        let Some(blocks) = content.as_array() else { continue };
        for block in blocks {
            if block.get("cache_control").is_some() {
                highest = Some(i);
            }
        }
    }
    highest.map(|i| i + 1).unwrap_or(0)
}
