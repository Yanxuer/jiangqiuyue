import os
import json
from pathlib import Path
from datetime import datetime
from core.config import MEMORY_PATH

RECENT_PATHS_FILE = os.path.join(MEMORY_PATH, "recent_paths.json")
MAX_RECENT_PATHS = 10

SYSTEM_BLOCKED_PREFIXES = [
    os.path.abspath(os.environ.get("SystemRoot", "C:\\Windows")).lower(),
    os.path.abspath("C:\\Windows").lower(),
    os.path.abspath("C:\\Program Files").lower(),
    os.path.abspath("C:\\Program Files (x86)").lower(),
    os.path.abspath("C:\\ProgramData").lower(),
    os.path.abspath("C:\\System Volume Information").lower(),
    os.path.abspath("C:\\$Recycle.Bin").lower(),
    os.path.abspath("C:\\Recovery").lower(),
    os.path.abspath("C:\\Boot").lower(),
    os.path.abspath("C:\\boot").lower(),
]

TEXT_EXTENSIONS = {
    ".txt", ".md", ".py", ".js", ".jsx", ".ts", ".tsx", ".html", ".htm",
    ".css", ".scss", ".less", ".json", ".xml", ".yaml", ".yml", ".toml",
    ".ini", ".cfg", ".conf", ".log", ".csv", ".env", ".bat", ".sh", ".ps1",
    ".java", ".cpp", ".c", ".h", ".hpp", ".cs", ".go", ".rs", ".rb", ".php",
    ".swift", ".kt", ".kts", ".vue", ".svelte", ".sql", ".r", ".m", ".mm",
    ".dockerfile", ".makefile", ".gradle", ".cmake",
}

SENSITIVE_KEYWORDS = [
    "password", "credential", "secret", "token", "key", "private",
    ".env", "id_rsa", "id_dsa", "id_ecdsa", "id_ed25519",
]


def _validate_path(path: str) -> str:
    abs_path = os.path.abspath(path)
    abs_lower = abs_path.lower()
    for prefix in SYSTEM_BLOCKED_PREFIXES:
        if abs_lower.startswith(prefix):
            raise PermissionError(f"禁止访问系统目录: {path}")
    return abs_path


def _is_sensitive_file(path: str) -> bool:
    name_lower = os.path.basename(path).lower()
    for keyword in SENSITIVE_KEYWORDS:
        if keyword in name_lower:
            return True
    return False


def load_recent_paths() -> list:
    if not os.path.exists(RECENT_PATHS_FILE):
        return []
    try:
        with open(RECENT_PATHS_FILE, "r", encoding="utf-8") as f:
            return json.load(f)
    except (json.JSONDecodeError, Exception):
        return []


def save_recent_paths(paths: list):
    os.makedirs(MEMORY_PATH, exist_ok=True)
    with open(RECENT_PATHS_FILE, "w", encoding="utf-8") as f:
        json.dump(paths, f, ensure_ascii=False, indent=2)


def add_recent_path(path: str) -> list:
    paths = load_recent_paths()
    normalized = os.path.abspath(path)
    paths = [p for p in paths if os.path.abspath(p.get("path", "")) != normalized]
    paths.insert(0, {
        "path": normalized,
        "name": os.path.basename(normalized) or normalized,
        "time": datetime.now().isoformat(),
    })
    paths = paths[:MAX_RECENT_PATHS]
    save_recent_paths(paths)
    return paths


def get_recent_paths() -> list:
    return load_recent_paths()


def list_directory(path: str) -> dict:
    try:
        path = _validate_path(path)
    except PermissionError as e:
        return {"success": False, "error": str(e)}

    if not os.path.exists(path):
        return {"success": False, "error": f"路径不存在: {path}"}
    if not os.path.isdir(path):
        return {"success": False, "error": f"不是目录: {path}"}

    files = []
    dirs = []
    try:
        for entry in sorted(os.scandir(path), key=lambda e: (not e.is_dir(), e.name.lower())):
            if entry.name.startswith("."):
                continue
            if entry.is_dir():
                dirs.append({"name": entry.name, "type": "directory"})
            elif entry.is_file():
                ext = os.path.splitext(entry.name)[1].lower()
                is_sensitive = _is_sensitive_file(entry.path)
                files.append({
                    "name": entry.name,
                    "type": "file",
                    "size": entry.stat().st_size,
                    "is_text": ext in TEXT_EXTENSIONS and not is_sensitive,
                })
    except PermissionError:
        return {"success": False, "error": f"无权限访问: {path}"}

    return {
        "success": True,
        "path": os.path.abspath(path),
        "name": os.path.basename(path) or path,
        "directories": dirs,
        "files": files,
        "total": len(dirs) + len(files),
    }


FILE_SIZE_LIMIT = 1024 * 1024


def read_text_file(path: str) -> dict:
    try:
        path = _validate_path(path)
    except PermissionError as e:
        return {"success": False, "error": str(e)}

    if not os.path.exists(path):
        return {"success": False, "error": f"文件不存在: {path}"}
    if not os.path.isfile(path):
        return {"success": False, "error": f"不是文件: {path}"}
    if _is_sensitive_file(path):
        return {"success": False, "error": "禁止读取敏感文件"}

    ext = os.path.splitext(path)[1].lower()
    if ext not in TEXT_EXTENSIONS:
        return {"success": False, "error": f"不支持的文件类型: {ext}", "ext": ext}

    file_size = os.path.getsize(path)
    if file_size > FILE_SIZE_LIMIT:
        return {"success": False, "error": f"文件过大 (>{FILE_SIZE_LIMIT//1024}KB)，无法直接读取", "size": file_size}

    try:
        with open(path, "r", encoding="utf-8", errors="replace") as f:
            content = f.read()
        return {
            "success": True,
            "path": os.path.abspath(path),
            "name": os.path.basename(path),
            "content": content,
            "size": file_size,
            "lines": content.count("\n") + 1,
        }
    except Exception as e:
        return {"success": False, "error": f"读取失败: {str(e)}"}