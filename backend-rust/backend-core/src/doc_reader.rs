use serde::{Deserialize, Serialize};
use std::path::Path;
use std::fs;
use chrono::Utc;

const SYSTEM_BLOCKED_PREFIXES: &[&str] = &[
    "C:\\Windows",
    "C:\\Program Files",
    "C:\\Program Files (x86)",
    "C:\\ProgramData",
    "C:\\System Volume Information",
    "C:\\$Recycle.Bin",
];

const TEXT_EXTENSIONS: &[&str] = &[
    ".txt", ".md", ".py", ".js", ".jsx", ".ts", ".tsx", ".html", ".htm",
    ".css", ".scss", ".less", ".json", ".xml", ".yaml", ".yml", ".toml",
    ".ini", ".cfg", ".conf", ".log", ".csv", ".env", ".bat", ".sh", ".ps1",
    ".java", ".cpp", ".c", ".h", ".hpp", ".cs", ".go", ".rs", ".rb", ".php",
    ".swift", ".kt", ".kts", ".vue", ".svelte", ".sql", ".r", ".m", ".mm",
];

const SENSITIVE_KEYWORDS: &[&str] = &[
    "password", "credential", "secret", "token", "key", "private",
    "id_rsa", "id_dsa", "id_ecdsa",
];

const FILE_SIZE_LIMIT: u64 = 1024 * 1024;
const MAX_RECENT_PATHS: usize = 10;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DirEntry {
    pub name: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub size: Option<u64>,
    pub is_text: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DirInfo {
    pub success: bool,
    pub path: Option<String>,
    pub name: Option<String>,
    pub directories: Vec<DirEntry>,
    pub files: Vec<DirEntry>,
    pub total: usize,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileReadResult {
    pub success: bool,
    pub path: Option<String>,
    pub name: Option<String>,
    pub content: Option<String>,
    pub size: Option<u64>,
    pub lines: Option<usize>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RecentPath {
    pub path: String,
    pub name: String,
    pub time: String,
}

pub struct DocReader;

impl DocReader {
    pub fn validate_path(path_str: &str) -> Result<String, String> {
        let abs_path = std::fs::canonicalize(path_str)
            .map_err(|_| format!("路径无效: {}", path_str))?;
        let abs_lower = abs_path.display().to_string().to_lowercase();

        for prefix in SYSTEM_BLOCKED_PREFIXES {
            if abs_lower.starts_with(&prefix.to_lowercase()) {
                return Err(format!("禁止访问系统目录: {}", path_str));
            }
        }

        Ok(abs_path.display().to_string())
    }

    fn is_sensitive_file(path: &Path) -> bool {
        let name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        SENSITIVE_KEYWORDS.iter().any(|kw| name.contains(kw))
    }

    pub fn list_directory(path_str: &str) -> DirInfo {
        let validated = match Self::validate_path(path_str) {
            Ok(p) => p,
            Err(e) => {
                return DirInfo {
                    success: false,
                    path: None,
                    name: None,
                    directories: Vec::new(),
                    files: Vec::new(),
                    total: 0,
                    error: Some(e),
                }
            }
        };

        let path = Path::new(&validated);
        if !path.exists() {
            return DirInfo {
                success: false,
                path: None,
                name: None,
                directories: Vec::new(),
                files: Vec::new(),
                total: 0,
                error: Some(format!("路径不存在: {}", validated)),
            };
        }
        if !path.is_dir() {
            return DirInfo {
                success: false,
                path: None,
                name: None,
                directories: Vec::new(),
                files: Vec::new(),
                total: 0,
                error: Some(format!("不是目录: {}", validated)),
            };
        }

        let mut dirs = Vec::new();
        let mut files = Vec::new();

        let mut entries: Vec<_> = match fs::read_dir(path) {
            Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
            Err(e) => {
                return DirInfo {
                    success: false,
                    path: None,
                    name: None,
                    directories: Vec::new(),
                    files: Vec::new(),
                    total: 0,
                    error: Some(format!("无权限访问: {}", e)),
                }
            }
        };

        entries.sort_by_key(|e| {
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            (std::cmp::Reverse(is_dir), e.file_name())
        });

        for entry in &entries {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }

            if let Ok(ftype) = entry.file_type() {
                if ftype.is_dir() {
                    dirs.push(DirEntry {
                        name,
                        entry_type: "directory".to_string(),
                        size: None,
                        is_text: None,
                    });
                } else if ftype.is_file() {
                    let ext = Path::new(&name)
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| format!(".{}", e.to_lowercase()))
                        .unwrap_or_default();
                    let is_text = TEXT_EXTENSIONS.contains(&ext.as_str())
                        && !Self::is_sensitive_file(&entry.path());
                    let size = entry.metadata().map(|m| m.len()).ok();
                    files.push(DirEntry {
                        name,
                        entry_type: "file".to_string(),
                        size,
                        is_text: Some(is_text),
                    });
                }
            }
        }

        DirInfo {
            success: true,
            path: Some(validated.clone()),
            name: Some(
                Path::new(&validated)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
            total: dirs.len() + files.len(),
            directories: dirs,
            files,
            error: None,
        }
    }

    pub fn read_text_file(path_str: &str) -> FileReadResult {
        let validated = match Self::validate_path(path_str) {
            Ok(p) => p,
            Err(e) => {
                return FileReadResult {
                    success: false,
                    path: None,
                    name: None,
                    content: None,
                    size: None,
                    lines: None,
                    error: Some(e),
                }
            }
        };

        let path = Path::new(&validated);
        if !path.exists() {
            return FileReadResult {
                success: false,
                path: None,
                name: None,
                content: None,
                size: None,
                lines: None,
                error: Some(format!("文件不存在: {}", validated)),
            };
        }
        if !path.is_file() {
            return FileReadResult {
                success: false,
                path: None,
                name: None,
                content: None,
                size: None,
                lines: None,
                error: Some(format!("不是文件: {}", validated)),
            };
        }
        if Self::is_sensitive_file(path) {
            return FileReadResult {
                success: false,
                path: None,
                name: None,
                content: None,
                size: None,
                lines: None,
                error: Some("禁止读取敏感文件".to_string()),
            };
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_lowercase()))
            .unwrap_or_default();

        if !TEXT_EXTENSIONS.contains(&ext.as_str()) {
            return FileReadResult {
                success: false,
                error: Some(format!("不支持的文件类型: {}", ext)),
                path: None,
                name: None,
                content: None,
                size: None,
                lines: None,
            };
        }

        let file_size = match fs::metadata(path).map(|m| m.len()) {
            Ok(s) => s,
            Err(e) => {
                return FileReadResult {
                    success: false,
                    path: None,
                    name: None,
                    content: None,
                    size: None,
                    lines: None,
                    error: Some(format!("读取元数据失败: {}", e)),
                }
            }
        };

        if file_size > FILE_SIZE_LIMIT {
            return FileReadResult {
                success: false,
                path: None,
                name: None,
                content: None,
                size: Some(file_size),
                lines: None,
                error: Some(format!("文件过大 (>{}KB)，无法直接读取", FILE_SIZE_LIMIT / 1024)),
            };
        }

        match fs::read_to_string(path) {
            Ok(content) => {
                let lines = content.lines().count();
                FileReadResult {
                    success: true,
                    path: Some(validated.clone()),
                    name: Some(
                        path.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                    ),
                    content: Some(content),
                    size: Some(file_size),
                    lines: Some(lines),
                    error: None,
                }
            }
            Err(e) => FileReadResult {
                success: false,
                path: None,
                name: None,
                content: None,
                size: None,
                lines: None,
                error: Some(format!("读取失败: {}", e)),
            },
        }
    }
}

pub fn load_recent_paths(memory_path: &Path) -> Vec<RecentPath> {
    let path = memory_path.join("recent_paths.json");
    if !path.exists() {
        return Vec::new();
    }
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn save_recent_paths(memory_path: &Path, paths: &[RecentPath]) {
    fs::create_dir_all(memory_path).ok();
    let path = memory_path.join("recent_paths.json");
    if let Ok(content) = serde_json::to_string_pretty(paths) {
        fs::write(path, content).ok();
    }
}

pub fn add_recent_path(memory_path: &Path, path_str: &str) -> Vec<RecentPath> {
    let mut paths = load_recent_paths(memory_path);
    let normalized = std::fs::canonicalize(path_str)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path_str.to_string());

    paths.retain(|p| {
        std::fs::canonicalize(&p.path)
            .map(|cp| cp.display().to_string() != normalized)
            .unwrap_or(true)
    });

    let name = Path::new(&normalized)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| normalized.clone());

    paths.insert(
        0,
        RecentPath {
            path: normalized,
            name,
            time: Utc::now().to_rfc3339(),
        },
    );

    paths.truncate(MAX_RECENT_PATHS);
    save_recent_paths(memory_path, &paths);
    paths
}