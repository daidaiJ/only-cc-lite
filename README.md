# only-cc-lite

从 [Headroom](https://github.com/chopratejas/headroom) 提取的轻量级上下文压缩库。

**零 ML 依赖** — 不需要 ONNX Runtime、fastembed、HuggingFace tokenizers。  
**代理友好** — 单函数调用完成拦截→压缩→返回指标，适合嵌入 HTTP 代理中间层。

## 压缩能力

| 内容类型 | 检测方式 | 压缩器 | 典型压缩率 |
|---------|---------|--------|-----------|
| JSON 数组 | serde_json 解析 | SmartCrusher | 60-90% |
| 构建/日志输出 | 正则匹配 ERROR/WARN/Traceback | LogCompressor | 50-80% |
| 搜索结果 | 正则匹配 `file:line:` | SearchCompressor | 50-80% |
| Git Diff | 正则/unidiff 解析 | DiffCompressor | 40-60% |

Token 计数使用字符密度估算（`chars / cpt`），按模型族校准：
- Claude: 3.5 chars/token
- Gemini/Command: 4.0 chars/token
- 其他: 4.0 chars/token

## 快速开始

```toml
[dependencies]
only-cc-lite = { git = "https://github.com/daidaiJ/only-cc-lite.git" }
```

---

## 1. 基本用法

```rust
use only_cc_lite::{compress_request, Provider};

let body = br#"{
    "model": "claude-sonnet-4-5-20250929",
    "messages": [
        {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "x", "content": [
                {"type": "text", "text": "[{\"id\":1,\"name\":\"a\"},{\"id\":2,\"name\":\"b\"},{\"id\":3,\"name\":\"c\"}]"}
            ]}
        ]}
    ]
}"#;

let outcome = compress_request(body, Provider::Anthropic, "claude-sonnet-4-5-20250929", None)?;

if let Some(compressed) = &outcome.body {
    println!("✅ 压缩生效");
    println!("   节省 tokens: {}", outcome.tokens_saved);
    println!("   节省 bytes:  {}", outcome.bytes_saved);
    println!("   使用策略:    {:?}", outcome.strategies);
    println!("   压缩比:      {:.1}%", outcome.compression_ratio() * 100.0);
} else {
    println!("⏭️ 无需压缩，转发原始 body");
}
```

---

## 2. 配置 SQLite CCR 缓存

CCR（Compress-Cache-Retrieve）在压缩时将原始内容存入本地数据库，并在压缩后的文本中注入 `<<ccr:HASH>>` 标记。LLM 可通过工具调用按哈希检索原始内容，实现**端到端无损压缩**。

### 初始化

```rust
use only_cc_lite::{CcrBackendConfig, CcrStore};
use only_cc_lite::ccr::backends::from_config;
use std::path::PathBuf;

// 数据库文件路径（Tauri 中建议放在 app data 目录）
let db_path = PathBuf::from("./data/ccr.db");

// 创建 SQLite 后端
// - ttl_seconds: 条目过期时间（秒），300 = 5 分钟
// - 数据库自动创建，WAL 模式，零配置
let ccr = from_config(&CcrBackendConfig::Sqlite {
    path: db_path,
    ttl_seconds: 300,
})?;

// 传入 compress_request
let outcome = compress_request(&body, Provider::Anthropic, model, Some(ccr.as_ref()))?;
```

### 数据库行为

| 特性 | 说明 |
|------|------|
| 自动建表 | 首次调用自动创建 `ccr_entries` 表 |
| TTL 过期 | 读取时惰性清除过期条目，不跑后台任务 |
| 并发安全 | WAL 模式，多线程/多进程可共享同一个 `.db` 文件 |
| 磁盘占用 | 每条目 ~原始内容大小 + 100 字节开销 |
| 哈希算法 | BLAKE3 → 24 字符 hex 前缀（96 位，碰撞概率极低） |

### 内存后端（测试用）

```rust
// 不持久化，进程重启后丢失，适合单元测试
let ccr = from_config(&CcrBackendConfig::InMemory {
    capacity: 1000,     // 最多缓存 1000 条
    ttl_seconds: 300,
})?;
```

---

## 3. 获取上下文压缩比例和收益

`CompressOutcome` 提供多维度的压缩指标：

```rust
let outcome = compress_request(&body, provider, model, ccr_store)?;

// ── 整体指标 ──
outcome.tokens_saved      // 节省的 token 数（估算）
outcome.bytes_saved       // 节省的字节数（精确）
outcome.compression_ratio() // 0.0 ~ 1.0，越小越好
                            // = compressed_bytes / original_bytes

// ── 策略明细 ──
outcome.strategies        // ["smart_crusher"] 或 ["log_compressor", "diff_compressor"]

// ── 逐 block 明细 ──
for block in &outcome.per_block {
    println!("block[{}]: {} → {} tokens (策略: {:?})",
        block.message_index,
        block.original_tokens,
        block.compressed_tokens,
        block.strategy,
    );
}
```

---

## 4. 结合上游响应估算实际节省

上游 LLM API 的响应中会报告 `usage.input_tokens` — 这是**压缩后**的输入 token 数。
结合 `outcome.tokens_saved` 可以反推**原始**输入 token 数和实际节省的成本。

### 原理

```
原始 input_tokens = API 报告的 input_tokens + tokens_saved
节省比例 = tokens_saved / 原始 input_tokens
节省成本 = tokens_saved × 模型单价 ($/M tokens)
```

### 完整示例

```rust
use only_cc_lite::{compress_request, Provider, get_tokenizer};

/// 代理拦截后的完整指标
struct RequestMetrics {
    /// 压缩前估算的 input tokens
    pub original_input_tokens: usize,
    /// API 实际返回的 input tokens（压缩后）
    pub actual_input_tokens: usize,
    /// 节省的 tokens
    pub tokens_saved: usize,
    /// 压缩比 (0.0 ~ 1.0)
    pub compression_ratio: f64,
    /// 估算节省的美元（按 Claude Sonnet $3/M input 计）
    pub estimated_cost_saved_usd: f64,
}

fn intercept_and_forward(
    body: &[u8],
    provider: Provider,
    model: &str,
) -> (Vec<u8>, only_cc_lite::CompressOutcome) {
    match compress_request(body, provider, model, None) {
        Ok(outcome) => {
            let forward_body = outcome.body.clone().unwrap_or_else(|| body.to_vec());
            (forward_body, outcome)
        }
        Err(_) => (body.to_vec(), only_cc_lite::CompressOutcome::passthrough()),
    }
}

fn calculate_metrics(
    outcome: &only_cc_lite::CompressOutcome,
    api_input_tokens: u64,  // 从上游响应的 usage.input_tokens 获取
    model: &str,
) -> RequestMetrics {
    let tokens_saved = outcome.tokens_saved as u64;

    // 反推原始 input tokens
    let original_input_tokens = api_input_tokens + tokens_saved;

    // 压缩比 = 1 - saved / original
    let compression_ratio = if original_input_tokens > 0 {
        1.0 - (tokens_saved as f64 / original_input_tokens as f64)
    } else {
        1.0
    };

    // 估算节省的成本
    let price_per_million = match model {
        m if m.contains("opus") => 15.0,       // $15/M input
        m if m.contains("sonnet") => 3.0,      // $3/M input
        m if m.contains("haiku") => 0.25,      // $0.25/M input
        m if m.contains("gpt-4o") => 2.5,      // $2.5/M input
        m if m.contains("gpt-4o-mini") => 0.15, // $0.15/M input
        _ => 3.0,                               // 默认假设 $3/M
    };
    let estimated_cost_saved_usd =
        (tokens_saved as f64 / 1_000_000.0) * price_per_million;

    RequestMetrics {
        original_input_tokens: original_input_tokens as usize,
        actual_input_tokens: api_input_tokens as usize,
        tokens_saved: tokens_saved as usize,
        compression_ratio,
        estimated_cost_saved_usd,
    }
}
```

### 在代理服务中使用

```rust
async fn proxy_handler(body: Bytes) -> Response {
    // 1. 压缩请求
    let (forward_body, outcome) = intercept_and_forward(
        &body, Provider::Anthropic, "claude-sonnet-4-5-20250929"
    );

    // 2. 转发到上游
    let upstream_resp = forward_to_anthropic(forward_body).await?;

    // 3. 从上游响应中提取 usage
    let api_input_tokens = upstream_resp
        .usage.as_ref()
        .map(|u| u.input_tokens)
        .unwrap_or(0);

    // 4. 计算实际节省
    let metrics = calculate_metrics(&outcome, api_input_tokens, "claude-sonnet-4-5-20250929");

    // 5. 上报指标
    println!(
        "📊 压缩报告: {} → {} tokens (节省 {:.1}%, ≈${:.4})",
        metrics.original_input_tokens,
        metrics.actual_input_tokens,
        (1.0 - metrics.compression_ratio) * 100.0,
        metrics.estimated_cost_saved_usd,
    );

    // 6. 返回响应给客户端
    upstream_resp.into_response()
}
```

### 从 SSE 流式响应中提取 usage

Anthropic 和 OpenAI 在流式响应的最后一帧会发送 usage：

```rust
/// 从 Anthropic SSE 流中提取 usage（message_delta 事件）
fn extract_anthropic_usage(events: &[SseEvent]) -> Option<(u64, u64)> {
    for event in events.iter().rev() {
        if event.event_name.as_deref() == Some("message_delta") {
            let data: serde_json::Value = serde_json::from_str(&event.data).ok()?;
            let usage = data.get("usage")?;
            let input = usage.get("input_tokens")?.as_u64()?;
            let output = usage.get("output_tokens")?.as_u64()?;
            return Some((input, output));
        }
    }
    None
}

/// 从 OpenAI SSE 流中提取 usage（最后一个 chunk，需 stream_options.include_usage=true）
fn extract_openai_usage(events: &[SseEvent]) -> Option<(u64, u64)> {
    for event in events.iter().rev() {
        if event.data.trim() == "[DONE]" { continue; }
        let data: serde_json::Value = serde_json::from_str(&event.data).ok()?;
        if let Some(usage) = data.get("usage") {
            let input = usage.get("prompt_tokens")?.as_u64()?;
            let output = usage.get("completion_tokens")?.as_u64()?;
            return Some((input, output));
        }
    }
    None
}
```

---

## API 概览

```rust
// ── 核心函数 ──
pub fn compress_request(
    body: &[u8],                       // 请求体 JSON bytes
    provider: Provider,                // Anthropic | OpenAiChat | OpenAiResponses
    model: &str,                       // 模型名，用于 tokenizer 校准
    ccr_store: Option<&dyn CcrStore>,  // None = 不启用 CCR
) -> Result<CompressOutcome, CompressError>;

// ── Provider ──
pub enum Provider { Anthropic, OpenAiChat, OpenAiResponses }

// ── 压缩结果 ──
pub struct CompressOutcome {
    pub body: Option<Vec<u8>>,       // None = 无需压缩
    pub tokens_saved: usize,         // 节省的 token 数（估算）
    pub bytes_saved: usize,          // 节省的字节数（精确）
    pub strategies: Vec<&'static str>,
    pub per_block: Vec<BlockReport>,
}

impl CompressOutcome {
    pub fn compression_ratio(&self) -> f64;  // compressed / original
    pub fn passthrough() -> Self;             // 空结果，用于错误降级
}

// ── Token 计数 ──
pub fn get_tokenizer(model: &str) -> Box<dyn Tokenizer>;

pub trait Tokenizer {
    fn count_text(&self, text: &str) -> usize;
    fn backend(&self) -> Backend;  // Always Estimation in lite
}
```

---

## 与 Headroom 的差异

| 特性 | headroom-core | only-cc-lite |
|------|--------------|-------------|
| 内容类型检测 | Magika ONNX + 正则 | 纯正则 |
| Token 计数 | tiktoken-rs + HuggingFace | 字符密度估算 |
| 语义相关性评分 | fastembed ONNX | BM25 纯关键词 |
| CCR 后端 | SQLite + Redis | SQLite + 内存 |
| 二进制膨胀 | ~50-80MB (ONNX) | <5MB |
| 传递依赖 | 4182 | ~80 |

压缩质量无显著差异 — live_zone 调度器使用纯正则检测内容类型，相关性评分当前未启用。

## 依赖

全部纯 Rust 或 bundled C（rusqlite）：

```
serde, serde_json, bytes, thiserror, tracing, regex, aho-corasick,
unidiff, md-5, sha2, blake3, flate2, rayon, toml, dashmap,
rusqlite (bundled), http
```

## 许可证

Apache-2.0，与 Headroom 一致。
