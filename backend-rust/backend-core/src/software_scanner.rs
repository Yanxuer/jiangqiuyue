use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::fs;
use chrono::Utc;
use winreg::enums::*;
use winreg::RegKey;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoftwareInfo {
    pub name: String,
    pub exec_path: String,
    pub icon_path: String,
    pub category: String,
    pub description: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoftwareCache {
    pub scanned_at: String,
    pub machine_id: String,
    pub count: usize,
    pub software: Vec<SoftwareInfo>,
}

fn get_machine_id() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| {
        std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string())
    })
}

const CATEGORY_KEYWORDS: &[(&str, &[&str])] = &[
    ("browser", &["browser", "chrome", "edge", "firefox", "safari", "opera", "chromium", "brave", "iexplore"]),
    ("editor", &["code", "sublime", "vim", "neovim", "notepad", "emacs", "atom", "brackets", "text", "ide"]),
    ("terminal", &["terminal", "cmd", "powershell", "windowsterminal", "conemu", "cmder", "hyper", "alacritty", "putty", "ssh", "warp"]),
    ("office", &["word", "excel", "powerpoint", "outlook", "onenote", "access", "libreoffice", "wps", "office"]),
    ("image", &["photoshop", "gimp", "krita", "paint", "illustrator", "figma", "sketch", "lightroom", "canvas", "photo", "draw", "inkscape", "corel"]),
    ("video", &["premiere", "after effects", "davinci", "final cut", "vegas", "kdenlive", "shotcut", "obs", "handbrake", "vlc", "mpv", "media player", "potplayer"]),
    ("audio", &["audacity", "ableton", "fl studio", "cubase", "logic", "reason", "studio one", "reaper"]),
    ("development", &["git", "python", "node", "npm", "docker", "kubernetes", "vscode", "visual studio", "intellij", "pycharm", "webstorm", "goland", "clion", "eclipse", "android studio", "xcode", "jupyter", "anaconda", "cmake", "mingw", "gradle", "maven"]),
    ("database", &["mysql", "postgresql", "mongodb", "sqlite", "redis", "oracle", "sql server", "dbeaver", "sequel pro", "heidisql", "navicat", "pgadmin"]),
    ("design", &["blender", "maya", "3ds max", "cinema 4d", "unity", "unreal", "godot", "fusion 360", "autocad", "solidworks", "sketchup"]),
    ("communication", &["discord", "slack", "teams", "zoom", "skype", "telegram", "whatsapp", "wechat", "qq", "dingtalk", "feishu"]),
    ("utility", &["calculator", "calendar", "clock", "notes", "evernote", "notion", "obsidian", "1password", "lastpass", "dropbox", "onedrive"]),
    ("game", &["steam", "epic", "battle.net", "origin", "ubisoft", "gog", "minecraft", "league", "dota", "counter-strike"]),
    ("music", &["spotify", "netease", "qq music", "apple music", "foobar", "winamp", "aimp", "musicbee"]),
    ("pdf", &["acrobat", "reader", "foxit", "sumatra", "pdf reader", "pdf viewer"]),
    ("compression", &["winrar", "7-zip", "winzip", "peazip", "bandizip", "tar", "gzip"]),
    ("download", &["aria2", "motrix", "xdown", "idm", "internet download manager", "transmission", "qbittorrent", "utorrent", "thunder"]),
];

const EXCLUDED_NAMES: &[&str] = &[
    "uninstall", "update", "setup", "install", "readme", "help",
    "documentation", "support", "feedback", "report", "get started",
    "what's new", "about", "license", "release notes",
];

const EXCLUDED_DIRS: &[&str] = &[
    "windows", "windows.old", "system32", "syswow64", "msocache",
    "$recycle.bin", "system volume information", "perflogs",
];

const COMMON_EXES: &[&str] = &[
    "chrome.exe", "firefox.exe", "msedge.exe", "code.exe",
    "notepad++.exe", "sublime_text.exe", "terminal.exe",
    "wt.exe", "powershell.exe", "cmd.exe",
    "spotify.exe", "discord.exe", "slack.exe",
    "obs64.exe", "obs32.exe", "vlc.exe",
    "python.exe", "pythonw.exe",
    "winrar.exe", "7zFM.exe", "sumatrapdf.exe",
];

fn classify_software(name: &str, exec_path: &str) -> String {
    let combined = format!("{} {}", name.to_lowercase(), exec_path.to_lowercase());

    for &(cat, keywords) in CATEGORY_KEYWORDS {
        for kw in keywords {
            if combined.contains(kw) {
                return cat.to_string();
            }
        }
    }
    "other".to_string()
}

fn is_excluded(name: &str) -> bool {
    let lower = name.to_lowercase().trim().to_string();
    EXCLUDED_NAMES.iter().any(|exc| lower.contains(exc))
}

fn scan_start_menu() -> Vec<SoftwareInfo> {
    let mut found = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let start_menu_dirs = vec![
        format!("{}\\Microsoft\\Windows\\Start Menu\\Programs",
            std::env::var("ProgramData").unwrap_or_default()),
        format!("{}\\Microsoft\\Windows\\Start Menu\\Programs",
            std::env::var("APPDATA").unwrap_or_default()),
    ];

    for base_dir in &start_menu_dirs {
        let dir = Path::new(base_dir);
        if !dir.is_dir() {
            continue;
        }
        for entry in walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            let path_lower = path.display().to_string().to_lowercase();
            if EXCLUDED_DIRS.iter().any(|exc| path_lower.contains(exc)) {
                continue;
            }
            if !path.extension().map(|e| e == "lnk").unwrap_or(false) {
                continue;
            }
            let name = path.file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if name.is_empty() || is_excluded(name) {
                continue;
            }
            let lower = name.to_lowercase();
            if seen.contains(&lower) {
                continue;
            }
            seen.insert(lower);

            let category = classify_software(name, &path.display().to_string());
            found.push(SoftwareInfo {
                name: name.to_string(),
                exec_path: path.display().to_string(),
                icon_path: path.display().to_string(),
                category,
                description: format!("从开始菜单发现: {}", name),
                source: "start_menu".to_string(),
            });
        }
    }
    found
}

fn scan_registry() -> Vec<SoftwareInfo> {
    let mut found = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    let registry_paths = [
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_CURRENT_USER, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    ];

    for &(hkey, key_path) in &registry_paths {
        let key = match RegKey::predef(hkey).open_subkey(key_path) {
            Ok(k) => k,
            Err(_) => continue,
        };

        for name_result in key.enum_keys().filter_map(|r| r.ok()) {
            let subkey = match key.open_subkey(&name_result) {
                Ok(k) => k,
                Err(_) => continue,
            };

            let display_name: Option<String> = subkey.get_value("DisplayName").ok();
            let display_name = match display_name {
                Some(ref n) if !n.is_empty() && !is_excluded(n) => n.clone(),
                _ => continue,
            };

            let lower = display_name.to_lowercase();
            if seen_names.contains(&lower) {
                continue;
            }
            seen_names.insert(lower);

            let exec_path: String = subkey.get_value("DisplayIcon").ok()
                .or_else(|| subkey.get_value("InstallLocation").ok())
                .unwrap_or_default();

            let publisher: String = subkey.get_value("Publisher").ok()
                .unwrap_or_default();

            let category = classify_software(&display_name, &exec_path);
            found.push(SoftwareInfo {
                name: display_name.clone(),
                exec_path: exec_path.clone(),
                icon_path: exec_path,
                category,
                description: format!("已安装: {}{}",
                    display_name,
                    if publisher.is_empty() { String::new() } else { format!(" (by {})", publisher) }),
                source: "registry".to_string(),
            });
        }
    }
    found
}

fn scan_common_paths() -> Vec<SoftwareInfo> {
    let mut found = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    let common_dirs = vec![
        std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string()),
        std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".to_string()),
        format!("{}\\Programs", std::env::var("LOCALAPPDATA").unwrap_or_default()),
    ];

    for base_dir in &common_dirs {
        let dir = Path::new(base_dir);
        if !dir.is_dir() {
            continue;
        }

        for entry in walkdir::WalkDir::new(dir)
            .max_depth(4)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            let path_lower = path.display().to_string().to_lowercase();
            if EXCLUDED_DIRS.iter().any(|exc| path_lower.contains(exc)) {
                continue;
            }
            let fname = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_lowercase();

            if !COMMON_EXES.contains(&fname.as_str()) {
                continue;
            }

            let name = Path::new(&fname)
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or(&fname);

            if is_excluded(name) {
                continue;
            }
            let lower = name.to_lowercase();
            if seen_names.contains(&lower) {
                continue;
            }
            seen_names.insert(lower);

            let category = classify_software(name, &path.display().to_string());
            found.push(SoftwareInfo {
                name: name.to_string(),
                exec_path: path.display().to_string(),
                icon_path: path.display().to_string(),
                category,
                description: format!("在安装目录发现: {}", name),
                source: "common_paths".to_string(),
            });
        }
    }
    found
}

fn merge_software_lists(lists: Vec<Vec<SoftwareInfo>>) -> Vec<SoftwareInfo> {
    let mut seen: HashMap<String, SoftwareInfo> = HashMap::new();

    for list in lists {
        for sw in list {
            let key = sw.name.to_lowercase();
            if let Some(existing) = seen.get(&key) {
                if existing.exec_path.is_empty() && !sw.exec_path.is_empty() {
                    seen.insert(key, sw);
                }
            } else {
                seen.insert(key, sw);
            }
        }
    }

    let mut result: Vec<SoftwareInfo> = seen.into_values().collect();
    result.sort_by(|a, b| a.category.cmp(&b.category).then(a.name.cmp(&b.name)));
    result
}

pub fn scan_all_software() -> Vec<SoftwareInfo> {
    log::info!("开始扫描电脑上的软件...");

    let start_menu = scan_start_menu();
    let registry = scan_registry();
    let common = scan_common_paths();

    let merged = merge_software_lists(vec![start_menu, registry, common]);

    log::info!("扫描完成! 共发现 {} 个软件", merged.len());
    merged
}

pub fn save_software_cache(memory_path: &Path, software_list: &[SoftwareInfo]) {
    fs::create_dir_all(memory_path).ok();
    let cache = SoftwareCache {
        scanned_at: Utc::now().to_rfc3339(),
        machine_id: get_machine_id(),
        count: software_list.len(),
        software: software_list.to_vec(),
    };

    let cache_path = memory_path.join("software_cache.json");
    if let Ok(content) = serde_json::to_string_pretty(&cache) {
        fs::write(&cache_path, content).ok();
    }

    let flag_path = memory_path.join(".software_scanned");
    fs::write(&flag_path, Utc::now().to_rfc3339()).ok();

    log::info!("已缓存 {} 个软件到 {:?}", software_list.len(), cache_path);
}

pub fn load_software_cache(memory_path: &Path) -> Vec<SoftwareInfo> {
    let cache_path = memory_path.join("software_cache.json");
    if !cache_path.exists() {
        return Vec::new();
    }
    match fs::read_to_string(&cache_path) {
        Ok(content) => {
            match serde_json::from_str::<SoftwareCache>(&content) {
                Ok(cache) => {
                    log::info!("从缓存加载了 {} 个软件", cache.software.len());
                    cache.software
                }
                Err(e) => {
                    log::warn!("缓存加载失败: {}", e);
                    Vec::new()
                }
            }
        }
        Err(_) => Vec::new(),
    }
}

pub fn is_software_scanned(memory_path: &Path) -> bool {
    memory_path.join(".software_scanned").exists()
}

pub fn search_software(query: &str, software_list: &[SoftwareInfo], top_k: usize) -> Vec<SoftwareInfo> {
    let query_lower = query.to_lowercase();
    let mut scored: Vec<(i32, &SoftwareInfo)> = Vec::new();

    for sw in software_list {
        let mut score = 0;
        if query_lower.len() >= 2 {
            if sw.name.to_lowercase().contains(&query_lower) {
                score += 10;
            }
            if sw.category.contains(&query_lower) {
                score += 5;
            }
            if sw.description.to_lowercase().contains(&query_lower) {
                score += 3;
            }
            if sw.exec_path.to_lowercase().contains(&query_lower) {
                score += 2;
            }
            for keyword in query_lower.split_whitespace() {
                if sw.name.to_lowercase().contains(keyword) {
                    score += 2;
                }
            }
        }
        if score > 0 {
            scored.push((score, sw));
        }
    }

    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().take(top_k).map(|(_, sw)| sw.clone()).collect()
}

pub fn get_all_categories(software_list: &[SoftwareInfo]) -> HashMap<String, Vec<String>> {
    let mut cats: HashMap<String, Vec<String>> = HashMap::new();
    for sw in software_list {
        cats.entry(sw.category.clone())
            .or_default()
            .push(sw.name.clone());
    }
    cats
}

pub fn get_software_by_category<'a>(software_list: &'a [SoftwareInfo], category: &str) -> Vec<&'a SoftwareInfo> {
    software_list.iter().filter(|sw| sw.category == category).collect()
}

pub fn scan_and_cache(memory_path: &Path) -> Vec<SoftwareInfo> {
    if is_software_scanned(memory_path) {
        // 加载缓存并检查设备是否变更
        let cached = load_software_cache(memory_path);
        let current_machine = get_machine_id();

        if !cached.is_empty() {
            // 读取缓存中的 machine_id
            let cache_path = memory_path.join("software_cache.json");
            let cached_machine = fs::read_to_string(&cache_path)
                .ok()
                .and_then(|s| serde_json::from_str::<SoftwareCache>(&s).ok())
                .map(|c| c.machine_id)
                .unwrap_or_default();

            if cached_machine == current_machine {
                // 同一设备：快速增量扫描（更新增删）
                log::info!("同一设备 ({}), 执行快速增量扫描...", current_machine);
                let updated = quick_rescan(memory_path, &cached);
                if updated.len() != cached.len() {
                    log::info!("增量扫描完成: {} -> {} 个软件", cached.len(), updated.len());
                } else {
                    log::info!("增量扫描完成: 无变化 ({} 个软件)", updated.len());
                }
                return updated;
            } else {
                // 新设备：重置缓存，全量扫描
                log::info!("检测到新设备 ({} -> {}), 重新全量扫描...",
                    if cached_machine.is_empty() { "无" } else { &cached_machine },
                    current_machine);
                clear_software_cache(memory_path);
            }
        }
    }

    let software_list = scan_all_software();
    save_software_cache(memory_path, &software_list);
    software_list
}

fn clear_software_cache(memory_path: &Path) {
    let cache_path = memory_path.join("software_cache.json");
    let flag_path = memory_path.join(".software_scanned");
    let _ = fs::remove_file(&cache_path);
    let _ = fs::remove_file(&flag_path);
    log::info!("已清除软件缓存");
}

fn quick_rescan(memory_path: &Path, cached: &[SoftwareInfo]) -> Vec<SoftwareInfo> {
    // 快速扫描：开始菜单 + 注册表（跳过深度目录遍历）
    let start_menu = scan_start_menu();
    let registry = scan_registry();
    let quick_new = merge_software_lists(vec![start_menu, registry]);

    let quick_map: HashMap<String, &SoftwareInfo> = quick_new.iter()
        .map(|sw| (sw.name.to_lowercase(), sw))
        .collect();

    let mut result: Vec<SoftwareInfo> = Vec::new();
    let mut removed_count: usize = 0;

    // 保留仍然存在且路径有效的软件
    for sw in cached {
        let key = sw.name.to_lowercase();
        if let Some(new_sw) = quick_map.get(&key) {
            // 在快速扫描中仍能找到 → 保留（使用最新信息）
            result.push((*new_sw).clone());
        } else {
            // 快速扫描中未找到 → 检查执行路径是否仍然存在
            let path = Path::new(&sw.exec_path);
            if path.exists() && sw.source != "registry" {
                // 路径仍存在 → 保留
                result.push(sw.clone());
            } else if sw.source == "common_paths" {
                // 来自深度扫描的软件，检查常见路径
                let fname = sw.exec_path.rsplit('\\').next().unwrap_or("");
                let fname_lower = fname.to_lowercase();
                if COMMON_EXES.contains(&fname_lower.as_str()) && path.exists() {
                    result.push(sw.clone());
                } else {
                    removed_count += 1;
                    log::info!("增量扫描: 移除已卸载软件 '{}'", sw.name);
                }
            } else {
                removed_count += 1;
                log::info!("增量扫描: 移除已卸载软件 '{}'", sw.name);
            }
        }
    }

    // 添加新发现的软件
    let result_names: std::collections::HashSet<String> = result.iter()
        .map(|sw| sw.name.to_lowercase())
        .collect();

    let mut added_count = 0;
    for sw in &quick_new {
        if !result_names.contains(&sw.name.to_lowercase()) {
            result.push(sw.clone());
            added_count += 1;
            log::info!("增量扫描: 发现新软件 '{}'", sw.name);
        }
    }

    if added_count > 0 || removed_count > 0 {
        log::info!("增量扫描: +{} / -{} 个软件", added_count, removed_count);
    }

    result.sort_by(|a, b| a.category.cmp(&b.category).then(a.name.cmp(&b.name)));
    save_software_cache(memory_path, &result);
    result
}