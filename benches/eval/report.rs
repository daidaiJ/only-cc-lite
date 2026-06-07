//! Report generation for compression evaluation results.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::Path;

use super::discovery::SessionSource;
use super::evaluator::{EvalResult, EvalSummary};

/// A formatted evaluation report.
pub struct Report {
    summary: EvalSummary,
}

impl Report {
    pub fn new(summary: EvalSummary) -> Self {
        Self { summary }
    }

    /// Generate a plain text report.
    pub fn to_text(&self) -> String {
        let mut out = String::new();

        out.push_str(&format!(
            "only-cc-lite Compression Evaluation Report\n\
             ===========================================\n\n\
             Generated: {}\n\n",
            chrono_now()
        ));

        // Overall summary
        out.push_str(&format!(
            "Overall Summary (Incremental Model)\n\
             -----------------------------------\n\
             Sessions evaluated:       {}\n\
             Total conversation turns: {}\n\
             Total input tokens:       {}\n\
             User content bytes:       {} (compressible)\n\
             Assistant content bytes:  {} (preserved intact)\n\
             Tokens saved:             {}\n\
             Bytes saved:              {}\n\
             User compression:         {:.1}%\n\n",
            self.summary.sessions_evaluated,
            self.summary.total_turns,
            self.summary.total_input_tokens,
            self.summary.total_user_bytes,
            self.summary.total_assistant_bytes,
            self.summary.total_tokens_saved,
            self.summary.total_bytes_saved,
            (1.0 - self.summary.overall_user_compression_ratio) * 100.0
        ));

        // By source
        out.push_str("By Source\n\
                      --------\n");
        for (source, count, saved, ratio) in &self.summary.by_source {
            let source_name = match source {
                SessionSource::ClaudeCode => "Claude Code",
                SessionSource::QwenCode => "Qwen Code",
            };
            out.push_str(&format!(
                "  {}:\n\
                 \tSessions: {}\n\
                 \tTokens saved: {}\n\
                 \tUser compression: {:.1}%\n\n",
                source_name,
                count,
                saved,
                (1.0 - ratio) * 100.0
            ));
        }

        // Per-session details
        out.push_str("Session Details\n\
                      ---------------\n");
        for result in &self.summary.results {
            out.push_str(&format_session_detail(result));
        }

        // Strategies breakdown
        out.push_str("\nStrategies Used\n\
                      ---------------\n");
        let mut strategy_counts: HashMap<&str, usize> = HashMap::new();
        for result in &self.summary.results {
            for turn in &result.turn_results {
                for strategy in &turn.strategies {
                    *strategy_counts.entry(strategy).or_insert(0) += 1;
                }
            }
        }
        if strategy_counts.is_empty() {
            out.push_str("  (none — no compressible content found)\n");
        } else {
            for (strategy, count) in &strategy_counts {
                out.push_str(&format!("  {}: {} turns\n", strategy, count));
            }
        }

        out
    }

    /// Save the report to a file.
    pub fn save_text(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.to_text())
    }

    /// Generate a JSON report for programmatic consumption.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;

        json!({
            "generated": chrono_now(),
            "model": "incremental",
            "summary": {
                "sessions_evaluated": self.summary.sessions_evaluated,
                "total_turns": self.summary.total_turns,
                "total_input_tokens": self.summary.total_input_tokens,
                "total_user_bytes": self.summary.total_user_bytes,
                "total_assistant_bytes": self.summary.total_assistant_bytes,
                "total_tokens_saved": self.summary.total_tokens_saved,
                "total_bytes_saved": self.summary.total_bytes_saved,
                "user_compression_ratio": self.summary.overall_user_compression_ratio,
            },
            "by_source": self.summary.by_source.iter().map(|(source, count, saved, ratio)| {
                json!({
                    "source": format!("{:?}", source),
                    "sessions": count,
                    "tokens_saved": saved,
                    "user_compression_ratio": ratio,
                })
            }).collect::<Vec<_>>(),
            "sessions": self.summary.results.iter().map(|r| {
                json!({
                    "source": format!("{:?}", r.source),
                    "session_id": r.session_id,
                    "model": r.model,
                    "turns": r.total_turns,
                    "input_tokens": r.total_input_tokens,
                    "user_bytes": r.total_user_bytes,
                    "assistant_bytes": r.total_assistant_bytes,
                    "tokens_saved": r.total_tokens_saved,
                    "bytes_saved": r.total_bytes_saved,
                    "user_compression_ratio": r.avg_user_compression_ratio,
                })
            }).collect::<Vec<_>>(),
        })
    }

    /// Save the JSON report to a file.
    pub fn save_json(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.to_json())
            .expect("JSON serialization should not fail");
        fs::write(path, json)
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_text())
    }
}

fn format_session_detail(result: &EvalResult) -> String {
    let source_name = match result.source {
        SessionSource::ClaudeCode => "Claude",
        SessionSource::QwenCode => "Qwen",
    };
    let mut out = format!(
        "  [{}] {} ({})\n\
         \tModel: {}\n\
         \tTurns: {}\n\
         \tUser bytes: {} | Assistant bytes: {} (preserved)\n\
         \tInput tokens: {}, Output tokens: {}\n\
         \tTokens saved: {} ({:.1}% user compression)\n\
         \tBytes saved: {}\n",
        source_name,
        result.session_id,
        result.model,
        result.model,
        result.total_turns,
        result.total_user_bytes,
        result.total_assistant_bytes,
        result.total_input_tokens,
        result.total_output_tokens,
        result.total_tokens_saved,
        (1.0 - result.avg_user_compression_ratio) * 100.0,
        result.total_bytes_saved
    );

    // Show top 3 turns by savings
    let mut top_turns: Vec<_> = result.turn_results.iter().collect();
    top_turns.sort_by(|a, b| b.tokens_saved.cmp(&a.tokens_saved));
    for turn in top_turns.iter().take(3) {
        if turn.tokens_saved > 0 {
            out.push_str(&format!(
                "\t  Turn {}: {} tokens saved ({:.1}%) [{}]\n",
                turn.turn_index,
                turn.tokens_saved,
                (1.0 - turn.user_compression_ratio) * 100.0,
                turn.strategies.join(", ")
            ));
        }
    }

    out.push('\n');
    out
}

fn chrono_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs();
            let days = secs / 86400;
            let year = 1970 + days / 365;
            let month = (days % 365) / 30 + 1;
            let day = (days % 365) % 30 + 1;
            format!("{:04}-{:02}-{:02}", year, month, day)
        })
        .unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_report_generation() {
        let discoveries = super::super::discovery::SessionDiscovery::discover_all();
        let sessions: Vec<super::super::parser::ParsedSession> = discoveries
            .iter()
            .filter_map(|d| super::super::parser::ParsedSession::parse(&d.path, d.source).ok())
            .filter(|s| !s.turns.is_empty())
            .collect();

        if sessions.is_empty() {
            println!("No parseable sessions found");
            return;
        }

        let summary = super::super::evaluator::evaluate_sessions(&sessions);
        let report = Report::new(summary);

        println!("{}", report);

        let text_path = Path::new("target/eval-report.txt");
        let json_path = Path::new("target/eval-report.json");

        match report.save_text(text_path) {
            Ok(_) => println!("Text report saved to: {}", text_path.display()),
            Err(e) => eprintln!("Failed to save text report: {}", e),
        }

        match report.save_json(json_path) {
            Ok(_) => println!("JSON report saved to: {}", json_path.display()),
            Err(e) => eprintln!("Failed to save JSON report: {}", e),
        }
    }
}
