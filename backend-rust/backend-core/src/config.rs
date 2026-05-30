use std::path::PathBuf;
use std::env;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub deepseek_api_key: String,
    pub deepseek_base_url: String,
    pub model: String,
    pub workspace: PathBuf,
    pub memory_path: PathBuf,
}

impl Config {
    pub fn from_env() -> Self {
        Config {
            deepseek_api_key: Self::load_api_key(),
            deepseek_base_url: env::var("DEEPSEEK_BASE_URL")
                .unwrap_or_else(|_| "https://api.deepseek.com".to_string()),
            model: env::var("MODEL").unwrap_or_else(|_| "deepseek-chat".to_string()),
            workspace: Self::resolve_path(
                env::var("WORKSPACE").unwrap_or_else(|_| "./workspace".to_string()),
            ),
            memory_path: Self::resolve_path(
                env::var("MEMORY_PATH").unwrap_or_else(|_| "./memory_db".to_string()),
            ),
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.deepseek_api_key.is_empty()
    }

    fn load_api_key() -> String {
        if let Ok(key) = env::var("DEEPSEEK_API_KEY") {
            if !key.is_empty() {
                return key;
            }
        }
        let cwd = env::current_dir().ok();
        let mut env_paths = vec![
            PathBuf::from("./.env"),
            PathBuf::from("../.env"),
        ];
        if let Some(ref cwd) = cwd {
            env_paths.push(cwd.join(".env"));
            env_paths.push(cwd.join("../.env"));
            env_paths.push(cwd.join("../../.env"));
        }
        for env_path in &env_paths {
            if env_path.exists() {
                if let Ok(content) = std::fs::read_to_string(env_path) {
                    for line in content.lines() {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with('#') {
                            continue;
                        }
                        if let Some(eq_pos) = line.find('=') {
                            let key = line[..eq_pos].trim();
                            let value = line[eq_pos + 1..].trim().trim_matches('"').trim_matches('\'');
                            if key == "DEEPSEEK_API_KEY" && !value.is_empty() {
                                return value.to_string();
                            }
                        }
                    }
                }
            }
        }
        String::new()
    }

    fn resolve_path(path_str: String) -> PathBuf {
        let path = PathBuf::from(&path_str);
        if path.is_relative() {
            if let Ok(cwd) = env::current_dir() {
                return cwd.join(&path);
            }
        }
        path
    }

    pub fn config_file_path(memory_path: &PathBuf) -> PathBuf {
        let mut p = memory_path.clone();
        if p.is_file() {
            p.pop();
        }
        p.join("runtime_config.json")
    }

    pub fn save_to_file(&self, memory_path: &PathBuf) {
        let path = Self::config_file_path(memory_path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
    }

    pub fn load_from_file(memory_path: &PathBuf) -> Option<Self> {
        let path = Self::config_file_path(memory_path);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str::<Config>(&content) {
                    return Some(config);
                }
            }
        }
        None
    }

    pub fn apply_runtime_config(&mut self, memory_path: &PathBuf) {
        if let Some(runtime) = Self::load_from_file(memory_path) {
            if !runtime.deepseek_api_key.is_empty() {
                self.deepseek_api_key = runtime.deepseek_api_key;
            }
            if !runtime.deepseek_base_url.is_empty() {
                self.deepseek_base_url = runtime.deepseek_base_url;
            }
            if !runtime.model.is_empty() {
                self.model = runtime.model;
            }
        }
    }
}