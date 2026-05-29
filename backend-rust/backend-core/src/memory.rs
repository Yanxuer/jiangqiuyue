use anyhow::{Context, Result};
use fastembed::{Embedding, EmbeddingModel, InitOptions, TextEmbedding};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;
use chrono::Utc;
use uuid::Uuid;

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
}

impl AgentMemory {
    pub fn new(memory_path: &PathBuf) -> Result<Self> {
        fs::create_dir_all(memory_path).context("创建记忆目录失败")?;
        let db_path = memory_path.join("memory.db");
        let db = Connection::open(&db_path).context("打开SQLite数据库失败")?;

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
        )
        .context("创建记忆表失败")?;

        let embedder = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2)
                .with_show_download_progress(false),
        )
        .context("初始化嵌入模型失败")?;

        Ok(AgentMemory { embedder: Some(embedder), db })
    }

    pub fn new_empty(memory_path: &PathBuf) -> Self {
        fs::create_dir_all(memory_path).ok();
        let db_path = memory_path.join("memory.db");
        let db = Connection::open(&db_path).unwrap_or_else(|_| {
            log::error!("无法打开记忆数据库，使用内存数据库");
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
        ).ok();

        log::warn!("记忆系统以降级模式启动（无嵌入模型），向量搜索功能不可用");
        AgentMemory { embedder: None, db }
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
        } else {
            self.db
                .execute(
                    "INSERT INTO memories (id, content, category, time, embedding) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, content, category, time, Vec::<u8>::new()],
                )
                .context("插入记忆失败")?;
        }

        Ok(id)
    }

    pub fn search(&mut self, query: &str, n_results: usize) -> Result<Vec<MemoryItem>> {
        let query_embedding = if let Some(ref embedder) = self.embedder {
            let embeddings = embedder
                .embed(vec![query], None)
                .context("生成查询嵌入向量失败")?;
            Some(embeddings[0].clone())
        } else {
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