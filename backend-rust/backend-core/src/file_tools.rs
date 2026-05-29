use std::path::PathBuf;
use std::fs;

pub struct FileTools {
    workspace: PathBuf,
}

impl FileTools {
    pub fn new(workspace: PathBuf) -> Self {
        fs::create_dir_all(&workspace).ok();
        FileTools { workspace }
    }

    pub fn safe_path(&self, filename: &str) -> Result<PathBuf, String> {
        let workspace_canonical = self.workspace.canonicalize().map_err(|e| e.to_string())?;
        let target = self.workspace.join(filename);
        let target_canonical = target.canonicalize().unwrap_or(target);
        if !target_canonical.starts_with(&workspace_canonical) {
            return Err(format!("非法路径: {}", filename));
        }
        Ok(target_canonical)
    }

    pub fn read_file(&self, filename: &str) -> String {
        let path = match self.safe_path(filename) {
            Ok(p) => p,
            Err(e) => return format!("[错误] 路径非法: {}", e),
        };
        if !path.exists() {
            return format!("[错误] 文件不存在: {}", filename);
        }
        match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => format!("[错误] 读取失败: {}", e),
        }
    }

    pub fn write_file(&self, filename: &str, content: &str) -> String {
        let path = match self.safe_path(filename) {
            Ok(p) => p,
            Err(e) => return format!("[错误] 路径非法: {}", e),
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        match fs::write(&path, content) {
            Ok(_) => format!("[成功] 已保存: {}", filename),
            Err(e) => format!("[错误] 写入失败: {}", e),
        }
    }

    pub fn list_files(&self, subdir: &str) -> String {
        let path = if subdir.is_empty() {
            self.workspace.clone()
        } else {
            match self.safe_path(subdir) {
                Ok(p) => p,
                Err(_) => return "[]".to_string(),
            }
        };
        if !path.exists() {
            return "[]".to_string();
        }
        let mut files = Vec::new();
        for entry in walkdir::WalkDir::new(&path).into_iter().filter_map(|e| e.ok()) {
            if entry.path().is_file() {
                if let Ok(rel) = entry.path().strip_prefix(&self.workspace) {
                    files.push(rel.display().to_string());
                }
            }
        }
        if files.is_empty() {
            return "(空目录)".to_string();
        }
        files.sort();
        files.join("\n")
    }
}