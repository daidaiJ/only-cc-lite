pub mod adaptive_sizer;
pub mod anchor_selector;
pub mod content_detector;
pub mod detection;
pub mod diff_compressor;
pub mod live_zone;
pub mod log_compressor;
pub mod safety;
pub mod search_compressor;
pub mod smart_crusher;
pub mod tag_protector;
pub mod unidiff_detector;

pub use content_detector::{
    detect_content_type, is_json_array_of_dicts, ContentType, DetectionResult,
};
pub use detection::detect;
pub use diff_compressor::{
    DiffCompressionResult, DiffCompressor, DiffCompressorConfig, DiffCompressorStats,
};
pub use live_zone::{
    compress_anthropic_live_zone, compress_anthropic_live_zone_with_ccr,
    compress_openai_chat_live_zone, compress_openai_responses_live_zone,
    summarize_openai_responses_no_change_reason, AuthMode, BlockAction, BlockOutcome,
    CompressionManifest, ExclusionReason, LiveZoneError, LiveZoneOutcome,
};
pub use log_compressor::{
    LogCompressionResult, LogCompressor, LogCompressorConfig, LogCompressorStats, LogFormat,
    LogLevel, LogLine,
};
pub use search_compressor::{
    FileMatches, SearchCompressionResult, SearchCompressor, SearchCompressorConfig,
    SearchCompressorStats, SearchMatch,
};
pub use smart_crusher::{SmartCrusher, SmartCrusherConfig};
pub use tag_protector::{is_known_html_tag, protect_tags, restore_tags, ProtectStats};
pub use unidiff_detector::{detect_diff, is_diff};
