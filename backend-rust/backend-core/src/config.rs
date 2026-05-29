use std::path::PathBuf;
use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub deepseek_api_key: String,
    pub deepseek_base_url: String,
    pub model: String,
    pub workspace: PathBuf,
    pub memory_path: PathBuf,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let api_key = Self::load_api_key()?;

        Ok(Config {
            deepseek_api_key: api_key,
            deepseek_base_url: env::var("DEEPSEEK_BASE_URL")
                .unwrap_or_else(|_| "https://api.deepseek.com".to_string()),
            model: env::var("MODEL").unwrap_or_else(|_| "deepseek-chat".to_string()),
            workspace: Self::resolve_path(
                env::var("WORKSPACE").unwrap_or_else(|_| "./workspace".to_string()),
            ),
            memory_path: Self::resolve_path(
                env::var("MEMORY_PATH").unwrap_or_else(|_| "./memory_db".to_string()),
            ),
        })
    }

    fn load_api_key() -> Result<String, String> {
        if let Ok(key) = env::var("DEEPSEEK_API_KEY") {
            if !key.is_empty() {
                return Ok(key);
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
                                return Ok(value.to_string());
                            }
                        }
                    }
                }
            }
        }

        Err(
            "未设置 DEEPSEEK_API_KEY 环境变量\n    \
             方式1: 在项目根目录创建 .env 文件，写入: DEEPSEEK_API_KEY=sk-xxx\n    \
             方式2: 通过 `set DEEPSEEK_API_KEY=sk-xxx` 设置环境变量"
                .to_string(),
        )
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
}