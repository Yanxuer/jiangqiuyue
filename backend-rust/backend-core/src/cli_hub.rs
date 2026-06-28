//! CLI-Hub 模块 — CLI-Anything 注册表解析、安装管理与 AI 工具集成
//!
//! 负责：
//! - 解析本地 registry.json / public_registry.json
//! - 跟踪已安装的 CLI
//! - 提供搜索、安装、卸载、更新功能
//! - 与 software_scanner 结果匹配，推荐相关 CLI

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;
use std::process::Command;
use log;

// ============================================================
// 数据结构
// ============================================================

/// 注册表中的单个 CLI 条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliEntry {
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: String,
    pub requires: Option<String>,
    pub homepage: Option<String>,
    pub source_url: Option<String>,
    pub install_cmd: Option<String>,
    pub uninstall_cmd: Option<String>,
    pub update_cmd: Option<String>,
    pub entry_point: Option<String>,
    pub skill_md: Option<String>,
    pub category: String,
    pub package_manager: Option<String>,
    pub npm_package: Option<String>,
    pub npx_cmd: Option<String>,
    pub install_strategy: Option<String>,
    pub install_notes: Option<String>,
    pub contributors: Option<Vec<Contributor>>,
    #[serde(default)]
    pub _source: String, // "harness" 或 "public"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contributor {
    pub name: String,
    pub url: Option<String>,
}

/// 注册表元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryMeta {
    pub repo: Option<String>,
    pub description: Option<String>,
    pub updated: Option<String>,
}

/// 完整注册表结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registry {
    pub meta: Option<RegistryMeta>,
    pub clis: Vec<CliEntry>,
}

/// 已安装 CLI 的跟踪信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledCli {
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub installed_at: String,
    pub source: String,
    pub install_strategy: String,
    pub entry_point: Option<String>,
}

/// CLI-Hub 管理器
pub struct CliHub {
    /// 已加载的所有 CLI 条目
    entries: Vec<CliEntry>,
    /// 已安装的 CLI 跟踪
    installed: HashMap<String, InstalledCli>,
    /// 安装状态文件路径
    installed_file: PathBuf,
    /// CLI-Anything 项目根目录
    cli_anything_root: PathBuf,
}

// ============================================================
// CliHub 实现
// ============================================================

impl CliHub {
    /// 创建新的 CliHub 实例，从本地注册表文件加载数据
    pub fn new(cli_anything_root: impl Into<PathBuf>) -> Self {
        let root = cli_anything_root.into();
        let installed_file = dirs_next().unwrap_or_else(|| PathBuf::from("."))
            .join(".cli-hub")
            .join("installed.json");

        let mut hub = CliHub {
            entries: Vec::new(),
            installed: HashMap::new(),
            installed_file,
            cli_anything_root: root,
        };

        hub.load_registries();
        hub.load_installed();
        hub
    }

    /// 加载本地注册表文件
    fn load_registries(&mut self) {
        log::info!("[CLI-Hub] ========== 开始加载注册表 ==========");
        log::info!("[CLI-Hub] CLI-Anything 根目录: {:?}", self.cli_anything_root);

        // 加载 harness 注册表
        let registry_path = self.cli_anything_root.join("registry.json");
        log::info!("[CLI-Hub] 查找 harness 注册表: {:?}", registry_path);
        if let Ok(content) = fs::read_to_string(&registry_path) {
            log::info!("[CLI-Hub] registry.json 文件大小: {} 字节", content.len());
            match serde_json::from_str::<Registry>(&content) {
                Ok(registry) => {
                    let count = registry.clis.len();
                    if let Some(ref meta) = registry.meta {
                        log::info!("[CLI-Hub] 注册表元数据: repo={:?}, updated={:?}", meta.repo, meta.updated);
                    }
                    for mut cli in registry.clis {
                        log::debug!("[CLI-Hub] 加载 harness CLI: {} (v{}, 分类: {})", cli.name, cli.version, cli.category);
                        cli._source = "harness".to_string();
                        self.entries.push(cli);
                    }
                    log::info!("[CLI-Hub] 加载 harness 注册表: {} 个 CLI", count);
                }
                Err(e) => {
                    log::error!("[CLI-Hub] 解析 registry.json 失败: {}", e);
                    log::error!("[CLI-Hub] 文件内容前 200 字符: {}", &content[..content.len().min(200)]);
                }
            }
        } else {
            log::warn!("[CLI-Hub] 未找到 registry.json: {:?}", registry_path);
        }

        // 加载 public 注册表
        let public_path = self.cli_anything_root.join("public_registry.json");
        log::info!("[CLI-Hub] 查找 public 注册表: {:?}", public_path);
        if let Ok(content) = fs::read_to_string(&public_path) {
            log::info!("[CLI-Hub] public_registry.json 文件大小: {} 字节", content.len());
            match serde_json::from_str::<Registry>(&content) {
                Ok(registry) => {
                    let count = registry.clis.len();
                    for mut cli in registry.clis {
                        log::debug!("[CLI-Hub] 加载 public CLI: {} (v{}, 分类: {})", cli.name, cli.version, cli.category);
                        cli._source = "public".to_string();
                        self.entries.push(cli);
                    }
                    log::info!("[CLI-Hub] 加载 public 注册表: {} 个 CLI", count);
                }
                Err(e) => {
                    log::error!("[CLI-Hub] 解析 public_registry.json 失败: {}", e);
                }
            }
        } else {
            log::info!("[CLI-Hub] 未找到 public_registry.json (这是正常的，如果只有 harness 注册表)");
        }

        // 统计分类
        let mut categories: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for entry in &self.entries {
            *categories.entry(entry.category.clone()).or_insert(0) += 1;
        }
        log::info!("[CLI-Hub] 分类统计: {:?}", categories);
        log::info!("[CLI-Hub] ========== 注册表加载完成: 总计 {} 个 CLI 条目 ==========", self.entries.len());
    }

    /// 加载已安装状态
    fn load_installed(&mut self) {
        log::info!("[CLI-Hub] 加载已安装状态: {:?}", self.installed_file);
        if let Ok(content) = fs::read_to_string(&self.installed_file) {
            log::info!("[CLI-Hub] installed.json 大小: {} 字节", content.len());
            match serde_json::from_str::<HashMap<String, InstalledCli>>(&content) {
                Ok(installed) => {
                    self.installed = installed;
                    log::info!("[CLI-Hub] 已安装 {} 个 CLI:", self.installed.len());
                    for (name, info) in &self.installed {
                        log::info!("[CLI-Hub]   - {} (v{}, 来源: {}, 安装时间: {})",
                            name, info.version, info.source, info.installed_at);
                    }
                }
                Err(e) => {
                    log::error!("[CLI-Hub] 解析 installed.json 失败: {}", e);
                }
            }
        } else {
            log::info!("[CLI-Hub] 未找到已安装记录，这是首次运行");
        }
    }

    /// 保存已安装状态
    fn save_installed(&self) {
        log::info!("[CLI-Hub] 保存已安装状态到: {:?}", self.installed_file);
        if let Some(parent) = self.installed_file.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                log::error!("[CLI-Hub] 创建安装记录目录失败: {}", e);
                return;
            }
            log::debug!("[CLI-Hub] 安装记录目录: {:?}", parent);
        }
        match serde_json::to_string_pretty(&self.installed) {
            Ok(json) => {
                match fs::write(&self.installed_file, &json) {
                    Ok(_) => log::info!("[CLI-Hub] 已安装状态保存成功 ({} 个 CLI, {} 字节)",
                        self.installed.len(), json.len()),
                    Err(e) => log::error!("[CLI-Hub] 写入安装记录失败: {}", e),
                }
            }
            Err(e) => log::error!("[CLI-Hub] 序列化安装记录失败: {}", e),
        }
    }

    // ============================================================
    // 查询方法
    // ============================================================

    /// 获取所有 CLI 条目
    pub fn all_entries(&self) -> &[CliEntry] {
        &self.entries
    }

    /// 按名称查找 CLI
    pub fn get_cli(&self, name: &str) -> Option<&CliEntry> {
        let name_lower = name.to_lowercase();
        self.entries.iter().find(|c| c.name.to_lowercase() == name_lower)
    }

    /// 搜索 CLI（按名称、描述、分类）
    pub fn search(&self, query: &str) -> Vec<&CliEntry> {
        let query_lower = query.to_lowercase();
        self.entries
            .iter()
            .filter(|c| {
                c.name.to_lowercase().contains(&query_lower)
                    || c.display_name.to_lowercase().contains(&query_lower)
                    || c.description.to_lowercase().contains(&query_lower)
                    || c.category.to_lowercase().contains(&query_lower)
            })
            .collect()
    }

    /// 按分类列出 CLI
    pub fn list_by_category(&self, category: &str) -> Vec<&CliEntry> {
        let cat_lower = category.to_lowercase();
        self.entries
            .iter()
            .filter(|c| c.category.to_lowercase() == cat_lower)
            .collect()
    }

    /// 获取所有分类
    pub fn categories(&self) -> Vec<String> {
        let mut cats: Vec<String> = self
            .entries
            .iter()
            .map(|c| c.category.clone())
            .collect();
        cats.sort();
        cats.dedup();
        cats
    }

    /// 按来源筛选
    pub fn by_source(&self, source: &str) -> Vec<&CliEntry> {
        self.entries
            .iter()
            .filter(|c| c._source == source)
            .collect()
    }

    /// 检查 CLI 是否已安装
    pub fn is_installed(&self, name: &str) -> bool {
        let name_lower = name.to_lowercase();
        self.installed
            .keys()
            .any(|k| k.to_lowercase() == name_lower)
            || self.installed
                .values()
                .any(|v| v.name.to_lowercase() == name_lower)
    }

    /// 获取已安装 CLI 列表
    pub fn installed_list(&self) -> Vec<&InstalledCli> {
        self.installed.values().collect()
    }

    /// 根据软件扫描结果推荐相关 CLI
    pub fn recommend_for_software(&self, software_names: &[String]) -> Vec<CliEntry> {
        let mut recommendations = Vec::new();
        for sw_name in software_names {
            let sw_lower = sw_name.to_lowercase();
            for entry in &self.entries {
                let entry_lower = entry.name.to_lowercase();
                let display_lower = entry.display_name.to_lowercase();
                if sw_lower.contains(&entry_lower)
                    || entry_lower.contains(&sw_lower)
                    || sw_lower.contains(&display_lower)
                    || display_lower.contains(&sw_lower)
                {
                    if !recommendations.iter().any(|r: &CliEntry| r.name == entry.name) {
                        recommendations.push(entry.clone());
                    }
                }
            }
        }
        recommendations
    }

    // ============================================================
    // 安装 / 卸载方法
    // ============================================================

    /// 安装 CLI
    pub fn install(&mut self, name: &str) -> Result<String, String> {
        log::info!("[CLI-Hub] ========== 开始安装 CLI: '{}' ==========", name);

        let cli = self.get_cli(name)
            .ok_or_else(|| {
                log::error!("[CLI-Hub] CLI '{}' 未在注册表中找到", name);
                format!("CLI '{}' 未在注册表中找到", name)
            })?
            .clone();

        log::info!("[CLI-Hub] CLI 信息: name={}, display_name={}, version={}, category={}",
            cli.name, cli.display_name, cli.version, cli.category);
        log::info!("[CLI-Hub] 来源: {}, 入口点: {:?}", cli._source, cli.entry_point);

        if self.is_installed(name) {
            log::info!("[CLI-Hub] {} 已经安装，跳过", cli.display_name);
            return Ok(format!("{} 已经安装", cli.display_name));
        }

        let install_cmd = cli.install_cmd.as_deref().unwrap_or("");
        let source = cli._source.clone();
        let strategy = cli.install_strategy.clone().unwrap_or_else(|| {
            if source == "harness" {
                "pip".to_string()
            } else {
                "command".to_string()
            }
        });

        log::info!("[CLI-Hub] 安装策略: {}", strategy);
        log::info!("[CLI-Hub] 安装命令: {}", if install_cmd.is_empty() { "(无)" } else { install_cmd });
        log::info!("[CLI-Hub] 包管理器: {:?}", cli.package_manager);
        log::info!("[CLI-Hub] npm 包名: {:?}", cli.npm_package);
        if let Some(ref notes) = cli.install_notes {
            log::info!("[CLI-Hub] 安装备注: {}", notes);
        }

        let install_start = std::time::Instant::now();
        let result = match strategy.as_str() {
            "pip" => self.pip_install(&cli, install_cmd),
            "npm" => self.npm_install(&cli, install_cmd),
            "command" => self.command_install(&cli, install_cmd),
            "bundled" => self.bundled_install(&cli),
            "uv" => self.uv_install(&cli, install_cmd),
            _ => {
                log::warn!("[CLI-Hub] 未知安装策略 '{}'，回退到 command", strategy);
                self.command_install(&cli, install_cmd)
            }
        };
        let install_elapsed = install_start.elapsed();

        match result {
            Ok(msg) => {
                // 记录安装状态
                let installed = InstalledCli {
                    name: cli.name.clone(),
                    display_name: cli.display_name.clone(),
                    version: cli.version.clone(),
                    installed_at: chrono::Utc::now().to_rfc3339(),
                    source: source.clone(),
                    install_strategy: strategy.clone(),
                    entry_point: cli.entry_point.clone(),
                };
                self.installed.insert(cli.name.clone(), installed);
                self.save_installed();
                log::info!("[CLI-Hub] {} 安装成功 (耗时 {:.2}s)", cli.display_name, install_elapsed.as_secs_f64());
                log::info!("[CLI-Hub] ========== 安装完成 ==========");
                Ok(msg)
            }
            Err(e) => {
                log::error!("[CLI-Hub] {} 安装失败 (耗时 {:.2}s): {}", cli.display_name, install_elapsed.as_secs_f64(), e);
                log::error!("[CLI-Hub] ========== 安装失败 ==========");
                Err(e)
            }
        }
    }

    /// 卸载 CLI
    pub fn uninstall(&mut self, name: &str) -> Result<String, String> {
        log::info!("[CLI-Hub] ========== 开始卸载 CLI: '{}' ==========", name);

        let cli = self.get_cli(name)
            .ok_or_else(|| {
                log::error!("[CLI-Hub] CLI '{}' 未在注册表中找到", name);
                format!("CLI '{}' 未在注册表中找到", name)
            })?
            .clone();

        log::info!("[CLI-Hub] CLI 信息: name={}, display_name={}, version={}",
            cli.name, cli.display_name, cli.version);

        if !self.is_installed(name) {
            log::info!("[CLI-Hub] {} 未安装，无需卸载", cli.display_name);
            return Ok(format!("{} 未安装", cli.display_name));
        }

        let uninstall_cmd = cli.uninstall_cmd.as_deref().unwrap_or("");
        let strategy = cli.install_strategy.clone().unwrap_or_else(|| {
            if cli._source == "harness" { "pip".to_string() } else { "command".to_string() }
        });

        log::info!("[CLI-Hub] 卸载策略: {}", strategy);
        log::info!("[CLI-Hub] 卸载命令: {}", if uninstall_cmd.is_empty() { "(无)" } else { uninstall_cmd });

        let uninstall_start = std::time::Instant::now();
        let result = match strategy.as_str() {
            "pip" => {
                let pkg = format!("cli-anything-{}", cli.name);
                log::info!("[CLI-Hub] pip 卸载包: {}", pkg);
                self.run_command(&format!("pip uninstall -y {}", pkg))
            }
            "npm" => {
                let pkg = cli.npm_package.as_deref().unwrap_or("");
                log::info!("[CLI-Hub] npm 卸载包: {}", pkg);
                self.run_command(&format!("npm uninstall -g {}", pkg))
            }
            "command" => {
                if uninstall_cmd.is_empty() {
                    Err(format!("{} 没有定义卸载命令", cli.display_name))
                } else {
                    self.run_command(uninstall_cmd)
                }
            }
            "bundled" => {
                log::info!("[CLI-Hub] {} 是内置工具，跳过程序卸载", cli.display_name);
                Ok(format!("{} 是内置工具，与其父应用一起管理", cli.display_name))
            }
            _ => {
                if uninstall_cmd.is_empty() {
                    Err(format!("{} 没有定义卸载命令", cli.display_name))
                } else {
                    self.run_command(uninstall_cmd)
                }
            }
        };
        let uninstall_elapsed = uninstall_start.elapsed();

        match result {
            Ok(msg) => {
                self.installed.remove(&cli.name);
                self.save_installed();
                log::info!("[CLI-Hub] {} 卸载成功 (耗时 {:.2}s)", cli.display_name, uninstall_elapsed.as_secs_f64());
                log::info!("[CLI-Hub] ========== 卸载完成 ==========");
                Ok(msg)
            }
            Err(e) => {
                log::error!("[CLI-Hub] {} 卸载失败 (耗时 {:.2}s): {}", cli.display_name, uninstall_elapsed.as_secs_f64(), e);
                log::error!("[CLI-Hub] ========== 卸载失败 ==========");
                Err(e)
            }
        }
    }

    // ============================================================
    // 安装策略实现
    // ============================================================

    fn pip_install(&self, cli: &CliEntry, install_cmd: &str) -> Result<String, String> {
        let cmd = if install_cmd.contains("pip install") {
            install_cmd.to_string()
        } else {
            format!("pip install {}", install_cmd)
        };
        log::info!("[CLI-Hub] pip 安装: {}", cmd);
        self.run_command(&cmd)
            .map(|_output| {
                log::info!("[CLI-Hub] pip 安装成功: {} (v{})", cli.display_name, cli.version);
                format!("{} (v{}) 安装成功", cli.display_name, cli.version)
            })
            .map_err(|e| {
                log::error!("[CLI-Hub] pip 安装失败: {}", e);
                e
            })
    }

    fn npm_install(&self, cli: &CliEntry, install_cmd: &str) -> Result<String, String> {
        let pkg = cli.npm_package.as_deref().unwrap_or(install_cmd);
        let cmd = if install_cmd.contains("npm install") {
            install_cmd.to_string()
        } else {
            format!("npm install -g {}", pkg)
        };
        log::info!("[CLI-Hub] npm 安装: {}", cmd);
        self.run_command(&cmd)
            .map(|_output| {
                log::info!("[CLI-Hub] npm 安装成功: {} (v{})", cli.display_name, cli.version);
                format!("{} (v{}) 安装成功", cli.display_name, cli.version)
            })
            .map_err(|e| {
                log::error!("[CLI-Hub] npm 安装失败: {}", e);
                e
            })
    }

    fn command_install(&self, cli: &CliEntry, install_cmd: &str) -> Result<String, String> {
        if install_cmd.is_empty() {
            log::error!("[CLI-Hub] {} 没有定义安装命令", cli.display_name);
            return Err(format!("{} 没有定义安装命令", cli.display_name));
        }
        log::info!("[CLI-Hub] 命令安装: {}", install_cmd);
        self.run_command(install_cmd)
            .map(|_output| {
                log::info!("[CLI-Hub] 命令安装成功: {} (v{})", cli.display_name, cli.version);
                format!("{} (v{}) 安装成功", cli.display_name, cli.version)
            })
            .map_err(|e| {
                log::error!("[CLI-Hub] 命令安装失败: {}", e);
                e
            })
    }

    fn uv_install(&self, cli: &CliEntry, install_cmd: &str) -> Result<String, String> {
        let cmd = if install_cmd.contains("uv ") {
            install_cmd.to_string()
        } else {
            format!("uv pip install {}", install_cmd)
        };
        log::info!("[CLI-Hub] uv 安装: {}", cmd);
        self.run_command(&cmd)
            .map(|_output| {
                log::info!("[CLI-Hub] uv 安装成功: {} (v{})", cli.display_name, cli.version);
                format!("{} (v{}) 安装成功", cli.display_name, cli.version)
            })
            .map_err(|e| {
                log::error!("[CLI-Hub] uv 安装失败: {}", e);
                e
            })
    }

    fn bundled_install(&self, cli: &CliEntry) -> Result<String, String> {
        let note = cli.install_notes.as_deref().unwrap_or("");
        if note.is_empty() {
            log::info!("[CLI-Hub] {} 是内置工具，无需安装", cli.display_name);
            Ok(format!("{} 是内置工具，无需安装", cli.display_name))
        } else {
            log::info!("[CLI-Hub] 内置工具 {}: {}", cli.display_name, note);
            Ok(format!("{}: {}", cli.display_name, note))
        }
    }

    // ============================================================
    // 命令执行
    // ============================================================

    fn run_command(&self, cmd_str: &str) -> Result<String, String> {
        let start = std::time::Instant::now();
        log::info!("[CLI-Hub] ┌─ 执行命令 ─────────────────────");
        log::info!("[CLI-Hub] │ 命令: {}", cmd_str);
        log::info!("[CLI-Hub] │ 平台: {}", if cfg!(target_os = "windows") { "Windows" } else { "Unix" });

        let output = if cfg!(target_os = "windows") {
            Command::new("cmd")
                .args(["/C", cmd_str])
                .output()
        } else {
            Command::new("sh")
                .args(["-c", cmd_str])
                .output()
        };

        let elapsed = start.elapsed();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let exit_code = out.status.code().unwrap_or(-1);

                log::info!("[CLI-Hub] │ 退出码: {}", exit_code);
                log::info!("[CLI-Hub] │ 耗时: {:.2}ms", elapsed.as_secs_f64() * 1000.0);
                log::info!("[CLI-Hub] │ stdout: {} 字符", stdout.len());
                log::info!("[CLI-Hub] │ stderr: {} 字符", stderr.len());

                if out.status.success() {
                    log::info!("[CLI-Hub] └─ 执行成功 ✓");
                    if !stderr.is_empty() {
                        log::debug!("[CLI-Hub]    stderr (非致命): {}", stderr);
                    }
                    Ok(stdout)
                } else {
                    let err_msg = if !stderr.is_empty() { stderr.clone() } else { stdout.clone() };
                    log::error!("[CLI-Hub] └─ 执行失败 ✗");
                    log::error!("[CLI-Hub]    错误: {}", err_msg);
                    Err(err_msg)
                }
            }
            Err(e) => {
                log::error!("[CLI-Hub] └─ 执行异常 ✗");
                log::error!("[CLI-Hub]    异常: {}", e);
                log::error!("[CLI-Hub]    类型: {:?}", e.kind());
                Err(format!("命令执行失败: {}", e))
            }
        }
    }

    /// 执行已安装 CLI 的命令
    pub fn execute_cli_command(&self, cli_name: &str, args: &[&str]) -> Result<String, String> {
        log::info!("[CLI-Hub] ========== 执行 CLI 命令 ==========");
        log::info!("[CLI-Hub] CLI 名称: {}", cli_name);
        log::info!("[CLI-Hub] 参数: {:?}", args);

        let installed = self.installed.get(cli_name)
            .ok_or_else(|| {
                log::error!("[CLI-Hub] CLI '{}' 未安装，无法执行", cli_name);
                format!("CLI '{}' 未安装", cli_name)
            })?;

        log::info!("[CLI-Hub] 已安装信息: version={}, source={}, entry_point={:?}",
            installed.version, installed.source, installed.entry_point);

        let entry = installed.entry_point.as_deref().unwrap_or(cli_name);
        let full_cmd = std::iter::once(entry)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");

        log::info!("[CLI-Hub] 完整命令: {}", full_cmd);
        log::info!("[CLI-Hub] ========== 开始执行 ==========");

        let result = self.run_command(&full_cmd);

        match &result {
            Ok(_) => log::info!("[CLI-Hub] CLI 命令执行成功"),
            Err(e) => log::error!("[CLI-Hub] CLI 命令执行失败: {}", e),
        }

        result
    }
}

/// 获取用户主目录（跨平台辅助函数）
fn dirs_next() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .ok()
}