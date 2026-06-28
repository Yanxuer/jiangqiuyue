# AI Agent Desktop — 江秋月

一个基于 Electron + Rust 的桌面 AI 助手，搭载虚拟人物"江秋月"，具备屏幕识别、文件操作、命令行执行、向量记忆等能力。

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面框架 | Electron 28 |
| 前端 | 原生 HTML/CSS/JS + Vite |
| 后端 | Rust (Axum HTTP Server) |
| 向量嵌入 | FastEmbed (ONNX) — 本地加载，完全离线运行 |
| 记忆存储 | SQLite + 向量检索 (可降级为 SQL LIKE 搜索) |
| AI 引擎 | OpenAI 兼容 API / Ollama / 自定义模型 |

## 特性

- **虚拟人物交互** — 江秋月浮窗，支持 idle / talking / click / thinking 动画状态
- **自主编程智能体** — 多轮迭代运行，自动规划任务、读写文件、执行命令、联网检索，迭代上限 30 轮
- **安全沙箱** — 危险命令黑名单检测，所有终端操作在隔离沙箱中运行
- **屏幕识别** — 支持截取屏幕并分析
- **文件操作** — 读取文档、搜索文件、精准代码替换（str_replace_edit）
- **命令行执行** — 安全确认机制下的命令执行，执行前后详细日志输出
- **向量记忆系统** — 语义检索历史对话，支持主界面手动切换向量/SQL模式，失败自动重试并弹窗提示
- **环境配置窗口** — 首次启动弹出，可视化配置 API 密钥、模型、部署地址
- **自定义模型支持** — 支持任意 OpenAI 兼容 API 的服务商或本地 Ollama 部署
- **CLI 工具管理** — 扫描、搜索、安装、推荐、执行 CLI 工具
- **软件管理** — 扫描并启动已安装软件
- **文档阅读** — 支持 .txt / .md / .pdf / Word / Excel / CSV
- **离线模型缓存** — 嵌入模型已内置打包

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
DEEPSEEK_API_KEY=sk-your-key
DEEPSEEK_BASE_URL=https://api.deepseek.com
MODEL=deepseek-v4-flash
```

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
