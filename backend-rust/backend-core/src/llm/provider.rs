use crate::llm::types::{LLMMessage, LLMResponse, LLMResponseMessage, ToolDefinition};
use crate::retry;

// ==================== LLM 提供商枚举 ====================

/// LLM 提供商配置
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
    pub max_retries: u32,
}

/// 提供商类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProviderKind {
    DeepSeek,
    OpenAI,
}

impl ProviderKind {
    /// 从字符串解析提供商类型
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "openai" => ProviderKind::OpenAI,
            _ => ProviderKind::DeepSeek, // 默认 DeepSeek
        }
    }

    /// 返回默认的 base_url
    pub fn default_base_url(&self) -> &str {
        match self {
            ProviderKind::DeepSeek => "https://api.deepseek.com",
            ProviderKind::OpenAI => "https://api.openai.com/v1",
        }
    }

    /// 返回默认的模型名称
    pub fn default_model(&self) -> &str {
        match self {
            ProviderKind::DeepSeek => "deepseek-v4-flash",
            ProviderKind::OpenAI => "gpt-4o",
        }
    }

    /// API 名称（用于重试日志）
    pub fn api_name(&self) -> &str {
        match self {
            ProviderKind::DeepSeek => "DeepSeek",
            ProviderKind::OpenAI => "OpenAI",
        }
    }
}

// ==================== LLMProvider 枚举多态 ====================

/// 使用 enum 而非 trait object，避免 Box 分配和 async-trait 依赖
pub enum LLMProvider {
    DeepSeek(OpenAICompatClient),
    OpenAI(OpenAICompatClient),
}

impl LLMProvider {
    /// 根据配置创建对应的 provider
    pub fn from_config(config: &ProviderConfig) -> Self {
        let client = OpenAICompatClient::new(config, config.kind.api_name());
        match config.kind {
            ProviderKind::DeepSeek => LLMProvider::DeepSeek(client),
            ProviderKind::OpenAI => LLMProvider::OpenAI(client),
        }
    }

    /// 统一的聊天接口（内部处理重试）
    pub async fn chat(
        &self,
        messages: &[LLMMessage],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<LLMResponse, String> {
        let (client, api_name) = match self {
            LLMProvider::DeepSeek(c) => (c, "DeepSeek"),
            LLMProvider::OpenAI(c) => (c, "OpenAI"),
        };
        retry::retry_with_backoff(api_name, client.config.max_retries, || {
            let messages = messages.to_vec();
            let tools = tools.map(|t| t.to_vec());
            let client = client.clone();
            async move { client.chat_inner(&messages, tools.as_deref()).await }
        })
        .await
    }

    /// 返回提供商名称
    pub fn name(&self) -> &str {
        match self {
            LLMProvider::DeepSeek(_) => "DeepSeek",
            LLMProvider::OpenAI(_) => "OpenAI",
        }
    }

    /// 返回模型名称
    pub fn model(&self) -> &str {
        match self {
            LLMProvider::DeepSeek(client) => &client.config.model,
            LLMProvider::OpenAI(client) => &client.config.model,
        }
    }
}

// ==================== OpenAI 兼容客户端（DeepSeek 和 OpenAI 共用） ====================

#[derive(Clone)]
pub struct OpenAICompatClient {
    config: ProviderConfig,
    log_prefix: String,
}

impl OpenAICompatClient {
    pub fn new(config: &ProviderConfig, log_prefix: &str) -> Self {
        Self {
            config: config.clone(),
            log_prefix: log_prefix.to_string(),
        }
    }

    /// 核心调用（不包含重试）
    async fn chat_inner(
        &self,
        messages: &[LLMMessage],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<LLMResponse, String> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| format!("创建HTTP客户端失败: {}", e))?;

        // 转换消息为 JSON
        let json_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                let mut map = serde_json::Map::new();
                map.insert("role".to_string(), serde_json::Value::String(m.role.clone()));
                if let Some(ref content) = m.content {
                    map.insert("content".to_string(), serde_json::Value::String(content.clone()));
                } else {
                    map.insert("content".to_string(), serde_json::Value::String(String::new()));
                }
                if let Some(ref tcs) = m.tool_calls {
                    let calls: Vec<serde_json::Value> = tcs
                        .iter()
                        .map(|tc| {
                            serde_json::json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": tc.arguments
                                }
                            })
                        })
                        .collect();
                    map.insert("tool_calls".to_string(), serde_json::Value::Array(calls));
                }
                if let Some(ref call_id) = m.tool_call_id {
                    map.insert("tool_call_id".to_string(), serde_json::Value::String(call_id.clone()));
                }
                if let Some(ref name) = m.name {
                    map.insert("name".to_string(), serde_json::Value::String(name.clone()));
                }
                serde_json::Value::Object(map)
            })
            .collect();

        // 构建工具定义
        let tools_json: Vec<serde_json::Value> = tools
            .map(|t| {
                t.iter()
                    .map(|td| {
                        serde_json::json!({
                            "type": "function",
                            "function": {
                                "name": td.name,
                                "description": td.description,
                                "parameters": td.parameters
                            }
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let request_body = serde_json::json!({
            "model": self.config.model,
            "messages": json_messages,
            "tools": if tools_json.is_empty() { serde_json::Value::Null } else { serde_json::Value::Array(tools_json) },
            "temperature": self.config.temperature,
        });

        log::info!("[{}] 调用 API: model={}, messages={}", self.log_prefix, self.config.model, messages.len());

        let response = client
            .post(format!("{}/chat/completions", self.config.base_url))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("API请求失败: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("API错误 ({}): {}", status, body));
        }

        #[derive(serde::Deserialize)]
        struct ChatChoice {
            message: ChatMessage,
        }
        #[derive(serde::Deserialize)]
        struct ChatMessage {
            role: String,
            content: Option<String>,
            tool_calls: Option<Vec<ChatToolCall>>,
        }
        #[derive(serde::Deserialize)]
        struct ChatToolCall {
            id: String,
            #[serde(rename = "type")]
            _type: String,
            function: ChatFunction,
        }
        #[derive(serde::Deserialize)]
        struct ChatFunction {
            name: String,
            arguments: String,
        }
        #[derive(serde::Deserialize)]
        struct ChatResponse {
            choices: Vec<ChatChoice>,
            usage: Option<Usage>,
        }
        #[derive(serde::Deserialize)]
        struct Usage {
            prompt_tokens: u64,
            completion_tokens: u64,
        }

        let resp: ChatResponse = response
            .json()
            .await
            .map_err(|e| format!("解析API响应失败: {}", e))?;

        let msg = &resp.choices[0].message;

        if let Some(ref content) = msg.content {
            log::info!("[{}] 响应: {} 字符", self.log_prefix, content.len());
        }
        if let Some(ref tcs) = msg.tool_calls {
            log::info!(
                "[{}] 请求工具: {:?}",
                self.log_prefix,
                tcs.iter().map(|tc| &tc.function.name).collect::<Vec<_>>()
            );
        }

        Ok(LLMResponse {
            message: LLMResponseMessage {
                role: msg.role.clone(),
                content: msg.content.clone(),
                tool_calls: msg.tool_calls.as_ref().map(|tcs| {
                    tcs.iter()
                        .map(|tc| crate::llm::types::LLMToolCall {
                            id: tc.id.clone(),
                            name: tc.function.name.clone(),
                            arguments: tc.function.arguments.clone(),
                        })
                        .collect()
                }),
            },
            usage: resp.usage.map(|u| crate::llm::types::LLMUsage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
            }),
        })
    }
}

// ==================== 集成测试 ====================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LLMConfig;
    use crate::llm::types::ToolDefinition;

    /// 辅助：构建 ProviderConfig
    fn make_provider_config(provider: &str) -> ProviderConfig {
        let kind = ProviderKind::from_str(provider);
        ProviderConfig {
            kind,
            api_key: "test-key".to_string(),
            base_url: kind.default_base_url().to_string(),
            model: kind.default_model().to_string(),
            temperature: 0.7,
            max_retries: 1,
        }
    }

    // ==================== ProviderKind 解析测试 ====================

    #[test]
    fn test_provider_kind_from_str_openai() {
        let kind = ProviderKind::from_str("openai");
        assert_eq!(kind, ProviderKind::OpenAI);
    }

    #[test]
    fn test_provider_kind_from_str_deepseek() {
        let kind = ProviderKind::from_str("deepseek");
        assert_eq!(kind, ProviderKind::DeepSeek);
    }

    #[test]
    fn test_provider_kind_from_str_unknown_defaults_to_deepseek() {
        // 未知 provider 默认回退到 DeepSeek（向后兼容）
        let kind = ProviderKind::from_str("unknown_provider");
        assert_eq!(kind, ProviderKind::DeepSeek);
    }

    // ==================== OpenAI Provider 默认值测试 ====================

    #[test]
    fn test_openai_default_base_url() {
        assert_eq!(
            ProviderKind::OpenAI.default_base_url(),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn test_openai_default_model() {
        assert_eq!(ProviderKind::OpenAI.default_model(), "gpt-4o");
    }

    #[test]
    fn test_openai_api_name() {
        assert_eq!(ProviderKind::OpenAI.api_name(), "OpenAI");
    }

    // ==================== DeepSeek 默认值测试（回归保护） ====================

    #[test]
    fn test_deepseek_default_base_url() {
        assert_eq!(
            ProviderKind::DeepSeek.default_base_url(),
            "https://api.deepseek.com"
        );
    }

    #[test]
    fn test_deepseek_default_model() {
        assert_eq!(ProviderKind::DeepSeek.default_model(), "deepseek-v4-flash");
    }

    #[test]
    fn test_deepseek_api_name() {
        assert_eq!(ProviderKind::DeepSeek.api_name(), "DeepSeek");
    }

    // ==================== LLMProvider 创建和属性测试 ====================

    #[test]
    fn test_create_openai_provider() {
        let config = make_provider_config("openai");
        let provider = LLMProvider::from_config(&config);
        assert_eq!(provider.name(), "OpenAI");
        assert_eq!(provider.model(), "gpt-4o");
    }

    #[test]
    fn test_create_deepseek_provider() {
        let config = make_provider_config("deepseek");
        let provider = LLMProvider::from_config(&config);
        assert_eq!(provider.name(), "DeepSeek");
        assert_eq!(provider.model(), "deepseek-v4-flash");
    }

    #[test]
    fn test_openai_provider_does_not_leak_deepseek_name() {
        let config = make_provider_config("openai");
        let provider = LLMProvider::from_config(&config);
        // 确保来回切换不会残留 DeepSeek 名称
        assert_ne!(provider.name(), "DeepSeek");
        assert_ne!(provider.model(), "deepseek-v4-flash");
    }

    // ==================== Config 多层级测试 ====================

    #[test]
    fn test_llmconfig_to_provider_config_openai() {
        let llm_config = LLMConfig {
            provider: "openai".to_string(),
            api_key: "sk-test".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o".to_string(),
            temperature: 0.7,
        };
        let provider_config = llm_config.to_provider_config();
        assert_eq!(provider_config.kind, ProviderKind::OpenAI);
        assert_eq!(provider_config.base_url, "https://api.openai.com/v1");
        assert_eq!(provider_config.model, "gpt-4o");
    }

    #[test]
    fn test_llmconfig_to_provider_config_deepseek() {
        let llm_config = LLMConfig {
            provider: "deepseek".to_string(),
            api_key: "sk-test".to_string(),
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-v4-flash".to_string(),
            temperature: 0.7,
        };
        let provider_config = llm_config.to_provider_config();
        assert_eq!(provider_config.kind, ProviderKind::DeepSeek);
    }

    #[test]
    fn test_config_defaults() {
        let default_config = LLMConfig::default();
        // 默认值应保持向后兼容（DeepSeek）
        assert_eq!(default_config.provider, "deepseek");
        assert_eq!(default_config.base_url, "https://api.deepseek.com");
        assert_eq!(default_config.model, "deepseek-v4-flash");
    }

    // ==================== ToolDefinition 序列化验证 ====================

    #[test]
    fn test_tool_definition_format_is_openai_compatible() {
        let tool = ToolDefinition {
            name: "web_search".to_string(),
            description: "搜索网页".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                }
            }),
        };
        // 验证序列化后的 JSON 为 OpenAI tool 格式
        let tools = vec![tool];
        let json: Vec<serde_json::Value> = tools
            .iter()
            .map(|td| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": td.name,
                        "description": td.description,
                        "parameters": td.parameters
                    }
                })
            })
            .collect();
        assert_eq!(json.len(), 1);
        assert_eq!(json[0]["type"], "function");
        assert_eq!(json[0]["function"]["name"], "web_search");
    }

    // ==================== LLMMessage 构造测试 ====================

    #[test]
    fn test_llm_message_construction_is_provider_agnostic() {
        use crate::llm::types::LLMMessage;
        // 用户消息
        let msg = LLMMessage::user("hello");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, Some("hello".to_string()));

        // 工具消息
        let tool_msg = LLMMessage::tool("call_1", "web_search", "result");
        assert_eq!(tool_msg.role, "tool");
        assert_eq!(tool_msg.tool_call_id, Some("call_1".to_string()));
        assert_eq!(tool_msg.name, Some("web_search".to_string()));

        // 所有消息类型不包含任何 provider-specific 字段
        let json = serde_json::to_value(&msg).unwrap();
        assert!(!json.to_string().to_lowercase().contains("deepseek"));
    }
}
