//! Parser for Claude Code and Qwen Code session logs.

use serde::Deserialize;
use std::fs;
use std::path::Path;

use super::discovery::SessionSource;

/// A parsed conversation turn (user message + assistant response).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ParsedTurn {
    pub user_content: String,
    pub assistant_content: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
}

/// A fully parsed session with metadata and turns.
#[derive(Debug, Clone)]
pub struct ParsedSession {
    pub source: SessionSource,
    pub session_id: String,
    pub model: String,
    pub turns: Vec<ParsedTurn>,
}

// ── Claude Code JSONL structures ──

#[derive(Debug, Deserialize)]
struct ClaudeEntry {
    #[serde(rename = "type")]
    entry_type: String,
    message: Option<ClaudeMessage>,
}

#[derive(Debug, Deserialize)]
struct ClaudeMessage {
    role: String,
    content: ClaudeContent,
    usage: Option<ClaudeUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ClaudeContent {
    Text(String),
    Blocks(Vec<ClaudeContentBlock>),
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ClaudeContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ClaudeUsage {
    #[serde(rename = "input_tokens")]
    input_tokens: u64,
    #[serde(rename = "output_tokens")]
    output_tokens: u64,
    #[serde(rename = "cache_read_input_tokens")]
    cache_read_input_tokens: Option<u64>,
    #[serde(rename = "cache_creation_input_tokens")]
    cache_creation_input_tokens: Option<u64>,
}

// ── Qwen Code JSONL structures ──

#[derive(Debug, Deserialize)]
struct QwenEntry {
    #[serde(rename = "type")]
    entry_type: String,
    message: Option<QwenMessage>,
    model: Option<String>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<QwenUsageMetadata>,
}

#[derive(Debug, Deserialize)]
struct QwenMessage {
    role: String,
    parts: Option<Vec<QwenPart>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
enum QwenPart {
    Thought { text: String, thought: bool },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: serde_json::Value,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: serde_json::Value,
    },
    Text { text: String },
}

#[derive(Debug, Deserialize)]
struct QwenUsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: u64,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: u64,
    #[serde(rename = "cachedContentTokenCount")]
    cached_content_token_count: Option<u64>,
}

impl ParsedSession {
    /// Parse a session log file.
    pub fn parse(path: &Path, source: SessionSource) -> Result<Self, String> {
        let content = fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        let session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();

        match source {
            SessionSource::ClaudeCode => parse_claude_session(&content, &session_id),
            SessionSource::QwenCode => parse_qwen_session(&content, &session_id),
        }
    }
}

fn extract_claude_text(content: &ClaudeContent) -> String {
    match content {
        ClaudeContent::Text(s) => s.clone(),
        ClaudeContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| b.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn parse_claude_session(content: &str, session_id: &str) -> Result<ParsedSession, String> {
    let mut turns = Vec::new();
    let mut current_user_content: Option<String> = None;
    let model = String::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let entry: ClaudeEntry = serde_json::from_str(line)
            .map_err(|e| format!("JSON parse error: {}", e))?;

        if let Some(ref msg) = entry.message {
            if entry.entry_type == "user" && msg.role == "user" {
                let text = extract_claude_text(&msg.content);
                // Skip meta/command messages
                if !text.starts_with("Caveat:") && !text.contains("<command-name>") && !text.contains("<local-command") {
                    current_user_content = Some(text);
                }
            } else if entry.entry_type == "assistant" && msg.role == "assistant" {
                if let Some(ref usage) = msg.usage {
                    let user_text = current_user_content.take().unwrap_or_default();
                    let assistant_text = extract_claude_text(&msg.content);
                    turns.push(ParsedTurn {
                        user_content: user_text,
                        assistant_content: assistant_text,
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                        cached_tokens: usage.cache_read_input_tokens.unwrap_or(0),
                    });
                }
            }
        }
    }

    Ok(ParsedSession {
        source: SessionSource::ClaudeCode,
        session_id: session_id.to_string(),
        model,
        turns,
    })
}

fn parse_qwen_session(content: &str, session_id: &str) -> Result<ParsedSession, String> {
    let mut turns = Vec::new();
    let mut user_content_parts: Vec<String> = Vec::new();
    let mut model = String::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let entry: QwenEntry = serde_json::from_str(line)
            .map_err(|e| format!("JSON parse error: {}", e))?;

        if let Some(ref m) = entry.model {
            if !m.is_empty() {
                model = m.clone();
            }
        }

        match entry.entry_type.as_str() {
            "user" => {
                // User message — extract text parts
                if let Some(ref msg) = entry.message {
                    if msg.role == "user" {
                        if let Some(ref parts) = msg.parts {
                            let text: String = parts
                                .iter()
                                .filter_map(|p| match p {
                                    QwenPart::Text { text } => Some(text.as_str()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            if !text.is_empty() {
                                user_content_parts.push(text);
                            }
                        }
                    }
                }
            }
            "tool_result" => {
                // Tool result — accumulate into user-side content
                if let Some(ref msg) = entry.message {
                    if let Some(ref parts) = msg.parts {
                        for part in parts {
                            match part {
                                QwenPart::FunctionResponse { function_response } => {
                                    if let Some(output) = function_response.get("response").and_then(|r| r.get("output")) {
                                        if let Some(text) = output.as_str() {
                                            user_content_parts.push(text.to_string());
                                        }
                                    }
                                }
                                QwenPart::Text { text } => {
                                    user_content_parts.push(text.clone());
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            "assistant" => {
                if let Some(ref msg) = entry.message {
                    if msg.role == "model" {
                        if let Some(ref parts) = msg.parts {
                            let assistant_text: String = parts
                                .iter()
                                .filter_map(|p| match p {
                                    QwenPart::Text { text, .. } => Some(text.as_str()),
                                    QwenPart::Thought { text, .. } => Some(text.as_str()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("\n");

                            if let Some(ref usage) = entry.usage_metadata {
                                let user_text = user_content_parts.join("\n");
                                user_content_parts.clear();
                                if !user_text.is_empty() || !assistant_text.is_empty() {
                                    turns.push(ParsedTurn {
                                        user_content: user_text,
                                        assistant_content: assistant_text,
                                        input_tokens: usage.prompt_token_count,
                                        output_tokens: usage.candidates_token_count,
                                        cached_tokens: usage.cached_content_token_count.unwrap_or(0),
                                    });
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(ParsedSession {
        source: SessionSource::QwenCode,
        session_id: session_id.to_string(),
        model,
        turns,
    })
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_parse_claude_session() {
        let sessions = super::super::discovery::SessionDiscovery::discover_all();
        let claude_sessions: Vec<_> = sessions
            .iter()
            .filter(|s| s.source == SessionSource::ClaudeCode)
            .collect();

        if let Some(session) = claude_sessions.first() {
            let parsed = ParsedSession::parse(&session.path, session.source).unwrap();
            println!("Claude session: {}", parsed.session_id);
            println!("  Model: {}", parsed.model);
            println!("  Turns: {}", parsed.turns.len());
            for (i, turn) in parsed.turns.iter().enumerate() {
                println!("  Turn {}: user={} chars, assistant={} chars, tokens={}/{}/{}",
                    i, turn.user_content.len(), turn.assistant_content.len(),
                    turn.input_tokens, turn.output_tokens, turn.cached_tokens);
            }
        }
    }

    #[test]
    fn test_parse_qwen_session() {
        let sessions = super::super::discovery::SessionDiscovery::discover_all();
        let qwen_sessions: Vec<_> = sessions
            .iter()
            .filter(|s| s.source == SessionSource::QwenCode)
            .collect();

        if let Some(session) = qwen_sessions.first() {
            let parsed = ParsedSession::parse(&session.path, session.source).unwrap();
            println!("Qwen session: {}", parsed.session_id);
            println!("  Model: {}", parsed.model);
            println!("  Turns: {}", parsed.turns.len());
            for (i, turn) in parsed.turns.iter().enumerate() {
                println!("  Turn {}: user={} chars, assistant={} chars, tokens={}/{}/{}",
                    i, turn.user_content.len(), turn.assistant_content.len(),
                    turn.input_tokens, turn.output_tokens, turn.cached_tokens);
            }
        }
    }
}
