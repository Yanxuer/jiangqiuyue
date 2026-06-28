use serde::{Deserialize, Serialize};

// ==================== 统一的 LLM 请求/响应类型 ====================

/// 聊天消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMMessage {
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<LLMToolCall>>,
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
}

/// 工具调用（LLM 请求执行什么工具）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String, // JSON 字符串
}

/// LLM 响应的消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponseMessage {
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<LLMToolCall>>,
}

/// LLM 响应（包含可选的使用量统计）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    pub message: LLMResponseMessage,
    pub usage: Option<LLMUsage>,
}

/// Token 使用量
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl LLMMessage {
    /// 构造用户消息
    pub fn user(content: &str) -> Self {
        LLMMessage {
            role: "user".to_string(),
            content: Some(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    /// 构造助手消息
    pub fn assistant(content: Option<String>, tool_calls: Option<Vec<LLMToolCall>>) -> Self {
        LLMMessage {
            role: "assistant".to_string(),
            content,
            tool_calls,
            tool_call_id: None,
            name: None,
        }
    }

    /// 构造工具返回消息
    pub fn tool(tool_call_id: &str, name: &str, content: &str) -> Self {
        LLMMessage {
            role: "tool".to_string(),
            content: Some(content.to_string()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.to_string()),
            name: Some(name.to_string()),
        }
    }
}

// ==================== 工具定义 ====================

/// LLM 工具定义，用于传递给 provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}
