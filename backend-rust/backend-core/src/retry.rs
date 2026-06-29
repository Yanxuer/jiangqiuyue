use rand::Rng;
use std::future::Future;

// ==================== 默认重试配置 ====================

/// 默认最大重试次数
pub const DEFAULT_MAX_RETRIES: u32 = 3;
/// 退避等待最小秒数
pub const BACKOFF_MIN_SECS: u64 = 3;
/// 退避等待最大秒数
pub const BACKOFF_MAX_SECS: u64 = 30;

// ==================== 错误分类 ====================

/// 判断 API 错误是否可重试。
///
/// **可重试**：网络层错误（连接超时、DNS 失败、连接重置等）、
/// HTTP 429（限流）、HTTP 5xx（服务端内部错误）。
///
/// **不可重试**：HTTP 4xx（客户端错误，除 429 外）、JSON 解析错误。
///
/// # 示例
///
/// ```
/// use backend_core::retry::is_retryable_error;
///
/// assert!(is_retryable_error("connection reset"));
/// assert!(is_retryable_error("API错误 (429): too many requests"));
/// assert!(!is_retryable_error("API错误 (400): bad request"));
/// ```
pub fn is_retryable_error(error_msg: &str) -> bool {
    let lower = error_msg.to_lowercase();

    // HTTP 4xx 客户端错误（除 429 外）不可重试，优先判断
    // 匹配 "API错误 (4xx)" 或 "(4xx)" 格式
    if let Some(code) = extract_http_status(&lower) {
        if code >= 400 && code < 500 && code != 429 {
            return false;
        }
    }

    // 网络层错误关键词
    let network_keywords = [
        "connection",
        "timeout",
        "timed out",
        "reset",
        "refused",
        "dns",
        "tls",
        "eof",
        "broken pipe",
        "channel closed",
        "request failed",
        "send failed",
        "connect failed",
    ];
    if network_keywords.iter().any(|kw| lower.contains(kw)) {
        return true;
    }

    // HTTP 429 限流
    if lower.contains("429") || lower.contains("rate limit") {
        return true;
    }

    // HTTP 5xx 服务端错误
    if lower.contains("5xx")
        || lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
    {
        return true;
    }

    false
}

/// 从错误消息中提取 HTTP 状态码
fn extract_http_status(lower_msg: &str) -> Option<u16> {
    // 匹配 "(404)" 或 "(500)" 等格式
    for part in lower_msg.split('(') {
        let trimmed = part.trim_start();
        if let Some(closing) = trimmed.find(')') {
            let num_str = &trimmed[..closing];
            if let Ok(code) = num_str.parse::<u16>() {
                if (100..600).contains(&code) {
                    return Some(code);
                }
            }
        }
    }
    None
}

// ==================== 通用重试函数 ====================

/// 带指数退避的通用 API 重试函数。
///
/// 参考 [Trae-Agent retry_utils.py] 的设计，在每次失败后随机等待 3–30 秒，
/// 最多重试 `max_retries` 次。对于不可重试的错误会立即短路返回。
///
/// # 类型参数
///
/// * `F` — 闭包类型，每次重试时调用
/// * `Fut` — 闭包返回的 Future 类型
/// * `T` — 成功时的返回值类型
///
/// # 参数
///
/// * `name` — API 名称（仅用于日志，如 `"DeepSeek"`、`"OpenAI"`）
/// * `max_retries` — 最大重试次数（不包含首次尝试）
/// * `f` — 执行 API 调用的闭包，返回 `Result<T, String>`
///
/// # 返回值
///
/// * `Ok(T)` — 调用成功
/// * `Err(String)` — 所有重试耗尽或遇到不可重试的错误
///
/// # 示例
///
/// ```ignore
/// let result = retry_with_backoff("MyAPI", 3, || async {
///     some_api_call().await
/// }).await;
/// ```
pub async fn retry_with_backoff<F, Fut, T>(
    name: &str,
    max_retries: u32,
    f: F,
) -> Result<T, String>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    let mut last_error = String::new();

    for attempt in 0..=max_retries {
        if attempt > 0 {
            let backoff_secs = rand::thread_rng()
                .gen_range(BACKOFF_MIN_SECS..=BACKOFF_MAX_SECS);
            log::warn!(
                "[Retry] {} API 调用失败（第 {}/{} 次），{} 秒后重试... 错误: {}",
                name,
                attempt,
                max_retries,
                backoff_secs,
                last_error
            );
            tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
        }

        log::info!(
            "[Retry] 调用 {} API (第 {}/{})",
            name,
            attempt + 1,
            max_retries + 1
        );

        match f().await {
            Ok(value) => return Ok(value),
            Err(e) => {
                last_error = e;
                if !is_retryable_error(&last_error) {
                    log::error!("[Retry] {} API 遇到不可重试的错误: {}", name, last_error);
                    return Err(last_error);
                }
                // 可重试，继续循环
            }
        }
    }

    log::error!(
        "[Retry] {} API 调用失败，已重试 {} 次，最终错误: {}",
        name,
        max_retries,
        last_error
    );
    Err(format!(
        "{} API 调用失败（已重试 {} 次）: {}",
        name, max_retries, last_error
    ))
}
