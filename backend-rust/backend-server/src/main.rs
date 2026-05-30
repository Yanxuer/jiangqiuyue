use std::sync::Arc;
use std::collections::HashMap;

use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Query, State},
    http::Method,
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Mutex};
use tower_http::cors::{Any, CorsLayer};

use backend_core::{
    agent::{Agent, AgentResult},
    cli_executor,
    config::Config,
    doc_reader::{self, DocReader},
    file_tools::FileTools,
    memory::{AgentMemory, MemoryMode, MemoryStatus},
    screen,
    software_scanner,
};

#[derive(Clone)]
struct AppState {
    agent: Arc<Mutex<Agent>>,
    config: Config,
    software_scan_complete: Arc<tokio::sync::Notify>,
    tx: broadcast::Sender<String>,
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    message: String,
    use_screen: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct FilePathRequest {
    path: String,
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryQuery {
    query: String,
}

#[derive(Debug, Deserialize)]
struct MemoryAdd {
    content: String,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    status: String,
}

#[derive(Debug, Serialize)]
struct PendingCommandsResponse {
    pending: Vec<PendingCommandInfo>,
}

#[derive(Debug, Serialize)]
struct PendingCommandInfo {
    command_id: String,
    command: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct ConfigUpdateRequest {
    deepseek_api_key: Option<String>,
    deepseek_base_url: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Serialize)]
struct ConfigResponse {
    configured: bool,
    deepseek_base_url: String,
    model: String,
    has_api_key: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let mut config = Config::from_env();
    log::info!("配置加载完成 (已配置: {})", config.is_configured());

    let memory_path = config.memory_path.clone();

    config.apply_runtime_config(&memory_path);

    log::info!("[诊断] MEMORY_PATH: {:?}", config.memory_path);
    log::info!("[诊断] WORKSPACE: {:?}", config.workspace);
    log::info!("[诊断] HF_HOME: {:?}", std::env::var("HF_HOME").unwrap_or_default());
    log::info!("[诊断] FASTEMBED_CACHE_DIR: {:?}", std::env::var("FASTEMBED_CACHE_DIR").unwrap_or_default());
    if let Ok(hf_home) = std::env::var("HF_HOME") {
        let hub_dir = std::path::Path::new(&hf_home).join("hub");
        let model_dir = hub_dir.join("models--Qdrant--all-MiniLM-L6-v2-onnx");
        log::info!("[诊断] 模型缓存目录存在: {} (snapshots:{}, refs:{})",
            model_dir.display(),
            model_dir.join("snapshots").exists(),
            model_dir.join("refs").exists(),
        );
        if model_dir.join("snapshots").exists() {
            if let Ok(entries) = std::fs::read_dir(model_dir.join("snapshots")) {
                for entry in entries.flatten() {
                    let snapshot_dir = entry.path();
                    log::info!("[诊断]   快照: {} (model.onnx存在:{})",
                        snapshot_dir.file_name().unwrap_or_default().to_string_lossy(),
                        snapshot_dir.join("model.onnx").exists(),
                    );
                }
            }
        }
    }

    let file_tools = Arc::new(FileTools::new(config.workspace.clone()));
    let memory = Arc::new(Mutex::new(
        AgentMemory::new(&config.memory_path)
    ));

    let agent = Arc::new(Mutex::new(Agent::new(
        config.clone(),
        file_tools,
        memory,
    )));

    let (tx, _rx) = broadcast::channel::<String>(100);

    let state = AppState {
        agent,
        config: config.clone(),
        software_scan_complete: Arc::new(tokio::sync::Notify::new()),
        tx,
    };

    let scan_state = state.clone();
    tokio::spawn(async move {
        log::info!("[启动] 开始后台静默扫描电脑软件...");
        let software_list = software_scanner::scan_and_cache(&scan_state.config.memory_path);
        log::info!("[启动] 软件扫描完成，已缓存 {} 个软件", software_list.len());
        scan_state.software_scan_complete.notify_waiters();
    });

    let state_clone = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        state_clone.software_scan_complete.notify_waiters();
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT])
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/config", get(config_get_handler).put(config_update_handler))
        .route("/chat", post(chat_handler))
        .route("/file/read", post(file_read_handler))
        .route("/file/write", post(file_write_handler))
        .route("/files", get(file_list_handler))
        .route("/memory/search", post(memory_search_handler))
        .route("/memory/add", post(memory_add_handler))
        .route("/memory/status", get(memory_status_handler))
        .route("/memory/retry", post(memory_retry_handler))
        .route("/memory/switch", post(memory_switch_handler))
        .route("/cli/pending", get(cli_pending_handler))
        .route("/cli/confirm/{command_id}", post(cli_confirm_handler))
        .route("/cli/reject/{command_id}", post(cli_reject_handler))
        .route("/software/status", get(software_status_handler))
        .route("/software/list", get(software_list_handler))
        .route("/software/search", get(software_search_handler))
        .route("/docs/list", get(docs_list_handler))
        .route("/docs/read", get(docs_read_handler))
        .route("/docs/select-path", post(docs_select_path_handler))
        .route("/docs/recent-paths", get(docs_recent_paths_handler))
        .route("/ws", get(ws_handler))
        .layer(cors)
        .with_state(state);

    let addr = "127.0.0.1:8000";
    log::info!("服务器启动于 http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// ==================== HTTP Handlers ====================

async fn health_check() -> Json<StatusResponse> {
    Json(StatusResponse {
        status: "ok".to_string(),
    })
}

async fn config_get_handler(
    State(state): State<AppState>,
) -> Json<ConfigResponse> {
    let agent = state.agent.lock().await;
    let cfg = agent.get_config();
    Json(ConfigResponse {
        configured: cfg.is_configured(),
        deepseek_base_url: cfg.deepseek_base_url.clone(),
        model: cfg.model.clone(),
        has_api_key: !cfg.deepseek_api_key.is_empty(),
    })
}

async fn config_update_handler(
    State(state): State<AppState>,
    Json(req): Json<ConfigUpdateRequest>,
) -> Json<serde_json::Value> {
    let mut agent = state.agent.lock().await;
    let mut new_config = agent.get_config().clone();

    if let Some(key) = &req.deepseek_api_key {
        if !key.is_empty() {
            new_config.deepseek_api_key = key.clone();
        }
    }
    if let Some(url) = &req.deepseek_base_url {
        if !url.is_empty() {
            new_config.deepseek_base_url = url.clone();
        }
    }
    if let Some(model) = &req.model {
        if !model.is_empty() {
            new_config.model = model.clone();
        }
    }

    new_config.save_to_file(&state.config.memory_path);
    agent.update_config(new_config);

    Json(serde_json::json!({
        "success": true,
        "configured": !agent.get_config().deepseek_api_key.is_empty()
    }))
}

async fn chat_handler(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Json<AgentResult> {
    let img_b64 = if req.use_screen.unwrap_or(false) {
        screen::capture_screen(1).ok()
    } else {
        None
    };

    let mut agent = state.agent.lock().await;
    let result = agent
        .run(&req.message, img_b64.as_deref())
        .await
        .unwrap_or_else(|e| AgentResult {
            reply: format!("抱歉，出错了: {}", e),
            tool_calls: Vec::new(),
        });

    let _ = state.tx.send(
        serde_json::json!({
            "type": "agent_reply",
            "reply": result.reply,
            "tools_used": result.tool_calls
        })
        .to_string(),
    );

    Json(result)
}

async fn file_read_handler(
    State(state): State<AppState>,
    Json(req): Json<FilePathRequest>,
) -> Json<serde_json::Value> {
    let agent = state.agent.lock().await;
    let content = agent.file_tools().read_file(&req.path);
    Json(serde_json::json!({"content": content}))
}

async fn file_write_handler(
    State(state): State<AppState>,
    Json(req): Json<FilePathRequest>,
) -> Json<serde_json::Value> {
    let agent = state.agent.lock().await;
    let content = req.content.unwrap_or_default();
    let result = agent.file_tools().write_file(&req.path, &content);
    Json(serde_json::json!({"result": result}))
}

async fn file_list_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let agent = state.agent.lock().await;
    let dir = params.get("dir").map(|s| s.as_str()).unwrap_or("");
    let files = agent.file_tools().list_files(dir);
    Json(serde_json::json!({"files": files}))
}

async fn memory_search_handler(
    State(state): State<AppState>,
    Json(req): Json<MemoryQuery>,
) -> Json<serde_json::Value> {
    let agent = state.agent.lock().await;
    let mut memory = agent.memory().lock().await;
    match memory.search(&req.query, 5) {
        Ok(results) => Json(serde_json::json!({"results": results})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

async fn memory_add_handler(
    State(state): State<AppState>,
    Json(req): Json<MemoryAdd>,
) -> Json<serde_json::Value> {
    let agent = state.agent.lock().await;
    let mut memory = agent.memory().lock().await;
    match memory.add(&req.content, "chat") {
        Ok(id) => Json(serde_json::json!({"id": id})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

async fn memory_status_handler(
    State(state): State<AppState>,
) -> Json<MemoryStatus> {
    let agent = state.agent.lock().await;
    let memory = agent.memory().lock().await;
    Json(memory.get_status())
}

async fn memory_retry_handler(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let agent = state.agent.lock().await;
    let mut memory = agent.memory().lock().await;
    match memory.retry_embedder() {
        Ok(()) => {
            let status = memory.get_status();
            Json(serde_json::json!({
                "success": true,
                "mode": status.mode,
                "available": status.available,
                "retry_count": status.retry_count,
            }))
        }
        Err(e) => {
            let status = memory.get_status();
            Json(serde_json::json!({
                "success": false,
                "mode": status.mode,
                "available": status.available,
                "retry_count": status.retry_count,
                "last_error": status.last_error,
                "error": e.to_string(),
            }))
        }
    }
}

async fn memory_switch_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let mode_str = params.get("mode").map(|s| s.as_str()).unwrap_or("sql");
    let mode = match mode_str {
        "vector" => MemoryMode::Vector,
        _ => MemoryMode::Sql,
    };

    let agent = state.agent.lock().await;
    let mut memory = agent.memory().lock().await;
    memory.switch_mode(mode.clone());

    Json(serde_json::json!({
        "success": true,
        "mode": mode.to_string(),
        "available": memory.is_available(),
    }))
}

async fn cli_pending_handler(
    State(state): State<AppState>,
) -> Json<PendingCommandsResponse> {
    let agent = state.agent.lock().await;
    let commands = agent.pending_commands.lock().await;
    let pending: Vec<PendingCommandInfo> = commands
        .iter()
        .map(|(cmd_id, data)| PendingCommandInfo {
            command_id: cmd_id.clone(),
            command: data.command.clone(),
            reason: data.reason.clone(),
        })
        .collect();
    Json(PendingCommandsResponse { pending })
}

async fn cli_confirm_handler(
    State(state): State<AppState>,
    axum::extract::Path(command_id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    let agent = state.agent.lock().await;
    let mut commands = agent.pending_commands.lock().await;
    let cmd_data = commands.remove(&command_id);

    match cmd_data {
        Some(data) => {
            let result = cli_executor::execute_command(&data.command, data.cwd.as_deref()).await;
            Json(serde_json::json!({
                "success": true,
                "result": result
            }))
        }
        None => Json(serde_json::json!({
            "success": false,
            "error": "命令不存在或已过期"
        })),
    }
}

async fn cli_reject_handler(
    State(state): State<AppState>,
    axum::extract::Path(command_id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    let agent = state.agent.lock().await;
    let mut commands = agent.pending_commands.lock().await;
    commands.remove(&command_id);
    Json(serde_json::json!({
        "success": true,
        "result": "操作已取消"
    }))
}

async fn software_status_handler(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let scanned = software_scanner::is_software_scanned(&state.config.memory_path);
    let (categories, total) = if scanned {
        let software_list = software_scanner::load_software_cache(&state.config.memory_path);
        let cats = software_scanner::get_all_categories(&software_list);
        let count: usize = cats.values().map(|v| v.len()).sum();
        (cats, count)
    } else {
        (HashMap::new(), 0)
    };

    Json(serde_json::json!({
        "scanned": scanned,
        "categories": categories,
        "total": total
    }))
}

async fn software_list_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let scanned = software_scanner::is_software_scanned(&state.config.memory_path);
    if !scanned {
        return Json(serde_json::json!({"software": [], "scanned": false}));
    }

    let software_list = software_scanner::load_software_cache(&state.config.memory_path);
    let category = params.get("category").map(|s| s.as_str()).unwrap_or("");

    let result: Vec<serde_json::Value> = if category.is_empty() {
        let cats = software_scanner::get_all_categories(&software_list);
        cats.into_iter()
            .flat_map(|(cat, names)| {
                names.into_iter().map(move |name| {
                    serde_json::json!({"name": name, "category": cat})
                })
            })
            .collect()
    } else {
        let sw_list = software_scanner::get_software_by_category(&software_list, category);
        sw_list
            .into_iter()
            .map(|sw| serde_json::json!({"name": sw.name, "category": sw.category}))
            .collect()
    };

    Json(serde_json::json!({"software": result, "scanned": true}))
}

async fn software_search_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let scanned = software_scanner::is_software_scanned(&state.config.memory_path);
    if !scanned {
        return Json(serde_json::json!({"software": [], "scanned": false}));
    }

    let query = params.get("query").map(|s| s.as_str()).unwrap_or("");
    let software_list = software_scanner::load_software_cache(&state.config.memory_path);
    let results = software_scanner::search_software(query, &software_list, 10);

    let software: Vec<serde_json::Value> = results
        .iter()
        .map(|sw| {
            serde_json::json!({
                "name": sw.name,
                "path": sw.exec_path,
                "category": sw.category,
                "description": sw.description,
                "score": 1.0
            })
        })
        .collect();

    Json(serde_json::json!({"software": software, "scanned": true}))
}

async fn docs_list_handler(
    Query(params): Query<HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let path = params.get("path").map(|s| s.as_str()).unwrap_or("");
    let result = DocReader::list_directory(path);
    Json(serde_json::to_value(result).unwrap_or_default())
}

async fn docs_read_handler(
    Query(params): Query<HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let path = params.get("path").map(|s| s.as_str()).unwrap_or("");
    let result = DocReader::read_text_file(path);
    Json(serde_json::to_value(result).unwrap_or_default())
}

#[derive(Debug, Deserialize)]
struct SelectPathRequest {
    path: String,
}

async fn docs_select_path_handler(
    State(state): State<AppState>,
    Json(req): Json<SelectPathRequest>,
) -> Json<serde_json::Value> {
    let recent = doc_reader::add_recent_path(&state.config.memory_path, &req.path);
    let dir_info = DocReader::list_directory(&req.path);
    Json(serde_json::json!({
        "recent_paths": recent,
        "directory": dir_info
    }))
}

async fn docs_recent_paths_handler(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let recent = doc_reader::load_recent_paths(&state.config.memory_path);
    Json(serde_json::json!({"recent_paths": recent}))
}

// ==================== WebSocket Handler ====================

async fn ws_handler(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();

    let mut rx = state.tx.subscribe();

    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                let data: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let action = data.get("action").and_then(|v| v.as_str()).unwrap_or("");

                let valid_actions = ["capture_and_ask", "show_window", "hide_window"];
                if !valid_actions.contains(&action) {
                    continue;
                }

                match action {
                    "capture_and_ask" => {
                        let img_b64 = screen::capture_screen(1).ok();
                        let question = data
                            .get("question")
                            .and_then(|v| v.as_str())
                            .unwrap_or("描述一下我屏幕上显示的内容");
                        let question = &question[..question.len().min(500)];

                        let mut agent = state.agent.lock().await;
                        let result = agent.run(question, img_b64.as_deref()).await;
                        if let Ok(result) = result {
                            let _ = state.tx.send(
                                serde_json::json!({
                                    "type": "screen_result",
                                    "data": result
                                })
                                .to_string(),
                            );
                        }
                    }
                    "show_window" | "hide_window" => {
                        let _ = state.tx.send(
                            serde_json::json!({
                                "type": "command",
                                "action": if action == "show_window" { "show_main" } else { "hide_main" }
                            })
                            .to_string(),
                        );
                    }
                    _ => {}
                }
            }
        }
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}