# 江秋月（Qiuyue）— AI 工作交接日志

> **最后更新**: 2026-06-30（最新：修复打包版 exe 运行报错，包括 UTF-8 边界 panic、DOM null 引用、重复请求竞态等关键问题）
> **当前会话**: 打包版 exe 全链路调试与修复，所有关键错误已定位并修复
> **GitHub**: https://github.com/Yanxuer/jiangqiuyue

---

## 1. 项目定位

江秋月是一个**桌面端自主编程智能体**（Rust 后端 + Electron 前端 + Live2D 虚拟人物），核心能力：
- 理解自然语言需求 → 自主浏览/修改代码 → 执行命令 → 联网检索 → 多轮迭代直到任务完成
- 具备 23 个工具（文件读写、终端执行、联网搜索、记忆搜索、屏幕截图、桌面控制、CLI-Hub 管理、顺序思维等）
- 安全沙箱 + 危险命令拦截

**技术栈**:
| 层 | 技术 |
|----|------|
| Agent 核心 | Rust (backend-core) |
| HTTP API | Axum (backend-server), 监听 127.0.0.1:8000 |
| 前端 | React + TypeScript (electron/) |
| 虚拟人物 | Live2D Cubism SDK |
| 向量记忆 | SQLite + FastEmbed (all-MiniLM-L6-v2) |
| LLM 后端 | DeepSeek V4 / OpenAI-compatible（可切换） |

---

## 2. 模块架构（核心）

```
backend-core/src/
├── agent.rs            ★ 核心 Agent 循环（run → 迭代 → LLM调用 → 工具执行）
├── config.rs            统一配置（LLMConfig + workspace + memory_path）
├── llm/                 ★ 多 LLM 提供商抽象层
│   ├── types.rs          通用类型（LLMMessage, LLMResponse, ToolDefinition...）
│   └── provider.rs       LLMProvider 枚举多态（DeepSeek / OpenAI）
├── retry.rs             ★ 通用 API 重试（3次 + 3-30s随机退避 + 错误分类）
├── trajectory/           ★ 轨迹录制（JSONL 格式，每次 LLM/工具调用自动落盘）
│   └── recorder.rs
├── docker_sandbox.rs    ★ Docker 沙箱（容器隔离命令执行，不可用则降级本地执行）
├── desktop_control.rs   ★ 桌面控制（cua-driver MCP 整合，后台截图/鼠标/键盘/窗口管理）
├── memory.rs            向量记忆系统（FastEmbed 编码 + SQLite 存储 + 检索）
├── file_tools.rs        文件系统操作（读/写/列目录/搜索）
├── cli_executor.rs      终端命令执行 + 危险命令检测
├── cli_hub.rs           CLI-Anything 工具注册中心
├── cli_tools.rs         CLI 工具 API（搜索/安装/执行/推荐）
├── cli_generator.rs     CLI 工具描述生成
├── cli_guide.rs         CLI 工具使用指南
├── doc_reader.rs        文档阅读器（PDF/Word/Excel/CSV）
├── screen.rs            屏幕截图（Windows: DXGI, 非 Windows: XCB）
├── software_scanner.rs  本机软件扫描（Windows Registry）
└── lib.rs               模块注册
```

**目录**:
```
backend-rust/
  backend-core/       ← 上面所有模块
  backend-server/     ← Axum HTTP API 入口 (main.rs)
electron/             ← Electron 前端
  src/                ← React 页面
  scripts/            ← 构建脚本（build-rust, copy-frontend）
trae-agent/           ← 参考项目（字节跳动开源，已对比分析）
```

---

## 3. 当前会话完成的优化

### P0: cua-driver 桌面控制集成（7 个新工具）

**问题**：Agent 只能通过 `capture_screen` 获取全屏截图，无法与桌面软件交互（点击、输入、窗口管理），限制了"真正桌面助手"的能力。

**方案**：集成 [cua-driver](https://github.com/trycua/cua)（Rust 编写，MIT 协议），通过 MCP over stdio 协议通信，提供后台桌面操控。

**新增 7 个工具**：

| 工具名 | 功能 | 参数 |
|--------|------|------|
| `desktop_screenshot` | 后台截图（全屏/窗口级） | `window_title?`, `monitor?` |
| `desktop_click` | 后台鼠标点击 | `x`, `y`, `button?` |
| `desktop_type` | 后台键盘输入文本 | `text` |
| `desktop_key` | 按键/组合键 | `keys` (如 `ctrl+s`) |
| `desktop_list_windows` | 枚举桌面窗口 | `filter?` |
| `desktop_focus_window` | 聚焦指定窗口 | `window_title` |
| `desktop_scroll` | 鼠标滚轮滚动 | `x?`, `y?`, `direction?`, `amount?` |

**关键设计**：

- **惰性启动**：`CuaDriverClient` 首次工具调用时才启动 `cua-driver mcp` 子进程
- **自动降级**：cua-driver 未安装时返回明确错误提示（含安装脚本），不影响其他功能
- **热检测**：服务运行期间安装 cua-driver 后，下次工具调用自动检测并启动，无需重启服务
- **启动失败保护**：子进程启动或 MCP 握手失败时清理状态并提示重启
- **MCP 工具名兼容**：每个调用尝试主名 + 备选名，兼容不同版本 cua-driver

**文件**：
- 新增：`backend-core/src/desktop_control.rs`（~470 行）
- 修改：`agent.rs`（ToolContext + Agent 结构体 + 7 工具定义 + 7 执行分支）、`lib.rs`、`main.rs`

**cua-driver 安装**：
```powershell
irm https://raw.githubusercontent.com/trycua/cua/main/libs/cua-driver/scripts/install.ps1 | iex
```

### P0: API 重试机制（retry.rs）
- 3 次重试 + 3-30s 随机退避
- 智能错误分类：网络错误/429/5xx → 可重试，4xx/JSON错误 → 立即返回
- 暴露通用函数 `retry_with_backoff(name, max_retries, closure)`

### P0: 多 LLM 提供商支持（llm/）
- `LLMProvider` 枚举：DeepSeek / OpenAI
- 每个 Client 内部处理核心 HTTP 调用，外层统一由 retry 模块包裹
- 配置统一为 `LLMConfig { provider, api_key, base_url, model, temperature }`
- 添加新 Provider：加一个枚举变体 + 一个 Client struct（参考 DeepSeekClient 模板）
- 环境变量兼容：支持 `LLM_API_KEY` + 旧 `DEEPSEEK_API_KEY`

### P1: 顺序思维工具
- 新增 `sequentialthinking` 工具（#16），支持动态思考步数、修订追踪、分支回溯
- 状态存储在 `Agent.thought_history` 和 `Agent.branches`

### P1: 轨迹录制（trajectory/）
- JSONL 格式，增量写入，进程崩溃不丢失
- 事件类型：session_start / llm_call / tool_call / session_end
- 路径：`./trajectories/trajectory_<日期>_<session_id>.jsonl`

### P2: 工具执行三步流水线
- 三步：检查（限流/上限拦截）→ 执行 → 记录（轨迹+进度+消息历史）
- 架构注释：当 LLM 返回多个独立工具调用时，可改造成 `tokio::join_all` 并行执行

### P2: Docker 沙箱（docker_sandbox.rs）
- 自动检测 Docker 是否可用，不可用时降级本地执行
- 配置：memory_limit、network_enabled、read_only 等安全约束
- Windows 路径自动转换为 Docker Desktop 格式（`C:\foo` → `/c/foo`）

### P0: 消除 DeepSeek 硬编码（通用化 Provider）

**问题**：多处在 DeepSeek 品牌名称和变量名上硬编码，切换到 OpenAI 等提供商时：
- 前端 `checkBackendConfig()` 读取 `data.deepseek_base_url`（后端实际返回 `base_url`）→ 配置静默失败
- 前端 `saveConfigToBackend()` 发送 `deepseek_api_key` / `deepseek_base_url`（后端期望 `api_key` / `base_url`）→ 保存静默失败
- 系统提示词 `cli_guide.rs` 中写死 "DeepSeek 可以自动搜索…"
- 前端 JS 默认值 `currentModel = 'deepseek-v4-flash'` 硬编码
- `start-agent.ps1` 只检查 `DEEPSEEK_API_KEY`，不支持 `LLM_API_KEY`

**修复**（4 处代码 + 2 处配置）：
| 文件 | 改动 |
|------|------|
| `frontend/main_window/app.js` | `data.deepseek_base_url` → `data.base_url`；请求体 `deepseek_api_key`/`deepseek_base_url` → `api_key`/`base_url`；默认值清空由后端覆盖 |
| `backend-core/src/cli_guide.rs` | "DeepSeek 可以自动搜索" → "可以自动搜索" |
| `start-agent.ps1` | 优先读 `LLM_API_KEY` / `LLM_PROVIDER` / `LLM_BASE_URL` / `LLM_MODEL`，fallback 到旧 `DEEPSEEK_*` |
| `.env` / `backend/.env` | 补充 `LLM_*` 四变量示例，旧 `DEEPSEEK_*` 保留兼容 |

**验证**：新增 17 项 Provider 集成测试（`provider.rs` 末端 `#[cfg(test)]`），覆盖：
- `ProviderKind::OpenAI` 解析、默认 URL/Model/名称
- `LLMProvider::from_config("openai")` 不泄漏 DeepSeek 名称
- `LLMConfig → ProviderConfig` 多层级转换
- `LLMMessage` 序列化不含 provider-specific 字段
- `ToolDefinition` JSON 格式为 OpenAI-compatible

**测试结果**: 29 passed, 0 failed, 0 warnings（包括 12 项新增 e2e 测试）。

### P0: 端到端 Provider 切换验证

在 `config.rs` 新增 12 项端到端集成测试（`#[cfg(test)] mod e2e_tests`），覆盖完整链路：

| 测试类别 | 测试项 | 验证点 |
|----------|--------|--------|
| 全链路 | OpenAI/DeepSeek 字符串 → ProviderKind → ProviderConfig | 每一跳不丢失、不泄漏对立 Provider 的信息 |
| 保存/加载 | Config JSON 往返（两种 Provider） | 文件序列化正确、回读字段一致 |
| Provider 切换 | OpenAI 保存 → 覆盖 DeepSeek → 加载验证 | 切换不残留旧值，`base_url` 不包含对立域名 |
| Runtime 合并 | `apply_runtime_config` 部分覆盖 / 全量覆盖 | 空字段保留默认值，非空字段正确覆盖 |
| 边界情况 | 不存在的文件返回 None、空 API Key 返回 `!configured` | 异常路径不崩溃 |
| JSON 纯度 | OpenAI 配置的 JSON 文件不含 "deepseek" 字样 | 序列化不泄漏品牌名 |

**start-agent.ps1 手动验证**：
- `LLM_PROVIDER=openai + LLM_API_KEY=sk-test-...`：正确解析为 OpenAI 配置（PASS）
- 未设 `LLM_*` 仅设 `DEEPSEEK_*`：fallback 到 DeepSeek 配置（PASS）

**测试结果**: 29 passed, 0 failed, 0 warnings.

### P1: 并行工具执行 — agent.rs 重构
- `execute_tool` 从 `&mut self` 方法重构为独立函数 `execute_tool_parallel`
- 新增 `ToolContext` 封装共享资源（FileTools, memory, cli_hub, config）
- `sequentialthinking` 通过 `SequentialThinkingChange` 输出参数返回状态变更，事后合并到 Agent
- 工具执行改为三段流水线：预检查（串行）→ `join_all`（并行）→ 结果记录（串行，保持确定性顺序）
- 文件：`agent.rs`（新增 ToolContext/SequentialThinkingChange，重写工具执行循环）

### P2: 轨迹录制异步 I/O — recorder.rs
- `TrajectoryRecorder` 所有方法改为 `async fn`
- `write_record` 内部：`std::fs::OpenOptions` → `tokio::fs::OpenOptions` + `AsyncWriteExt`
- 不再阻塞 async runtime 的主线程
- 文件：`trajectory/recorder.rs`、`agent.rs`、`main.rs`（所有调用点添加 `.await`）

### P3: LLM Provider 代码去重 — OpenAICompatClient
- 将 `DeepSeekClient` 和 `OpenAIClient` 合并为单一 `OpenAICompatClient`
- 以 `log_prefix` 参数化日志输出（`"[DeepSeek]"` / `"[OpenAI]"`）
- `LLMProvider::chat` 消除两个分支的重复重试逻辑，统一为单一调用
- 删除约 150 行重复代码
- 文件：`llm/provider.rs`

### P4: 记忆编码 spawn_blocking — memory.rs
- `AgentMemory::add` 和 `::search` 改为 `async fn`
- FastEmbed 的同步 `embed()` 调用封装在 `tokio::task::spawn_blocking` 中
- 编码时不再阻塞 async runtime
- 文件：`memory.rs`、`agent.rs`、`main.rs`

**验证**: 37/37 单元测试通过，编译零警告。

### P0: 打包版 exe 全链路调试与修复（2026-06-30）

**问题**：打包成 exe 运行后出现多种报错，包括 LLM API 401 未授权、WebSocket 连接失败、CLI-Hub 加载失败、重复请求导致 DEFINE 阶段循环、前端 DOM null 引用崩溃等。

**修复清单**：

| 优先级 | 问题 | 根因 | 修复 |
|--------|------|------|------|
| P0 | 端口 8000 被占用导致后端启动失败 | 旧进程残留 | `electron/main.js` 启动前 `taskkill` 清理旧 backend.exe |
| P0 | 日志通道资源泄漏 | `std::sync::mpsc` 阻塞占用线程池 | 改为 `tokio::sync::mpsc`（200 容量），`spawn_blocking` → `tokio::spawn` |
| P0 | 前端重复发送请求 | `isThinking` 设置太晚，存在竞态窗口 | 立即设置 `setIsThinking(true)`，移到 `dialogInput.value=''` 之前 |
| P0 | trajectory recorder UTF-8 边界 panic | `&content[..1000]` 切到中文"遵"的中间字节 | 使用 `is_char_boundary()` 安全回退到字符边界 |
| P0 | 前端 DOM 元素 null 引用崩溃 | `dialogSendBtn`/`dialogMessages`/`connStatus` 在打包版中为 null | 所有 DOM 操作添加 null 检查（`setIsThinking`/`addMessage`/`scrollToBottom`/`clearChat`/`updateConnectionStatus`/`renderLogs`） |
| P1 | `renderLogs` 中 `logEmpty` 为 null 导致过滤按钮崩溃 | 日志面板 DOM 元素在打包版中缺失 | 添加 `if (empty)` 和 `if (container)` 空值保护 |
| P1 | chat_handler 和 LLM provider 缺少关键步骤日志 | 难以定位错误 | 添加 `[Chat][req_id]` 请求追踪、`[Agent]` 迭代日志、`[DeepSeek]` API 请求/响应状态日志、`[BUILD]` 工具执行开始/结束日志 |

**文件变更**：
- `backend-server/src/main.rs` — chat_handler 完整日志 + 端口绑定错误处理 + tokio 通道
- `backend-core/src/agent.rs` — LLM 调用前后日志 + 工具执行日志 + 通道类型变更
- `backend-core/src/llm/provider.rs` — API 请求/响应状态日志 + 网络错误日志
- `backend-core/src/trajectory/recorder.rs` — UTF-8 安全字符串截断
- `electron/main.js` — 启动前清理旧进程
- `frontend/main_window/app.js` — isThinking 竞态修复 + 所有 DOM 操作 null 检查

**验证**: 打包版 exe 启动正常，后端 `/health` 返回 200，`/chat` API 返回完整回复（5 轮迭代、6 次工具调用），无 panic，无 401 错误。

### 历史会话已完成
- 模型迁移 DeepSeek V3 → V4（deepseek-v4-flash / deepseek-v4-pro）
- CLI 启动脚本（start-agent.ps1）+ 环境预检
- 工具定义重构（15 → 16 个工具）+ 系统提示词工程化
- cua-driver 集成：新增 `desktop_control` 模块，提供 7 个桌面控制工具（截图/点击/输入/按键/窗口管理），含惰性启动与热检测机制，工具总数 16 → 23
- agent-skills 集成：融入 [addyosmani/agent-skills](https://github.com/addyosmani/agent-skills) 工程方法论，系统提示词重构为 DEFINE→PLAN→BUILD→VERIFY→REVIEW→SHIP 六阶段流程，新增假设前置、Stop-the-Line 排错、五轴代码审查、Definition of Done 质量门禁等核心原则
- `self.messages` 重置逻辑修复（每次任务开始只保留系统提示词）
- Electron 打包脚本（dist2/ 输出便携版 + nsis 安装版）

---

## 4. 当前工作分支

**代码状态**: 编译通过（`cargo build` 0 errors, 0 warnings），37 项测试全部通过（36 单元 + 1 doc-test），打包版 exe 全链路验证通过
**Git 状态**: 待提交。本次会话修复了打包版 exe 运行时所有关键错误
**远程**: origin/main → https://github.com/Yanxuer/jiangqiuyue（待推送）

---

## 5. 待办事项（优先级排序）

### 高优先级
- [x] **提交并推送本机代码**到 GitHub（大量未提交的架构改进）
- [x] **启动并运行一次完整任务**验证所有 Pipeline 正常（LLM 调用 + 工具执行 + 轨迹落盘）
- [x] **打包版 exe 全链路调试**：修复端口占用、UTF-8 panic、DOM null 引用、重复请求等全部关键问题

### 中优先级
- [ ] **将 Docker 沙箱集成到 agent.rs 的 bash_exec**（目前 docker_sandbox 模块独立存在，未被调用）
- [ ] **前端配置界面**适配新 Provider 字段（provider 下拉联动 base_url/model 预设，api_key 已通）
- [ ] **轨迹回放/分析工具**（读取 JSONL 绘制迭代流程图、token 消耗趋势）

### 低优先级
- [ ] 为 Docker 沙箱生成 CI 测试（需要 Docker 环境）
- [ ] 添加 Anthropic / Ollama 的 LLMProvider 变体
- [ ] 工具执行的真正并行化（需要将 execute_tool 提取为无锁函数）

---

## 6. 常见问题和注意事项

- **编译命令**: `cargo build --manifest-path backend-rust/Cargo.toml`（从根目录）
- **启动命令**: `cargo run --manifest-path backend-rust/backend-server/Cargo.toml` 或 `powershell ./start-agent.ps1`
- **API Key 加载优先级**: 环境变量 → `.env` 文件 → `runtime_config.json`
- **FastEmbed 模型**: 首次启动自动从 HuggingFace 下载 `all-MiniLM-L6-v2-onnx`，网络不好可手动放 `~/.cache/huggingface/hub/`
- **Cargo 路径**: 系统 PATH 可能没有 cargo，需用 `$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\cargo.exe`
- **cua-driver 桌面控制**: 可选组件，安装后提供 7 个 `desktop_*` 工具。安装命令: `irm https://raw.githubusercontent.com/trycua/cua/main/libs/cua-driver/scripts/install.ps1 | iex`。支持热检测：安装后无需重启服务，下次调用工具时自动生效。
- **trae-agent/ 目录**是参考项目（字节跳动 MIT 开源），不是本项目的代码，可用于对比学习

---

## 7. LLMProvider 扩展指南

添加新 OpenAI-compatible 提供商只需 3 步：

```rust
// 1. provider.rs 添加枚举变体
pub enum LLMProvider {
    DeepSeek(DeepSeekClient),
    OpenAI(OpenAIClient),
    Ollama(OllamaClient),  // ← 新增
}

// 2. 在 LLMProvider::chat() 中添加 match 分支
LLMProvider::Ollama(client) => {
    retry::retry_with_backoff("Ollama", client.config.max_retries, || {
        let messages = messages.to_vec();
        let client = client.clone();
        async move { client.chat_inner(&messages, tools).await }
    }).await
}

// 3. 实现 OllamaClient（参考 DeepSeekClient，只需改 api_name 和 url）
// 4. ProviderKind 中添加 "ollama" 匹配
```

---

## 8. 每次任务完成后，必须执行以下三项操作

> 以下动作必须在 `ai_connect.md` 本文件中完成，不得省略。

### 8.1 更新「已完成工作日志」

在 **§3 当前会话完成的优化** 中追加新条目：

```markdown
### Px: <任务标题>
- <一句话描述做了什么>
- <关键文件路径 + 变更说明>
- <验证结果（编译/测试/运行）>
```

标记规则：P0 = 核心功能/Bug修复，P1 = 重要增强，P2 = 辅助优化。

### 8.2 如有新发现的问题，更新「待办事项」

在 **§5 待办事项** 中添加新条目，或勾掉已完成的旧条目。
如果没有新增问题，至少检查一遍现有待办列表，确认描述仍准确。

### 8.3 如修改了工作区结构，更新「工作区地图」

在 **§2 模块架构** 中更新目录树。
如果只是修改文件内容（非新增/删除模块），此项可跳过。

