//! CLI 工具使用引导教程
//!
//! 提供 CLI-Hub 的完整使用指南，帮助用户和 AI Agent 了解如何使用 CLI 工具。
//! 教程内容可通过 API 获取并在前端展示。

use serde::Serialize;

/// 教程章节
#[derive(Debug, Clone, Serialize)]
pub struct GuideSection {
    pub title: String,
    pub icon: String,
    pub content: String,
}

/// 完整的 CLI 使用引导教程
pub fn get_guide() -> Vec<GuideSection> {
    vec![
        GuideSection {
            title: "什么是 CLI-Hub？".to_string(),
            icon: "house".to_string(),
            content: r#"CLI-Hub 是 CLI-Anything 生态的组件管理中心。

它让你能够通过 AI 代理，用命令行（CLI）的方式操控各种桌面软件。
比如：用命令让 Blender 渲染一个场景、让 GIMP 批量处理图片、让 Audacity 剪辑音频。

核心概念：
- **CLI 工具**：每个软件对应一个 CLI 工具包，提供该软件的命令行操作接口
- **注册表**：所有可用 CLI 工具的目录，分为 Harness（社区构建）和 Public（官方 CLI）
- **安装状态**：跟踪哪些 CLI 已安装在你的电脑上
- **AI 代理**：DeepSeek 可以自动搜索、推荐、安装和执行 CLI 工具"#.to_string(),
        },
        GuideSection {
            title: "如何使用 CLI 工具？".to_string(),
            icon: "play".to_string(),
            content: r#"有三种方式使用 CLI 工具：

### 1. 通过 AI 对话（推荐）
直接在聊天窗口告诉江秋月你想做什么，AI 会自动：
- 搜索合适的 CLI 工具
- 推荐匹配的 CLI
- 安装你需要的 CLI
- 执行具体命令

示例对话：
- "帮我把这张图片转成黑白的"
- "用 Blender 创建一个新项目"
- "我电脑上有哪些软件可以用 CLI 操控？"

### 2. 通过 CLI-Hub 界面
点击左侧导航栏的 "CLI-Hub" 按钮，可以：
- 浏览所有可用 CLI 工具
- 按分类筛选（3D、音频、视频、图像、AI 等）
- 搜索特定的 CLI
- 查看详细信息
- 一键安装/卸载

### 3. 通过 API
高级用户可以直接调用 REST API：
- GET /api/cli-hub/list - 列出所有 CLI
- GET /api/cli-hub/search?q=xxx - 搜索 CLI
- POST /api/cli-hub/install - 安装 CLI
- POST /api/cli-hub/execute - 执行 CLI 命令"#.to_string(),
        },
        GuideSection {
            title: "安装你的第一个 CLI 工具".to_string(),
            icon: "download".to_string(),
            content: r#"安装 CLI 工具非常简单：

### 步骤 1：打开 CLI-Hub
点击左侧导航栏的 "CLI-Hub" 按钮。

### 步骤 2：选择工具
在列表中浏览或搜索你需要的 CLI 工具。每个工具都有：
- 名称和版本号
- 功能描述
- 所属分类
- 安装状态标识

### 步骤 3：点击安装
点击工具卡片，进入详情页，点击 "安装" 按钮。

### 步骤 4：开始使用
安装完成后，你可以：
- 在聊天中直接让 AI 执行命令
- 切换到 "已安装" 标签查看已安装的工具
- 点击 "卸载" 按钮移除不需要的工具

支持的安装策略：
- **pip**：Python 包安装（最常用）
- **npm**：Node.js 包安装
- **uv**：使用 uv 包管理器安装
- **command**：自定义安装命令
- **bundled**：内置工具无需安装"#.to_string(),
        },
        GuideSection {
            title: "AI 可用的 CLI 操作".to_string(),
            icon: "robot".to_string(),
            content: r#"AI 代理（江秋月）可以使用以下 CLI 相关操作：

### search_clis
搜索可用的 CLI 工具。当你说"有没有操作 Blender 的命令行工具"时，AI 会调用这个。

### list_clis
列出所有 CLI 工具，支持按分类和来源筛选。

### get_cli_info
获取某个 CLI 工具的详细信息，包括安装说明、依赖要求等。

### install_cli
安装指定的 CLI 工具。

### execute_cli
执行已安装 CLI 的命令。AI 会先确认 CLI 已安装，然后执行命令。

### recommend_clis
根据你电脑上已安装的软件，推荐可用的 CLI 工具。

---

### 安全机制
所有 CLI 命令执行前都会经过安全检查：
- 危险命令拦截（如 rm -rf、format、shutdown 等）
- 命令含义分析
- 受影响文件识别
- 用户确认机制（高危操作需要你手动确认）"#.to_string(),
        },
        GuideSection {
            title: "支持的软件类别".to_string(),
            icon: "grid".to_string(),
            content: r#"CLI-Hub 注册表按类别组织，以下是主要类别：

| 类别 | 说明 | 示例 |
|------|------|------|
| **3d** | 3D 建模和渲染 | Blender, Maya |
| **audio** | 音频编辑和处理 | Audacity, FFmpeg |
| **video** | 视频编辑和处理 | FFmpeg, OBS |
| **image** | 图像编辑和处理 | GIMP, ImageMagick |
| **ai** | AI 和机器学习 | Ollama, Stable Diffusion |
| **devops** | 开发运维工具 | Docker, Git |
| **web** | Web 开发工具 | Node.js, npm |
| **database** | 数据库管理 | PostgreSQL, Redis |
| **utility** | 通用工具 | 7-Zip, curl |
| **office** | 办公软件 | LibreOffice |

每个类别下都有 Harness 和 Public 两种来源的 CLI 工具。"#.to_string(),
        },
        GuideSection {
            title: "常见问题".to_string(),
            icon: "question".to_string(),
            content: r#"### Q: CLI 工具安装失败怎么办？
A: 检查以下几点：
1. 确保 Python/pip 或 Node.js/npm 已正确安装
2. 查看错误日志了解具体原因（日志中有详细记录）
3. 尝试手动安装依赖（如 pip install xxx）
4. 检查网络连接是否正常

### Q: 安装后无法执行命令？
A: 可能原因：
1. CLI 入口点未正确注册（检查 PATH）
2. 依赖项未完全安装
3. 软件本身未安装（CLI 只是操作接口，需要软件本体）

### Q: 可以为未支持的软件创建 CLI 吗？
A: 可以！CLI-Anything 提供了完整的 7 阶段 CLI 生成流水线：
1. 代码库分析
2. CLI 架构设计
3. 实现
4. 测试计划
5. 测试实现
6. 文档生成
7. 发布

让 AI 调用 generate_cli 工具即可开始。

### Q: 如何卸载 CLI 工具？
A: 在 CLI-Hub 界面中，切换到"已安装"标签，点击要卸载的工具，然后点击"卸载"按钮。

### Q: CLI 工具安全吗？
A: 所有 CLI 命令执行前都经过多层安全检查：
- 危险命令模式匹配
- 命令黑名单过滤
- 操作分类识别
- 用户确认机制
- 执行日志完整记录"#.to_string(),
        },
    ]
}

/// 获取教程的 Markdown 格式文本
pub fn get_guide_markdown() -> String {
    let guide = get_guide();
    let mut md = String::from("# CLI-Hub 使用指南\n\n");

    for section in &guide {
        md.push_str(&format!("## {}\n\n", section.title));
        md.push_str(&section.content);
        md.push_str("\n\n---\n\n");
    }

    md.push_str("> 有问题？直接在聊天中问江秋月吧~");
    md
}

/// 获取教程的简短快速入门版本
pub fn get_quick_start() -> String {
    r#"# CLI-Hub 快速入门

## 三步开始
1. **浏览** → 点击左侧 "CLI-Hub" 查看所有可用工具
2. **安装** → 选择需要的工具，点击 "安装"
3. **使用** → 在聊天中让 AI 执行命令，或直接调用 API

## 对话示例
- "帮我找一下可以操作 Blender 的 CLI 工具"
- "安装 Blender 的 CLI 工具"
- "用 Blender CLI 创建一个新项目"

## 注意事项
- 需要先安装对应的软件本体（CLI 只是操控接口）
- 安装需要网络连接
- 所有命令执行前会经过安全检查
"#.to_string()
}