//! Auto-discovery of session logs from Claude Code and Qwen Code.

use std::path::{Path, PathBuf};

/// Which AI coding tool produced the session log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSource {
    ClaudeCode,
    QwenCode,
}

/// A discovered session log file.
#[derive(Debug, Clone)]
pub struct SessionDiscovery {
    pub source: SessionSource,
    pub path: PathBuf,
    pub session_id: String,
}

impl SessionDiscovery {
    /// Discover all session logs from well-known locations.
    pub fn discover_all() -> Vec<Self> {
        let home = dirs_home();
        let mut sessions = Vec::new();

        // Claude Code: ~/.claude/projects/*/*.jsonl
        if let Some(claude_dir) = home.as_ref().map(|h| h.join(".claude").join("projects")) {
            sessions.extend(discover_claude_sessions(&claude_dir));
        }

        // Qwen Code: ~/.qwen/projects/*/chats/*.jsonl
        if let Some(qwen_dir) = home.as_ref().map(|h| h.join(".qwen").join("projects")) {
            sessions.extend(discover_qwen_sessions(&qwen_dir));
        }

        sessions
    }

    /// Discover sessions from a specific directory, auto-detecting the source.
    #[allow(dead_code)]
    pub fn discover_from(dir: &Path) -> Vec<Self> {
        let mut sessions = Vec::new();
        sessions.extend(discover_claude_sessions(dir));
        sessions.extend(discover_qwen_sessions(dir));
        sessions
    }
}

fn discover_claude_sessions(projects_dir: &Path) -> Vec<SessionDiscovery> {
    let mut sessions = Vec::new();
    let Ok(entries) = std::fs::read_dir(projects_dir) else {
        return sessions;
    };

    for entry in entries.flatten() {
        let project_dir = entry.path();
        if !project_dir.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&project_dir) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                let session_id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string();
                sessions.push(SessionDiscovery {
                    source: SessionSource::ClaudeCode,
                    path,
                    session_id,
                });
            }
        }
    }
    sessions
}

fn discover_qwen_sessions(projects_dir: &Path) -> Vec<SessionDiscovery> {
    let mut sessions = Vec::new();
    let Ok(entries) = std::fs::read_dir(projects_dir) else {
        return sessions;
    };

    for entry in entries.flatten() {
        let project_dir = entry.path();
        if !project_dir.is_dir() {
            continue;
        }
        let chats_dir = project_dir.join("chats");
        if !chats_dir.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&chats_dir) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                let session_id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string();
                sessions.push(SessionDiscovery {
                    source: SessionSource::QwenCode,
                    path,
                    session_id,
                });
            }
        }
    }
    sessions
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_discover_all() {
        let sessions = SessionDiscovery::discover_all();
        println!("Discovered {} sessions:", sessions.len());
        for s in &sessions {
            println!("  [{:?}] {} -> {}", s.source, s.session_id, s.path.display());
        }
        // Should find at least the sessions we know exist
        assert!(
            !sessions.is_empty(),
            "Expected at least one session log to be discovered"
        );
    }
}
