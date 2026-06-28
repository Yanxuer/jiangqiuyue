use crate::llm::types::{LLMMessage, LLMToolCall, LLMUsage};
use chrono::Local;
use serde::Serialize;
use std::path::PathBuf;

// ==================== 轨迹录制器 ====================

/// 轨迹录制器，将任务执行过程以 JSONL 格式写入文件。
///
/// 每条记录独立一行 JSON，支持增量写入——即使中途进程崩溃，
/// 之前的记录也不会丢失。
///
/// 生成文件路径：`<trajectory_dir>/trajectory_<session_id>.jsonl`
pub struct TrajectoryRecorder {
    file_path: PathBuf,
    session_id: String,
    start_time: String,
    task: String,
    provider: String,
    model: String,
    record_count: u64,
}

impl TrajectoryRecorder {
    /// 创建新的轨迹录制器
    ///
    /// # 参数
    ///
    /// * `trajectory_dir` — 轨迹文件存放目录
    pub fn new(trajectory_dir: &str) -> Result<Self, String> {
        let dir = PathBuf::from(trajectory_dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建轨迹目录失败: {}", e))?;

        let session_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let timestamp = Local::now().format("%Y%m%d_%H%M%S");
        let file_name = format!("trajectory_{}_{}.jsonl", timestamp, session_id);
        let file_path = dir.join(&file_name);

        log::info!("[Trajectory] 录制文件: {}", file_path.display());

        Ok(TrajectoryRecorder {
            file_path,
            session_id,
            start_time: Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string(),
            task: String::new(),
            provider: String::new(),
            model: String::new(),
            record_count: 0,
        })
    }

    /// 任务开始
    pub fn start(&mut self, task: &str, provider: &str, model: &str) {
        self.task = task.to_string();
        self.provider = provider.to_string();
        self.model = model.to_string();
        self.start_time = Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string();

        self.write_record(&TrajectoryEvent::SessionStart {
            session_id: self.session_id.clone(),
            start_time: self.start_time.clone(),
            task: task.to_string(),
            provider: provider.to_string(),
            model: model.to_string(),
        });
    }

    /// 记录 LLM 调用
    pub fn record_llm_call(
        &mut self,
        iteration: u32,
        messages: &[LLMMessage],
        response_content: Option<&str>,
        tool_calls: Option<&[LLMToolCall]>,
        usage: Option<&LLMUsage>,
        error: Option<&str>,
    ) {
        let input_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                let mut obj = serde_json::Map::new();
                obj.insert("role".to_string(), serde_json::Value::String(m.role.clone()));
                if let Some(ref content) = m.content {
                    // 截取过长内容，避免文件膨胀
                    let truncated = if content.len() > 1000 {
                        format!("{}... [共 {} 字符]", &content[..1000], content.len())
                    } else {
                        content.clone()
                    };
                    obj.insert("content".to_string(), serde_json::Value::String(truncated));
                }
                serde_json::Value::Object(obj)
            })
            .collect();

        let response = TrajectoryLLMResponse {
            content: response_content.map(|c| {
                if c.len() > 2000 {
                    format!("{}... [共 {} 字符]", &c[..2000], c.len())
                } else {
                    c.to_string()
                }
            }),
            tool_calls: tool_calls.map(|tcs| {
                tcs.iter()
                    .map(|tc| TrajectoryToolCall {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: {
                            if tc.arguments.len() > 500 {
                                format!("{}...", &tc.arguments[..500])
                            } else {
                                tc.arguments.clone()
                            }
                        },
                    })
                    .collect()
            }),
            usage: usage.map(|u| TrajectoryUsage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
            }),
            error: error.map(|e| e.to_string()),
        };

        self.write_record(&TrajectoryEvent::LLMCall {
            iteration,
            timestamp: Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string(),
            provider: self.provider.clone(),
            model: self.model.clone(),
            input_messages,
            response,
        });
    }

    /// 记录工具调用
    pub fn record_tool_call(
        &mut self,
        iteration: u32,
        tool_name: &str,
        args: &serde_json::Value,
        result: &serde_json::Value,
    ) {
        let result_truncated = serde_json::to_string(result).unwrap_or_default();
        let result_str = if result_truncated.len() > 2000 {
            format!("{}... [共 {} 字符]", &result_truncated[..2000], result_truncated.len())
        } else {
            result_truncated
        };

        self.write_record(&TrajectoryEvent::ToolCall {
            iteration,
            timestamp: Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string(),
            tool_name: tool_name.to_string(),
            arguments: args.clone(),
            result: result_str,
        });
    }

    /// 任务结束
    pub fn finalize(&mut self, success: bool, summary: Option<&str>) {
        let end_time = Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string();
        self.write_record(&TrajectoryEvent::SessionEnd {
            session_id: self.session_id.clone(),
            start_time: self.start_time.clone(),
            end_time,
            success,
            summary: summary.map(|s| s.to_string()),
            total_records: self.record_count,
        });
        log::info!(
            "[Trajectory] 录制完成: {} ({}) 条记录",
            self.file_path.display(),
            self.record_count
        );
    }

    /// 获取轨迹文件路径
    pub fn file_path(&self) -> &std::path::Path {
        &self.file_path
    }

    /// 写入一条 JSON 记录
    fn write_record(&mut self, event: &TrajectoryEvent) {
        let json = match serde_json::to_string(event) {
            Ok(j) => j,
            Err(e) => {
                log::error!("[Trajectory] 序列化失败: {}", e);
                return;
            }
        };

        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
        {
            Ok(mut file) => {
                use std::io::Write;
                if let Err(e) = writeln!(file, "{}", json) {
                    log::error!("[Trajectory] 写入失败: {}", e);
                } else {
                    self.record_count += 1;
                }
            }
            Err(e) => {
                log::error!("[Trajectory] 打开文件失败: {}", e);
            }
        }
    }
}

// ==================== 轨迹事件类型 ====================

#[derive(Serialize)]
#[serde(tag = "type")]
enum TrajectoryEvent {
    #[serde(rename = "session_start")]
    SessionStart {
        session_id: String,
        start_time: String,
        task: String,
        provider: String,
        model: String,
    },
    #[serde(rename = "llm_call")]
    LLMCall {
        iteration: u32,
        timestamp: String,
        provider: String,
        model: String,
        input_messages: Vec<serde_json::Value>,
        response: TrajectoryLLMResponse,
    },
    #[serde(rename = "tool_call")]
    ToolCall {
        iteration: u32,
        timestamp: String,
        tool_name: String,
        arguments: serde_json::Value,
        result: String,
    },
    #[serde(rename = "session_end")]
    SessionEnd {
        session_id: String,
        start_time: String,
        end_time: String,
        success: bool,
        summary: Option<String>,
        total_records: u64,
    },
}

#[derive(Serialize)]
struct TrajectoryLLMResponse {
    content: Option<String>,
    tool_calls: Option<Vec<TrajectoryToolCall>>,
    usage: Option<TrajectoryUsage>,
    error: Option<String>,
}

#[derive(Serialize)]
struct TrajectoryToolCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Serialize)]
struct TrajectoryUsage {
    input_tokens: u64,
    output_tokens: u64,
}
