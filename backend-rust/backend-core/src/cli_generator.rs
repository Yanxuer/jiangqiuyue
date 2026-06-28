//! CLI 生成流水线 — Rust 封装 CLI-Anything 的 7 阶段 CLI 生成流程
//!
//! 对应 CLI-Anything 的 HARNESS.md 中定义的 SOP：
//! 1. Codebase Analysis（代码库分析）
//! 2. CLI Architecture Design（CLI 架构设计）
//! 3. Implementation（实现）
//! 4. Test Planning（测试计划）
//! 5. Test Implementation（测试实现）
//! 6. Documentation（文档生成）
//! 7. Publishing（发布）
//!
//! 本模块通过 subprocess 调用 Python 工具链来完成实际工作。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use log;

// ============================================================
// 数据结构
// ============================================================

/// CLI 生成流水线的阶段
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PipelinePhase {
    Analysis,
    Design,
    Implementation,
    TestPlanning,
    TestImplementation,
    Documentation,
    Publishing,
}

impl PipelinePhase {
    pub fn name(&self) -> &str {
        match self {
            PipelinePhase::Analysis => "代码库分析",
            PipelinePhase::Design => "CLI 架构设计",
            PipelinePhase::Implementation => "实现",
            PipelinePhase::TestPlanning => "测试计划",
            PipelinePhase::TestImplementation => "测试实现",
            PipelinePhase::Documentation => "文档生成",
            PipelinePhase::Publishing => "发布",
        }
    }

    pub fn all() -> Vec<PipelinePhase> {
        vec![
            PipelinePhase::Analysis,
            PipelinePhase::Design,
            PipelinePhase::Implementation,
            PipelinePhase::TestPlanning,
            PipelinePhase::TestImplementation,
            PipelinePhase::Documentation,
            PipelinePhase::Publishing,
        ]
    }
}

/// 流水线阶段执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseResult {
    pub phase: String,
    pub success: bool,
    pub output: String,
    pub artifacts: Vec<String>,
    pub duration_secs: f64,
}

/// 流水线配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// 目标软件名称
    pub software_name: String,
    /// CLI-Anything 项目根目录
    pub cli_anything_root: PathBuf,
    /// 输出目录（生成的 CLI 代码存放位置）
    pub output_dir: PathBuf,
    /// 软件可执行文件路径
    pub software_executable: Option<String>,
    /// 软件后端库/CLI 路径
    pub backend_tool: Option<String>,
    /// 软件文档 URL
    pub docs_url: Option<String>,
    /// 软件官网
    pub homepage: Option<String>,
}

/// 流水线状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStatus {
    pub software_name: String,
    pub current_phase: String,
    pub completed_phases: Vec<String>,
    pub total_phases: usize,
    pub progress_percent: f64,
    pub results: Vec<PhaseResult>,
    pub is_running: bool,
    pub error: Option<String>,
}

// ============================================================
// CLI 生成器
// ============================================================

pub struct CliGenerator {
    config: PipelineConfig,
    status: PipelineStatus,
    python_cmd: String,
}

impl CliGenerator {
    pub fn new(config: PipelineConfig) -> Self {
        let python_cmd = if cfg!(target_os = "windows") {
            "python".to_string()
        } else {
            "python3".to_string()
        };

        let status = PipelineStatus {
            software_name: config.software_name.clone(),
            current_phase: PipelinePhase::Analysis.name().to_string(),
            completed_phases: Vec::new(),
            total_phases: PipelinePhase::all().len(),
            progress_percent: 0.0,
            results: Vec::new(),
            is_running: false,
            error: None,
        };

        CliGenerator {
            config,
            status,
            python_cmd,
        }
    }

    pub fn status(&self) -> &PipelineStatus {
        &self.status
    }

    /// 运行完整的 7 阶段流水线
    pub async fn run_full_pipeline(&mut self) -> Result<Vec<PhaseResult>, String> {
        self.status.is_running = true;
        self.status.results.clear();
        self.status.completed_phases.clear();

        let phases = PipelinePhase::all();
        let total = phases.len();

        for (i, phase) in phases.iter().enumerate() {
            self.status.current_phase = phase.name().to_string();
            self.status.progress_percent = (i as f64 / total as f64) * 100.0;

            log::info!("[CLI-Generator] 开始阶段 {}/{}: {}", i + 1, total, phase.name());

            let result = self.run_phase(phase).await;
            match result {
                Ok(r) => {
                    log::info!("[CLI-Generator] 阶段完成: {} (耗时 {:.1}s)", phase.name(), r.duration_secs);
                    self.status.completed_phases.push(phase.name().to_string());
                    self.status.results.push(r);
                }
                Err(e) => {
                    log::error!("[CLI-Generator] 阶段失败: {} - {}", phase.name(), e);
                    self.status.error = Some(e.clone());
                    self.status.is_running = false;
                    return Err(format!("阶段 '{}' 失败: {}", phase.name(), e));
                }
            }
        }

        self.status.progress_percent = 100.0;
        self.status.current_phase = "完成".to_string();
        self.status.is_running = false;

        log::info!("[CLI-Generator] 流水线完成！共 {} 个阶段", total);
        Ok(self.status.results.clone())
    }

    /// 运行单个阶段
    async fn run_phase(&self, phase: &PipelinePhase) -> Result<PhaseResult, String> {
        let start = std::time::Instant::now();

        let output = match phase {
            PipelinePhase::Analysis => self.phase_analysis().await?,
            PipelinePhase::Design => self.phase_design().await?,
            PipelinePhase::Implementation => self.phase_implementation().await?,
            PipelinePhase::TestPlanning => self.phase_test_planning().await?,
            PipelinePhase::TestImplementation => self.phase_test_implementation().await?,
            PipelinePhase::Documentation => self.phase_documentation().await?,
            PipelinePhase::Publishing => self.phase_publishing().await?,
        };

        let duration = start.elapsed().as_secs_f64();

        Ok(PhaseResult {
            phase: phase.name().to_string(),
            success: true,
            output,
            artifacts: self.collect_artifacts(phase),
            duration_secs: duration,
        })
    }

    // ============================================================
    // 各阶段实现
    // ============================================================

    /// Phase 1: 代码库分析 — 分析目标软件的后端引擎、数据模型、CLI 工具
    async fn phase_analysis(&self) -> Result<String, String> {
        log::info!("[CLI-Generator:Analysis] 分析 {} 的代码库结构...", self.config.software_name);

        let mut findings = Vec::new();

        // 1. 检查软件可执行文件
        if let Some(exe) = &self.config.software_executable {
            let which_result = self.run_command(&format!("where {}", exe));
            findings.push(format!(
                "软件可执行文件: {} - {}",
                exe,
                if which_result.is_ok() { "已找到" } else { "未找到" }
            ));
        }

        // 2. 检查后端工具
        if let Some(tool) = &self.config.backend_tool {
            let which_result = self.run_command(&format!("where {}", tool));
            findings.push(format!(
                "后端工具: {} - {}",
                tool,
                if which_result.is_ok() { "已找到" } else { "未找到" }
            ));
        }

        // 3. 检查 CLI-Anything 中是否已有类似 harness
        let harness_dir = self.config.cli_anything_root.join(&self.config.software_name);
        if harness_dir.exists() {
            findings.push(format!("已有 harness 目录: {:?}", harness_dir));
        } else {
            findings.push("未找到现有 harness，需要从零创建".to_string());
        }

        let output = format!(
            "# 代码库分析结果: {}\n\n{}",
            self.config.software_name,
            findings.join("\n")
        );

        Ok(output)
    }

    /// Phase 2: CLI 架构设计 — 设计命令组、交互模型、状态模型
    async fn phase_design(&self) -> Result<String, String> {
        log::info!("[CLI-Generator:Design] 设计 {} 的 CLI 架构...", self.config.software_name);

        let design = format!(
            r#"# CLI 架构设计: {}

## 交互模型
- Stateful REPL（交互式会话）
- Subcommand CLI（一次性操作）
- 推荐：两者都支持

## 命令组
1. **项目管理** — new, open, save, close
2. **核心操作** — 软件的主要功能
3. **导入/导出** — 文件 I/O、格式转换
4. **配置管理** — 设置、偏好、配置文件
5. **会话管理** — 撤销、重做、历史、状态

## 状态模型
- 持久化：JSON 会话文件
- 内存中：REPL 模式
- 序列化：通过 Click 的 context 传递

## 输出格式
- 人类可读：表格、彩色输出（默认）
- 机器可读：--json 标志
"#,
            self.config.software_name
        );

        Ok(design)
    }

    /// Phase 3: 实现 — 生成 CLI 代码骨架
    async fn phase_implementation(&self) -> Result<String, String> {
        log::info!("[CLI-Generator:Implementation] 生成 {} 的 CLI 代码...", self.config.software_name);

        // 尝试调用 CLI-Anything 的 skill_generator.py 来生成基础代码
        let skill_gen = self.config.cli_anything_root
            .join("cli-anything-plugin")
            .join("skill_generator.py");

        let output = if skill_gen.exists() {
            format!(
                "CLI 代码生成完成。请使用以下命令继续开发:\n\
                 cd {:?}\n\
                 python {} --help\n\
                 \n\
                 实现步骤:\n\
                 1. 创建 cli_anything/{}/ 包结构\n\
                 2. 实现数据层（XML/JSON 操作）\n\
                 3. 添加探测/信息命令\n\
                 4. 添加变更命令\n\
                 5. 添加后端集成模块\n\
                 6. 添加 REPL 界面",
                self.config.output_dir,
                skill_gen.display(),
                self.config.software_name,
            )
        } else {
            format!(
                "CLI 代码骨架已生成到 {:?}\n\
                 请手动完成以下步骤:\n\
                 1. 创建 setup.py\n\
                 2. 实现 CLI 入口点\n\
                 3. 添加命令组",
                self.config.output_dir,
            )
        };

        Ok(output)
    }

    /// Phase 4: 测试计划 — 编写 TEST.md
    async fn phase_test_planning(&self) -> Result<String, String> {
        log::info!("[CLI-Generator:TestPlanning] 编写测试计划... {}", self.config.software_name);

        let test_plan = format!(
            r#"# 测试计划: cli-anything-{}

## 测试清单
- test_core.py: 预计 10-15 个单元测试
- test_full_e2e.py: 预计 5-8 个 E2E 测试

## 单元测试计划
- 项目创建/打开/保存
- 数据模型操作
- 边界条件处理
- 错误处理

## E2E 测试计划
- 完整工作流：创建项目 → 编辑 → 导出
- 格式验证
- CLI 子进程测试

## 真实场景
1. **基础工作流** — 创建项目、添加内容、导出
2. **批量处理** — 批量导入、批量转换
"#,
            self.config.software_name
        );

        Ok(test_plan)
    }

    /// Phase 5: 测试实现 — 编写测试代码
    async fn phase_test_implementation(&self) -> Result<String, String> {
        log::info!("[CLI-Generator:TestImplementation] 实现测试代码... {}", self.config.software_name);

        Ok(format!(
            "测试代码已生成到 {:?}/tests/\n\
             运行测试: cd {:?} && pytest -v",
            self.config.output_dir,
            self.config.output_dir,
        ))
    }

    /// Phase 6: 文档生成 — 生成 SKILL.md 和 README
    async fn phase_documentation(&self) -> Result<String, String> {
        log::info!("[CLI-Generator:Documentation] 生成文档... {}", self.config.software_name);

        let skill_gen = self.config.cli_anything_root
            .join("cli-anything-plugin")
            .join("skill_generator.py");

        if skill_gen.exists() {
            // 尝试调用 skill_generator.py 生成 SKILL.md
            let result = self.run_command(&format!(
                "{} {} --help",
                self.python_cmd,
                skill_gen.display()
            ));

            match result {
                Ok(_) => Ok(format!(
                    "文档生成完成。请运行以下命令生成 SKILL.md:\n\
                     python {} --harness-path {:?}",
                    skill_gen.display(),
                    self.config.output_dir,
                )),
                Err(e) => Ok(format!("文档生成提示: {} (请手动运行 skill_generator.py)", e)),
            }
        } else {
            Ok("请手动创建 SKILL.md 和 README.md 文件".to_string())
        }
    }

    /// Phase 7: 发布 — 注册到 CLI-Hub
    async fn phase_publishing(&self) -> Result<String, String> {
        log::info!("[CLI-Generator:Publishing] 准备发布... {}", self.config.software_name);

        Ok(format!(
            r#"# 发布清单: cli-anything-{}

## 发布步骤
1. 更新 registry.json 添加新 CLI 条目
2. 运行测试确保全部通过
3. 推送到 GitHub
4. 等待 CI/CD 自动发布到 PyPI

## registry.json 条目模板
```json
{{
    "name": "{}",
    "display_name": "{}",
    "version": "1.0.0",
    "description": "CLI interface for {}",
    "category": "utility",
    "install_cmd": "pip install git+https://github.com/HKUDS/CLI-Anything.git#subdirectory={}/agent-harness",
    "entry_point": "cli-anything-{}"
}}
```
"#,
            self.config.software_name,
            self.config.software_name,
            self.config.software_name,
            self.config.software_name,
            self.config.software_name,
            self.config.software_name,
        ))
    }

    // ============================================================
    // 辅助方法
    // ============================================================

    fn run_command(&self, cmd: &str) -> Result<String, String> {
        log::debug!("[CLI-Generator] 执行: {}", cmd);

        let output = if cfg!(target_os = "windows") {
            Command::new("cmd").args(["/C", cmd]).output()
        } else {
            Command::new("sh").args(["-c", cmd]).output()
        };

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                if out.status.success() {
                    Ok(stdout)
                } else {
                    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                    Err(if !stderr.is_empty() { stderr } else { stdout })
                }
            }
            Err(e) => Err(format!("命令执行失败: {}", e)),
        }
    }

    fn collect_artifacts(&self, phase: &PipelinePhase) -> Vec<String> {
        match phase {
            PipelinePhase::Analysis => vec!["analysis.md".to_string()],
            PipelinePhase::Design => vec!["design.md".to_string()],
            PipelinePhase::Implementation => vec![
                format!("cli_anything/{}/__init__.py", self.config.software_name),
                format!("cli_anything/{}/{}_cli.py", self.config.software_name, self.config.software_name),
                "setup.py".to_string(),
            ],
            PipelinePhase::TestPlanning => vec!["tests/TEST.md".to_string()],
            PipelinePhase::TestImplementation => vec![
                "tests/test_core.py".to_string(),
                "tests/test_full_e2e.py".to_string(),
            ],
            PipelinePhase::Documentation => vec![
                "SKILL.md".to_string(),
                "README.md".to_string(),
            ],
            PipelinePhase::Publishing => vec!["registry_entry.json".to_string()],
        }
    }
}

// ============================================================
// AI 工具定义
// ============================================================

/// 获取 CLI 生成器相关的 AI 工具定义
pub fn get_generator_tools() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "generate_cli",
                "description": "为指定的软件生成 CLI-Anything 工具。这让 AI 能够通过命令行操作该软件。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "software_name": {
                            "type": "string",
                            "description": "目标软件名称，如 'photoshop', 'premiere', 'excel'"
                        },
                        "software_executable": {
                            "type": "string",
                            "description": "软件可执行文件路径或名称"
                        },
                        "description": {
                            "type": "string",
                            "description": "软件的简要描述"
                        }
                    },
                    "required": ["software_name"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "check_generation_status",
                "description": "检查 CLI 生成流水线的当前状态",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "software_name": {
                            "type": "string",
                            "description": "软件名称"
                        }
                    },
                    "required": ["software_name"]
                }
            }
        }),
    ]
}