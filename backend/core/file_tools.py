import os
from pathlib import Path
from core.config import WORKSPACE

def safe_path(filename: str) -> Path:
    """防止目录遍历攻击"""
    target = Path(WORKSPACE) / filename
    target = target.resolve()
    if not str(target).startswith(str(Path(WORKSPACE).resolve())):
        raise ValueError(f"非法路径: {filename}")
    return target

def read_file(filename: str) -> str:
    path = safe_path(filename)
    if not path.exists():
        return f"[错误] 文件不存在: {filename}"
    try:
        return path.read_text(encoding='utf-8')
    except Exception as e:
        return f"[错误] 读取失败: {str(e)}"

def write_file(filename: str, content: str) -> str:
    path = safe_path(filename)
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding='utf-8')
        return f"[成功] 已保存: {filename}"
    except Exception as e:
        return f"[错误] 写入失败: {str(e)}"

def list_files(subdir: str = "") -> str:
    path = safe_path(subdir) if subdir else Path(WORKSPACE)
    if not path.exists():
        return "[]"
    files = [str(p.relative_to(WORKSPACE)) for p in path.rglob("*") if p.is_file()]
    return "\n".join(files) or "(空目录)"