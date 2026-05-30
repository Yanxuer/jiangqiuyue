use anyhow::{Context, Result};
use fastembed::{Embedding, TextEmbedding, UserDefinedEmbeddingModel, TokenizerFiles, Pooling, InitOptionsUserDefined, read_file_to_bytes};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;
use chrono::Utc;
use uuid::Uuid;

const MAX_RETRY_COUNT: u32 = 3;
const MAX_VECTOR_LOGS: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMode {
    Vector,
    Sql,
    Retrying,
}

impl std::fmt::Display for MemoryMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryMode::Vector => write!(f, "vector"),
            MemoryMode::Sql => write!(f, "sql"),
            MemoryMode::Retrying => write!(f, "retrying"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorLogEntry {
    pub time: String,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStatus {
    pub mode: MemoryMode,
    pub available: bool,
    pub retry_count: u32,
    pub max_retries: u32,
    pub last_error: Option<String>,
    pub vector_logs: Vec<VectorLogEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MemoryItem {
    pub id: String,
    pub content: String,
    pub category: String,
    pub time: String,
    pub distance: Option<f32>,
}

pub struct AgentMemory {
    embedder: Option<TextEmbedding>,
    db: Connection,
    mode: MemoryMode,
    retry_count: u32,
    last_error: Option<String>,
    vector_logs: Vec<VectorLogEntry>,
}

impl AgentMemory {
    fn add_vector_log(&mut self, level: &str, message: String) {
        let entry = VectorLogEntry {
            time: Utc::now().format("%H:%M:%S").to_string(),
            level: level.to_string(),
            message: message.clone(),
        };
        self.vector_logs.push(entry);
        if self.vector_logs.len() > MAX_VECTOR_LOGS {
            self.vector_logs.remove(0);
        }
        log::info!(target: "vector_memory", "[{}] {}", level, message);
    }

    pub fn new(memory_path: &PathBuf) -> Self {
        fs::create_dir_all(memory_path).unwrap_or_else(|e| {
            log::error!("创建记忆目录失败: {}", e);
        });
        let db_path = memory_path.join("memory.db");
        let db = Connection::open(&db_path).unwrap_or_else(|e| {
            log::error!("打开SQLite数据库失败: {}，使用内存数据库", e);
            Connection::open_in_memory().unwrap()
        });

        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                category TEXT NOT NULL DEFAULT 'chat',
                time TEXT NOT NULL,
                embedding BLOB
            );
            CREATE INDEX IF NOT EXISTS idx_memories_category ON memories(category);
            CREATE INDEX IF NOT EXISTS idx_memories_time ON memories(time);",
        ).unwrap_or_else(|e| {
            log::error!("创建记忆表失败: {}", e);
        });

        let mut memory = AgentMemory {
            embedder: None,
            db,
            mode: MemoryMode::Sql,
            retry_count: 0,
            last_error: None,
            vector_logs: Vec::new(),
        };

        memory.try_init_embedder();
        memory
    }

    fn try_init_embedder(&mut self) {
        self.add_vector_log("INFO", "正在初始化嵌入模型...".to_string());

        let result = self.init_embedder_from_local();

        match result {
            Ok(embedder) => {
                self.embedder = Some(embedder);
                self.mode = MemoryMode::Vector;
                self.retry_count = 0;
                self.last_error = None;
                self.add_vector_log("INFO", "嵌入模型初始化成功，向量搜索已启用".to_string());
                log::info!("嵌入模型初始化成功");
            }
            Err(e) => {
                let err_msg = format!("{:#}", e);
                self.embedder = None;
                self.mode = MemoryMode::Sql;
                self.retry_count += 1;
                self.last_error = Some(err_msg.clone());
                self.add_vector_log("ERROR", format!("初始化嵌入模型失败: {}", err_msg));
                log::error!("初始化嵌入模型失败: {:#}", e);
            }
        }
    }

    fn init_embedder_from_local(&mut self) -> Result<TextEmbedding> {
        let hf_home = std::env::var("HF_HOME").context("HF_HOME 环境变量未设置")?;
        let model_dir = std::path::Path::new(&hf_home)
            .join("hub")
            .join("models--Qdrant--all-MiniLM-L6-v2-onnx")
            .join("snapshots");

        if !model_dir.exists() {
            anyhow::bail!("模型快照目录不存在: {}", model_dir.display());
        }

        // Find the snapshot directory (the only subdirectory in snapshots/)
        let mut snapshot_dir: Option<PathBuf> = None;
        if let Ok(entries) = std::fs::read_dir(&model_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let onnx_file = path.join("model.onnx");
                    if onnx_file.exists() {
                        snapshot_dir = Some(path);
                        self.add_vector_log("INFO", format!("找到模型快照: {} (model.onnx存在)", 
                            entry.file_name().to_string_lossy()));
                        break;
                    }
                }
            }
        }

        let snapshot_dir = snapshot_dir.context("未找到包含 model.onnx 的快照目录")?;

        self.add_vector_log("INFO", format!("正在从本地加载模型文件: {}", snapshot_dir.display()));

        let onnx_bytes = read_file_to_bytes(&snapshot_dir.join("model.onnx"))
            .context("读取 model.onnx 失败")?;

        let tokenizer_path = snapshot_dir.join("tokenizer.json");
        let config_path = snapshot_dir.join("config.json");
        let special_tokens_map_path = snapshot_dir.join("special_tokens_map.json");
        let tokenizer_config_path = snapshot_dir.join("tokenizer_config.json");

        let mut missing = Vec::new();
        if !tokenizer_path.exists() { missing.push("tokenizer.json"); }
        if !config_path.exists() { missing.push("config.json"); }
        if !special_tokens_map_path.exists() { missing.push("special_tokens_map.json"); }
        if !tokenizer_config_path.exists() { missing.push("tokenizer_config.json"); }
        if !missing.is_empty() {
            anyhow::bail!("缺少tokenizer文件: {}", missing.join(", "));
        }

        let tokenizer_files = TokenizerFiles {
            tokenizer_file: read_file_to_bytes(&tokenizer_path)?,
            config_file: read_file_to_bytes(&config_path)?,
            special_tokens_map_file: read_file_to_bytes(&special_tokens_map_path)?,
            tokenizer_config_file: read_file_to_bytes(&tokenizer_config_path)?,
        };

        let model = UserDefinedEmbeddingModel::new(onnx_bytes, tokenizer_files)
            .with_pooling(Pooling::Mean);

        let text_embedding = TextEmbedding::try_new_from_user_defined(
            model,
            InitOptionsUserDefined::default(),
        )?;

        Ok(text_embedding)
    }

    pub fn retry_embedder(&mut self) -> Result<()> {
        if self.mode == MemoryMode::Retrying {
            return Err(anyhow::anyhow!("正在重试中，请稍候"));
        }
        self.mode = MemoryMode::Retrying;
        self.add_vector_log("INFO", format!("开始第 {} 次重试初始化嵌入模型...", self.retry_count + 1));
        self.try_init_embedder();
        if self.embedder.is_some() {
            self.add_vector_log("INFO", "向量模式重试成功".to_string());
            Ok(())
        } else {
            let err_msg = self.last_error.clone().unwrap_or_default();
            self.add_vector_log("ERROR", format!("重试失败 ({}/{})", self.retry_count, MAX_RETRY_COUNT));
            Err(anyhow::anyhow!(err_msg))
        }
    }

    pub fn switch_mode(&mut self, mode: MemoryMode) {
        match mode {
            MemoryMode::Vector => {
                if self.embedder.is_some() {
                    self.mode = MemoryMode::Vector;
                    self.add_vector_log("INFO", "手动切换到向量模式".to_string());
                } else {
                    self.add_vector_log("WARN", "无法切换到向量模式：嵌入模型未初始化".to_string());
                }
            }
            MemoryMode::Sql => {
                self.mode = MemoryMode::Sql;
                self.add_vector_log("INFO", "手动切换到SQL模式".to_string());
            }
            MemoryMode::Retrying => {}
        }
    }

    pub fn get_status(&self) -> MemoryStatus {
        MemoryStatus {
            mode: self.mode.clone(),
            available: self.embedder.is_some(),
            retry_count: self.retry_count,
            max_retries: MAX_RETRY_COUNT,
            last_error: self.last_error.clone(),
            vector_logs: self.vector_logs.clone(),
        }
    }

    pub fn is_available(&self) -> bool {
        self.embedder.is_some()
    }

    pub fn add(&mut self, content: &str, category: &str) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let time = Utc::now().to_rfc3339();

        if let Some(ref embedder) = self.embedder {
            let embeddings = embedder
                .embed(vec![content], None)
                .context("生成嵌入向量失败")?;

            let embedding_bytes = self.serialize_embedding(&embeddings[0]);

            self.db
                .execute(
                    "INSERT INTO memories (id, content, category, time, embedding) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, content, category, time, embedding_bytes],
                )
                .context("插入记忆失败")?;
            self.add_vector_log("INFO", format!("添加记忆 (向量模式): {}...", &content[..content.len().min(50)]));
        } else {
            self.db
                .execute(
                    "INSERT INTO memories (id, content, category, time, embedding) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, content, category, time, Vec::<u8>::new()],
                )
                .context("插入记忆失败")?;
            self.add_vector_log("INFO", format!("添加记忆 (SQL模式): {}...", &content[..content.len().min(50)]));
        }

        Ok(id)
    }

    pub fn search(&mut self, query: &str, n_results: usize) -> Result<Vec<MemoryItem>> {
        let query_embedding = if let Some(ref embedder) = self.embedder {
            let embeddings = embedder
                .embed(vec![query], None)
                .context("生成查询嵌入向量失败")?;
            self.add_vector_log("INFO", format!("向量搜索: \"{}\" (top {})", &query[..query.len().min(30)], n_results));
            Some(embeddings[0].clone())
        } else {
            self.add_vector_log("INFO", format!("SQL搜索: \"{}\" (top {})", &query[..query.len().min(30)], n_results));
            None
        };

        if let Some(ref query_emb) = query_embedding {
            let mut stmt = self
                .db
                .prepare("SELECT id, content, category, time, embedding FROM memories")
                .context("查询记忆失败")?;

            let mut scored: Vec<(f32, MemoryItem)> = Vec::new();

            let rows = stmt
                .query_map([], |row| {
                    let id: String = row.get(0)?;
                    let content: String = row.get(1)?;
                    let category: String = row.get(2)?;
                    let time: String = row.get(3)?;
                    let embedding_blob: Vec<u8> = row.get(4)?;
                    Ok((id, content, category, time, embedding_blob))
                })
                .context("读取记忆记录失败")?;

            for row in rows {
                if let Ok((id, content, category, time, embedding_blob)) = row {
                    if let Some(stored_emb) = self.deserialize_embedding(&embedding_blob) {
                        let distance = cosine_similarity(query_emb, &stored_emb);
                        scored.push((
                            distance,
                            MemoryItem {
                                id,
                                content,
                                category,
                                time,
                                distance: Some(distance),
                            },
                        ));
                    }
                }
            }

            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            Ok(scored.into_iter().take(n_results).map(|(_, item)| item).collect())
        } else {
            // 降级模式：使用SQL LIKE进行文本匹配搜索
            let search_pattern = format!("%{}%", query.replace('%', "%%"));
            let mut stmt = self
                .db
                .prepare(
                    "SELECT id, content, category, time FROM memories \
                     WHERE content LIKE ?1 \
                     ORDER BY time DESC LIMIT ?2"
                )
                .context("查询记忆失败")?;

            let items = stmt
                .query_map(params![search_pattern, n_results as i64], |row| {
                    Ok(MemoryItem {
                        id: row.get(0)?,
                        content: row.get(1)?,
                        category: row.get(2)?,
                        time: row.get(3)?,
                        distance: None,
                    })
                })
                .context("读取记忆记录失败")?
                .filter_map(|r| r.ok())
                .collect();

            Ok(items)
        }
    }

    pub fn get_recent(&self, n: usize) -> Result<Vec<MemoryItem>> {
        let mut stmt = self
            .db
            .prepare("SELECT id, content, category, time FROM memories ORDER BY time DESC LIMIT ?1")
            .context("查询最近记忆失败")?;

        let items = stmt
            .query_map(params![n as i64], |row| {
                Ok(MemoryItem {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    category: row.get(2)?,
                    time: row.get(3)?,
                    distance: None,
                })
            })
            .context("读取最近记忆失败")?
            .filter_map(|r| r.ok())
            .collect();

        Ok(items)
    }

    fn serialize_embedding(&self, embedding: &Embedding) -> Vec<u8> {
        let bytes: Vec<u8> = embedding
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        bytes
    }

    fn deserialize_embedding(&self, bytes: &[u8]) -> Option<Vec<f32>> {
        if bytes.len() % 4 != 0 {
            return None;
        }
        let chunks = bytes.chunks_exact(4);
        let result: Vec<f32> = chunks
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        Some(result)
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}