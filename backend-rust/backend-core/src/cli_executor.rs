use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CLIRequest {
    pub command: String,
    pub cwd: Option<String>,
    pub safe: bool,
    pub reason: Option<String>,
    pub affected_files: Vec<String>,
    pub operation_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CLIResult {
    pub success: bool,
    pub exit_code: i32,
    pub output: String,
}

const DANGEROUS_PATTERNS: &[&str] = &[
    r"\brm\s+-rf\b",
    r"\bsudo\b",
    r"\bdel\s+/[fFsSqQ]",
    r"\bformat\b",
    r"\bmkfs\b",
    r"\bdd\b",
    r"\bshutdown\b",
    r"\breboot\b",
    r"\bpoweroff\b",
    r"\binit\b",
    r"\bdiskpart\b",
    r"\bfdisk\b",
    r"\bchmod\s+777\b",
    r"\bchown\b",
    r"\bpasswd\b",
    r"\buseradd\b",
    r"\buserdel\b",
    r"\breg\s+delete\b",
    r"\breg\s+add\b",
    r"taskkill\s+/f\b",
    r"\brd\s+/\w*[sq]",
    r"\brmdir\s+/\w*[sq]",
    r"\btakeown\b",
    r"\bicacls\b",
    r"\bcacls\b",
    r"\bvssadmin\b",
    r"\bwevtutil\s+cl\b",
    r"\bbcdedit\b",
    r"\bfsutil\b",
    r"\bmshta\b",
    r"\bcscript\s+.*\.vbs\b",
    r"\bwscript\b",
];

const BLOCKED_COMMANDS: &[&str] = &[
    "rm", "sudo", "del", "format", "mkfs", "dd", "shutdown", "reboot",
    "poweroff", "init", "diskpart", "fdisk", "chmod", "chown", "passwd",
    "useradd", "userdel", "reg", "taskkill", "rd", "rmdir", "takeown",
    "icacls", "cacls", "attrib", "bcdedit", "vssadmin", "wevtutil",
    "fsutil", "mshta", "cscript", "wscript",
];

pub fn analyze_command(command: &str) -> CLIRequest {
    log::info!("[CLI-Executor] ========== 开始分析命令 ==========");
    log::info!("[CLI-Executor] 原始命令: {}", command);
    log::info!("[CLI-Executor] 命令长度: {} 字符", command.len());

    // 命令分词
    let tokens: Vec<&str> = command.split_whitespace().collect();
    log::info!("[CLI-Executor] 命令分词: {} 个 token -> {:?}", tokens.len(), tokens);
    log::info!("[CLI-Executor] 第一段(主命令): '{}'", tokens.first().unwrap_or(&""));

    let mut req = CLIRequest {
        command: command.to_string(),
        cwd: None,
        safe: true,
        reason: None,
        affected_files: Vec::new(),
        operation_type: None,
    };

    let cmd = extract_primary_command(command);
    log::info!("[CLI-Executor] 提取主命令: '{}'", cmd);

    // 1. 危险模式扫描
    log::info!("[CLI-Executor] --- 步骤1: 危险模式扫描 (共{}个模式) ---", DANGEROUS_PATTERNS.len());
    let findings = scan_dangerous_patterns(command);
    if !findings.is_empty() {
        log::warn!("[CLI-Executor] ⚠ 发现 {} 个危险模式匹配", findings.len());
        for (i, f) in findings.iter().enumerate() {
            log::warn!("[CLI-Executor]   危险 #{}: {}", i + 1, f);
        }
        req.safe = false;
        req.reason = Some(findings.join("; "));
        log::warn!("[CLI-Executor] 命令被拦截: {}", req.reason.as_ref().unwrap());
        log::info!("[CLI-Executor] ========== 命令分析完成 (被拦截) ==========");
        return req;
    }
    log::info!("[CLI-Executor] 危险模式扫描: 通过 ✓");

    // 2. 黑名单检查
    log::info!("[CLI-Executor] --- 步骤2: 黑名单检查 (共{}个命令) ---", BLOCKED_COMMANDS.len());
    if BLOCKED_COMMANDS.contains(&cmd.as_str()) {
        log::warn!("[CLI-Executor] ⚠ 命令 '{}' 在黑名单中", cmd);
        req.safe = false;
        req.reason = Some(format!("命令 '{}' 被列入黑名单", cmd));
        log::info!("[CLI-Executor] ========== 命令分析完成 (被拦截) ==========");
        return req;
    }
    log::info!("[CLI-Executor] 黑名单检查: 通过 ✓");

    // 3. 操作分类
    log::info!("[CLI-Executor] --- 步骤3: 操作分类 ---");
    req.operation_type = Some(classify_operation(command));
    log::info!("[CLI-Executor] 操作类型: {}", req.operation_type.as_ref().unwrap());

    // 4. 提取受影响文件
    log::info!("[CLI-Executor] --- 步骤4: 提取受影响文件 ---");
    req.affected_files = extract_affected_paths(command);
    if !req.affected_files.is_empty() {
        log::info!("[CLI-Executor] 受影响文件 ({}个):", req.affected_files.len());
        for (i, f) in req.affected_files.iter().enumerate() {
            log::info!("[CLI-Executor]   文件 #{}: {}", i + 1, f);
        }
    } else {
        log::info!("[CLI-Executor] 未检测到受影响文件");
    }

    log::info!("[CLI-Executor] ========== 命令分析完成: safe={}, 类型={}, 受影响文件数={} ==========",
        req.safe, req.operation_type.as_ref().unwrap_or(&"未知".to_string()), req.affected_files.len());
    req
}

pub async fn execute_command(command: &str, cwd: Option<&str>) -> CLIResult {
    let cwd = cwd.unwrap_or(".");
    let start = std::time::Instant::now();

    log::info!("[CLI-Executor] ========== 开始执行命令 ==========");
    log::info!("[CLI-Executor] 命令: {}", command);
    log::info!("[CLI-Executor] 工作目录: {}", cwd);
    log::info!("[CLI-Executor] 平台: {}", if cfg!(target_os = "windows") { "Windows" } else { "Unix" });

    // 验证工作目录
    let cwd_path = std::path::Path::new(cwd);
    if !cwd_path.exists() {
        log::warn!("[CLI-Executor] ⚠ 工作目录不存在: {}", cwd);
    } else if !cwd_path.is_dir() {
        log::error!("[CLI-Executor] ✗ 工作目录路径不是目录: {}", cwd);
    } else {
        log::info!("[CLI-Executor] 工作目录验证: 存在 ✓");
    }

    // 构建执行命令
    let shell = if cfg!(target_os = "windows") { "cmd" } else { "sh" };
    let shell_arg = if cfg!(target_os = "windows") { "/C" } else { "-c" };
    log::info!("[CLI-Executor] 使用 Shell: {} {} \"{}\"", shell, shell_arg, command);

    log::info!("[CLI-Executor] 正在启动进程...");
    let result = Command::new(shell)
        .args([shell_arg, command])
        .current_dir(cwd)
        .output()
        .await;

    let elapsed = start.elapsed();
    log::info!("[CLI-Executor] 进程已结束, 耗时: {:.2}ms ({:.3}s)",
        elapsed.as_secs_f64() * 1000.0, elapsed.as_secs_f64());

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);

            log::info!("[CLI-Executor] ───── 执行结果摘要 ─────");
            log::info!("[CLI-Executor] 退出码: {}", exit_code);
            log::info!("[CLI-Executor] 执行耗时: {:.2}ms", elapsed.as_secs_f64() * 1000.0);
            log::info!("[CLI-Executor] stdout 大小: {} 字节 ({} 行)",
                output.stdout.len(),
                stdout.lines().count());
            log::info!("[CLI-Executor] stderr 大小: {} 字节 ({} 行)",
                output.stderr.len(),
                stderr.lines().count());

            if output.status.success() {
                log::info!("[CLI-Executor] 状态: 成功 ✓");
                if !stderr.is_empty() {
                    log::warn!("[CLI-Executor] ⚠ stderr 有输出 (非致命, {} 字节)", stderr.len());
                    // 输出 stderr 前 500 字符用于诊断
                    let stderr_preview = if stderr.len() > 500 { &stderr[..500] } else { &stderr };
                    log::warn!("[CLI-Executor] stderr 预览: {}", stderr_preview);
                }
            } else {
                log::error!("[CLI-Executor] 状态: 失败 ✗ (exit={})", exit_code);
                if !stderr.is_empty() {
                    log::error!("[CLI-Executor] stderr: {}", stderr);
                }
                if !stdout.is_empty() {
                    log::error!("[CLI-Executor] stdout (失败时): {}", stdout);
                }
            }

            let mut combined = stdout;
            if !stderr.is_empty() {
                combined.push_str(&format!("\n[STDERR]\n{}", stderr));
            }

            let final_output = if combined.len() > 5000 {
                log::info!("[CLI-Executor] 输出截断: {} -> 5000 字符 (截去 {} 字符)",
                    combined.len(), combined.len() - 5000);
                combined[..5000].to_string()
            } else if combined.is_empty() {
                log::info!("[CLI-Executor] 输出为空 (无stdout/stderr)");
                "(无输出)".to_string()
            } else {
                combined
            };

            log::info!("[CLI-Executor] 最终输出大小: {} 字符", final_output.len());
            log::info!("[CLI-Executor] ========== 命令执行完成 ==========");
            CLIResult {
                success: output.status.success(),
                exit_code,
                output: final_output,
            }
        }
        Err(e) => {
            log::error!("[CLI-Executor] ───── 执行异常 ─────");
            log::error!("[CLI-Executor] 错误信息: {}", e);
            log::error!("[CLI-Executor] 错误类型: {:?}", e.kind());
            log::error!("[CLI-Executor] 执行耗时: {:.2}ms (异常终止)", elapsed.as_secs_f64() * 1000.0);
            log::error!("[CLI-Executor] 原始命令: {}", command);
            log::error!("[CLI-Executor] 工作目录: {}", cwd);
            log::info!("[CLI-Executor] ========== 命令执行失败 (异常) ==========");
            CLIResult {
                success: false,
                exit_code: -1,
                output: format!("[-] 执行失败: {}", e),
            }
        }
    }
}

fn scan_dangerous_patterns(command: &str) -> Vec<String> {
    let mut findings = Vec::new();
    let mut patterns_checked = 0;
    for pattern in DANGEROUS_PATTERNS {
        if let Ok(re) = Regex::new(pattern) {
            patterns_checked += 1;
            if re.is_match(command) {
                let matched = re.find(command).map(|m| m.as_str().to_string()).unwrap_or_default();
                log::warn!("[CLI-Executor] 危险模式命中: '{}' -> 匹配段: '{}'", pattern, matched);
                findings.push(format!("匹配危险模式: {} (命中: {})", pattern, matched));
            }
        }
    }
    log::debug!("[CLI-Executor] 危险模式扫描: 检查了 {} 个模式, 命中 {} 个", patterns_checked, findings.len());
    findings
}

fn extract_primary_command(command: &str) -> String {
    let parts: Vec<&str> = command.split_whitespace().collect();
    parts.first().unwrap_or(&"").to_lowercase()
}

fn extract_affected_paths(command: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let parts: Vec<&str> = command.split_whitespace().collect();

    for (i, part) in parts.iter().enumerate() {
        let lower = part.to_lowercase();
        if matches!(lower.as_str(), "rm" | "del" | "copy" | "move" | "cp" | "mv" | "rename" | "ren") {
            if i + 1 < parts.len() && !parts[i + 1].starts_with('-') {
                if let Ok(abs) = std::fs::canonicalize(parts[i + 1]) {
                    log::debug!("[CLI-Executor] 文件操作路径: {} -> {:?}", parts[i + 1], abs);
                    paths.push(abs.display().to_string());
                }
            }
        }
        if part.contains('/') || part.contains('\\') || part.contains(':') {
            if let Ok(expanded) = std::fs::canonicalize(part) {
                log::debug!("[CLI-Executor] 检测到路径参数: {} -> {:?}", part, expanded);
                paths.push(expanded.display().to_string());
            }
        }
    }

    paths.sort();
    paths.dedup();
    log::debug!("[CLI-Executor] 提取受影响文件: {} 个", paths.len());
    paths
}

fn classify_operation(command: &str) -> String {
    let cmd = extract_primary_command(command);
    log::debug!("[CLI-Executor] 分类操作: 主命令='{}', 完整命令='{}'", cmd, command);

    let dangerous_ops = [
        ("rm", "删除文件/目录"),
        ("del", "删除文件"),
        ("format", "格式化磁盘"),
        ("mkfs", "创建文件系统"),
        ("dd", "低级别磁盘操作"),
        ("shutdown", "关机"),
        ("reboot", "重启"),
        ("diskpart", "磁盘分区"),
        ("fdisk", "磁盘分区"),
        ("rd", "删除目录树"),
        ("rmdir", "删除目录"),
        ("takeown", "夺取文件所有权"),
        ("icacls", "修改文件权限"),
        ("cacls", "修改文件权限"),
        ("vssadmin", "卷影副本操作"),
        ("wevtutil", "事件日志操作"),
        ("fsutil", "文件系统工具"),
        ("mshta", "执行HTA脚本"),
        ("cscript", "执行脚本"),
        ("wscript", "执行脚本"),
    ];

    if let Some(&(_, desc)) = dangerous_ops.iter().find(|&&(name, _)| name == cmd) {
        let result = format!("[!] {}", desc);
        log::warn!("[CLI-Executor] 操作分类: 危险操作 -> {}", result);
        return result;
    }

    let write_ops = [
        ("echo", "写入内容"),
        ("copy", "复制文件"),
        ("cp", "复制文件"),
        ("move", "移动文件"),
        ("mv", "移动文件"),
        ("rename", "重命名"),
        ("ren", "重命名"),
        ("mkdir", "创建目录"),
        ("md", "创建目录"),
    ];

    if let Some(&(_, desc)) = write_ops.iter().find(|&&(name, _)| name == cmd) {
        let result = format!("[+] {}", desc);
        log::info!("[CLI-Executor] 操作分类: 写入操作 -> {}", result);
        return result;
    }

    let read_ops = [
        ("dir", "列出目录"),
        ("ls", "列出目录"),
        ("type", "查看文件内容"),
        ("cat", "查看文件内容"),
        ("where", "查找文件"),
        ("pip", "安装Python包"),
        ("npm", "安装Node包"),
    ];

    if let Some(&(_, desc)) = read_ops.iter().find(|&&(name, _)| name == cmd) {
        let result = format!("[i] {}", desc);
        log::info!("[CLI-Executor] 操作分类: 读取/安装操作 -> {}", result);
        return result;
    }

    if command.contains('>') || command.contains(">>") || command.contains('|') {
        let result = "[+] 写入/管道操作".to_string();
        log::info!("[CLI-Executor] 操作分类: 管道/重定向 -> {}", result);
        return result;
    }

    let result = format!("[*] 执行命令: {}", cmd);
    log::info!("[CLI-Executor] 操作分类: 通用命令 -> {}", result);
    result
}