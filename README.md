# AI Agent Desktop — 江秋月

一个基于 Electron + Rust 的桌面 AI 助手，搭载虚拟人物"江秋月"，具备屏幕识别、文件操作、命令行执行、向量记忆、桌面控制等能力。

> **最新更新 (2026-06-30)**: 修复打包版 exe 全链路运行问题，支持实时日志流（WebSocket 推送 DEFINE→SHIP 六阶段状态），新增日志导出功能。

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面框架 | Electron 28 |
| 前端 | 原生 HTML/CSS/JS + Vite |
| 后端 | Rust (Axum HTTP Server) |
| 向量嵌入 | FastEmbed (ONNX) — 本地加载，完全离线运行 |
| 记忆存储 | SQLite + 向量检索 (可降级为 SQL LIKE 搜索) |
| AI 引擎 | OpenAI 兼容 API / Ollama / 自定义模型 |

## 项目特长

### 🧠 自主编程智能体

- **23 个工具**覆盖完整开发链路：文件读写（精准 `str_replace_edit`）、终端执行、联网搜索、屏幕截图、桌面控制（cua-driver）、CLI 管理、软件启动、顺序思维
- **六阶段工程生命周期**：基于 [agent-skills](https://github.com/addyosmani/agent-skills) 方法论，DEFINE→PLAN→BUILD→VERIFY→REVIEW→SHIP，每阶段有明确的质量门禁
- **多轮迭代自动规划**：LLM 自主拆解任务 → 工具执行 → 结果反馈 → 下一次决策，迭代上限 30 轮
- **三步流水线并行化**：预检查（串行安全限流）→ `tokio::join_all` 并行执行多个工具 → 结果按原始顺序记录，LLM 一次返回多个工具调用时显著提速

### 🔌 零开销多 LLM 提供商架构

- **枚举多态设计**（`enum LLMProvider`），无 trait object 的 `Box` 分配和 `async-trait` 开销
- **统一 OpenAI 兼容客户端**（`OpenAICompatClient`）：DeepSeek 和 OpenAI 共享同一套 HTTP 调用逻辑，仅基 URL 和日志前缀不同，无重复代码
- **通用 API 重试**：3 次指数退避重试（3-30s 随机抖动），智能区分 429/5xx（可重试）和 4xx（立即返回）
- 添加新 Provider 只需加一个枚举变体，成本极低

### ⚡ 全链路异步 I/O，零阻塞

- **轨迹录制异步化**：JSONL 增量落盘使用 `tokio::fs` + `AsyncWriteExt`，不阻塞 async runtime
- **记忆编码异步化**：FastEmbed ONNX 推理通过 `spawn_blocking` 送入专用线程池，保证主事件循环不卡顿
- Agent 主循环、HTTP API、工具执行全路径无同步阻塞

### 🛡️ Docker 沙箱 + 安全防线

- 容器级命令隔离（memory_limit / network_enabled / read_only 约束）
- Docker 不可用时**自动降级**本地执行，开发体验不中断
- 危险命令黑名单检测 + 执行前后详细日志输出

### 🧪 37 项集成测试，端到端验证

- **Provider 层 17 项**：枚举解析、默认值、LLMConfig 转换、ToolDefinition OpenAI 兼容性、Provider 切换不泄漏品牌名
- **Config 层 12 项**：三级配置优先级、JSON 往返序列化、Runtime 合并、Provider 切换完整性、边界情况
- **Desktop 层 7 项**：cua-driver 二进制检测、状态机转换、热检测、工具调用、错误处理、模拟环境集成

### 🧠 向量记忆系统

- **本地离线运行**：FastEmbed (ONNX) + all-MiniLM-L6-v2，无需网络
- **语义搜索 + 自动降级**：启动优先向量模式，失败自动切 SQL LIKE
- **主界面可视化切换**，后台自动重试，失败弹窗提示

### 🎨 虚拟人物交互

- Live2D 浮窗人物"江秋月"，idle / talking / click / thinking 动画状态
- 自定义人格与对话风格

### 📦 零配置启动

- 打包版内置模型缓存（~86MB），换电脑免下载
- 可视化配置窗口 + .env + 环境变量三级配置
- CLI 启动脚本支持一站式编译+测试+调参

## 项目结构

```
ai-agent-desktop/
├── frontend/                 # 前端界面
│   ├── main_window/          # 主对话窗口
│   ├── float_window/         # 虚拟人物浮窗
│   └── scripts/              # 构建脚本
├── electron/                 # Electron 壳层
│   ├── frontend-dist/        # 前端构建产物 (生成)
│   ├── rust-dist/            # Rust 后端产物 (生成)
│   └── scripts/              # 打包 & 下载脚本
├── backend-rust/             # Rust 后端
│   ├── backend-core/         # 核心库 (Agent/工具/记忆)
│   ├── backend-server/       # HTTP 服务 (Axum)
│   └── download-model/       # 模型下载工具
├── resources/                # 资源文件 (图标等)
└── docs/                     # 文档
```

## 快速开始

### 前置要求

- Node.js 18+
- Rust 1.75+
- 一个兼容 OpenAI API 的大模型服务 (或本地 Ollama)

## 配置说明

本项目支持两套配置方式，可同时使用，运行时配置优先级高于 `.env` 文件。

### 方式一：可视化配置窗口（推荐，打包版用户）

首次启动打包版 exe 时，会自动弹出环境配置窗口，可视化填写：

- **API Key** — 服务商提供的密钥（如 DeepSeek / OpenAI）
- **接口地址** — API 的基础 URL（默认 `https://api.deepseek.com`）
- **模型名称** — 使用的模型名（如 `deepseek-v4-flash` / `deepseek-v4-pro` / `gpt-4o`）

配置保存在 `memory_db/runtime_config.json`，无需手动编辑文件。

### 方式二：`.env` 文件（开发者 / 源码运行）

```bash
# 在 backend-rust/ 目录下创建 .env
# （已包含在 .gitignore 中，不会提交到仓库）

# === 通用 LLM 配置（推荐） ===
LLM_API_KEY=sk-your-key
LLM_PROVIDER=deepseek
LLM_BASE_URL=https://api.deepseek.com
LLM_MODEL=deepseek-v4-flash

# === 旧版兼容（DEEPSEEK_* 作为 fallback） ===
DEEPSEEK_API_KEY=sk-your-key
DEEPSEEK_BASE_URL=https://api.deepseek.com
MODEL=deepseek-v4-flash
```

**多提供商切换**：

| 提供商 | LLM_PROVIDER | LLM_BASE_URL | 默认模型 |
|--------|-------------|-------------|---------|
| DeepSeek | `deepseek` | `https://api.deepseek.com` | `deepseek-v4-flash` |
| OpenAI | `openai` | `https://api.openai.com/v1` | `gpt-4o` |
| 自定义兼容 | 任意 | 自定义 | 自定义 |

> 未知的 `LLM_PROVIDER` 值默认回退到 DeepSeek 配置，保证向后兼容。

### 优先级（高 → 低）

```
可视化窗口配置 (runtime_config.json)  →  .env 文件  →  系统环境变量
```

### 安装 & 运行

```bash
# 1. 安装前端依赖
cd frontend && npm install

# 2. 安装 Electron 依赖
cd ../electron && npm install

# 3. 开发模式启动
npm run dev
```

### 下载嵌入模型（向量记忆必需）

```bash
cd electron
node scripts/download-model.js
```

模型将下载到 `electron/rust-dist/model_cache/` 目录。

> 如果在中国大陆网络环境下，脚本会自动使用 `hf-mirror.com` 镜像下载。

### 构建打包

```bash
cd electron
npm run build
```

产物位于 `electron/dist2/`:

| 文件 | 说明 |
|------|------|
| `Qiuyue2.0.exe` | 便携版 (直接运行，绿色免安装) |
| `Qiuyue2.0 Setup.exe` | NSIS 安装版 |

两个版本均内置了嵌入模型缓存（约 86MB），换电脑无需重新下载。

### CLI 启动脚本（开发者调试用）

```bash
# 基础启动（沙箱隔离 + 编译 + 启动服务）
.\start-agent.ps1

# 启动后自动运行模拟任务，验证工具调用流程
.\start-agent.ps1 -Test

# 仅编译不启动
.\start-agent.ps1 -BuildOnly

# 跳过沙箱检查
.\start-agent.ps1 -NoSandbox

# 自定义端口和日志级别
.\start-agent.ps1 -Port 8080 -LogLevel debug
```

## 智能体工具栈

| 工具 | 用途 | 分类 |
|------|------|------|
| `view_file` | 读取文件/列出目录 | 标准 |
| `str_replace_edit` | 修改/新建/删除代码 | 标准 |
| `bash_exec` | 执行 shell 命令 | 标准 |
| `web_search` | 检索文档/报错方案 | 标准 |
| `task_complete` | 结束任务输出总结 | 标准 |
| `capture_screen` | 截取用户屏幕 | 扩展 |
| `search_memory` | 搜索长期记忆 | 扩展 |
| `add_memory` | 保存到长期记忆 | 扩展 |
| `find_software` | 搜索本机软件 | 扩展 |
| `launch_software` | 启动软件 | 扩展 |
| `list_clis` | 列出 CLI 工具 | 扩展 |
| `search_clis` | 搜索 CLI 工具 | 扩展 |
| `install_cli` | 安装 CLI 工具 | 扩展 |
| `execute_cli` | 执行 CLI 命令 | 扩展 |
| `recommend_clis` | 推荐 CLI 工具 | 扩展 |

## 向量记忆系统

该项目使用 **FastEmbed** + **ONNX Runtime** 在本地运行嵌入模型，完全离线，无需联网：

- **默认优先**：启动时自动加载向量模型，启用语义搜索
- **自动降级**：如果模型加载失败，自动降级为 SQL LIKE 模糊搜索
- **手动切换**：在主界面记忆面板中，可随时切换 向量/SQL 模式
- **重试机制**：选择向量模式后，后台自动重试，显示详细日志；失败数次后弹出错误窗口并自动降级为 SQL

## 模型下载(Hugging Face 镜像)

由于国内网络环境，模型下载可能不稳定。本项目内置了镜像支持：

**环境变量**：设置 `HF_ENDPOINT=https://hf-mirror.com` 即可使用国内镜像加速下载。

打包版已预置 `model_cache`，直接运行即可，无需额外下载。

## License

MIT
