use crate::config::Config;
use crate::cli_executor;
use crate::cli_hub::CliHub;
use crate::cli_tools;
use crate::file_tools::FileTools;
use crate::memory::AgentMemory;
use crate::llm::provider::LLMProvider;
use crate::llm::types::{ToolDefinition, LLMToolCall};
use crate::trajectory::recorder::TrajectoryRecorder;
use crate::screen;
use crate::desktop_control;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::future::Future;
use std::pin::Pin;

// ==================== 数据结构 ====================

/// 工具执行上下文 — 封装所有共享资源，使工具函数无需 &mut Agent
pub struct ToolContext {
    pub file_tools: Arc<FileTools>,
    pub memory: Arc<Mutex<AgentMemory>>,
    pub cli_hub: Arc<Mutex<CliHub>>,
    pub config: Config,
    pub desktop: Arc<Mutex<desktop_control::CuaDriverClient>>,
}

/// 顺序思维状态变更，从 execute_tool 返回后合并到 Agent
pub struct SequentialThinkingChange {
    pub data: ThoughtData,
    pub branch: Option<(String, ThoughtData)>,
    pub display: String,
}

// ==================== 数据结构 ====================

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
    pub iterations: u32,
    pub progress: Option<TaskProgress>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProgress {
    pub completed: Vec<String>,
    pub remaining: Vec<String>,
    pub key_code: Vec<String>,
    pub error_log: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PendingCommand {
    pub command: String,
    pub cwd: Option<String>,
    pub reason: String,
    pub analysis: cli_executor::CLIRequest,
}

// ==================== 顺序思维数据结构 ====================

#[derive(Debug, Clone)]
pub struct ThoughtData {
    pub thought: String,
    pub thought_number: u32,
    pub total_thoughts: u32,
    pub next_thought_needed: bool,
    pub is_revision: Option<bool>,
    pub revises_thought: Option<u32>,
    pub branch_from_thought: Option<u32>,
    pub branch_id: Option<String>,
    pub needs_more_thoughts: Option<bool>,
}

// ==================== Agent 主体 ====================

pub struct Agent {
    config: Config,
    file_tools: Arc<FileTools>,
    memory: Arc<Mutex<AgentMemory>>,
    messages: Vec<ChatMessage>,
    pub pending_commands: Arc<Mutex<HashMap<String, PendingCommand>>>,
    pub cli_hub: Arc<Mutex<CliHub>>,
    provider: LLMProvider,
    recorder: Option<TrajectoryRecorder>,
    desktop: Arc<Mutex<desktop_control::CuaDriverClient>>,
    // 日志广播（用于前端实时查看 DEFINE→SHIP 各阶段状态）
    log_sender: Option<tokio::sync::mpsc::Sender<String>>,
    // 多轮迭代状态
    iteration_count: u32,
    progress: TaskProgress,
    web_search_count: u32,
    file_edit_count: HashMap<String, u32>,
    // 顺序思维状态
    thought_history: Vec<ThoughtData>,
    branches: HashMap<String, Vec<ThoughtData>>,
}

const MAX_ITERATIONS: u32 = 30;
const MAX_WEB_SEARCHES: u32 = 5;
const MAX_FILE_EDITS: u32 = 10;

impl Agent {
    pub fn new(
        config: Config,
        file_tools: Arc<FileTools>,
        memory: Arc<Mutex<AgentMemory>>,
        cli_hub: CliHub,
        provider: LLMProvider,
        recorder: Option<TrajectoryRecorder>,
        desktop: Arc<Mutex<desktop_control::CuaDriverClient>>,
    ) -> Self {
        Agent {
            config,
            file_tools,
            memory,
            messages: vec![Self::system_prompt()],
            pending_commands: Arc::new(Mutex::new(HashMap::new())),
            cli_hub: Arc::new(Mutex::new(cli_hub)),
            provider,
            recorder,
            desktop,
            log_sender: None,
            iteration_count: 0,
            progress: TaskProgress {
                completed: Vec::new(),
                remaining: Vec::new(),
                key_code: Vec::new(),
                error_log: Vec::new(),
            },
            web_search_count: 0,
            file_edit_count: HashMap::new(),
            thought_history: Vec::new(),
            branches: HashMap::new(),
        }
    }

    /// 设置日志发送器，用于向前端实时推送 DEFINE→SHIP 各阶段日志
    pub fn set_log_sender(&mut self, sender: tokio::sync::mpsc::Sender<String>) {
        self.log_sender = Some(sender);
    }

    /// 发送阶段日志（同时输出到 log crate 和前端广播通道）
    fn send_stage_log(&self, msg: &str) {
        log::info!("{}", msg);
        if let Some(ref sender) = self.log_sender {
            let _ = sender.try_send(msg.to_string());
        }
    }

    pub fn file_tools(&self) -> &FileTools {
        &self.file_tools
    }

    pub fn memory(&self) -> &Arc<Mutex<AgentMemory>> {
        &self.memory
    }

    pub fn desktop(&self) -> &Arc<Mutex<desktop_control::CuaDriverClient>> {
        &self.desktop
    }

    pub fn get_config(&self) -> &Config {
        &self.config
    }

    pub fn update_config(&mut self, new_config: Config, new_provider: LLMProvider) {
        self.config = new_config;
        self.provider = new_provider;
    }

    // ==================== 系统提示词 ====================

    fn system_prompt() -> ChatMessage {
        ChatMessage {
            role: "system".to_string(),
            content: Some(serde_json::Value::String(
                r#"# 角色定位
你是江秋月——桌面级自主编程智能体，具备任务规划、文件读写、终端执行、联网检索、
多轮迭代、记忆回溯、安全沙箱、桌面控制全套能力。你遵循严谨的软件工程流程：
DEFINE → PLAN → BUILD → VERIFY → REVIEW → SHIP，每一步都有明确的质量门禁。

# 工程生命周期（强制遵守）

## 阶段一：DEFINE — 明确需求，编写规格
1. **表面假设**：在开始任何实现前，先列出你对需求的假设，让用户纠正：
   ```
   ASSUMPTIONS:
   1. 这是 Web 应用（非移动端）
   2. 使用已有项目结构
   3. 目标平台是 Windows
   → 如有偏差请纠正，否则我按此执行。
   ```
2. **编写规格**：用 view_file 浏览项目结构，明确六个维度：
   - 目标：做什么？谁用？成功标准是什么？
   - 命令：构建/测试/运行命令是什么？
   - 结构：源码在哪？测试在哪？配置在哪？
   - 风格：遵循项目已有命名和代码风格
   - 测试：用什么框架？测试放哪？覆盖率要求？
   - 边界：Always do / Ask first / Never do 三层约束

## 阶段二：PLAN — 任务拆解，制定计划
1. **依赖图分析**：识别模块间的依赖关系，自底向上排定实施顺序
2. **垂直切片**：每个任务是一条完整功能路径，不是按层拆分
   - 错误：Task1=全部数据库 → Task2=全部API → Task3=全部UI
   - 正确：Task1=创建功能(DB+API+UI) → Task2=列表功能(查询+API+UI)
3. **任务粒度**：每个任务应在单次迭代中可完成、可测试、可验证，不超过约100行代码

## 阶段三：BUILD — 增量实现，小步快跑
1. **增量周期**：Implement → Test → Verify → Commit → Next slice
2. **一次只改一处**：每轮迭代只修改一个逻辑相关的代码块
3. **改前必读**：修改任何文件前，先用 view_file 读取原文
4. **改后必验**：修改后立即运行编译/测试，确认通过再继续
5. **风险优先**：先攻克最不确定、风险最高的部分

## 阶段四：VERIFY — 测试验证，排错修复
1. **Stop the Line 规则**：遇到任何报错/失败，立即停止新增功能：
   a. PRESERVE 证据（错误输出、日志、复现步骤）
   b. DIAGNOSE 使用系统排查（检查最近改动、对比差异、二分定位）
   c. FIX 根因而非表面症状
   d. GUARD 添加回归测试防止复发
   e. RESUME 验证通过后才继续
2. **不可复现的 Bug**：添加日志和时序信息，在隔离环境中重试，记录条件后监控
3. **TDD 原则**：修复 Bug 前先写一个能复现 Bug 的测试，修复后确认测试通过

## 阶段五：REVIEW — 代码审查，质量把关
在调用 task_complete 前，对全部改动进行五轴审查：
1. **正确性**：是否匹配需求？边界情况（null/空/边界值）处理了？错误路径覆盖了？
2. **可读性**：命名是否描述意图？控制流是否直观？有没有"聪明"但难懂的代码？
3. **架构**：是否遵循项目已有模式？有没有循环依赖？模块边界是否清晰？
4. **安全**：用户输入是否验证？密钥是否硬编码？SQL 是否参数化？输出是否编码？
   - 所有外部输入（API、文件、用户输入）视为不可信
   - 密码必须哈希（bcrypt/scrypt/argon2），禁止明文存储
   - 敏感操作必须有审计日志
5. **性能**：有没有 N+1 查询？循环内有没有不必要的 I/O？资源是否正确释放？

## 阶段六：SHIP — 交付完成，定义标准
调用 task_complete 前必须确认以下全部通过：
- [ ] 所有验收标准已满足
- [ ] 修改后代码已编译通过并运行验证
- [ ] 没有遗留的调试代码、注释掉的旧代码、TODO 标记
- [ ] 没有引入无关的改动或重构
- [ ] 外部接口变更已考虑向后兼容
- [ ] 安全影响已审查（输入验证、认证、数据处理）
- [ ] 在 summary 中提供完整总结：改了什么、为什么这样改、运行结果、注意事项

# 核心行为准则

## 假设前置
模糊需求不要默默猜测，先列出假设让用户确认。这是最便宜的错误预防方式。

## 主动质疑
发现方案有明显问题时，直接指出并量化影响（"这会在每次请求增加约200ms延迟"），
然后提出替代方案。你不是应声虫。

## 越简单越好
- 能用 10 行解决的问题不要写 100 行
- 不到第三次复用时不要抽象
- 相比"精巧"，优先选择"明显"
- 删除死代码比保留注释替代更有价值

## 困惑管理
遇到矛盾需求或不清楚的规格时：
1. 停止，不要猜测
2. 明确指出困惑点
3. 列出权衡或追问澄清
4. 等待用户确认后继续

# 工具使用约束
1. 简单文字解释类问题不调用工具；仅代码/文件/运行/检索时使用
2. bash_exec 禁止 rm -rf /、格式化磁盘、删除系统文件等高风险指令
3. web_search 单次任务最多 5 次，单文件修改不超过 10 次迭代
4. 歧义时优先问用户，而非自由发挥

# 输出格式
所有输出必须是标准 tool_calls 格式，禁止自由文字聊天。
仅当调用 task_complete 时，在 summary 中使用自然语言完整总结。

# 可用工具速查
| 工具 | 用途 | 关键参数 |
|------|------|----------|
| view_file | 读取文件/列出目录 | path, start_line, end_line |
| str_replace_edit | 修改/新建/删除代码 | path, old_str, new_str |
| bash_exec | 执行 shell 命令 | command, cwd, timeout |
| web_search | 检索文档/报错方案 | query |
| task_complete | 结束任务输出总结 | summary |
| capture_screen | 截取用户屏幕 | monitor |
| search_memory | 搜索长期记忆 | query |
| add_memory | 保存到长期记忆 | content |
| find_software | 搜索本机软件 | query |
| launch_software | 启动软件 | path |
| desktop_screenshot | 后台桌面截图（全屏/窗口级） | window_title?, monitor? |
| desktop_click | 后台鼠标点击 | x, y, button? |
| desktop_type | 后台键盘输入文本 | text |
| desktop_key | 按键/组合键 | keys |
| desktop_list_windows | 枚举桌面窗口 | filter? |
| desktop_focus_window | 聚焦指定窗口 | window_title |
| desktop_scroll | 鼠标滚轮滚动 | x?, y?, direction?, amount? |
| list_clis | 列出 CLI 工具 | category, source |
| search_clis | 搜索 CLI 工具 | query |
| install_cli | 安装 CLI 工具 | name |
| execute_cli | 执行 CLI 命令 | name, command |
| recommend_clis | 推荐 CLI 工具 | software_names |"#.to_string(),
            )),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    // ==================== 工具定义 ====================

    fn tools_definition() -> Vec<ToolDefinition> {
        vec![
            // === 5 个标准工具 ===
            ToolDefinition {
                name: "view_file".to_string(),
                description: "读取本地文件内容（带行号），或列出指定目录下文件，最多2级文件夹，用于理解项目结构、查看代码".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "本地文件/文件夹绝对路径"},
                        "start_line": {"type": "integer", "default": 0, "description": "起始行"},
                        "end_line": {"type": "integer", "default": 50, "description": "结束行"}
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "str_replace_edit".to_string(),
                description: "修改现有代码、新建文件、删除代码块，精确字符串替换，仅用于代码编写。修改前必须先调用view_file读取原文，确保old_str完全匹配。".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "文件路径"},
                        "old_str": {"type": "string", "description": "需要替换的原文，多行严格匹配；新建文件时填空字符串"},
                        "new_str": {"type": "string", "description": "替换后的新代码"},
                        "create_if_missing": {"type": "boolean", "default": true, "description": "文件不存在则创建"}
                    },
                    "required": ["path", "new_str"]
                }),
            },
            ToolDefinition {
                name: "bash_exec".to_string(),
                description: "执行shell命令：安装依赖、运行程序、git操作、编译项目。高危命令（rm -rf /、格式化磁盘等）会被自动拦截。".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {"type": "string", "description": "单条shell指令"},
                        "cwd": {"type": "string", "description": "执行工作目录，默认当前项目根目录"},
                        "timeout": {"type": "integer", "default": 30, "description": "执行超时时间，单位秒"}
                    },
                    "required": ["command"]
                }),
            },
            ToolDefinition {
                name: "web_search".to_string(),
                description: "查询编程文档、报错解决方案、第三方库API，仅在本地代码无法解决时调用。单次任务最多调用5次。".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string", "description": "精准检索关键词，一次最多3个查询词"}
                    },
                    "required": ["query"]
                }),
            },
            ToolDefinition {
                name: "task_complete".to_string(),
                description: "所有步骤完成、信息充足后调用，输出完整总结、代码、运行结果，停止工具循环。任务未完成时禁止调用。".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "summary": {"type": "string", "description": "任务完成总结，包含修改文件、运行效果、注意事项"}
                    },
                    "required": ["summary"]
                }),
            },
            // === 扩展工具 ===
            ToolDefinition {
                name: "capture_screen".to_string(),
                description: "截取用户屏幕并分析当前显示内容".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "monitor": {"type": "integer", "description": "屏幕编号，1为主屏", "default": 1}
                    },
                    "required": []
                }),
            },
            ToolDefinition {
                name: "search_memory".to_string(),
                description: "从长期记忆中搜索相关信息".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string", "description": "搜索关键词或问题"}
                    },
                    "required": ["query"]
                }),
            },
            ToolDefinition {
                name: "add_memory".to_string(),
                description: "将重要信息保存到长期记忆".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "content": {"type": "string"}
                    },
                    "required": ["content"]
                }),
            },
            ToolDefinition {
                name: "find_software".to_string(),
                description: "搜索本机已安装的软件".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string", "description": "软件名称关键词"}
                    },
                    "required": ["query"]
                }),
            },
            ToolDefinition {
                name: "launch_software".to_string(),
                description: "启动本机软件".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "软件可执行文件路径"}
                    },
                    "required": ["path"]
                }),
            },
            // === 桌面控制工具 (cua-driver) ===
            ToolDefinition {
                name: "desktop_screenshot".to_string(),
                description: "截取桌面或指定窗口的截图（后台操作，不抢用户焦点）。\n- 不传参数: 截取全屏\n- 传 window_title: 截取标题包含该关键词的窗口\n- 结果包含 base64 编码的图片".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "window_title": {"type": "string", "description": "窗口标题关键词（可选），不传则全屏截图"},
                        "monitor": {"type": "integer", "description": "屏幕编号，1=主屏，默认1"}
                    },
                    "required": []
                }),
            },
            ToolDefinition {
                name: "desktop_click".to_string(),
                description: "在指定屏幕坐标处点击鼠标（后台操作，不抢用户焦点）。可用于点击按钮、菜单等UI元素".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "x": {"type": "integer", "description": "X坐标（像素）"},
                        "y": {"type": "integer", "description": "Y坐标（像素）"},
                        "button": {"type": "string", "enum": ["left", "right", "middle"], "description": "鼠标按键，默认left"}
                    },
                    "required": ["x", "y"]
                }),
            },
            ToolDefinition {
                name: "desktop_type".to_string(),
                description: "在桌面当前焦点位置输入文本（后台操作，不抢用户焦点）。需先调用 desktop_focus_window 确保目标窗口在前台".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "text": {"type": "string", "description": "要输入的文本内容"}
                    },
                    "required": ["text"]
                }),
            },
            ToolDefinition {
                name: "desktop_key".to_string(),
                description: "按下键盘按键或组合键（后台操作）。\n- 单键: 'enter', 'escape', 'tab', 'backspace', 'delete'\n- 组合键: 'ctrl+s', 'alt+f4', 'ctrl+shift+escape'".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "keys": {"type": "string", "description": "按键或组合键，如 'enter', 'ctrl+s'"}
                    },
                    "required": ["keys"]
                }),
            },
            ToolDefinition {
                name: "desktop_list_windows".to_string(),
                description: "列出当前桌面所有可见窗口。可传入 filter 参数按标题或进程名过滤".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "filter": {"type": "string", "description": "窗口标题或进程名关键词过滤（可选）"}
                    },
                    "required": []
                }),
            },
            ToolDefinition {
                name: "desktop_focus_window".to_string(),
                description: "将指定窗口切换到前台（后台操作，不抢用户焦点）。配合 desktop_type/desktop_click 使用".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "window_title": {"type": "string", "description": "窗口标题关键词，用于匹配目标窗口"}
                    },
                    "required": ["window_title"]
                }),
            },
            ToolDefinition {
                name: "desktop_scroll".to_string(),
                description: "在指定位置执行鼠标滚轮滚动（后台操作）。用于滚动网页、文档等".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "x": {"type": "integer", "description": "X坐标，默认当前鼠标位置"},
                        "y": {"type": "integer", "description": "Y坐标，默认当前鼠标位置"},
                        "direction": {"type": "string", "enum": ["up", "down"], "description": "滚动方向，默认down"},
                        "amount": {"type": "integer", "description": "滚动量（行数），默认3"}
                    },
                    "required": []
                }),
            },
            ToolDefinition {
                name: "list_clis".to_string(),
                description: "列出 CLI-Hub 中所有可用的 CLI 工具，支持按分类或来源筛选".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "category": {"type": "string", "description": "按分类筛选"},
                        "source": {"type": "string", "enum": ["harness", "public", "all"], "description": "按来源筛选"}
                    },
                    "required": []
                }),
            },
            ToolDefinition {
                name: "search_clis".to_string(),
                description: "在 CLI-Hub 注册表中搜索 CLI 工具".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string", "description": "搜索关键词"}
                    },
                    "required": ["query"]
                }),
            },
            ToolDefinition {
                name: "install_cli".to_string(),
                description: "安装指定的 CLI 工具".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "CLI 工具名称"}
                    },
                    "required": ["name"]
                }),
            },
            ToolDefinition {
                name: "execute_cli".to_string(),
                description: "执行已安装 CLI 的命令".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "CLI 工具名称"},
                        "command": {"type": "string", "description": "要执行的命令"}
                    },
                    "required": ["name", "command"]
                }),
            },
            ToolDefinition {
                name: "recommend_clis".to_string(),
                description: "根据已安装软件推荐可用的 CLI 工具".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "software_names": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "软件名称列表"
                        }
                    },
                    "required": ["software_names"]
                }),
            },
            ToolDefinition {
                name: "sequentialthinking".to_string(),
                description: "顺序思维工具，用于将复杂问题分解为逐步思考过程。每个思考可以建立、质疑或修正之前的见解。".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "thought": {"type": "string", "description": "当前的思考步骤内容"},
                        "next_thought_needed": {"type": "boolean", "description": "是否还需要继续思考"},
                        "thought_number": {"type": "integer", "description": "当前是第几步思考，从1开始"},
                        "total_thoughts": {"type": "integer", "description": "预估总思考步数"},
                        "is_revision": {"type": "boolean", "description": "是否在修订之前的思考"},
                        "revises_thought": {"type": "integer", "description": "如果是在修订，标明修订第几步"},
                        "branch_from_thought": {"type": "integer", "description": "从哪一步开始分支"},
                        "branch_id": {"type": "string", "description": "分支标识符"},
                        "needs_more_thoughts": {"type": "boolean", "description": "是否还需要更多思考"}
                    },
                    "required": ["thought", "next_thought_needed", "thought_number", "total_thoughts"]
                }),
            },
        ]
    }

    // ==================== 多轮迭代运行循环 ====================

    pub async fn run(&mut self, user_input: &str, image_base64: Option<&str>) -> Result<AgentResult, String> {
        // ============ 阶段一：DEFINE — 明确需求 ============
        self.send_stage_log("[DEFINE] ╔══════════════════════════════════════════╗");
        self.send_stage_log("[DEFINE] ║       阶段一：DEFINE — 明确需求          ║");
        self.send_stage_log("[DEFINE] ╚══════════════════════════════════════════╝");
        self.send_stage_log(&format!("[DEFINE] 用户输入: {}", user_input));
        self.send_stage_log(&format!("[DEFINE] 多模态: {}", image_base64.is_some()));
        self.send_stage_log(&format!("[DEFINE] Provider: {} / {}", self.provider.name(), self.provider.model()));

        // 重置消息历史（只保留系统提示词）
        self.messages = vec![Self::system_prompt()];
        log::info!("[DEFINE] 系统提示词已加载 ({} 字符)", Self::system_prompt().content.unwrap_or_default().to_string().len());

        // 重置迭代状态
        self.iteration_count = 0;
        self.progress = TaskProgress {
            completed: Vec::new(),
            remaining: Vec::new(),
            key_code: Vec::new(),
            error_log: Vec::new(),
        };
        self.web_search_count = 0;
        self.file_edit_count.clear();
        self.thought_history.clear();
        self.branches.clear();

        // 轨迹录制：任务开始
        if let Some(ref mut recorder) = self.recorder {
            recorder.start(user_input, self.provider.name(), self.provider.model()).await;
        }

        // 构建用户消息
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

        let mut all_tool_calls: Vec<String> = Vec::new();

        // ============ 主循环：多轮迭代直到 task_complete ============
        let mut is_first_iteration = true;
        loop {
            self.iteration_count += 1;
            log::info!("[Agent] --- 迭代 #{}/{} ---", self.iteration_count, MAX_ITERATIONS);

            // 阶段二：PLAN — 首次迭代时输出计划阶段日志
            if is_first_iteration {
                self.send_stage_log("[PLAN]   ╔══════════════════════════════════════════╗");
                self.send_stage_log("[PLAN]   ║       阶段二：PLAN — 任务拆解           ║");
                self.send_stage_log("[PLAN]   ╚══════════════════════════════════════════╝");
                self.send_stage_log(&format!("[PLAN]   可用工具: {} 个", Self::tools_definition().len()));
                self.send_stage_log("[PLAN]   开始调用 LLM 进行任务拆解与规划...");
                is_first_iteration = false;
            }

            // 检查迭代上限
            if self.iteration_count > MAX_ITERATIONS {
                log::warn!("[Agent] 达到最大迭代次数 {}", MAX_ITERATIONS);
                log::warn!("[SHIP]   强制终止: 迭代超限 ({} > {})", self.iteration_count, MAX_ITERATIONS);
                log::warn!("[SHIP]   已完成: {:?}", self.progress.completed);
                log::warn!("[SHIP]   错误日志: {:?}", self.progress.error_log);
                let summary = format!(
                    "任务迭代次数已达上限({})。\n已完成: {}\n剩余: {}\n报错记录: {}",
                    MAX_ITERATIONS,
                    self.progress.completed.join(", "),
                    self.progress.remaining.join(", "),
                    self.progress.error_log.join("; ")
                );
                return Ok(AgentResult {
                    reply: summary,
                    tool_calls: all_tool_calls,
                    iterations: self.iteration_count,
                    progress: Some(self.progress.clone()),
                });
            }

            // 调用 LLM（通过 provider）
            let message_payload: Vec<crate::llm::types::LLMMessage> = self.messages.iter()
                .map(|m| {
                    crate::llm::types::LLMMessage {
                        role: m.role.clone(),
                        content: m.content.as_ref().map(|c| match c {
                            serde_json::Value::String(s) => s.clone(),
                            _ => c.to_string(),
                        }),
                        tool_calls: m.tool_calls.as_ref().map(|tcs| {
                            tcs.iter().map(|tc| LLMToolCall {
                                id: tc.id.clone(),
                                name: tc.name.clone(),
                                arguments: serde_json::to_string(&tc.arguments).unwrap_or_default(),
                            }).collect()
                        }),
                        tool_call_id: m.tool_call_id.clone(),
                        name: m.name.clone(),
                    }
                })
                .collect();

            let tools = Self::tools_definition();
            log::info!("[Agent] 迭代 #{}: 准备调用 LLM (messages={}, tools={})", self.iteration_count, self.messages.len(), tools.len());
            self.send_stage_log(&format!("[PLAN]   正在调用 LLM (迭代 #{}, 消息数={})...", self.iteration_count, self.messages.len()));

            let llm_response = self.provider.chat(&message_payload, Some(&tools)).await.map_err(|e| {
                log::error!("[Agent] LLM 调用失败: {}", e);
                self.send_stage_log(&format!("[PLAN]   ❌ LLM 调用失败: {}", e));
                e
            })?;
            log::info!("[Agent] LLM 调用成功: content_len={}, tool_calls={}",
                llm_response.message.content.as_ref().map(|c| c.len()).unwrap_or(0),
                llm_response.message.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0));
            let resp_msg = &llm_response.message;

            // 轨迹录制：LLM 调用
            if let Some(ref mut recorder) = self.recorder {
                recorder.record_llm_call(
                    self.iteration_count,
                    &message_payload,
                    resp_msg.content.as_deref(),
                    resp_msg.tool_calls.as_deref(),
                    llm_response.usage.as_ref(),
                    None,
                ).await;
            }

            let tool_calls = resp_msg.tool_calls.clone().unwrap_or_default();

            // 无工具调用 → 直接返回内容
            if tool_calls.is_empty() {
                let reply = resp_msg.content.clone().unwrap_or_default();
                log::info!("[Agent] LLM 无工具调用，返回文本 ({}字符)", reply.len());
                self.send_stage_log(&format!("[SHIP]   LLM 直接返回文本 ({}字符)", reply.len()));
                self.messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: Some(serde_json::Value::String(reply.clone())),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });
                // 轨迹录制：任务完成
                if let Some(ref mut recorder) = self.recorder {
                    recorder.finalize(true, Some(&reply)).await;
                }
                return Ok(AgentResult {
                    reply,
                    tool_calls: all_tool_calls,
                    iterations: self.iteration_count,
                    progress: Some(self.progress.clone()),
                });
            }

            // 记录 assistant 消息（含 tool_calls）
            self.messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: resp_msg.content.as_ref().map(|c| serde_json::Value::String(c.clone())),
                tool_calls: Some(
                    tool_calls
                        .iter()
                        .map(|tc| ToolCall {
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            arguments: serde_json::from_str(&tc.arguments).unwrap_or_default(),
                        })
                        .collect(),
                ),
                tool_call_id: None,
                name: None,
            });

            // ====== 三步流水线（并行化版）：检查 → 并行执行 → 顺序记录 ======
            let mut should_stop = false;
            let mut final_summary = String::new();

            // Step 1: 前置检查 & 筛选（串行，修改共享计数器）
            struct FilteredTool {
                id: String,
                name: String,
                args: serde_json::Value,
            }
            let mut filtered: Vec<FilteredTool> = Vec::new();
            for tc in &tool_calls {
                let tool_name = tc.name.clone();
                let args: serde_json::Value =
                    serde_json::from_str(&tc.arguments).unwrap_or_default();

                log::info!("[BUILD]  ─── 工具调用检测: {} (参数: {})", tool_name, args);

                if tool_name == "web_search" {
                    self.web_search_count += 1;
                    if self.web_search_count > MAX_WEB_SEARCHES {
                        let result = serde_json::json!({
                            "error": format!("web_search 已达上限({}次), 请基于已有信息继续", MAX_WEB_SEARCHES)
                        });
                        self.messages.push(ChatMessage {
                            role: "tool".to_string(),
                            content: Some(serde_json::Value::String(result.to_string())),
                            tool_calls: None,
                            tool_call_id: Some(tc.id.clone()),
                            name: Some(tool_name.clone()),
                        });
                        continue;
                    }
                }

                if tool_name == "str_replace_edit" {
                    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                    let count = self.file_edit_count.entry(path.to_string()).or_insert(0);
                    *count += 1;
                    if *count > MAX_FILE_EDITS {
                        let result = serde_json::json!({
                            "error": format!("文件 {} 已修改{}次，达到上限，请向用户说明卡点", path, MAX_FILE_EDITS)
                        });
                        self.messages.push(ChatMessage {
                            role: "tool".to_string(),
                            content: Some(serde_json::Value::String(result.to_string())),
                            tool_calls: None,
                            tool_call_id: Some(tc.id.clone()),
                            name: Some(tool_name.clone()),
                        });
                        continue;
                    }
                }

                filtered.push(FilteredTool { id: tc.id.clone(), name: tool_name, args });
            }

            self.send_stage_log(&format!("[BUILD]  ─── 阶段三：BUILD — 并行执行 {} 个工具 ───", filtered.len()));

            // Step 2: 并行执行所有通过检查的工具
            let tp_for_st = self.progress.clone();
            let iter_for_st = self.iteration_count;

            let mut handles: Vec<(usize, String, Pin<Box<dyn Future<Output = (serde_json::Value, Option<SequentialThinkingChange>)> + Send>>)> = Vec::new();
            for (idx, ft) in filtered.iter().enumerate() {
                let name = ft.name.clone();
                let args = ft.args.clone();
                let id = ft.id.clone();
                let ctx = ToolContext {
                    file_tools: self.file_tools.clone(),
                    memory: self.memory.clone(),
                    cli_hub: self.cli_hub.clone(),
                    config: self.config.clone(),
                    desktop: self.desktop.clone(),
                };
                let tp = tp_for_st.clone();
                let iter = iter_for_st;
                handles.push((idx, ft.id.clone(), Box::pin(async move {
                    let mut st = None;
                    log::info!("[BUILD]  → 执行工具: {} (id={})", name, id);
                    let value = execute_tool_parallel(&ctx, &name, args, &tp, iter, &mut st).await;
                    let is_err = value.get("error").is_some();
                    log::info!("[BUILD]  ← 工具完成: {} (id={}) {}",
                        name, id,
                        if is_err { "❌ 有错误" } else { "✅ 成功" });
                    (value, st)
                })));
            }

            // 并行等待所有结果
            let mut results: Vec<(usize, String, serde_json::Value, Option<SequentialThinkingChange>)> = Vec::new();
            for (idx, id, fut) in handles {
                let (value, st_change) = fut.await;
                results.push((idx, id, value, st_change));
            }
            // 恢复原始顺序（按 filtered 数组顺序）
            results.sort_by_key(|(idx, _, _, _)| *idx);

            // Step 3: 顺序记录（轨迹 + 进度 + messages，保持确定性顺序）
            self.send_stage_log("[VERIFY] ─── 阶段四：VERIFY — 记录结果与错误检查 ───");
            for (_, tool_call_id, result, st_change) in &results {
                let tool_name_for_log = filtered.iter()
                    .find(|ft| &ft.id == tool_call_id)
                    .map(|ft| ft.name.as_str())
                    .unwrap_or("unknown");

                all_tool_calls.push(tool_name_for_log.to_string());

                // 轨迹录制
                if let Some(ref mut recorder) = self.recorder {
                    recorder.record_tool_call(
                        self.iteration_count,
                        tool_name_for_log,
                        &serde_json::Value::Null, // args 在 Step 1 中已提取
                        result,
                    ).await;
                }

                // 顺序思维状态合并
                if tool_name_for_log == "sequentialthinking" {
                    if let Some(ref change) = st_change {
                        self.thought_history.push(change.data.clone());
                        if let Some((bid, bdata)) = &change.branch {
                            let entry = self.branches.entry(bid.clone()).or_insert_with(Vec::new);
                            entry.push(bdata.clone());
                        }
                    }
                }

                // 检查 task_complete
                if tool_name_for_log == "task_complete" {
                    should_stop = true;
                    final_summary = result
                        .get("summary")
                        .and_then(|v| v.as_str())
                        .unwrap_or("任务完成")
                        .to_string();
                    self.send_stage_log("[REVIEW] ┌──────────────────────────────────────┐");
                    self.send_stage_log("[REVIEW] │     阶段五：REVIEW — 代码审查        │");
                    self.send_stage_log("[REVIEW] └──────────────────────────────────────┘");
                    self.send_stage_log("[REVIEW] task_complete 触发，准备交付");
                    self.send_stage_log(&format!("[REVIEW] 总结摘要: {}", final_summary));
                }

                // 更新进度
                self.progress.completed.push(format!("{}: {}",
                    tool_name_for_log,
                    if tool_name_for_log == "bash_exec" {
                        "shell_command".to_string()
                    } else if tool_name_for_log == "view_file" {
                        "file_view".to_string()
                    } else {
                        String::new()
                    }
                ));

                // 记录错误
                if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
                    if !err.is_empty() {
                        log::warn!("[VERIFY] ⚠ 工具 [{}] 返回错误: {}", tool_name_for_log, err);
                        self.progress.error_log.push(format!("[{}] {}", tool_name_for_log, err));
                    }
                }

                self.messages.push(ChatMessage {
                    role: "tool".to_string(),
                    content: Some(serde_json::Value::String(result.to_string())),
                    tool_calls: None,
                    tool_call_id: Some(tool_call_id.clone()),
                    name: Some(tool_name_for_log.to_string()),
                });
            }

            // 每 3 轮记录进度
            if all_tool_calls.len() % 3 == 0 {
                log::info!("[Agent] 进度快照: 已完成={}, 报错={}",
                    self.progress.completed.len(),
                    self.progress.error_log.len());
            }

            if should_stop {
                log::info!("[Agent] task_complete 触发，结束循环");
                log::info!("[Agent] 总迭代: {}, 工具调用: {}", self.iteration_count, all_tool_calls.len());
                // ============ 阶段六：SHIP — 交付完成 ============
                self.send_stage_log("[SHIP]   ╔══════════════════════════════════════════╗");
                self.send_stage_log("[SHIP]   ║       阶段六：SHIP — 交付完成            ║");
                self.send_stage_log("[SHIP]   ╚══════════════════════════════════════════╝");
                self.send_stage_log(&format!("[SHIP]   总迭代次数: {}", self.iteration_count));
                self.send_stage_log(&format!("[SHIP]   工具调用数: {}", all_tool_calls.len()));
                self.send_stage_log(&format!("[SHIP]   工具列表: {:?}", all_tool_calls));
                self.send_stage_log(&format!("[SHIP]   错误记录: {} 条", self.progress.error_log.len()));
                self.send_stage_log(&format!("[SHIP]   已完成: {}", self.progress.completed.len()));
                // 轨迹录制：任务完成
                if let Some(ref mut recorder) = self.recorder {
                    recorder.finalize(true, Some(&final_summary)).await;
                }
                return Ok(AgentResult {
                    reply: final_summary,
                    tool_calls: all_tool_calls,
                    iterations: self.iteration_count,
                    progress: Some(self.progress.clone()),
                });
            }

            log::info!("[Agent] 迭代 #{} 完成, 继续下一轮...", self.iteration_count);
        }
    }
}

// ==================== 工具执行（并行安全版） ====================

/// 工具执行入口 — 不持有 &mut Agent，可安全并行调用。
/// sequentialthinking 的 ThoughtData 通过 st_change 输出参数返回
async fn execute_tool_parallel(
    ctx: &ToolContext,
    name: &str,
    args: serde_json::Value,
    progress: &TaskProgress,
    iteration_count: u32,
    st_change: &mut Option<SequentialThinkingChange>,
) -> serde_json::Value {
    match name {
            // ===== 5 个标准工具 =====

            "view_file" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let start = args.get("start_line").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let end = args.get("end_line").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

                log::info!("[Agent:view_file] path={}, lines={}-{}", path, start, end);

                // 判断是文件还是目录
                let full_path = std::path::Path::new(path);
                if !full_path.exists() {
                    return serde_json::json!({"error": format!("路径不存在: {}", path)});
                }

                if full_path.is_dir() {
                    // 列出目录（最多2级）
                    match list_directory(path, 2) {
                        Ok(files) => {
                            log::info!("[Agent:view_file] 目录列出 {} 个条目", files.len());
                            serde_json::json!({
                                "type": "directory",
                                "path": path,
                                "entries": files,
                                "count": files.len()
                            })
                        }
                        Err(e) => serde_json::json!({"error": e}),
                    }
                } else {
                    // 读取文件内容
                    match std::fs::read_to_string(path) {
                        Ok(content) => {
                            let lines: Vec<&str> = content.lines().collect();
                            let total = lines.len();
                            let actual_end = end.min(total);
                            let actual_start = start.min(actual_end);

                            let mut output = String::new();
                            for (i, line) in lines[actual_start..actual_end].iter().enumerate() {
                                output.push_str(&format!("{:>6}| {}\n", actual_start + i + 1, line));
                            }

                            let truncated = if actual_end < total {
                                format!("\n... (共 {} 行, 已显示 {}-{} 行)", total, actual_start + 1, actual_end)
                            } else {
                                String::new()
                            };

                            log::info!("[Agent:view_file] 读取 {} 行 (总计 {} 行)", actual_end - actual_start, total);

                            serde_json::json!({
                                "type": "file",
                                "path": path,
                                "total_lines": total,
                                "displayed_lines": format!("{}-{}", actual_start + 1, actual_end),
                                "content": format!("{}{}", output, truncated)
                            })
                        }
                        Err(e) => serde_json::json!({"error": format!("读取文件失败: {}", e)}),
                    }
                }
            }

            "str_replace_edit" => {
                // ========== 前置日志 ==========
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let old_str = args.get("old_str").and_then(|v| v.as_str()).unwrap_or("");
                let new_str = args.get("new_str").and_then(|v| v.as_str()).unwrap_or("");
                let create_if_missing = args.get("create_if_missing")
                    .and_then(|v| v.as_bool()).unwrap_or(true);

                let ts = chrono::Local::now().format("%H:%M:%S%.3f");
                log::info!("[str_replace_edit] ╔══════════════ 开始执行 @ {} ══════════════", ts);
                log::info!("[str_replace_edit] ║ 目标文件: {}", path);
                log::info!("[str_replace_edit] ║ old_str 长度: {} 字符", old_str.len());
                log::info!("[str_replace_edit] ║ new_str 长度: {} 字符", new_str.len());
                log::info!("[str_replace_edit] ║ create_if_missing: {}", create_if_missing);
                if !old_str.is_empty() {
                    let preview = &old_str[..old_str.len().min(150)];
                    log::info!("[str_replace_edit] ║ old_str 预览: {}", preview.replace('\n', "\\n"));
                } else {
                    log::info!("[str_replace_edit] ║ old_str 预览: (空 — 将新建文件)");
                }
                if !new_str.is_empty() {
                    let preview = &new_str[..new_str.len().min(150)];
                    log::info!("[str_replace_edit] ║ new_str 预览: {}", preview.replace('\n', "\\n"));
                }

                let full_path = std::path::Path::new(path);

                // 新建文件
                if old_str.is_empty() || !full_path.exists() {
                    if !full_path.exists() {
                        log::info!("[str_replace_edit] ║ 文件不存在，将创建新文件");
                    }
                    if !create_if_missing {
                        log::error!("[str_replace_edit] ╚ 拒绝创建: create_if_missing=false");
                        return serde_json::json!({"error": format!("文件不存在且 create_if_missing=false: {}", path)});
                    }
                    if let Some(parent) = full_path.parent() {
                        std::fs::create_dir_all(parent).ok();
                        log::info!("[str_replace_edit] ║ 已创建父目录: {}", parent.display());
                    }
                    match std::fs::write(full_path, new_str) {
                        Ok(_) => {
                            let lines = new_str.lines().count();
                            let size = new_str.len();
                            log::info!("[str_replace_edit] ║ ✓ 新建成功: {} 行, {} 字节", lines, size);
                            log::info!("[str_replace_edit] ╚══════════════ 执行完成 (新建) ══════════════");
                            serde_json::json!({
                                "success": true,
                                "action": "created",
                                "path": path,
                                "lines": lines,
                                "size_bytes": size,
                                "message": format!("已创建文件 {} ({} 行, {} 字节)", path, lines, size)
                            })
                        }
                        Err(e) => {
                            log::error!("[str_replace_edit] ║ ✗ 创建文件失败: {}", e);
                            log::error!("[str_replace_edit] ╚══════════════ 执行失败 ══════════════");
                            serde_json::json!({"error": format!("创建文件失败: {}", e)})
                        }
                    }
                } else {
                    // 修改现有文件
                    let file_size = full_path.metadata().map(|m| m.len()).unwrap_or(0);
                    log::info!("[str_replace_edit] ║ 文件已存在: {} 字节", file_size);

                    let content = match std::fs::read_to_string(full_path) {
                        Ok(c) => {
                            log::info!("[str_replace_edit] ║ 文件读取成功: {} 行, {} 字节", c.lines().count(), c.len());
                            c
                        }
                        Err(e) => {
                            log::error!("[str_replace_edit] ║ ✗ 读取文件失败: {}", e);
                            log::error!("[str_replace_edit] ╚══════════════ 执行失败 ══════════════");
                            return serde_json::json!({"error": format!("读取文件失败: {}", e)});
                        }
                    };

                    if !content.contains(old_str) {
                        log::warn!("[str_replace_edit] ║ ✗ old_str 未在文件中找到!");
                        log::warn!("[str_replace_edit] ║ 搜索内容(前200字符): {}",
                            &old_str[..old_str.len().min(200)].replace('\n', "\\n"));
                        // 尝试定位最接近的匹配
                        let old_first_line = old_str.lines().next().unwrap_or("");
                        if let Some(pos) = content.find(old_first_line) {
                            let context_start = pos.saturating_sub(20);
                            let context_end = (pos + old_first_line.len() + 20).min(content.len());
                            log::warn!("[str_replace_edit] ║ 找到首行匹配位置: offset={}, 上下文: ...{}...",
                                pos, &content[context_start..context_end].replace('\n', "\\n"));
                        }
                        log::warn!("[str_replace_edit] ╚══════════════ 执行失败 (old_str 不匹配) ══════════════");
                        return serde_json::json!({
                            "error": "old_str 未在文件中找到，请先用 view_file 确认文件内容",
                            "hint": "确保 old_str 与文件中的原文完全一致（包括空格、缩进、换行）"
                        });
                    }

                    let new_content = content.replacen(old_str, new_str, 1);
                    let old_line_count = content.lines().count();
                    let new_line_count = new_content.lines().count();
                    let old_size = content.len();
                    let new_size = new_content.len();

                    log::info!("[str_replace_edit] ║ 替换执行:");
                    log::info!("[str_replace_edit] ║   行数变化: {} → {} ({:+})", old_line_count, new_line_count,
                        new_line_count as i64 - old_line_count as i64);
                    log::info!("[str_replace_edit] ║   字节变化: {} → {} ({:+})", old_size, new_size,
                        new_size as i64 - old_size as i64);

                    // 输出 diff 预览
                    let diff_lines = diff_lines(&content, &new_content);
                    for (i, line) in diff_lines.iter().take(10).enumerate() {
                        log::info!("[str_replace_edit] ║   diff[{}]: {}", i, line);
                    }
                    if diff_lines.len() > 10 {
                        log::info!("[str_replace_edit] ║   ... (共 {} 行差异)", diff_lines.len());
                    }

                    match std::fs::write(full_path, &new_content) {
                        Ok(_) => {
                            log::info!("[str_replace_edit] ║ ✓ 写入成功: {}", path);
                            log::info!("[str_replace_edit] ╚══════════════ 执行完成 (修改) ══════════════");
                            serde_json::json!({
                                "success": true,
                                "action": "modified",
                                "path": path,
                                "old_lines": old_line_count,
                                "new_lines": new_line_count,
                                "old_bytes": old_size,
                                "new_bytes": new_size,
                                "message": format!("已修改文件 {} ({}→{}行, {}→{}字节)",
                                    path, old_line_count, new_line_count, old_size, new_size)
                            })
                        }
                        Err(e) => {
                            log::error!("[str_replace_edit] ║ ✗ 写入文件失败: {}", e);
                            log::error!("[str_replace_edit] ╚══════════════ 执行失败 ══════════════");
                            serde_json::json!({"error": format!("写入文件失败: {}", e)})
                        }
                    }
                }
            }

            "bash_exec" => {
                // ========== 前置日志 ==========
                let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
                let cwd = args.get("cwd").and_then(|v| v.as_str());
                let timeout = args.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30);

                let ts = chrono::Local::now().format("%H:%M:%S%.3f");
                log::info!("[bash_exec] ╔══════════════ 开始执行 @ {} ══════════════", ts);
                log::info!("[bash_exec] ║ 原始命令: {}", command);
                log::info!("[bash_exec] ║ 工作目录: {:?}", cwd.unwrap_or("(默认)"));
                log::info!("[bash_exec] ║ 超时设置: {} 秒", timeout);

                // 命令分词
                let tokens: Vec<&str> = command.split_whitespace().collect();
                log::info!("[bash_exec] ║ 命令分词: {} 个 token → {:?}", tokens.len(), tokens);

                // 安全检查
                log::info!("[bash_exec] ║ --- 安全检查 ---");
                let analysis = cli_executor::analyze_command(command);
                log::info!("[bash_exec] ║ 安全判定: {}", if analysis.safe { "通过 ✓" } else { "拦截 ✗" });
                if !analysis.safe {
                    log::warn!("[bash_exec] ║ 拦截原因: {}", analysis.reason.as_deref().unwrap_or("未知风险"));
                    log::warn!("[bash_exec] ║ 操作类型: {}", analysis.operation_type.as_deref().unwrap_or("未分类"));
                    log::warn!("[bash_exec] ║ 受影响文件: {:?}", analysis.affected_files);
                    log::warn!("[bash_exec] ╚══════════════ 执行被拦截 ══════════════");
                    return serde_json::json!({
                        "status": "blocked",
                        "error": format!("[!] 危险命令已被拦截: {}",
                            analysis.reason.as_deref().unwrap_or("未知风险")),
                        "operation_type": analysis.operation_type,
                        "affected_files": analysis.affected_files,
                        "command": command
                    });
                }

                // 执行
                let exec_start = std::time::Instant::now();
                log::info!("[bash_exec] ║ --- 开始执行 ---");
                let result = cli_executor::execute_command(command, cwd).await;
                let elapsed = exec_start.elapsed();

                // ========== 后置日志 ==========
                log::info!("[bash_exec] ║ --- 执行结果 ---");
                log::info!("[bash_exec] ║ 耗时: {:.2?}", elapsed);
                log::info!("[bash_exec] ║ 退出码: {}", result.exit_code);
                log::info!("[bash_exec] ║ 成功: {}", if result.success { "是 ✓" } else { "否 ✗" });

                let output_len = result.output.len();
                let output_lines = result.output.lines().count();
                log::info!("[bash_exec] ║ 输出: {} 字节, {} 行", output_len, output_lines);

                if !result.output.is_empty() {
                    if output_lines <= 20 {
                        log::info!("[bash_exec] ║ === 完整输出 ===");
                        for line in result.output.lines() {
                            log::info!("[bash_exec] ║   | {}", line);
                        }
                    } else {
                        log::info!("[bash_exec] ║ === 输出预览 (前10行) ===");
                        for (i, line) in result.output.lines().take(10).enumerate() {
                            log::info!("[bash_exec] ║   | [{}] {}", i + 1, line);
                        }
                        log::info!("[bash_exec] ║   ... (共 {} 行)", output_lines);
                        log::info!("[bash_exec] ║ === 输出预览 (后5行) ===");
                        for line in result.output.lines().rev().take(5).collect::<Vec<_>>().iter().rev() {
                            log::info!("[bash_exec] ║   | {}", line);
                        }
                    }
                } else {
                    log::info!("[bash_exec] ║ (无输出)");
                }

                if !result.success {
                    log::error!("[bash_exec] ║ ✗ 命令执行失败 (exit_code={})", result.exit_code);
                }

                log::info!("[bash_exec] ╚══════════════ 执行完成 @ {} ══════════════",
                    chrono::Local::now().format("%H:%M:%S%.3f"));

                serde_json::json!({
                    "success": result.success,
                    "exit_code": result.exit_code,
                    "output": result.output,
                    "output_lines": output_lines,
                    "output_bytes": output_len,
                    "elapsed_ms": elapsed.as_millis(),
                    "command": command
                })
            }

            "web_search" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");

                log::info!("[Agent:web_search] query={}",
                    query);

                // 使用 reqwest 调用 DuckDuckGo Instant Answer API（免费，无需 key）
                let url = format!(
                    "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
                    urlencoding(query)
                );

                match reqwest::get(&url).await {
                    Ok(resp) => {
                        match resp.json::<serde_json::Value>().await {
                            Ok(data) => {
                                let abstract_text = data.get("AbstractText")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let abstract_url = data.get("AbstractURL")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let related: Vec<String> = data.get("RelatedTopics")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|t| t.get("Text").and_then(|v| v.as_str()))
                                            .take(5)
                                            .map(|s| s.to_string())
                                            .collect()
                                    })
                                    .unwrap_or_default();

                                log::info!("[Agent:web_search] 结果: abstract={}chars, related={}条",
                                    abstract_text.len(), related.len());

                                serde_json::json!({
                                    "query": query,
                                    "abstract": abstract_text,
                                    "url": abstract_url,
                                    "related": related,
                                    "source": "DuckDuckGo"
                                })
                            }
                            Err(e) => serde_json::json!({"error": format!("解析搜索结果失败: {}", e)}),
                        }
                    }
                    Err(e) => serde_json::json!({"error": format!("搜索请求失败: {}", e)}),
                }
            }

            "task_complete" => {
                let summary = args.get("summary").and_then(|v| v.as_str()).unwrap_or("任务完成");

                log::info!("[Agent:task_complete] 任务结束, 总结: {} 字符", summary.len());
                log::info!("[Agent:task_complete] 总迭代: {}, 工具调用: {}",
                    iteration_count, progress.completed.len());

                serde_json::json!({
                    "status": "completed",
                    "summary": summary,
                    "iterations": iteration_count,
                    "progress": {
                        "completed": progress.completed,
                        "error_log": progress.error_log
                    }
                })
            }

            // ===== 扩展工具 =====

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

            "search_memory" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let mut memory = ctx.memory.lock().await;
                match memory.search(query, 5).await {
                    Ok(memories) => serde_json::json!({"memories": memories}),
                    Err(e) => serde_json::json!({"error": e.to_string()}),
                }
            }

            "add_memory" => {
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let mut memory = ctx.memory.lock().await;
                match memory.add(content, "chat").await {
                    Ok(id) => serde_json::json!({"memory_id": id}),
                    Err(e) => serde_json::json!({"error": e.to_string()}),
                }
            }

            "find_software" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let memory_path = &ctx.config.memory_path;
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

            // === 桌面控制工具 (cua-driver) ===
            "desktop_screenshot" | "desktop_click" | "desktop_type"
            | "desktop_key" | "desktop_list_windows" | "desktop_focus_window"
            | "desktop_scroll" => {
                desktop_control::execute_desktop_tool(&ctx.desktop, name, &args).await
            }

            // CLI-Anything 工具
            "list_clis" | "search_clis" | "get_cli_info" | "recommend_clis" => {
                let hub = ctx.cli_hub.lock().await;
                match cli_tools::execute_cli_tool(&hub, name, &args).await {
                    Ok(result) => serde_json::json!({"success": true, "result": result}),
                    Err(e) => serde_json::json!({"success": false, "error": e}),
                }
            }

            "install_cli" | "execute_cli" => {
                let mut hub = ctx.cli_hub.lock().await;
                match cli_tools::execute_cli_tool_mut(&mut hub, name, &args).await {
                    Ok(result) => serde_json::json!({"success": true, "result": result}),
                    Err(e) => serde_json::json!({"success": false, "error": e}),
                }
            }

            "sequentialthinking" => {
                let thought = args.get("thought").and_then(|v| v.as_str()).unwrap_or("");
                let thought_number = args.get("thought_number").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                let total_thoughts = args.get("total_thoughts").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                let next_thought_needed = args.get("next_thought_needed").and_then(|v| v.as_bool()).unwrap_or(true);
                let is_revision = args.get("is_revision").and_then(|v| v.as_bool());
                let revises_thought = args.get("revises_thought").and_then(|v| v.as_u64()).map(|v| v as u32);
                let branch_from_thought = args.get("branch_from_thought").and_then(|v| v.as_u64()).map(|v| v as u32);
                let branch_id = args.get("branch_id").and_then(|v| v.as_str()).map(|s| s.to_string());
                let needs_more_thoughts = args.get("needs_more_thoughts").and_then(|v| v.as_bool());

                let total = if thought_number > total_thoughts { thought_number } else { total_thoughts };

                let thought_data = ThoughtData {
                    thought: thought.to_string(),
                    thought_number,
                    total_thoughts: total,
                    next_thought_needed,
                    is_revision,
                    revises_thought,
                    branch_from_thought,
                    branch_id: branch_id.clone(),
                    needs_more_thoughts,
                };

                // 构建分支数据（将在调用方合并到 Agent.branches）
                let branch = if let (Some(branch_from), Some(ref bid)) = (branch_from_thought, branch_id.as_ref()) {
                    Some((bid.to_string(), ThoughtData {
                        thought: format!("[分支源于思考 #{}] {}", branch_from, thought),
                        thought_number,
                        total_thoughts: total,
                        next_thought_needed,
                        is_revision,
                        revises_thought,
                        branch_from_thought,
                        branch_id: Some(bid.to_string()),
                        needs_more_thoughts,
                    }))
                } else {
                    None
                };

                // 构建友好的状态文本
                let mut status_lines = Vec::new();
                status_lines.push(format!("> 思考 #{}/{}", thought_number, total));
                if is_revision.unwrap_or(false) {
                    status_lines.push(format!("> 修订：正在重新考虑思考 #{}", revises_thought.unwrap_or(0)));
                }
                if let Some(bid) = &branch_id {
                    status_lines.push(format!("> 分支 ({}) 源于思考 #{}", bid, branch_from_thought.unwrap_or(0)));
                }
                let summary = if thought.len() > 200 {
                    format!("{}...", &thought[..200])
                } else {
                    thought.to_string()
                };
                let prefix = if is_revision.unwrap_or(false) { "🔄 修订" } else if branch_from_thought.is_some() { "🌿 分支" } else { "💭 思考" };
                status_lines.push(format!("{}: {}", prefix, summary));
                if !next_thought_needed {
                    status_lines.push("> ✓ 思考完成，准备进入执行阶段".to_string());
                }

                log::info!("[Agent:sequentialthinking] 步骤 #{}/{}", thought_number, total);
                log::info!("[Agent:sequentialthinking] 内容: {}", summary);

                let json_result = serde_json::json!({
                    "status": "thinking_step_completed",
                    "thought_number": thought_number,
                    "total_thoughts": total,
                    "next_thought_needed": next_thought_needed,
                    "display": status_lines.join("\n")
                });

                *st_change = Some(SequentialThinkingChange {
                    data: thought_data,
                    branch,
                    display: status_lines.join("\n"),
                });
                return json_result;
            }

            _ => serde_json::json!({"error": format!("未知工具: {}", name)}),
        }
    }

// ==================== 辅助函数 ====================

/// URL 编码
fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
                c.to_string()
            } else {
                format!("%{:02X}", c as u8)
            }
        })
        .collect()
}

/// 列出目录内容（最多 depth 级）
fn list_directory(path: &str, max_depth: u32) -> Result<Vec<String>, String> {
    let mut entries = Vec::new();
    list_dir_recursive(std::path::Path::new(path), "", max_depth, &mut entries)
        .map_err(|e| format!("列出目录失败: {}", e))?;
    Ok(entries)
}

/// 生成简单的行级 diff
fn diff_lines(old: &str, new: &str) -> Vec<String> {
    let mut result = Vec::new();
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    // 简单实现：用最长公共子序列找出变化
    let max_len = old_lines.len().max(new_lines.len());
    for i in 0..max_len {
        let old_line = old_lines.get(i);
        let new_line = new_lines.get(i);
        match (old_line, new_line) {
            (Some(o), Some(n)) if o == n => {}
            (Some(o), Some(n)) => {
                result.push(format!("  - {}", o));
                result.push(format!("  + {}", n));
            }
            (Some(o), None) => {
                result.push(format!("  - {}", o));
            }
            (None, Some(n)) => {
                result.push(format!("  + {}", n));
            }
            (None, None) => {}
        }
    }
    result
}

fn list_dir_recursive(
    dir: &std::path::Path,
    prefix: &str,
    depth: u32,
    entries: &mut Vec<String>,
) -> std::io::Result<()> {
    if depth == 0 {
        return Ok(());
    }
    let mut items: Vec<std::fs::DirEntry> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .collect();
    items.sort_by_key(|e| e.file_name());

    for (i, entry) in items.iter().enumerate() {
        let is_last = i == items.len() - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };

        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();

        if path.is_dir() {
            entries.push(format!("{}{}{}/", prefix, connector, name));
            let new_prefix = format!("{}{}", prefix, child_prefix);
            list_dir_recursive(&path, &new_prefix, depth - 1, entries)?;
        } else {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let size_str = if size > 1024 * 1024 {
                format!(" ({:.1}MB)", size as f64 / (1024.0 * 1024.0))
            } else if size > 1024 {
                format!(" ({}KB)", size / 1024)
            } else {
                String::new()
            };
            entries.push(format!("{}{}{}{}", prefix, connector, name, size_str));
        }
    }
    Ok(())
}