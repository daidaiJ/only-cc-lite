//! Content type detection chain — lite version.
//!
//! Lite build skips Tier 1 (magika ONNX) and goes directly to:
//!   Tier 1: unidiff parser → GitDiff
//!   Tier 2: regex-based content_detector → JsonArray / Html / SearchResults / BuildOutput / SourceCode
//!   Tier 3: PlainText (fallthrough)

use crate::transforms::content_detector::{detect_content_type, ContentType};
use crate::transforms::unidiff_detector::is_diff;

/// Run the detection chain on `content` and return the chosen [`ContentType`].
pub fn detect(content: &str) -> ContentType {
    if content.is_empty() {
        return ContentType::PlainText;
    }

    // Tier 1: unidiff parser
    if is_diff(content) {
        return ContentType::GitDiff;
    }

    // Tier 2: regex-based content detector (replaces magika)
    detect_content_type(content).content_type
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_short_circuits_to_plain_text() {
        assert_eq!(detect(""), ContentType::PlainText);
    }

    #[test]
    fn json_array_detected() {
        let payload = r#"[{"id": 1}, {"id": 2}, {"id": 3}]"#;
        assert_eq!(detect(payload), ContentType::JsonArray);
    }

    #[test]
    fn git_diff_detected() {
        let diff = "diff --git a/foo.py b/foo.py\n--- a/foo.py\n+++ b/foo.py\n@@ -1,1 +1,2 @@\n def hello():\n+    print(\"new\")\n";
        assert_eq!(detect(diff), ContentType::GitDiff);
    }
}
