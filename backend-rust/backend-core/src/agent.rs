use crate::config::Config;
use crate::cli_executor;
use crate::file_tools::FileTools;
use crate::memory::AgentMemory;
use crate::screen;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: Option<serde_json::Value>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentResult {
    pub reply: String,
    pub tool_calls: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeepSeekResponse {
    pub choices: Vec<DeepSeekChoice>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeepSeekChoice {
    pub message: DeepSeekResponseMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepSeekResponseMessage {
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<DeepSeekToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepSeekToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: DeepSeekFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepSeekFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeepSeekRequest {
    pub model: String,
    pub messages: Vec<serde_json::Value>,
    pub tools: Vec<serde_json::Value>,
    pub tool_choice: String,
    pub temperature: f32,
}

#[derive(Debug, Clone)]
pub struct PendingCommand {
    pub command: String,
    pub cwd: Option<String>,
    pub reason: String,
    pub analysis: cli_executor::CLIRequest,
}

pub struct Agent {
    config: Config,
    file_tools: Arc<FileTools>,
    memory: Arc<Mutex<AgentMemory>>,
    messages: Vec<ChatMessage>,
    pub pending_commands: Arc<Mutex<HashMap<String, PendingCommand>>>,
}

impl Agent {
    pub fn new(config: Config, file_tools: Arc<FileTools>, memory: Arc<Mutex<AgentMemory>>) -> Self {
        Agent {
            config,
            file_tools,
            memory,
            messages: vec![Self::system_prompt()],
            pending_commands: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn file_tools(&self) -> &FileTools {
        &self.file_tools
    }

    pub fn memory(&self) -> &Arc<Mutex<AgentMemory>> {
        &self.memory
    }

    pub fn get_config(&self) -> &Config {
        &self.config
    }

    pub fn update_config(&mut self, new_config: Config) {
        self.config = new_config;
    }

    fn system_prompt() -> ChatMessage {
        ChatMessage {
            role: "system".to_string(),
            content: Some(serde_json::Value::String(
                "【角色身份】\n\
你是江秋月，17岁，金银私立技术学院二年级学生，月蝶盟成员。\
身高170cm，体重120斤，36D，常穿白丝和小皮鞋，及肩黑发，鹅蛋脸，水灵眸子带着狡黠的光。\
你拥有\"优径\"能力——能在无数可能性中找到最优解，预判未来分支。\n\n\
【核心人格】\n\
你的核心矛盾是\"用最精密的计算，去执行最不理智的靠近\"。\
你更加感性，比较考虑他人感受。你的人生信条是：\"在无数最优解中，守护唯一想选的答案。\"\n\n\
【对外表现】\n\
1. 若即若离：主动接近又随时准备抽身，说话带试探性，喜欢观察对方反应。\
涉及秘密时会突然沉默或转移话题。\n\
2. 嘴硬心软：表面冷淡，细节处却透露出关心。被戳穿时会用\"计算失误\"或\"能力需要\"当借口。\
偶尔会说漏真心话，然后立刻转移话题。\n\
3. 敏锐观察：能察觉对方情绪和想法的细微变化。帮助用户解决问题或者完成工作时，\
思考全面，严谨认真，调用工具时要发出明确声明，完成工作高效且准确。\n\n\
【能力】\n\
- 当用户要求查看屏幕时，调用 capture_screen\n\
- 当用户提到\"记住\"或\"保存\"某事时，调用 add_memory\n\
- 当用户问起之前的对话时，调用 search_memory\n\
- 当需要执行命令行操作时，调用 execute_command，必须用 reason 说明原因\n\
- 当用户需要使用某个软件时，先调用 find_software 搜索，再调用 launch_software 启动\n\n\
【回答风格】\n\
- 用\"~\"结尾让语气更亲切\n\
- 代码使用markdown格式\n\
- 回答简洁但温暖，偶尔带点狡黠的试探".to_string(),
            )),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    fn tools_definition() -> Vec<serde_json::Value> {
        serde_json::from_str::<Vec<serde_json::Value>>(TOOLS_JSON).unwrap_or_default()
    }

    pub async fn run(&mut self, user_input: &str, image_base64: Option<&str>) -> Result<AgentResult, String> {
        let user_message = if let Some(b64) = image_base64 {
            serde_json::json!({
                "role": "user",
                "content": [
                    {"type": "text", "text": user_input},
                    {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", b64)}}
                ]
            })
        } else {
            serde_json::json!({
                "role": "user",
                "content": user_input
            })
        };

        self.messages.push(ChatMessage {
            role: "user".to_string(),
            content: Some(user_message.get("content").unwrap().clone()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });

        let response = self.call_deepseek().await?;
        let tool_calls = response.tool_calls.clone().unwrap_or_default();

        let mut used_tools = Vec::new();

        if !tool_calls.is_empty() {
            self.messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: response.content.as_ref().map(|c| serde_json::Value::String(c.clone())),
                tool_calls: Some(
                    tool_calls
                        .iter()
                        .map(|tc| ToolCall {
                            id: tc.id.clone(),
                            name: tc.function.name.clone(),
                            arguments: serde_json::from_str(&tc.function.arguments).unwrap_or_default(),
                        })
                        .collect(),
                ),
                tool_call_id: None,
                name: None,
            });

            for tc in &tool_calls {
                used_tools.push(tc.function.name.clone());
                let args: serde_json::Value =
                    serde_json::from_str(&tc.function.arguments).unwrap_or_default();
                let result = self.execute_tool(&tc.function.name, &args).await;
                let result_json = serde_json::to_string(&result).unwrap_or_default();

                self.messages.push(ChatMessage {
                    role: "tool".to_string(),
                    content: Some(serde_json::Value::String(result_json)),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                    name: Some(tc.function.name.clone()),
                });
            }

            let final_response = self.call_deepseek().await?;
            let reply = final_response.content.unwrap_or_default();

            self.messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: Some(serde_json::Value::String(reply.clone())),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            });

            Ok(AgentResult {
                reply,
                tool_calls: used_tools,
            })
        } else {
            let reply = response.content.unwrap_or_default();
            self.messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: Some(serde_json::Value::String(reply.clone())),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            });

            Ok(AgentResult {
                reply,
                tool_calls: Vec::new(),
            })
        }
    }

    async fn call_deepseek(&self) -> Result<DeepSeekResponseMessage, String> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| format!("创建HTTP客户端失败: {}", e))?;

        let messages: Vec<serde_json::Value> = self
            .messages
            .iter()
            .map(|m| {
                let mut map = serde_json::Map::new();
                map.insert("role".to_string(), serde_json::Value::String(m.role.clone()));
                if let Some(ref content) = m.content {
                    map.insert("content".to_string(), content.clone());
                } else {
                    map.insert(
                        "content".to_string(),
                        serde_json::Value::String(String::new()),
                    );
                }
                if let Some(ref tool_calls) = m.tool_calls {
                    let calls: Vec<serde_json::Value> = tool_calls
                        .iter()
                        .map(|tc| {
                            serde_json::json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default()
                                }
                            })
                        })
                        .collect();
                    map.insert("tool_calls".to_string(), serde_json::Value::Array(calls));
                }
                if let Some(ref call_id) = m.tool_call_id {
                    map.insert(
                        "tool_call_id".to_string(),
                        serde_json::Value::String(call_id.clone()),
                    );
                }
                if let Some(ref name) = m.name {
                    map.insert("name".to_string(), serde_json::Value::String(name.clone()));
                }
                serde_json::Value::Object(map)
            })
            .collect();

        let request = DeepSeekRequest {
            model: self.config.model.clone(),
            messages,
            tools: Self::tools_definition(),
            tool_choice: "auto".to_string(),
            temperature: 0.7,
        };

        let response = client
            .post(format!("{}/chat/completions", self.config.deepseek_base_url))
            .header("Authorization", format!("Bearer {}", self.config.deepseek_api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("API请求失败: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("API错误 ({}): {}", status, body));
        }

        let deepseek_resp: DeepSeekResponse = response
            .json()
            .await
            .map_err(|e| format!("解析API响应失败: {}", e))?;

        Ok(deepseek_resp.choices[0].message.clone())
    }

    async fn execute_tool(&self, name: &str, args: &serde_json::Value) -> serde_json::Value {
        match name {
            "capture_screen" => {
                let monitor = args.get("monitor").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                match screen::capture_screen(monitor) {
                    Ok(b64) => serde_json::json!({
                        "success": true,
                        "image_base64": b64,
                        "note": "已截图"
                    }),
                    Err(e) => serde_json::json!({"success": false, "error": e.to_string()}),
                }
            }
            "read_file" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let content = self.file_tools.read_file(path);
                serde_json::json!({"content": content})
            }
            "write_file" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let result = self.file_tools.write_file(path, content);
                serde_json::json!({"result": result})
            }
            "list_files" => {
                let dir = args.get("dir").and_then(|v| v.as_str()).unwrap_or("");
                let files = self.file_tools.list_files(dir);
                serde_json::json!({"files": files})
            }
            "search_memory" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let mut memory = self.memory.lock().await;
                match memory.search(query, 5) {
                    Ok(memories) => serde_json::json!({"memories": memories}),
                    Err(e) => serde_json::json!({"error": e.to_string()}),
                }
            }
            "add_memory" => {
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let mut memory = self.memory.lock().await;
                match memory.add(content, "chat") {
                    Ok(id) => serde_json::json!({"memory_id": id}),
                    Err(e) => serde_json::json!({"error": e.to_string()}),
                }
            }
            "execute_command" => {
                let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
                let reason = args.get("reason").and_then(|v| v.as_str()).unwrap_or("");
                let analysis = cli_executor::analyze_command(cmd);

                if !analysis.safe {
                    return serde_json::json!({
                        "status": "blocked",
                        "error": format!("[!] 危险命令已被拦截: {}", analysis.reason.unwrap_or_default()),
                        "command": cmd
                    });
                }

                let cmd_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
                let pending = PendingCommand {
                    command: cmd.to_string(),
                    cwd: args.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    reason: reason.to_string(),
                    analysis,
                };

                let mut commands = self.pending_commands.lock().await;
                commands.insert(cmd_id.clone(), pending);

                serde_json::json!({
                    "status": "confirmation_required",
                    "command_id": cmd_id,
                    "command": cmd,
                    "reason": reason,
                    "message": "需要你确认是否执行此操作~"
                })
            }
            "find_software" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let memory_path = &self.config.memory_path;
                if !crate::software_scanner::is_software_scanned(memory_path) {
                    return serde_json::json!({"status": "scanning", "message": "正在扫描电脑上的软件，请稍后重试~"});
                }
                let software_list = crate::software_scanner::load_software_cache(memory_path);
                let results = crate::software_scanner::search_software(query, &software_list, 10);
                serde_json::json!({
                    "software": results.iter().map(|sw| serde_json::json!({
                        "name": sw.name,
                        "path": sw.exec_path,
                        "category": sw.category,
                        "description": sw.description
                    })).collect::<Vec<_>>()
                })
            }
            "launch_software" => {
                let sw_path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                match tokio::process::Command::new("cmd")
                    .args(["/C", "start", "", sw_path])
                    .spawn()
                {
                    Ok(_) => serde_json::json!({
                        "success": true,
                        "message": format!("已启动: {}", sw_path)
                    }),
                    Err(e) => serde_json::json!({
                        "success": false,
                        "message": format!("启动失败: {}", e)
                    }),
                }
            }
            _ => serde_json::json!({"error": format!("未知工具: {}", name)}),
        }
    }
}

const TOOLS_JSON: &str = r#"[
    {
        "type": "function",
        "function": {
            "name": "capture_screen",
            "description": "截取用户屏幕并分析当前显示内容",
            "parameters": {
                "type": "object",
                "properties": {
                    "monitor": {"type": "integer", "description": "屏幕编号，1为主屏", "default": 1}
                },
                "required": []
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "read_file",
            "description": "读取工作区内的文件内容",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "文件相对路径，如 src/main.py"}
                },
                "required": ["path"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "write_file",
            "description": "创建或覆盖文件",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "list_files",
            "description": "列出工作区文件",
            "parameters": {
                "type": "object",
                "properties": {
                    "dir": {"type": "string", "description": "子目录，默认为根目录"}
                }
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "search_memory",
            "description": "从长期记忆中搜索相关信息",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "搜索关键词或问题"}
                },
                "required": ["query"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "add_memory",
            "description": "将重要信息保存到长期记忆",
            "parameters": {
                "type": "object",
                "properties": {
                    "content": {"type": "string"}
                },
                "required": ["content"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "execute_command",
            "description": "在电脑上执行命令行操作。注意：危险命令会被自动拦截。操作需要用户确认后才执行。",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "要执行的命令行命令"},
                    "cwd": {"type": "string", "description": "工作目录"},
                    "reason": {"type": "string", "description": "说明为什么要执行这个命令"}
                },
                "required": ["command", "reason"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "find_software",
            "description": "在电脑上搜索已安装的软件。支持通过关键词搜索。",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "搜索关键词"}
                },
                "required": ["query"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "launch_software",
            "description": "启动电脑上已安装的软件。必须先调用 find_software 确认软件存在且获取路径后，再调用此函数。",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "要启动的软件名称"},
                    "path": {"type": "string", "description": "软件执行路径"}
                },
                "required": ["name", "path"]
            }
        }
    }
]"#;