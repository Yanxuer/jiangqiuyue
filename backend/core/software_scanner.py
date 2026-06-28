import os
import re
import json
import subprocess
import winreg
from pathlib import Path
from datetime import datetime
from core.config import MEMORY_PATH

SOFTWARE_CACHE_FILE = os.path.join(MEMORY_PATH, "software_cache.json")
SOFTWARE_SCAN_FLAG = os.path.join(MEMORY_PATH, ".software_scanned")

CATEGORY_KEYWORDS = {
    "browser": ["browser", "chrome", "edge", "firefox", "safari", "opera", "chromium", "brave", "iexplore", "internet"],
    "editor": ["code", "sublime", "vim", "neovim", "notepad", "emacs", "atom", "brackets", "text", "ide"],
    "terminal": ["terminal", "cmd", "powershell", "windowsterminal", "conemu", "cmder", "hyper", "alacritty", "putty", "ssh", "warp"],
    "office": ["word", "excel", "powerpoint", "outlook", "onenote", "access", "libreoffice", "wps", "office"],
    "image": ["photoshop", "gimp", "krita", "paint", "illustrator", "figma", "sketch", "lightroom", "canvas", "photo", "draw", "inkscape", "corel"],
    "video": ["premiere", "after effects", "davinci", "final cut", "vegas", "kdenlive", "shotcut", "obs", "handbrake", "vlc", "mpv", "media player", "potplayer"],
    "audio": ["audacity", "ableton", "fl studio", "cubase", "logic", "reason", "studio one", "reaper", "garageband"],
    "development": ["git", "python", "node", "npm", "docker", "kubernetes", "vscode", "visual studio", "intellij", "pycharm", "webstorm", "goland", "clion", "eclipse", "android studio", "xcode", "jupyter", "anaconda", "cmake", "mingw", "cygwin", "gradle", "maven"],
    "database": ["mysql", "postgresql", "mongodb", "sqlite", "redis", "oracle", "sql server", "dbeaver", "sequel pro", "heidisql", "navicat", "pgadmin", "workbench"],
    "design": ["blender", "maya", "3ds max", "cinema 4d", "unity", "unreal", "godot", "fusion 360", "autocad", "solidworks", "sketchup", "rhino"],
    "communication": ["discord", "slack", "teams", "zoom", "skype", "telegram", "whatsapp", "wechat", "qq", "dingtalk", "lark", "feishu"],
    "utility": ["calculator", "calendar", "clock", "notes", "evernote", "notion", "obsidian", "roam", "1password", "lastpass", "dropbox", "onedrive", "google drive"],
    "game": ["steam", "epic", "battle.net", "origin", "ubisoft", "gog", "minecraft", "league", "dota", "counter-strike"],
    "music": ["spotify", "netease", "qq music", "apple music", "foobar", "winamp", "aimp", "musicbee"],
    "pdf": ["acrobat", "reader", "foxit", "sumatra", "pdf reader", "pdf viewer", "pdf-xchange"],
    "compression": ["winrar", "7-zip", "winzip", "peazip", "bandizip", "tar", "gzip"],
    "download": ["aria2", "motrix", "xdown", "idm", "internet download manager", "transmission", "qbittorrent", "utorrent", "thunder"],
}

EXCLUDED_NAMES = [
    "uninstall", "update", "setup", "install", "readme", "help",
    "documentation", "support", "feedback", "report", "get started",
    "what's new", "about", "license", "release notes",
]

EXCLUDED_DIRS = [
    "windows", "windows.old", "system32", "syswow64", "msocache",
    "$recycle.bin", "system volume information", "perflogs",
    "programdata\\microsoft", "appdata\\local\\temp", "appdata\\locallow",
    "appdata\\roaming\\microsoft", "common files",
]


class SoftwareInfo:
    def __init__(self, name: str, exec_path: str, icon_path: str = "",
                 category: str = "other", description: str = "",
                 source: str = "unknown"):
        self.name = name
        self.exec_path = exec_path
        self.icon_path = icon_path or exec_path
        self.category = category
        self.description = description or name
        self.source = source

    def to_dict(self):
        return {
            "name": self.name,
            "exec_path": self.exec_path,
            "icon_path": self.icon_path,
            "category": self.category,
            "description": self.description,
            "source": self.source,
        }

    @staticmethod
    def from_dict(d):
        return SoftwareInfo(
            name=d["name"],
            exec_path=d["exec_path"],
            icon_path=d.get("icon_path", d["exec_path"]),
            category=d.get("category", "other"),
            description=d.get("description", d["name"]),
            source=d.get("source", "cache"),
        )

    def __repr__(self):
        return f"[{self.category}] {self.name}"


def classify_software(name: str, exec_path: str = "") -> str:
    name_lower = name.lower()
    path_lower = exec_path.lower()

    combined = name_lower + " " + path_lower

    for cat, keywords in CATEGORY_KEYWORDS.items():
        for kw in keywords:
            if kw in combined:
                return cat
    return "other"


def is_excluded(name: str) -> bool:
    name_lower = name.lower().strip()
    for exc in EXCLUDED_NAMES:
        if exc in name_lower:
            return True
    return False


def scan_start_menu() -> list:
    found = []
    start_menu_dirs = [
        os.environ.get("ProgramData", "") + "\\Microsoft\\Windows\\Start Menu\\Programs",
        os.environ.get("APPDATA", "") + "\\Microsoft\\Windows\\Start Menu\\Programs",
        os.environ.get("ALLUSERSPROFILE", "") + "\\Microsoft\\Windows\\Start Menu\\Programs",
    ]

    seen = set()

    for base_dir in start_menu_dirs:
        if not os.path.isdir(base_dir):
            continue
        try:
            for root, dirs, files in os.walk(base_dir):
                skip = False
                for exc in EXCLUDED_DIRS:
                    if exc.lower() in root.lower():
                        skip = True
                        break
                if skip:
                    continue

                for f in files:
                    if not f.endswith(".lnk"):
                        continue
                    name = os.path.splitext(f)[0]
                    if is_excluded(name):
                        continue
                    lnk_path = os.path.join(root, f)

                    lower = name.lower()
                    if lower in seen:
                        continue
                    seen.add(lower)

                    category = classify_software(name, lnk_path)
                    found.append(SoftwareInfo(
                        name=name,
                        exec_path=lnk_path,
                        icon_path=lnk_path,
                        category=category,
                        description=f"从开始菜单发现: {name}",
                        source="start_menu",
                    ))
        except (PermissionError, OSError):
            continue

    return found


def scan_registry() -> list:
    found = []
    seen_names = set()

    registry_paths = [
        (winreg.HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (winreg.HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (winreg.HKEY_CURRENT_USER, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    ]

    for hkey, key_path in registry_paths:
        try:
            key = winreg.OpenKey(hkey, key_path, 0, winreg.KEY_READ)
            try:
                i = 0
                while True:
                    try:
                        subkey_name = winreg.EnumKey(key, i)
                        i += 1
                        subkey = winreg.OpenKey(key, subkey_name)
                        try:
                            name, _ = winreg.QueryValueEx(subkey, "DisplayName")
                            if not name or is_excluded(name):
                                continue

                            lower = name.lower()
                            if lower in seen_names:
                                continue
                            seen_names.add(lower)

                            exec_path = ""
                            try:
                                exec_path, _ = winreg.QueryValueEx(subkey, "DisplayIcon")
                            except (FileNotFoundError, OSError):
                                pass
                            try:
                                if not exec_path:
                                    exec_path, _ = winreg.QueryValueEx(subkey, "InstallLocation")
                            except (FileNotFoundError, OSError):
                                pass

                            publisher = ""
                            try:
                                publisher, _ = winreg.QueryValueEx(subkey, "Publisher")
                            except (FileNotFoundError, OSError):
                                pass

                            category = classify_software(name, exec_path)
                            found.append(SoftwareInfo(
                                name=name,
                                exec_path=exec_path,
                                icon_path=exec_path,
                                category=category,
                                description=f"已安装: {name}" + (f" (by {publisher})" if publisher else ""),
                                source="registry",
                            ))
                        except (FileNotFoundError, OSError):
                            pass
                        finally:
                            winreg.CloseKey(subkey)
                    except (FileNotFoundError, OSError):
                        break
            finally:
                winreg.CloseKey(key)
        except (FileNotFoundError, OSError):
            continue

    return found


def scan_common_paths() -> list:
    found = []
    seen_names = set()

    common_dirs = [
        os.environ.get("ProgramFiles", "C:\\Program Files"),
        os.environ.get("ProgramFiles(x86)", "C:\\Program Files (x86)"),
        os.environ.get("LOCALAPPDATA", "") + "\\Programs",
        os.environ.get("LOCALAPPDATA", "") + "\\Microsoft\\WinGet\\Packages",
    ]

    common_exes = [
        "chrome.exe", "firefox.exe", "msedge.exe", "code.exe",
        "notepad++.exe", "sublime_text.exe", "terminal.exe",
        "wt.exe", "powershell.exe", "cmd.exe",
        "spotify.exe", "discord.exe", "slack.exe",
        "obs64.exe", "obs32.exe", "vlc.exe",
        "git-bash.exe", "git-cmd.exe",
        "python.exe", "pythonw.exe",
        "winrar.exe", "7zFM.exe",
        "sumatraPDF.exe",
    ]

    for base_dir in common_dirs:
        if not os.path.isdir(base_dir):
            continue
        try:
            for root, dirs, files in os.walk(base_dir):
                skip = False
                for exc in EXCLUDED_DIRS:
                    if exc.lower() in root.lower():
                        skip = True
                        break
                if skip:
                    continue
                for f in files:
                    if f.lower() not in common_exes:
                        continue
                    name = os.path.splitext(f)[0]
                    if is_excluded(name):
                        continue
                    lower = name.lower()
                    if lower in seen_names:
                        continue
                    seen_names.add(lower)

                    full_path = os.path.join(root, f)
                    category = classify_software(name, full_path)
                    found.append(SoftwareInfo(
                        name=name.capitalize(),
                        exec_path=full_path,
                        icon_path=full_path,
                        category=category,
                        description=f"在安装目录发现: {name}",
                        source="common_paths",
                    ))
        except (PermissionError, OSError):
            continue

    return found


def merge_software_lists(*lists):
    seen = {}
    merged = []

    for sw_list in lists:
        for sw in sw_list:
            key = sw.name.lower()
            if key not in seen:
                seen[key] = sw
                merged.append(sw)
            else:
                existing = seen[key]
                if not existing.exec_path and sw.exec_path:
                    existing.exec_path = sw.exec_path
                    existing.icon_path = sw.icon_path or existing.icon_path
                    existing.source = sw.source
                    if sw.category != "other":
                        existing.category = sw.category

    return merged


def scan_all_software() -> list:
    start_menu = scan_start_menu()
    registry = scan_registry()
    common = scan_common_paths()

    merged = merge_software_lists(start_menu, registry, common)

    categorized = {}
    for sw in merged:
        cat = sw.category
        if cat not in categorized:
            categorized[cat] = []
        categorized[cat].append(sw)

    result = []
    for cat in sorted(categorized.keys()):
        categorized[cat].sort(key=lambda x: x.name.lower())
        result.extend(categorized[cat])

    return result


def save_software_cache(software_list: list):
    os.makedirs(MEMORY_PATH, exist_ok=True)
    data = {
        "scanned_at": datetime.now().isoformat(),
        "count": len(software_list),
        "software": [sw.to_dict() for sw in software_list],
    }
    with open(SOFTWARE_CACHE_FILE, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False, indent=2)

    with open(SOFTWARE_SCAN_FLAG, "w") as f:
        f.write(datetime.now().isoformat())

    print(f"[SoftwareScanner] 已缓存 {len(software_list)} 个软件到 {SOFTWARE_CACHE_FILE}")


def load_software_cache() -> list:
    if not os.path.exists(SOFTWARE_CACHE_FILE):
        return []
    try:
        with open(SOFTWARE_CACHE_FILE, "r", encoding="utf-8") as f:
            data = json.load(f)
        software = [SoftwareInfo.from_dict(d) for d in data.get("software", [])]
        print(f"[SoftwareScanner] 从缓存加载了 {len(software)} 个软件")
        return software
    except (json.JSONDecodeError, KeyError, Exception) as e:
        print(f"[SoftwareScanner] 缓存加载失败: {e}")
        return []


def is_software_scanned() -> bool:
    return os.path.exists(SOFTWARE_SCAN_FLAG)


def get_scan_time() -> str:
    if not os.path.exists(SOFTWARE_SCAN_FLAG):
        return ""
    try:
        with open(SOFTWARE_SCAN_FLAG, "r") as f:
            return f.read().strip()
    except Exception:
        return ""


def search_software(query: str, software_list: list, top_k: int = 10) -> list:
    query_lower = query.lower()

    scored = []
    for sw in software_list:
        score = 0
        if query_lower in sw.name.lower():
            score += 10
        if query_lower in sw.category:
            score += 5
        if query_lower in sw.description.lower():
            score += 3
        if sw.exec_path and query_lower in sw.exec_path.lower():
            score += 2

        for keyword in query_lower.split():
            if keyword in sw.name.lower():
                score += 2

        if score > 0:
            scored.append((score, sw))

    scored.sort(key=lambda x: -x[0])
    return [sw for _, sw in scored[:top_k]]


def launch_software(exec_path: str) -> dict:
    import subprocess
    try:
        if exec_path.lower().endswith(".lnk"):
            os.startfile(exec_path)
        else:
            subprocess.Popen([exec_path], shell=True)
        return {"success": True, "message": f"已启动: {os.path.basename(exec_path)}"}
    except Exception as e:
        return {"success": False, "message": f"启动失败: {str(e)}"}


def get_all_software() -> list:
    return load_software_cache()


def get_software_by_category(category: str) -> list:
    all_sw = load_software_cache()
    return [sw for sw in all_sw if sw.category == category]


def get_all_categories() -> dict:
    all_sw = load_software_cache()
    cats = {}
    for sw in all_sw:
        if sw.category not in cats:
            cats[sw.category] = []
        cats[sw.category].append(sw.name)
    return cats


def get_software_count() -> int:
    all_sw = load_software_cache()
    return len(all_sw)


def scan_and_cache() -> list:
    if is_software_scanned():
        print("[SoftwareScanner] 软件已扫描过，从缓存加载")
        cached = load_software_cache()
        if cached:
            return cached

    print("[SoftwareScanner] 开始扫描电脑上的软件...")
    software_list = scan_all_software()
    save_software_cache(software_list)

    total = len(software_list)
    categories = {}
    for sw in software_list:
        categories[sw.category] = categories.get(sw.category, 0) + 1

    print(f"[SoftwareScanner] 扫描完成! 共发现 {total} 个软件，涵盖 {len(categories)} 个分类")
    for cat, count in sorted(categories.items()):
        print(f"  {cat}: {count} 个")

    return software_list
