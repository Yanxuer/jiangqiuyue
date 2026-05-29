import subprocess
import re
import os
import shlex
from pathlib import Path

DANGEROUS_PATTERNS = [
    r'\brm\s+-rf\b',
    r'\brm\s+-r\s+-f\b',
    r'\bsudo\b',
    r'\bdel\s+/[fFsSqQ]',
    r'\bformat\b',
    r'\bmkfs\b',
    r'\bdd\b',
    r'\bshutdown\b',
    r'\breboot\b',
    r'\bpoweroff\b',
    r'\binit\b',
    r'\bdestroy\b',
    r'\bwipe\b',
    r'\bdiskpart\b',
    r'\bfdisk\b',
    r'\bmkfs\.\w+',
    r'\bchmod\s+777\b',
    r'\bchown\b',
    r'\bpasswd\b',
    r'\buseradd\b',
    r'\buserdel\b',
    r'\breg\s+delete\b',
    r'\breg\s+add\b',
    r'taskkill\s+/f\b',
    r'\b>?\s*/dev/sd[a-z]',
    r':\(\)\{\s*:\|\:&\s*\};',
    r'\bmv\s+/\s+',
    r'\bcp\s+/\s+',
    # Windows 危险命令补充
    r'\brd\s+/\w*[sq]',
    r'\brmdir\s+/\w*[sq]',
    r'\btakeown\b',
    r'\bicacls\b',
    r'\bcacls\b',
    r'\bvssadmin\b',
    r'\bwevtutil\s+cl\b',
    r'\bbcdedit\b',
    r'\bfsutil\b',
    r'\bmshta\b',
    r'\bcscript\s+.*\.vbs\b',
    r'\bwscript\b',
]

BLOCKED_COMMANDS = [
    'rm', 'sudo', 'del', 'format', 'mkfs', 'dd',
    'shutdown', 'reboot', 'poweroff', 'init', 'diskpart',
    'fdisk', 'chmod', 'chown', 'passwd', 'useradd', 'userdel',
    'reg', 'taskkill', 'wget', 'curl',
    # Windows 危险命令补充
    'rd', 'rmdir',
    'takeown',
    'icacls', 'cacls',
    'attrib',
    'bcdedit',
    'vssadmin',
    'wevtutil',
    'fsutil',
    'mshta',
    'cscript',
    'wscript',
]

class CLIRequest:
    def __init__(self, command: str, cwd: str = None):
        self.command = command.strip()
        self.cwd = cwd or os.getcwd()
        self.safe = True
        self.reason = None
        self.affected_files = []
        self.operation_type = None

    def __repr__(self):
        return f"CLIRequest(command={self.command!r}, safe={self.safe})"


def scan_dangerous_patterns(command: str) -> list:
    findings = []
    for pattern in DANGEROUS_PATTERNS:
        if re.search(pattern, command, re.IGNORECASE):
            findings.append(f"匹配危险模式: {pattern}")
    return findings


def extract_primary_command(command: str) -> str:
    parts = shlex.split(command)
    if not parts:
        return ""
    return parts[0].lower()


def extract_affected_paths(command: str) -> list:
    paths = []
    parts = shlex.split(command)
    for i, part in enumerate(parts):
        part_lower = part.lower()
        if part_lower in ('rm', 'del', 'copy', 'move', 'cp', 'mv', 'rename', 'ren'):
            if i + 1 < len(parts) and not parts[i+1].startswith('-'):
                paths.append(os.path.abspath(parts[i+1]))
        if part.startswith(('.', '/', '\\', '~')) or ':' in part:
            if os.path.isfile(part) or os.path.isdir(part) or os.path.exists(part):
                paths.append(os.path.abspath(part))
        if '/' in part or '\\' in part:
            possible = os.path.abspath(os.path.expanduser(part))
            if os.path.exists(possible):
                paths.append(possible)
    return list(set(paths))


def classify_operation(command: str) -> str:
    cmd = extract_primary_command(command)
    dangerous_ops = {
        'rm': '删除文件/目录',
        'del': '删除文件',
        'format': '格式化磁盘',
        'mkfs': '创建文件系统',
        'dd': '低级别磁盘操作',
        'shutdown': '关机',
        'reboot': '重启',
        'diskpart': '磁盘分区',
        'fdisk': '磁盘分区',
        'rd': '删除目录树',
        'rmdir': '删除目录',
        'takeown': '夺取文件所有权',
        'icacls': '修改文件权限',
        'cacls': '修改文件权限',
        'attrib': '修改文件属性',
        'bcdedit': '修改启动配置',
        'vssadmin': '卷影副本操作',
        'wevtutil': '事件日志操作',
        'fsutil': '文件系统工具',
        'mshta': '执行HTA脚本',
        'cscript': '执行脚本',
        'wscript': '执行脚本',
    }
    write_ops = {
        'echo': '写入内容',
        'copy': '复制文件',
        'cp': '复制文件',
        'move': '移动文件',
        'mv': '移动文件',
        'rename': '重命名',
        'ren': '重命名',
        'mkdir': '创建目录',
        'md': '创建目录',
    }
    read_ops = {
        'dir': '列出目录',
        'ls': '列出目录',
        'type': '查看文件内容',
        'cat': '查看文件内容',
        'find': '查找文件',
        'where': '查找文件',
        'echo': '回显内容',
        'pip': '安装Python包',
        'npm': '安装Node包',
    }

    if cmd in dangerous_ops:
        return f"[!] {dangerous_ops[cmd]}"
    if cmd in write_ops:
        return f"[+] {write_ops[cmd]}"
    if cmd in read_ops:
        return f"[i] {read_ops[cmd]}"
    if any(flag in command for flag in ('>', '>>', '|')):
        return "[+] 写入/管道操作"
    return f"[*] 执行命令: {cmd}"


def analyze_command(command: str) -> CLIRequest:
    req = CLIRequest(command)
    cmd = extract_primary_command(command)
    findings = scan_dangerous_patterns(command)
    if findings:
        req.safe = False
        req.reason = "; ".join(findings)
        return req
    if cmd in BLOCKED_COMMANDS:
        req.safe = False
        req.reason = f"命令 '{cmd}' 被列入黑名单"
        return req
    req.operation_type = classify_operation(command)
    req.affected_files = extract_affected_paths(command)
    return req


def execute_command(command: str, cwd: str = None) -> dict:
    try:
        result = subprocess.run(
            command,
            shell=True,
            cwd=cwd or os.getcwd(),
            capture_output=True,
            text=True,
            timeout=300
        )
        output = result.stdout
        if result.stderr:
            output += f"\n[STDERR]\n{result.stderr}"
        return {
            "success": result.returncode == 0,
            "exit_code": result.returncode,
            "output": output[:5000] if output else "(无输出)",
        }
    except subprocess.TimeoutExpired:
        return {"success": False, "exit_code": -1, "output": "[-] 执行超时 (5分钟)"}
    except Exception as e:
        return {"success": False, "exit_code": -1, "output": f"[!] 执行失败: {str(e)}"}
