//! Compression evaluator — runs parsed sessions through the compressor.
//!
//! **Incremental model**: compresses only user-side content (tool results,
//! file reads, search output). Assistant messages are passed through untouched
//! so prefix cache remains valid.

use only_cc_lite::{compress_request, Provider};

use super::converter;
use super::discovery::SessionSource;
use super::parser::{ParsedSession, ParsedTurn};

/// Result of compressing a single turn.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TurnResult {
    pub turn_index: usize,
    /// User content bytes (the compressible part).
    pub user_content_bytes: usize,
    /// Assistant content bytes (kept intact, not compressed).
    pub assistant_content_bytes: usize,
    pub original_input_tokens: u64,
    pub original_output_tokens: u64,
    /// Compressed size of user content, if compression was applied.
    pub compressed_user_bytes: Option<usize>,
    pub tokens_saved: usize,
    pub bytes_saved: usize,
    pub strategies: Vec<&'static str>,
    /// Compression ratio of user content only (compressed / original).
    pub user_compression_ratio: f64,
}

/// Result of evaluating a full session.
#[derive(Debug, Clone)]
pub struct EvalResult {
    pub source: SessionSource,
    pub session_id: String,
    pub model: String,
    pub total_turns: usize,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_user_bytes: usize,
    pub total_assistant_bytes: usize,
    pub total_tokens_saved: usize,
    pub total_bytes_saved: usize,
    pub avg_user_compression_ratio: f64,
    pub turn_results: Vec<TurnResult>,
}

/// Aggregate summary across all evaluated sessions.
#[derive(Debug, Clone)]
pub struct EvalSummary {
    pub sessions_evaluated: usize,
    pub total_turns: usize,
    pub total_input_tokens: u64,
    pub total_user_bytes: usize,
    pub total_assistant_bytes: usize,
    pub total_tokens_saved: usize,
    pub total_bytes_saved: usize,
    pub overall_user_compression_ratio: f64,
    pub by_source: Vec<(SessionSource, usize, usize, f64)>,
    pub results: Vec<EvalResult>,
}

impl EvalResult {
    fn from_session(session: &ParsedSession, turn_results: Vec<TurnResult>) -> Self {
        let total_input_tokens: u64 = turn_results.iter().map(|t| t.original_input_tokens).sum();
        let total_output_tokens: u64 = turn_results.iter().map(|t| t.original_output_tokens).sum();
        let total_user_bytes: usize = turn_results.iter().map(|t| t.user_content_bytes).sum();
        let total_assistant_bytes: usize = turn_results.iter().map(|t| t.assistant_content_bytes).sum();
        let total_tokens_saved: usize = turn_results.iter().map(|t| t.tokens_saved).sum();
        let total_bytes_saved: usize = turn_results.iter().map(|t| t.bytes_saved).sum();
        let avg_user_compression_ratio = if !turn_results.is_empty() {
            turn_results
                .iter()
                .map(|t| t.user_compression_ratio)
                .sum::<f64>()
                / turn_results.len() as f64
        } else {
            1.0
        };

        Self {
            source: session.source,
            session_id: session.session_id.clone(),
            model: session.model.clone(),
            total_turns: session.turns.len(),
            total_input_tokens,
            total_output_tokens,
            total_user_bytes,
            total_assistant_bytes,
            total_tokens_saved,
            total_bytes_saved,
            avg_user_compression_ratio,
            turn_results,
        }
    }
}

/// Evaluate a single session using accumulated requests.
///
/// Each turn N is evaluated as an accumulated request containing all messages
/// from turns 0..=N. This lets the compressor's live-zone logic see the full
/// conversation context and target only the latest user message for compression.
pub fn evaluate_session(session: &ParsedSession) -> EvalResult {
    let provider = match session.source {
        SessionSource::ClaudeCode => Provider::Anthropic,
        SessionSource::QwenCode => Provider::OpenAiChat,
    };

    let model = if session.model.is_empty() {
        match session.source {
            SessionSource::ClaudeCode => "claude-sonnet-4-20250514",
            SessionSource::QwenCode => "mimo-v2.5-pro",
        }
    } else {
        &session.model
    };

    let source = match session.source {
        SessionSource::ClaudeCode => SessionSource::ClaudeCode,
        SessionSource::QwenCode => SessionSource::QwenCode,
    };

    // Build accumulated requests: turn N has history 0..=N
    let accumulated_requests = converter::turns_to_requests(&session.turns, source, model);

    let turn_results: Vec<TurnResult> = accumulated_requests
        .iter()
        .enumerate()
        .map(|(i, body)| {
            let turn = &session.turns[i];
            evaluate_accumulated_turn(i, turn, body, provider, model)
        })
        .collect();

    EvalResult::from_session(session, turn_results)
}

/// Evaluate a single accumulated turn.
///
/// `body` is the full accumulated request (0..=N messages). The compressor's
/// live-zone logic targets only the latest user message for compression while
/// leaving frozen history (assistant messages + earlier turns) untouched.
fn evaluate_accumulated_turn(
    index: usize,
    turn: &ParsedTurn,
    body: &[u8],
    provider: Provider,
    model: &str,
) -> TurnResult {
    let user_content_bytes = turn.user_content.len();
    let assistant_content_bytes = turn.assistant_content.len();

    match compress_request(body, provider, model, None) {
        Ok(outcome) => {
            let user_compression_ratio = if outcome.bytes_saved > 0 && user_content_bytes > 0 {
                let original = user_content_bytes;
                let compressed = original.saturating_sub(outcome.bytes_saved);
                compressed as f64 / original as f64
            } else {
                1.0
            };

            TurnResult {
                turn_index: index,
                user_content_bytes,
                assistant_content_bytes,
                original_input_tokens: turn.input_tokens,
                original_output_tokens: turn.output_tokens,
                compressed_user_bytes: outcome.body.as_ref().map(|b| b.len()),
                tokens_saved: outcome.tokens_saved,
                bytes_saved: outcome.bytes_saved,
                strategies: outcome.strategies,
                user_compression_ratio,
            }
        }
        Err(_) => TurnResult {
            turn_index: index,
            user_content_bytes,
            assistant_content_bytes,
            original_input_tokens: turn.input_tokens,
            original_output_tokens: turn.output_tokens,
            compressed_user_bytes: None,
            tokens_saved: 0,
            bytes_saved: 0,
            strategies: vec![],
            user_compression_ratio: 1.0,
        },
    }
}

/// Evaluate multiple sessions and produce an aggregate summary.
pub fn evaluate_sessions(sessions: &[ParsedSession]) -> EvalSummary {
    let results: Vec<EvalResult> = sessions.iter().map(evaluate_session).collect();

    let total_turns: usize = results.iter().map(|r| r.total_turns).sum();
    let total_input_tokens: u64 = results.iter().map(|r| r.total_input_tokens).sum();
    let total_user_bytes: usize = results.iter().map(|r| r.total_user_bytes).sum();
    let total_assistant_bytes: usize = results.iter().map(|r| r.total_assistant_bytes).sum();
    let total_tokens_saved: usize = results.iter().map(|r| r.total_tokens_saved).sum();
    let total_bytes_saved: usize = results.iter().map(|r| r.total_bytes_saved).sum();

    let overall_user_compression_ratio = if total_user_bytes > 0 {
        let compressed = total_user_bytes.saturating_sub(total_bytes_saved);
        compressed as f64 / total_user_bytes as f64
    } else {
        1.0
    };

    let mut by_source: Vec<(SessionSource, usize, usize, f64)> = Vec::new();
    for source in [SessionSource::ClaudeCode, SessionSource::QwenCode] {
        let source_results: Vec<_> = results.iter().filter(|r| r.source == source).collect();
        if !source_results.is_empty() {
            let sessions_count = source_results.len();
            let tokens_saved: usize = source_results.iter().map(|r| r.total_tokens_saved).sum();
            let avg_ratio = source_results
                .iter()
                .map(|r| r.avg_user_compression_ratio)
                .sum::<f64>()
                / sessions_count as f64;
            by_source.push((source, sessions_count, tokens_saved, avg_ratio));
        }
    }

    EvalSummary {
        sessions_evaluated: results.len(),
        total_turns,
        total_input_tokens,
        total_user_bytes,
        total_assistant_bytes,
        total_tokens_saved,
        total_bytes_saved,
        overall_user_compression_ratio,
        by_source,
        results,
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_evaluate_all_sessions() {
        let discoveries = super::super::discovery::SessionDiscovery::discover_all();
        let sessions: Vec<ParsedSession> = discoveries
            .iter()
            .filter_map(|d| ParsedSession::parse(&d.path, d.source).ok())
            .filter(|s| !s.turns.is_empty())
            .collect();

        if sessions.is_empty() {
            println!("No parseable sessions found");
            return;
        }

        let summary = evaluate_sessions(&sessions);

        println!("\n{}", "=".repeat(60));
        println!("Compression Evaluation Summary (Incremental Model)");
        println!("{}", "=".repeat(60));
        println!("Sessions evaluated:      {}", summary.sessions_evaluated);
        println!("Total turns:             {}", summary.total_turns);
        println!("Total input tokens:      {}", summary.total_input_tokens);
        println!("User content bytes:      {}", summary.total_user_bytes);
        println!("Assistant content bytes: {} (not compressed)", summary.total_assistant_bytes);
        println!("Tokens saved:            {}", summary.total_tokens_saved);
        println!("Bytes saved:             {}", summary.total_bytes_saved);
        println!(
            "User compression ratio:  {:.1}% (lower = better)",
            summary.overall_user_compression_ratio * 100.0
        );

        println!("\nBy source:");
        for (source, count, saved, ratio) in &summary.by_source {
            println!(
                "  {:?}: {} sessions, {} tokens saved, {:.1}% user ratio",
                source, count, saved, ratio * 100.0
            );
        }
    }
}
