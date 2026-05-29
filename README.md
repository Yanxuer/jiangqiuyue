# AI Agent Desktop — 江秋月

一个基于 Electron + Rust 的桌面 AI 助手，搭载虚拟人物"江秋月"，具备屏幕识别、文件操作、命令行执行、记忆存储等能力。

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面框架 | Electron 28 |
| 前端 | 原生 HTML/CSS/JS + Vite |
| 后端 | Rust (Axum HTTP Server) |
| 向量嵌入 | FastEmbed (ONNX) |
| 记忆存储 | SQLite + 向量检索 (降级: LIKE 搜索) |
| AI 引擎 | OpenAI 兼容 API / Ollama |

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
│   └── scripts/              # 打包脚本
├── backend-rust/             # Rust 后端
│   ├── backend-core/         # 核心库 (Agent/工具/记忆)
│   └── backend-server/       # HTTP 服务 (Axum)
└── resources/                # 资源文件
```

## 快速开始

### 前置要求

- Node.js 18+
- Rust 1.75+
- 一个兼容 OpenAI API 的大模型服务 (或本地 Ollama)

### 配置环境变量

```bash
# 在项目根目录创建 .env
cp .env.example .env
```

编辑 `.env` 填入你的 API 配置：

```env
OPENAI_API_KEY=sk-your-key
OPENAI_BASE_URL=https://api.openai.com/v1
MODEL_NAME=gpt-4o
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

### 构建打包

```bash
cd electron
npm run build
```

产物位于 `electron/dist/AI Agent *.exe`。

## 功能

- **虚拟人物交互** — 江秋月浮窗，支持 idle / talking / click / thinking 动画状态
- **屏幕识别** — 支持截取屏幕并分析
- **文件操作** — 读取文档、搜索文件
- **命令行执行** — 安全确认机制下的命令执行
- **智能记忆** — 向量数据库存储，支持语义搜索；不可用时自动降级为 SQL LIKE 搜索
- **软件管理** — 扫描并启动已安装软件
- **文档阅读** — 支持 .txt / .md / .pdf / Word / Excel / CSV

## License

MIT