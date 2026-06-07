//! Converts parsed session turns into API protocol request bodies.
//!
//! **Incremental compression model**: only compress user-side content
//! (tool results, file contents, search results), keep assistant messages
//! intact so prefix caching still works.
//!
//! Claude Code logs → Anthropic Messages API format
//! Qwen Code logs  → OpenAI Chat Completions format

use serde_json::{json, Value};

use super::discovery::SessionSource;
use super::parser::ParsedTurn;

/// Build a single-turn request body.
///
/// - `assistant_content` goes in as-is (not compressed, preserves prefix cache)
/// - `user_content` is the compressible target
#[allow(dead_code)]
pub fn turn_to_request(turn: &ParsedTurn, source: SessionSource, model: &str) -> Vec<u8> {
    match source {
        SessionSource::ClaudeCode => to_anthropic(turn, model),
        SessionSource::QwenCode => to_openai_chat(turn, model),
    }
}

/// Build accumulated request bodies (turn N includes history 0..=N).
///
/// History is built from real conversation pairs — assistant messages stay
/// untouched so prefix cache hits, only the final user message is compressible.
pub fn turns_to_requests(
    turns: &[ParsedTurn],
    source: SessionSource,
    model: &str,
) -> Vec<Vec<u8>> {
    match source {
        SessionSource::ClaudeCode => turns_to_anthropic(turns, model),
        SessionSource::QwenCode => turns_to_openai(turns, model),
    }
}

/// Build a request where only user content is marked as compressible.
///
/// Returns `(request_body, compressible_user_content)`.
#[allow(dead_code)]
pub fn turn_to_request_with_compressible(
    turn: &ParsedTurn,
    source: SessionSource,
    model: &str,
) -> (Vec<u8>, String) {
    let body = turn_to_request(turn, source, model);
    (body, turn.user_content.clone())
}

// ── Anthropic Messages API ──

#[allow(dead_code)]
fn to_anthropic(turn: &ParsedTurn, model: &str) -> Vec<u8> {
    json!({
        "model": model,
        "max_tokens": 4096,
        "messages": [
            {
                "role": "user",
                "content": [{ "type": "text", "text": &turn.user_content }]
            },
            {
                "role": "assistant",
                "content": [{ "type": "text", "text": &turn.assistant_content }]
            }
        ]
    })
    .to_string()
    .into_bytes()
}

fn turns_to_anthropic(turns: &[ParsedTurn], model: &str) -> Vec<Vec<u8>> {
    let mut messages: Vec<Value> = Vec::new();
    let mut requests = Vec::new();

    for turn in turns {
        // Append this turn's messages to history
        messages.push(json!({
            "role": "user",
            "content": [{ "type": "text", "text": &turn.user_content }]
        }));
        messages.push(json!({
            "role": "assistant",
            "content": [{ "type": "text", "text": &turn.assistant_content }]
        }));

        // Snapshot current history into a request
        requests.push(
            json!({
                "model": model,
                "max_tokens": 4096,
                "messages": messages.clone()
            })
            .to_string()
            .into_bytes(),
        );
    }
    requests
}

// ── OpenAI Chat Completions API ──

#[allow(dead_code)]
fn to_openai_chat(turn: &ParsedTurn, model: &str) -> Vec<u8> {
    json!({
        "model": model,
        "messages": [
            { "role": "user", "content": &turn.user_content },
            { "role": "assistant", "content": &turn.assistant_content }
        ]
    })
    .to_string()
    .into_bytes()
}

fn turns_to_openai(turns: &[ParsedTurn], model: &str) -> Vec<Vec<u8>> {
    let mut messages: Vec<Value> = Vec::new();
    let mut requests = Vec::new();

    for turn in turns {
        messages.push(json!({ "role": "user", "content": &turn.user_content }));
        messages.push(json!({ "role": "assistant", "content": &turn.assistant_content }));

        requests.push(
            json!({
                "model": model,
                "messages": messages.clone()
            })
            .to_string()
            .into_bytes(),
        );
    }
    requests
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[allow(dead_code)]
    fn sample_turn() -> ParsedTurn {
        ParsedTurn {
            user_content: "What is Rust?".to_string(),
            assistant_content: "Rust is a systems programming language.".to_string(),
            input_tokens: 100,
            output_tokens: 50,
            cached_tokens: 0,
        }
    }

    #[test]
    fn test_anthropic_single_turn() {
        let turn = sample_turn();
        let body = turn_to_request(&turn, SessionSource::ClaudeCode, "claude-sonnet-4-20250514");
        let parsed: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(parsed["model"], "claude-sonnet-4-20250514");
        assert_eq!(parsed["messages"][0]["role"], "user");
        assert_eq!(parsed["messages"][1]["role"], "assistant");
        // user content is the compressible part
        let user_text = parsed["messages"][0]["content"][0]["text"].as_str().unwrap();
        assert_eq!(user_text, "What is Rust?");
    }

    #[test]
    fn test_openai_single_turn() {
        let turn = sample_turn();
        let body = turn_to_request(&turn, SessionSource::QwenCode, "mimo-v2.5-pro");
        let parsed: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(parsed["model"], "mimo-v2.5-pro");
        assert_eq!(parsed["messages"][0]["role"], "user");
        assert_eq!(parsed["messages"][1]["role"], "assistant");
    }

    #[test]
    fn test_accumulated_history_preserves_assistant() {
        let turns = vec![
            ParsedTurn {
                user_content: "Hello".to_string(),
                assistant_content: "Hi!".to_string(),
                input_tokens: 10,
                output_tokens: 5,
                cached_tokens: 0,
            },
            ParsedTurn {
                user_content: "What is Rust?".to_string(),
                assistant_content: "A systems language.".to_string(),
                input_tokens: 50,
                output_tokens: 20,
                cached_tokens: 10,
            },
        ];

        let requests = turns_to_requests(&turns, SessionSource::QwenCode, "mimo-v2.5-pro");
        assert_eq!(requests.len(), 2);

        // Turn 0: [user "Hello", assistant "Hi!"]
        let r1: Value = serde_json::from_slice(&requests[0]).unwrap();
        assert_eq!(r1["messages"].as_array().unwrap().len(), 2);
        assert_eq!(r1["messages"][0]["content"], "Hello");
        assert_eq!(r1["messages"][1]["content"], "Hi!");

        // Turn 1: [user "Hello", assistant "Hi!", user "What is Rust?", assistant "A systems language."]
        let r2: Value = serde_json::from_slice(&requests[1]).unwrap();
        let msgs = r2["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 4);
        // Assistant history is preserved intact
        assert_eq!(msgs[1]["content"], "Hi!");
        assert_eq!(msgs[3]["content"], "A systems language.");
    }

    #[test]
    fn test_compressible_content_is_user_only() {
        let turn = sample_turn();
        let (body, compressible) = turn_to_request_with_compressible(
            &turn, SessionSource::ClaudeCode, "claude-sonnet-4-20250514",
        );

        // compressible part is user content only
        assert_eq!(compressible, "What is Rust?");

        // assistant content in body is not in compressible
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        let assistant_text = parsed["messages"][1]["content"][0]["text"].as_str().unwrap();
        assert_eq!(assistant_text, "Rust is a systems programming language.");
        assert!(!compressible.contains("Rust is a systems"));
    }
}
