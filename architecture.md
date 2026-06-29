# 江秋月 — 架构设计文档

> 最后更新: 2026-06-30 | 版本: 2.1

---

## 1. 系统总览

```
┌────────────────────────────────────────────────────────────┐
│                     Electron 前端                           │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐ │
│  │ main_window  │  │ float_window │  │ 设置向导/模型选择 │ │
│  │  (对话界面)   │  │  (Live2D)   │  │  (首次启动弹出)   │ │
│  └──────┬───────┘  └──────────────┘  └────────┬─────────┘ │
│         │          IPC / HTTP                  │           │
└─────────┼──────────────────────────────────────┼───────────┘
          │                  ↑                   │
          ▼                  │                   ▼
┌─────────────────────────────────────────────────────────────┐
│              backend-server (Axum HTTP)                      │
│  监听 127.0.0.1:8000                                        │
│                                                             │
│  GET  /health          ← 健康检查                            │
│  GET  /config          ← 获取 LLM 配置                       │
│  PUT  /config          ← 更新 LLM 配置 (api_key/base_url/    │
│                          model/provider)                     │
│  POST /chat            ← 发送任务，返回 AgentResult           │
│  GET  /ws              ← WebSocket 日志流 (DEFINE→SHIP)   │
│  POST /file/read       ← 文件操作代理 (路由到 FileTools)  │
│  POST /memory/*        ← 记忆操作代理                         │
│  POST /cli/*           ← CLI-Hub 操作代理                     │
│  POST /docs/*          ← 文档阅读代理                         │
│  GET  /software/*      ← 软件扫描代理                         │
└───────────┬─────────────────────────────────────────────────┘
            │
            ▼
┌─────────────────────────────────────────────────────────────┐
│              backend-core (核心引擎)                          │
│                                                             │
│  ┌───────────────────────────────────────────────────────┐ │
│  │                    Agent (主循环)                      │ │
│  │  run(user_input) → loop { LLM调用 → 工具执行 → 记录 }  │ │
│  │  迭代上限 30 轮 | 工具上限 web_search×5 / file_edit×10 │ │
│  └───┬───────┬───────┬────────┬────────┬────────────────┘ │
│      │       │       │        │        │                   │
│      ▼       ▼       ▼        ▼        ▼                   │
│  ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐ ┌──────────────┐    │
│  │ LLM  │ │重试  │ │轨迹  │ │工具  │ │  安全/沙箱    │    │
│  │Provider│ │retry │ │traj. │ │执行  │ │  exec+sandbox │    │
│  └──────┘ └──────┘ └──────┘ └──────┘ └──────────────┘    │
│                                                             │
│  辅助模块: Config / Memory / FileTools / Screen / DocReader / DesktopControl  │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. 核心组件详解

### 2.1 Agent 主循环（agent.rs）

```
run(user_input, image_base64?)
    │
    ├─ 1. 重置状态 (messages=system_prompt, iteration=0, progress 清空)
    ├─ 2. 轨迹录制: recorder.start()
    ├─ 3. 构建 user_message (纯文本 / 含 base64 图片)
    │
    └─ 4. 主循环 (loop)
        │
        ├─ 检查 iteration_count > 30? → 返回超限总结
        │
        ├─ 转换 self.messages → LLMMessage[] (provider-agnostic)
        │
        ├─ LLM 调用: provider.chat(messages, &tools)
        │   │
        │   │  ┌─ LLMProvider::chat() ──────────────────────┐
        │   │  │  match provider {                          │
        │   │  │    DeepSeek(c) → retry → c.chat_inner()    │
        │   │  │    OpenAI(c)   → retry → c.chat_inner()    │
        │   │  │  }                                         │
        │   │  └────────────────────────────────────────────┘
        │   │
        │   ├─ 轨迹录制: recorder.record_llm_call()
        │   │
        │   ▼
        │   tool_calls = 响应中的工具调用列表
        │   │
        │   ├─ 空? → 返回文本内容 (finalize 轨迹)
        │   │
        │   ├─ 记录 assistant 消息到 self.messages
        │   │
        │   └─ 工具执行 (三步流水线 — 并行化版)
            │
            ├─ Step 1: 前置检查 (串行，修改共享计数器)
            │   ├─ web_search: 次数超限? → 注入错误消息，跳过
            │   └─ str_replace_edit: 单文件超限? → 注入错误消息，跳过
            │
            ├─ Step 2: 并行执行
            │   ├─ 通过检查的工具统一通过 tokio::join_all 派发
            │   ├─ ToolContext 封装共享资源（file_tools, memory, cli_hub, config）
            │   ├─ execute_tool_parallel 为独立函数，无需 &mut Agent
            │   ├─ sequentialthinking 通过输出参数 SequentialThinkingChange 返回状态
            │   └─ 所有工具结果在 join_all 完成后按原始顺序收集
            │
            └─ Step 3: 顺序记录 (保持确定性)
                ├─ 轨迹: recorder.record_tool_call().await（异步 I/O）
                ├─ 进度: progress.completed / error_log
                ├─ 状态合并: thought_history / branches (来自 SequentialThinkingChange)
                └─ 消息: self.messages.push(ChatMessage::tool(...))
        │
        └─ 循环继续 (除非 should_stop)
```

**关键设计决策**：

| 决策 | 理由 |
|------|------|
| 单轮单次 LLM 调用 | 保证上下文连贯，工具结果即时回传 |
| 三步流水线（并行化 Step 2） | Step 1 串行（修改共享计数器），Step 2 通过 join_all 并行执行，Step 3 串行记录（保持确定性顺序） |
| `self.messages` 每次任务重置 | 防止跨任务上下文污染，仅保留 system prompt |
| ChatMessage 用 `serde_json::Value` 存 content | 兼容纯文本和 Vision API 的数组格式 |
| ToolContext 封装共享资源 | 使工具函数无需 `&mut Agent`，安全上线并行执行 |

---

### 2.2 LLM Provider 抽象层（llm/）

```
                    ┌─────────────────────┐
                    │    LLMProvider      │  枚举多态（无 trait object 分配）
                    │  (provider.rs)       │
                    │                     │
                    │  DeepSeek(client)    │
                    │  OpenAI(client)      │  ← 共用 OpenAICompatClient
                    │  Ollama(client) ← 扩展点 │
                    └─────────┬───────────┘
                              │ chat(messages, tools)
                              ▼
                    ┌─────────────────────┐
                    │   retry_with_backoff │  retry.rs
                    │   (3次, 3-30s退避)   │
                    └─────────┬───────────┘
                              ▼
                    ┌─────────────────────┐
                    │  client.chat_inner()│  各 Client 内部实现
                    │  POST /chat/completions
                    │  Header: Bearer {key}
                    └─────────────────────┘
```

**类型流转**：

```
Config (from_env / runtime_config.json)
  └→ LLMConfig { provider, api_key, base_url, model, temperature }
       └→ to_provider_config()
            └→ ProviderConfig { kind: ProviderKind, ... }
                 └→ LLMProvider::from_config()
                      └→ LLMProvider 枚举实例
```

**扩展新 Provider 的步骤**：

1. `ProviderKind` 加变体 + `from_str()` / `default_base_url()` / `default_model()` / `api_name()` match 分支
2. `LLMProvider` 加变体 + `from_config()` / `chat()` / `name()` / `model()` match 分支
3. 实现 `XxxClient` struct（参考 DeepSeekClient，只需改 `api_name` 和日志前缀）
4. 如协议非 OpenAI-compatible（如 Anthropic），需独立实现 `chat_inner()`

**目前支持的 Provider**：

| Provider | 协议 | 认证 | 图片支持 |
|----------|------|------|---------|
| DeepSeek | OpenAI Chat API | `Bearer {key}` | Vision API 格式 |
| OpenAI | OpenAI Chat API | `Bearer {key}` | Vision API 格式 |
| (扩展点) Ollama | OpenAI Chat API | 无/`Bearer` | 取决于模型 |
| (扩展点) Anthropic | Anthropic Messages API | `x-api-key` | 需独立适配 |

---

### 2.3 重试机制（retry.rs）

```
retry_with_backoff(name, max_retries, closure)
    │
    └─ for attempt in 0..=max_retries:
        │
        ├─ attempt > 0? → 随机等待 3~30s
        │
        ├─ 调用闭包
        │   ├─ Ok → 立即返回
        │   └─ Err(e) →
        │       ├─ is_retryable(e)?
        │       │   ├─ 网络错误 (connection/timeout/reset/refused/dns) → 重试
        │       │   ├─ HTTP 429 (rate limit) → 重试
        │       │   ├─ HTTP 5xx → 重试
        │       │   └─ HTTP 4xx (非429) / JSON解析错误 → 立刻返回错误
        │       └─ 是最后一次尝试? → 返回汇总错误
```

---

### 2.4 工具执行与安全（cli_executor.rs）

```
bash_exec(command, cwd, timeout)
    │
    ├─ 1. 危险模式匹配 (DANGEROUS_PATTERNS: 45+ regex)
    │   └─ 命中? → 拦截，记录危险命令列表
    │
    ├─ 2. 命令黑名单 (BLOCKED_COMMANDS: 60+ 命令名)
    │   └─ 命中? → 拦截
    │
    ├─ 3. 构建 CLIRequest { command, cwd, safe, reason, ... }
    │
    ├─ 4. Docker 沙箱 (docker_sandbox.rs)
    │   ├─ Docker 可用? → docker run --rm --read-only --network=none
    │   └─ 不可用? → 降级本地执行
    │
    └─ 5. 执行 → CLIResult { success, exit_code, output }
```

**安全层次**：

| 层 | 机制 | 说明 |
|----|------|------|
| L1 | 正则模式匹配 | 45+ 危险模式（`rm -rf`, `sudo`, `format`, `diskpart` 等） |
| L2 | 命令黑名单 | 60+ 高风险命令名直接拒绝 |
| L3 | Docker 沙箱 | 容器隔离（只读 rootfs + 无网络） |
| L4 | 超时控制 | 默认 60s，可配置 |

---

### 2.5 桌面控制（desktop_control.rs）

集成 [cua-driver](https://github.com/trycua/cua) 提供后台桌面操控能力（截图 / 鼠标 / 键盘 / 窗口管理），不抢用户焦点。

```
CuaDriverClient (状态机 + 惰性启动)
    │
    ├─ 状态: NotInstalled → InstalledNotStarted → Starting → Ready / Failed
    │
    ├─ ensure_started()  ← 每次工具调用前的入口
    │   ├─ Ready?         → 直接返回 Ok
    │   ├─ NotInstalled?  → find_binary_path() 热检测
    │   │                   ├─ 找到二进制 → InstalledNotStarted (热检测生效)
    │   │                   └─ 未找到     → 返回安装提示
    │   ├─ Starting?      → 复用现有 child + stdin/stdout
    │   └─ Starting(初次) → spawn("cua-driver mcp")
    │                        ├─ 建立 stdin / stdout_reader 句柄
    │                        ├─ send_request("initialize")
    │                        └─ send_request("tools/list")
    │
    └─ call_tool(name, args)
        ├─ JSON-RPC over stdio (一行请求 + 一行响应)
        ├─ 工具名兼容回退 (mouse_click → click 等)
        └─ 返回 DesktopResult { success, message, data }
```

**热检测机制**：服务运行期间用户安装 cua-driver 后无需重启，下次调用 `ensure_started` 会重新扫描 PATH 并自动启动。

**7 个工具**：`desktop_screenshot` / `desktop_click` / `desktop_type` / `desktop_key` / `desktop_list_windows` / `desktop_focus_window` / `desktop_scroll`

---

### 2.6 轨迹录制（trajectory/recorder.rs）

```
TrajectoryRecorder
    │
    ├─ new(dir) → 创建 JSONL 文件: trajectory_<date>_<session_id>.jsonl
    │
    ├─ start(user_input, provider, model)
    │   └─ 写入: { type: "session_start", timestamp, user_input, provider, model }
    │
    ├─ record_llm_call(iteration, messages, content, tool_calls, usage, error)
    │   └─ 写入: { type: "llm_call", iteration, messages_count, tool_calls, usage }
    │
    ├─ record_tool_call(iteration, tool_name, arguments, result)
    │   └─ 写入: { type: "tool_call", iteration, tool_name, arguments, result }
    │
    └─ finalize(success, summary?)
        └─ 写入: { type: "session_end", success, summary, timestamp }
```

**特点**：增量写入（每条 `write_all` + `flush`），进程崩溃不丢失。

---

### 2.7 配置层级（config.rs）

```
环境变量 (LLM_* / DEEPSEEK_*)
    │
    ├─ .env 文件 (DEEPSEEK_API_KEY / LLM_API_KEY)
    │
    └─ runtime_config.json (内存路径下，由前端 /config API 写入)
         │
         ▼
    config.apply_runtime_config() 合并
         │
         ▼
    LLMConfig { provider, api_key, base_url, model, temperature }
         │
         ▼
    to_provider_config() → LLMProvider 实例
```

**加载优先级**：

```
runtime_config.json  >  .env 文件  >  系统环境变量  >  LLMConfig::default()
(仅覆盖非空字段)      (fallback)     (最优先源)       (兜底 DeepSeek)
```

---

### 2.8 消息格式转换流程

```
Agent 内部 (ChatMessage)           LLM Provider (LLMMessage)         HTTP (JSON)
─────────────────────────          ─────────────────────────         ──────────
role: "system"          ──map──→  role: "system"             ──→  {"role":"system"}
content: Value::String  ──map──→  content: Some("text")      ──→  {"content":"text"}
content: Value::Array   ──map──→  content: Some("[{...}]")   ──→  {"content":[...]}
tool_calls: Vec<ToolCall>──map──→ tool_calls: Vec<LLMToolCall>──→  {"tool_calls":[...]}

关键: ChatMessage.content 是 serde_json::Value（为兼容 Vision 数组格式）
     LLMMessage.content 是 Option<String>（Provider 协议层统一为字符串）
```

---

## 3. 请求生命周期（端到端）

```
用户输入 "修复 app.js 的 bug"
    │
    │  Electron 前端 POST /chat
    ▼
┌─────────────────────────────────────────────────┐
│ main.rs: chat_handler()                         │
│  ├─ 请求追踪: req_id = timestamp (毫秒)          │
│  ├─ 可选: capture_screen() 获取截图 base64       │
│  ├─ 创建日志广播通道: tokio::sync::mpsc(200)     │
│  ├─ 后台任务: log_rx → WebSocket 广播            │
│  └─ agent.run(user_input, image_base64?).await  │
└─────────────────────┬───────────────────────────┘
                      ▼
┌─────────────────────────────────────────────────┐
│ Agent::run()                                     │
│  ├─ 重置状态                                     │
│  ├─ 构建 user_message                            │
│  └─ 主循环 ▼                                    │
│                                                  │
│  迭代 #1:                                        │
│    LLMProvider::chat(messages, tools)             │
│      └─ retry → POST DeepSeek/OpenAI API         │
│         返回: tool_calls=[view_file("app.js")]    │
│                                                  │
│    工具执行: view_file("app.js")                  │
│      └─ file_tools.read_file() → 文件内容         │
│    记录到 messages                                │
│    轨迹落盘                                      │
│                                                  │
│  迭代 #2:                                        │
│    LLMProvider::chat(messages, tools)             │
│      ← messages 现在包含上一轮的工具结果           │
│      →返回: tool_calls=[str_replace_edit(...)]    │
│                                                  │
│    工具执行: str_replace_edit("app.js", ...)      │
│      └─ file_tools.str_replace_edit() → 替换成功   │
│    记录到 messages                                │
│                                                  │
│  迭代 #3:                                        │
│    LLM::chat(messages, tools)                     │
│      →返回: tool_calls=[bash_exec("node app.js")] │
│                                                  │
│    bash_exec 执行 → 安全检测 → 运行 → 输出        │
│    记录到 messages                                │
│                                                  │
│  迭代 #4:                                        │
│    LLM::chat(messages, tools)                     │
│      →返回: tool_calls=[task_complete("修复完成")] │
│                                                  │
│    task_complete → should_stop = true             │
│    轨迹 finalize                                  │
│    return AgentResult { reply, tool_calls, ... }  │
└─────────────────────┬───────────────────────────┘
                      ▼
┌─────────────────────────────────────────────────┐
│ main.rs: 返回 JSON                               │
│  {                                               │
│    "reply": "修复完成 - app.js 中的 bug 已解决",   │
│    "tool_calls": ["view_file","str_replace_edit", │
│                    "bash_exec","task_complete"],   │
│    "iterations": 4,                               │
│    "progress": { ... }                            │
│  }                                               │
└─────────────────────────────────────────────────┘
```

---

## 4. 模块依赖图

```
backend-server (main.rs)
    │
    ├── backend-core::agent::Agent           ← 核心
    │   ├── backend-core::llm::provider      ← LLM 调用
    │   │   ├── backend-core::llm::types     ← 通用类型
    │   │   └── backend-core::retry          ← 重试
    │   ├── backend-core::cli_executor        ← 命令执行
    │   │   └── backend-core::docker_sandbox  ← 沙箱隔离
    │   ├── backend-core::file_tools         ← 文件操作
    │   ├── backend-core::memory             ← 向量记忆
    │   ├── backend-core::cli_hub            ← CLI 工具注册
    │   │   ├── backend-core::cli_tools
    │   │   ├── backend-core::cli_generator
    │   │   └── backend-core::cli_guide
    │   ├── backend-core::screen             ← 屏幕截图
    │   ├── backend-core::trajectory          ← 轨迹录制
    │   └── backend-core::config             ← 配置管理
    │       └── backend-core::llm::provider   ← ProviderKind 解析
    │
    ├── backend-core::software_scanner       ← 独立后台任务
    └── backend-core::doc_reader             ← 独立路由
```

---

## 5. 关键数据流

### 5.1 Provider 切换数据流

```
前端设置页面
  │ 用户选择 OpenAI + 输入 API Key
  │ PUT /config { provider: "openai", api_key: "sk-...", base_url: "...", model: "gpt-4o" }
  ▼
main.rs: config_update_handler()
  ├─ 更新 config.llm 字段
  ├─ config.save_to_file() → runtime_config.json
  ├─ LLMProvider::from_config() → 创建新 OpenAI provider
  └─ agent.update_config(new_config, new_provider)
       └─ agent.provider = LLMProvider::OpenAI(OpenAIClient)
```

### 5.2 消息历史积累

```
第 1 轮 LLM 调用时 messages:
  [system, user("修复 app.js 的 bug")]

第 2 轮 LLM 调用时 messages (经过 1 轮工具执行):
  [system, user("修复 app.js 的 bug"),
   assistant(tool_calls=[view_file("app.js")]),
   tool(tool_call_id="1", content="...文件内容...")]

第 N 轮:
  [system, user, assistant/工具结果对交替...]
```

---

## 6. 扩展指南

### 添加新工具

1. 在 `Agent::tools_definition()` 中追加 `ToolDefinition`
2. 在工具执行 switch 中添加新的 `tool_name` 分支
3. 实现工具逻辑（可直接在 agent.rs 或新建模块）

### 添加新 Provider

见 [§2.2 LLM Provider 抽象层](#22-llm-provider-抽象层) 中的扩展步骤。

### 添加新 HTTP 路由

1. 在 `main.rs` 中定义 `#[derive(Deserialize)] struct XxxRequest` 和 `#[derive(Serialize)] struct XxxResponse`
2. 实现 `async fn xxx_handler(State(state): State<AppState>, ...) -> Json<XxxResponse>`
3. 在 `Router::new()` 中 `.route("/xxx", post(xxx_handler))`

---

## 7. 系统提示词架构（agent-skills 集成）

系统提示词基于 [addyosmani/agent-skills](https://github.com/addyosmani/agent-skills) 工程方法论设计，遵循六阶段软件工程生命周期：

```
DEFINE         PLAN          BUILD         VERIFY         REVIEW          SHIP
┌──────┐      ┌──────┐      ┌──────┐      ┌──────┐      ┌──────┐      ┌──────┐
│ 需求 │ ──▶  │ 计划 │ ──▶  │ 实现 │ ──▶  │ 验证 │ ──▶  │ 审查 │ ──▶  │ 交付 │
│ 规格 │      │ 拆解 │      │ 增量 │      │ 排错 │      │ 五轴 │      │ 门禁 │
└──────┘      └──────┘      └──────┘      └──────┘      └──────┘      └──────┘
```

### 核心原则来源

| 原则 | 来源 Skill | 在系统提示词中的体现 |
|------|-----------|-------------------|
| 假设前置 | `using-agent-skills` | DEFINE 阶段：列出 ASSUMPTIONS 让用户纠正 |
| 垂直切片 | `planning-and-task-breakdown` | PLAN 阶段：按功能路径拆分，非按层拆分 |
| 增量实现 | `incremental-implementation` | BUILD 阶段：Implement→Test→Verify→Commit 循环 |
| Stop the Line | `debugging-and-error-recovery` | VERIFY 阶段：遇错即停，PRESERVE→DIAGNOSE→FIX→GUARD→RESUME |
| 五轴审查 | `code-review-and-quality` | REVIEW 阶段：正确性/可读性/架构/安全/性能 |
| Definition of Done | `references/definition-of-done.md` | SHIP 阶段：7 项质量门禁 checklist |
| 安全加固 | `security-and-hardening` | REVIEW 安全轴：输入验证、参数化查询、输出编码 |
| 代码简化 | `code-simplification` | 行为准则：越简单越好，不到第三次不复用抽象 |

### agent-skills 项目结构

```
agent-skills/
├── skills/           ← 26 个工程技能 (SKILL.md per directory)
│   ├── spec-driven-development/     ← 规格驱动开发
│   ├── planning-and-task-breakdown/ ← 任务拆解
│   ├── incremental-implementation/  ← 增量实现
│   ├── test-driven-development/     ← 测试驱动
│   ├── code-review-and-quality/     ← 代码审查
│   ├── code-simplification/         ← 代码简化
│   ├── debugging-and-error-recovery/← 调试排错
│   ├── security-and-hardening/      ← 安全加固
│   ├── context-engineering/         ← 上下文工程
│   ├── shipping-and-launch/         ← 发布上线
│   └── ...                          ← 更多专项技能
├── agents/           ← 可复用角色 (code-reviewer, security-auditor, test-engineer)
├── references/       ← 参考清单 (testing, security, performance, accessibility, DoD)
└── docs/             ← 多工具集成指南
```
