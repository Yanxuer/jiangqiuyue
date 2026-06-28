use std::path::PathBuf;
use std::env;
use serde::{Deserialize, Serialize};
use crate::llm::provider::{ProviderConfig, ProviderKind};

/// LLM 提供商配置（运行时可序列化保存/读取）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LLMConfig {
    pub provider: String,          // "deepseek" / "openai" / ...
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
}

impl Default for LLMConfig {
    fn default() -> Self {
        LLMConfig {
            provider: "deepseek".to_string(),
            api_key: String::new(),
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-v4-flash".to_string(),
            temperature: 0.7,
        }
    }
}

impl LLMConfig {
    /// 转换为 ProviderConfig（供 LLMProvider 使用）
    pub fn to_provider_config(&self) -> ProviderConfig {
        ProviderConfig {
            kind: ProviderKind::from_str(&self.provider),
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            temperature: self.temperature,
            max_retries: crate::retry::DEFAULT_MAX_RETRIES,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub llm: LLMConfig,
    pub workspace: PathBuf,
    pub memory_path: PathBuf,
}

impl Config {
    pub fn from_env() -> Self {
        // 从环境变量确定 provider
        let provider = env::var("LLM_PROVIDER")
            .or_else(|_| env::var("PROVIDER"))
            .unwrap_or_else(|_| "deepseek".to_string());

        let kind = ProviderKind::from_str(&provider);

        // API key 兼容旧环境变量名
        let api_key = env::var("LLM_API_KEY")
            .or_else(|_| env::var("DEEPSEEK_API_KEY"))
            .or_else(|_| Self::load_api_key_from_env())
            .unwrap_or_default();

        let base_url = env::var("LLM_BASE_URL")
            .or_else(|_| env::var("DEEPSEEK_BASE_URL"))
            .unwrap_or_else(|_| kind.default_base_url().to_string());

        let model = env::var("LLM_MODEL")
            .or_else(|_| env::var("MODEL"))
            .unwrap_or_else(|_| kind.default_model().to_string());

        Config {
            llm: LLMConfig {
                provider,
                api_key,
                base_url,
                model,
                temperature: 0.7,
            },
            workspace: Self::resolve_path(
                env::var("WORKSPACE").unwrap_or_else(|_| "./workspace".to_string()),
            ),
            memory_path: Self::resolve_path(
                env::var("MEMORY_PATH").unwrap_or_else(|_| "./memory_db".to_string()),
            ),
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.llm.api_key.is_empty()
    }

    /// 从 .env 文件加载 DEEPSEEK_API_KEY（兼容旧版）
    fn load_api_key_from_env() -> Result<String, ()> {
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
                    // 同时支持 DEEPSEEK_API_KEY 和 LLM_API_KEY
                    for key_name in &["DEEPSEEK_API_KEY", "LLM_API_KEY"] {
                        for line in content.lines() {
                            let line = line.trim();
                            if line.is_empty() || line.starts_with('#') {
                                continue;
                            }
                            if let Some(eq_pos) = line.find('=') {
                                let key = line[..eq_pos].trim();
                                let value = line[eq_pos + 1..].trim().trim_matches('"').trim_matches('\'');
                                if key == *key_name && !value.is_empty() {
                                    return Ok(value.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        Err(())
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
            if !runtime.llm.api_key.is_empty() {
                self.llm.api_key = runtime.llm.api_key;
            }
            if !runtime.llm.base_url.is_empty() {
                self.llm.base_url = runtime.llm.base_url;
            }
            if !runtime.llm.model.is_empty() {
                self.llm.model = runtime.llm.model;
            }
            if !runtime.llm.provider.is_empty() {
                self.llm.provider = runtime.llm.provider;
            }
        }
    }
}

// ==================== 端到端集成测试 ====================

#[cfg(test)]
mod e2e_tests {
    use super::*;
    use crate::llm::provider::ProviderKind;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_dir() -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("qiuyue_e2e_test_{}", id));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn cleanup(dir: &PathBuf) {
        let _ = fs::remove_dir_all(dir);
    }

    // ==================== 测试 1: 全链路 — 环境变量模拟 → Provider ====================

    #[test]
    fn test_full_pipeline_deepseek_to_provider() {
        // 模拟 LLM_PROVIDER=deepseek 时的完整链路
        let llm_config = LLMConfig {
            provider: "deepseek".to_string(),
            api_key: "sk-deepseek-key".to_string(),
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-v4-flash".to_string(),
            temperature: 0.7,
        };
        let provider_config = llm_config.to_provider_config();

        // 验证每一跳都不丢失信息
        assert_eq!(provider_config.kind, ProviderKind::DeepSeek);
        assert_eq!(provider_config.api_key, "sk-deepseek-key");
        assert_eq!(provider_config.base_url, "https://api.deepseek.com");
        assert_eq!(provider_config.model, "deepseek-v4-flash");
    }

    #[test]
    fn test_full_pipeline_openai_to_provider() {
        // 模拟 LLM_PROVIDER=openai 时的完整链路
        let llm_config = LLMConfig {
            provider: "openai".to_string(),
            api_key: "sk-openai-key".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o".to_string(),
            temperature: 0.7,
        };
        let provider_config = llm_config.to_provider_config();

        // 验证没有泄漏 DeepSeek 的任何值
        assert_eq!(provider_config.kind, ProviderKind::OpenAI);
        assert_eq!(provider_config.api_key, "sk-openai-key");
        assert_eq!(provider_config.base_url, "https://api.openai.com/v1");
        assert_eq!(provider_config.model, "gpt-4o");

        // 确保不会意外变成 DeepSeek
        assert_ne!(provider_config.kind, ProviderKind::DeepSeek);
        assert_ne!(provider_config.base_url, "https://api.deepseek.com");
    }

    // ==================== 测试 2: Config 保存/加载 往返 ====================

    #[test]
    fn test_config_save_load_roundtrip_deepseek() {
        let tmp = temp_dir();
        let config = Config {
            llm: LLMConfig {
                provider: "deepseek".to_string(),
                api_key: "sk-deepseek-test".to_string(),
                base_url: "https://api.deepseek.com".to_string(),
                model: "deepseek-v4-flash".to_string(),
                temperature: 0.7,
            },
            workspace: PathBuf::from("./workspace"),
            memory_path: tmp.clone(),
        };

        config.save_to_file(&tmp);

        let loaded = Config::load_from_file(&tmp).expect("应成功加载配置文件");
        assert_eq!(loaded.llm.provider, "deepseek");
        assert_eq!(loaded.llm.api_key, "sk-deepseek-test");
        assert_eq!(loaded.llm.base_url, "https://api.deepseek.com");
        assert_eq!(loaded.llm.model, "deepseek-v4-flash");
        assert!(loaded.is_configured());
        cleanup(&tmp);
    }

    #[test]
    fn test_config_save_load_roundtrip_openai() {
        let tmp = temp_dir();
        let config = Config {
            llm: LLMConfig {
                provider: "openai".to_string(),
                api_key: "sk-openai-test".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4o".to_string(),
                temperature: 0.7,
            },
            workspace: PathBuf::from("./workspace"),
            memory_path: tmp.clone(),
        };

        config.save_to_file(&tmp);

        let loaded = Config::load_from_file(&tmp).expect("应成功加载 OpenAI 配置");
        assert_eq!(loaded.llm.provider, "openai");
        assert_eq!(loaded.llm.base_url, "https://api.openai.com/v1");
        assert_eq!(loaded.llm.model, "gpt-4o");
        assert!(loaded.is_configured());
        cleanup(&tmp);
    }

    #[test]
    fn test_config_save_load_switch_provider() {
        let tmp = temp_dir();

        // 先保存 OpenAI 配置
        let openai_config = Config {
            llm: LLMConfig {
                provider: "openai".to_string(),
                api_key: "sk-openai".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4o".to_string(),
                temperature: 0.7,
            },
            workspace: PathBuf::from("./workspace"),
            memory_path: tmp.clone(),
        };
        openai_config.save_to_file(&tmp);

        // 验证加载后是 OpenAI
        let loaded = Config::load_from_file(&tmp).expect("应加载 OpenAI 配置");
        assert_eq!(loaded.llm.provider, "openai");
        assert!(loaded.llm.base_url.contains("openai"));

        // 再覆盖保存 DeepSeek 配置（模拟用户切换 provider）
        let ds_config = Config {
            llm: LLMConfig {
                provider: "deepseek".to_string(),
                api_key: "sk-deepseek".to_string(),
                base_url: "https://api.deepseek.com".to_string(),
                model: "deepseek-v4-flash".to_string(),
                temperature: 0.7,
            },
            workspace: PathBuf::from("./workspace"),
            memory_path: tmp.clone(),
        };
        ds_config.save_to_file(&tmp);

        // 验证切换后是 DeepSeek（不能残留 OpenAI）
        let loaded2 = Config::load_from_file(&tmp).expect("应加载 DeepSeek 配置");
        assert_eq!(loaded2.llm.provider, "deepseek");
        assert!(loaded2.llm.base_url.contains("deepseek"));
        assert_ne!(loaded2.llm.base_url, "https://api.openai.com/v1");

        cleanup(&tmp);
    }

    // ==================== 测试 3: apply_runtime_config 合并逻辑 ====================

    #[test]
    fn test_apply_runtime_config_preserves_defaults() {
        let tmp = temp_dir();

        // 运行时只覆盖 model
        let partial = Config {
            llm: LLMConfig {
                provider: String::new(),
                api_key: String::new(),
                base_url: String::new(),
                model: "custom-model-v2".to_string(),
                temperature: 0.0,
            },
            workspace: PathBuf::from("./workspace"),
            memory_path: tmp.clone(),
        };
        partial.save_to_file(&tmp);

        // 默认配置带完整默认值
        let mut default_cfg = Config {
            llm: LLMConfig::default(),
            workspace: PathBuf::from("./workspace"),
            memory_path: tmp.clone(),
        };

        default_cfg.apply_runtime_config(&tmp);

        // provider 和 api_key 等空字段不应被覆盖
        assert_eq!(default_cfg.llm.provider, "deepseek");
        // model 被运行时覆盖
        assert_eq!(default_cfg.llm.model, "custom-model-v2");
        cleanup(&tmp);
    }

    #[test]
    fn test_apply_runtime_config_full_override() {
        let tmp = temp_dir();

        let runtime = Config {
            llm: LLMConfig {
                provider: "openai".to_string(),
                api_key: "sk-runtime-key".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4o".to_string(),
                temperature: 0.7,
            },
            workspace: PathBuf::from("./workspace"),
            memory_path: tmp.clone(),
        };
        runtime.save_to_file(&tmp);

        let mut default_cfg = Config {
            llm: LLMConfig::default(),
            workspace: PathBuf::from("./workspace"),
            memory_path: tmp.clone(),
        };

        default_cfg.apply_runtime_config(&tmp);

        assert_eq!(default_cfg.llm.provider, "openai");
        assert_eq!(default_cfg.llm.api_key, "sk-runtime-key");
        assert_eq!(default_cfg.llm.base_url, "https://api.openai.com/v1");
        assert_eq!(default_cfg.llm.model, "gpt-4o");
        assert!(default_cfg.is_configured());
        cleanup(&tmp);
    }

    // ==================== 测试 4: 边界情况 ====================

    #[test]
    fn test_load_from_nonexistent_file_returns_none() {
        let nonexistent = PathBuf::from("./__nonexistent_test_dir__");
        let result = Config::load_from_file(&nonexistent);
        assert!(result.is_none());
    }

    #[test]
    fn test_is_configured_empty_api_key_returns_false() {
        let config = Config {
            llm: LLMConfig {
                provider: "openai".to_string(),
                api_key: String::new(), // 空 key
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4o".to_string(),
                temperature: 0.7,
            },
            workspace: PathBuf::from("./workspace"),
            memory_path: PathBuf::from("./memory"),
        };
        assert!(!config.is_configured());
    }

    #[test]
    fn test_default_config_deepseek_does_not_leak_openai() {
        let default_llm = LLMConfig::default();
        assert_eq!(default_llm.provider, "deepseek");
        assert_eq!(default_llm.base_url, "https://api.deepseek.com");
        assert_eq!(default_llm.model, "deepseek-v4-flash");
        // 确保默认值不会意外变成 OpenAI
        assert_ne!(default_llm.provider, "openai");
        assert_ne!(default_llm.model, "gpt-4o");
    }

    // ==================== 测试 5: ProviderConfig 往返序列化 ====================

    #[test]
    fn test_config_json_roundtrip_openai() {
        let tmp = temp_dir();
        let config = Config {
            llm: LLMConfig {
                provider: "openai".to_string(),
                api_key: "sk-json-test".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4o".to_string(),
                temperature: 0.7,
            },
            workspace: PathBuf::from("./workspace"),
            memory_path: tmp.clone(),
        };

        config.save_to_file(&tmp);

        // 读取原始 JSON，验证不含 "deepseek" 字样
        let json_path = Config::config_file_path(&tmp);
        let raw = fs::read_to_string(&json_path).expect("应能读取 JSON 文件");
        let raw_lower = raw.to_lowercase();
        assert!(
            !raw_lower.contains("deepseek"),
            "OpenAI 配置的 JSON 不应包含 'deepseek':\n{}",
            raw
        );

        // 从 JSON 回读
        let loaded: Config = serde_json::from_str(&raw).expect("JSON 反序列化应成功");
        assert_eq!(loaded.llm.provider, "openai");
        assert_eq!(loaded.llm.model, "gpt-4o");
        cleanup(&tmp);
    }

    #[test]
    fn test_config_json_roundtrip_deepseek() {
        let tmp = temp_dir();
        let config = Config {
            llm: LLMConfig {
                provider: "deepseek".to_string(),
                api_key: "sk-json-ds".to_string(),
                base_url: "https://api.deepseek.com".to_string(),
                model: "deepseek-v4-flash".to_string(),
                temperature: 0.7,
            },
            workspace: PathBuf::from("./workspace"),
            memory_path: tmp.clone(),
        };

        config.save_to_file(&tmp);

        let json_path = Config::config_file_path(&tmp);
        let raw = fs::read_to_string(&json_path).expect("应能读取 JSON 文件");
        let loaded: Config = serde_json::from_str(&raw).expect("JSON 反序列化应成功");
        assert_eq!(loaded.llm.provider, "deepseek");
        assert_eq!(loaded.llm.model, "deepseek-v4-flash");
        cleanup(&tmp);
    }
}
