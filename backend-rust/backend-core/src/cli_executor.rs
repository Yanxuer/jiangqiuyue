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
    let mut req = CLIRequest {
        command: command.to_string(),
        cwd: None,
        safe: true,
        reason: None,
        affected_files: Vec::new(),
        operation_type: None,
    };

    let cmd = extract_primary_command(command);

    let findings = scan_dangerous_patterns(command);
    if !findings.is_empty() {
        req.safe = false;
        req.reason = Some(findings.join("; "));
        return req;
    }

    if BLOCKED_COMMANDS.contains(&cmd.as_str()) {
        req.safe = false;
        req.reason = Some(format!("命令 '{}' 被列入黑名单", cmd));
        return req;
    }

    req.operation_type = Some(classify_operation(command));
    req.affected_files = extract_affected_paths(command);
    req
}

pub async fn execute_command(command: &str, cwd: Option<&str>) -> CLIResult {
    let cwd = cwd.unwrap_or(".");
    let result = Command::new("cmd")
        .args(["/C", command])
        .current_dir(cwd)
        .output()
        .await;

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let mut combined = stdout;
            if !stderr.is_empty() {
                combined.push_str(&format!("\n[STDERR]\n{}", stderr));
            }
            CLIResult {
                success: output.status.success(),
                exit_code: output.status.code().unwrap_or(-1),
                output: if combined.len() > 5000 {
                    combined[..5000].to_string()
                } else if combined.is_empty() {
                    "(无输出)".to_string()
                } else {
                    combined
                },
            }
        }
        Err(e) => CLIResult {
            success: false,
            exit_code: -1,
            output: format!("[-] 执行失败: {}", e),
        },
    }
}

fn scan_dangerous_patterns(command: &str) -> Vec<String> {
    let mut findings = Vec::new();
    for pattern in DANGEROUS_PATTERNS {
        if let Ok(re) = Regex::new(pattern) {
            if re.is_match(command) {
                findings.push(format!("匹配危险模式: {}", pattern));
            }
        }
    }
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
                    paths.push(abs.display().to_string());
                }
            }
        }
        if part.contains('/') || part.contains('\\') || part.contains(':') {
            if let Ok(expanded) = std::fs::canonicalize(part) {
                paths.push(expanded.display().to_string());
            }
        }
    }

    paths.sort();
    paths.dedup();
    paths
}

fn classify_operation(command: &str) -> String {
    let cmd = extract_primary_command(command);

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
        return format!("[!] {}", desc);
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
        return format!("[+] {}", desc);
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
        return format!("[i] {}", desc);
    }

    if command.contains('>') || command.contains(">>") || command.contains('|') {
        return "[+] 写入/管道操作".to_string();
    }

    format!("[*] 执行命令: {}", cmd)
}