//! 桌面控制模块 — 通过 cua-driver (MCP over stdio) 实现后台桌面操控
//!
//! 提供截图、鼠标、键盘、窗口管理等能力，不抢用户焦点。
//! cua-driver 未安装时自动降级，返回明确错误提示。

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

// ==================== MCP JSON-RPC 数据结构 ====================

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    params: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: Option<u64>,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)]
    code: i64,
    message: String,
}

// ==================== 工具调用结果 ====================

#[derive(Debug, Serialize)]
pub struct DesktopResult {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows: Option<Vec<WindowInfo>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WindowInfo {
    pub title: String,
    pub pid: u32,
    pub window_id: u32,
    pub process_name: String,
}

// ==================== CuaDriverClient ====================

/// CuaDriver 客户端 — 惰性启动，通过 MCP/JSON-RPC over stdio 通信
pub struct CuaDriverClient {
    /// 子进程句柄
    child: Option<Child>,
    /// stdin 写入端
    stdin: Option<ChildStdin>,
    /// stdout 读取端（BufReader, 按行读取）
    #[allow(clippy::type_complexity)]
    stdout_reader: Option<BufReader<ChildStdout>>,
    /// 是否已初始化 MCP 握手
    initialized: bool,
    /// 请求 ID 计数器
    request_id: u64,
    /// cua-driver 是否可用
    available: bool,
    /// 可用性已检测过（避免重复检测）
    checked: bool,
    /// 测试用：覆盖二进制路径，跳过 PATH 搜索
    #[cfg(test)]
    binary_path_override: Option<std::path::PathBuf>,
}

impl CuaDriverClient {
    pub fn new() -> Self {
        Self {
            child: None,
            stdin: None,
            stdout_reader: None,
            initialized: false,
            request_id: 1,
            available: false,
            checked: false,
            #[cfg(test)]
            binary_path_override: None,
        }
    }

    /// 设置测试用二进制路径，跳过 PATH 搜索
    #[cfg(test)]
    pub fn set_binary_path(&mut self, path: std::path::PathBuf) {
        self.binary_path_override = Some(path);
    }

    /// 惰性启动：首次调用时检测并启动 cua-driver 子进程。
    ///
    /// 如果 cua-driver 在服务启动后安装，再次调用时会自动检测到
    /// 并尝试启动，无需重启服务。仅在启动失败时才建议重启。
    pub async fn ensure_started(&mut self) -> Result<(), String> {
        if self.available && self.initialized {
            return Ok(());
        }

        // 如果之前检测过且不可用，重新检测（用户可能安装后未重启）
        if self.checked && !self.available {
            log::info!(
                "[DesktopControl] 之前未检测到 cua-driver，重新扫描..."
            );
            if !self.check_binary_exists() {
                return Err(
                    "cua-driver 未安装或不可用。请运行安装脚本:\n  \
                     irm https://raw.githubusercontent.com/trycua/cua/main/libs/cua-driver/scripts/install.ps1 | iex"
                        .into(),
                );
            }
            // 检测到新安装的 cua-driver，重置状态走正常启动流程
            log::info!(
                "[DesktopControl] 检测到新安装的 cua-driver，将自动启动（无需重启服务）"
            );
            self.checked = false;
        }

        self.checked = true;

        // 启动 cua-driver MCP 子进程
        let binary_path = match self.find_binary_path() {
            Some(p) => p,
            None => {
                log::warn!("[DesktopControl] cua-driver 未找到，桌面控制功能不可用");
                self.available = false;
                return Err(
                    "cua-driver 未安装或不可用。请运行安装脚本:\n  \
                 irm https://raw.githubusercontent.com/trycua/cua/main/libs/cua-driver/scripts/install.ps1 | iex"
                        .into(),
                );
            }
        };
        log::info!("[DesktopControl] 正在启动 cua-driver mcp ({})...", binary_path.display());
        let mut child = match Command::new(&binary_path)
            .arg("mcp")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                self.available = false;
                log::error!(
                    "[DesktopControl] 无法启动 cua-driver 子进程: {}。\
                     如刚安装 cua-driver，请重启服务后再试",
                    e
                );
                return Err(format!(
                    "无法启动 cua-driver 子进程: {}。{}\n  \
                     请重启江秋月服务后再试",
                    e,
                    if self.checked {
                        "如刚安装 cua-driver，可能需要重启终端使 PATH 生效。"
                    } else {
                        ""
                    }
                ));
            }
        };

        let stdin = child
            .stdin
            .take()
            .ok_or("无法获取 cua-driver stdin")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("无法获取 cua-driver stdout")?;

        let stdout_reader = BufReader::new(stdout);

        self.child = Some(child);
        self.stdin = Some(stdin);
        self.stdout_reader = Some(stdout_reader);
        self.available = true;

        // MCP 初始化握手
        match self.initialize_handshake().await {
            Ok(()) => {
                log::info!("[DesktopControl] cua-driver 启动成功，MCP 握手完成");
                Ok(())
            }
            Err(e) => {
                // 握手失败，清理状态
                self.available = false;
                self.child = None;
                self.stdin = None;
                self.stdout_reader = None;
                log::error!(
                    "[DesktopControl] MCP 握手失败: {}。如刚安装 cua-driver，请重启服务",
                    e
                );
                Err(format!(
                    "cua-driver MCP 握手失败: {}。请重启江秋月服务后再试",
                    e
                ))
            }
        }
    }

    /// 检测 cua-driver 二进制是否存在（PATH 或常见安装路径，或测试覆盖路径）
    fn check_binary_exists(&self) -> bool {
        self.find_binary_path().is_some()
    }

    /// 查找 cua-driver 的完整路径
    fn find_binary_path(&self) -> Option<std::path::PathBuf> {
        // 测试覆盖路径优先
        #[cfg(test)]
        if let Some(ref override_path) = self.binary_path_override {
            return Some(override_path.clone());
        }

        // 检查 PATH 环境变量中的每个目录
        if let Ok(path_var) = std::env::var("PATH") {
            for dir in path_var.split(';') {
                let dir = dir.trim();
                if dir.is_empty() {
                    continue;
                }
                // 检查常见扩展名
                for ext in &[".exe", ".cmd", ".bat", ".com"] {
                    let candidate = std::path::PathBuf::from(dir).join(format!("cua-driver{}", ext));
                    if candidate.exists() {
                        log::info!("[DesktopControl] 检测到 cua-driver: {}", candidate.display());
                        return Some(candidate);
                    }
                }
            }
        }

        // 检查常见安装路径
        let home = std::env::var("USERPROFILE").unwrap_or_default();
        let candidate = std::path::PathBuf::from(&home)
            .join(".cargo")
            .join("bin")
            .join("cua-driver.exe");
        if candidate.exists() {
            log::info!("[DesktopControl] 在 ~/.cargo/bin 中找到 cua-driver");
            return Some(candidate);
        }

        None
    }

    /// MCP 协议初始化握手
    async fn initialize_handshake(&mut self) -> Result<(), String> {
        // 发送 initialize 请求
        let init_params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "jiangqiuyue-desktop-control",
                "version": "1.0.0"
            }
        });

        let response = self
            .send_request("initialize", init_params)
            .await?;

        log::info!(
            "[DesktopControl] MCP initialize 响应: {}",
            serde_json::to_string(&response).unwrap_or_default()
        );

        // 发送 initialized 通知（不需要响应）
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });

        if let Some(ref mut stdin) = self.stdin {
            let mut msg = serde_json::to_vec(&notification)
                .map_err(|e| format!("序列化 initialized 通知失败: {}", e))?;
            msg.push(b'\n');
            stdin
                .write_all(&msg)
                .await
                .map_err(|e| format!("发送 initialized 通知失败: {}", e))?;
            stdin
                .flush()
                .await
                .map_err(|e| format!("flush stdin 失败: {}", e))?;
        }

        self.initialized = true;
        Ok(())
    }

    /// 发送 JSON-RPC 请求并等待响应
    async fn send_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let id = self.request_id;
        self.request_id += 1;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        };

        let mut request_bytes = serde_json::to_vec(&request)
            .map_err(|e| format!("序列化请求失败: {}", e))?;
        request_bytes.push(b'\n');

        // 写入 stdin
        if let Some(ref mut stdin) = self.stdin {
            stdin
                .write_all(&request_bytes)
                .await
                .map_err(|e| format!("写入 stdin 失败: {}", e))?;
            stdin
                .flush()
                .await
                .map_err(|e| format!("flush stdin 失败: {}", e))?;
        } else {
            return Err("cua-driver stdin 不可用".into());
        }

        // 读取 stdout 响应（按行）
        if let Some(ref mut reader) = self.stdout_reader {
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .await
                .map_err(|e| format!("读取 cua-driver 响应失败: {}", e))?;

            let response: JsonRpcResponse = serde_json::from_str(line.trim())
                .map_err(|e| {
                    format!(
                        "解析 cua-driver 响应失败: {} (原始: {})",
                        e,
                        line.trim()
                    )
                })?;

            if let Some(err) = response.error {
                return Err(format!("cua-driver 返回错误: {}", err.message));
            }

            Ok(response.result.unwrap_or(serde_json::Value::Null))
        } else {
            Err("cua-driver stdout 不可用".into())
        }
    }

    /// 调用 MCP 工具
    async fn call_tool(
        &mut self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let params = serde_json::json!({
            "name": tool_name,
            "arguments": arguments
        });

        self.send_request("tools/call", params).await
    }

    // ==================== 对外工具接口 ====================

    /// 截取屏幕/窗口截图
    pub async fn screenshot(
        &mut self,
        window_title: Option<&str>,
        monitor: Option<usize>,
    ) -> DesktopResult {
        match self.ensure_started().await {
            Err(e) => return DesktopResult::fail(e),
            Ok(()) => {}
        }

        let args = if let Some(title) = window_title {
            // 先查找窗口
            match self.list_windows_internal().await {
                Ok(windows) => {
                    let lower = title.to_lowercase();
                    if let Some(win) = windows.iter().find(|w| {
                        w.title.to_lowercase().contains(&lower)
                    }) {
                        serde_json::json!({
                            "pid": win.pid,
                            "window_id": win.window_id
                        })
                    } else {
                        return DesktopResult::fail(format!(
                            "未找到标题包含 '{}' 的窗口",
                            title
                        ));
                    }
                }
                Err(_) => {
                    // 降级为全屏
                    serde_json::json!({ "monitor": monitor.unwrap_or(1) })
                }
            }
        } else {
            serde_json::json!({ "monitor": monitor.unwrap_or(1) })
        };

        match self.call_tool("screenshot", args).await {
            Ok(result) => {
                // 尝试从返回结果中提取 base64 图片
                let b64 = result
                    .get("data")
                    .and_then(|v| v.as_str())
                    .or_else(|| result.get("image_base64").and_then(|v| v.as_str()))
                    .or_else(|| {
                        result
                            .get("content")
                            .and_then(|v| v.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|item| item.get("data"))
                            .and_then(|v| v.as_str())
                    })
                    .map(|s| s.to_string());

                DesktopResult {
                    success: true,
                    message: "截图成功".into(),
                    image_base64: b64,
                    windows: None,
                }
            }
            Err(e) => DesktopResult::fail(format!("截图失败: {}", e)),
        }
    }

    /// 鼠标点击
    pub async fn click(&mut self, x: i32, y: i32, button: &str) -> DesktopResult {
        match self.ensure_started().await {
            Err(e) => return DesktopResult::fail(e),
            Ok(()) => {}
        }

        let args = serde_json::json!({
            "x": x,
            "y": y,
            "button": button
        });

        match self.call_tool("mouse_click", args).await {
            Ok(_) => DesktopResult::ok(format!("已点击 ({}, {})", x, y)),
            Err(e) => {
                // 尝试备选工具名
                let args2 = serde_json::json!({
                    "x": x,
                    "y": y,
                    "button": button
                });
                match self.call_tool("click", args2).await {
                    Ok(_) => DesktopResult::ok(format!("已点击 ({}, {})", x, y)),
                    Err(e2) => DesktopResult::fail(format!(
                        "点击失败 (mouse_click: {}, click: {})",
                        e, e2
                    )),
                }
            }
        }
    }

    /// 键盘输入文本
    pub async fn type_text(&mut self, text: &str) -> DesktopResult {
        match self.ensure_started().await {
            Err(e) => return DesktopResult::fail(e),
            Ok(()) => {}
        }

        let args = serde_json::json!({ "text": text });

        match self.call_tool("type_text", args).await {
            Ok(_) => DesktopResult::ok(format!("已输入文本: {}", text)),
            Err(e) => {
                // 尝试备选工具名
                let args2 = serde_json::json!({ "text": text });
                match self.call_tool("keyboard_type", args2).await {
                    Ok(_) => DesktopResult::ok(format!("已输入文本: {}", text)),
                    Err(e2) => DesktopResult::fail(format!(
                        "输入失败 (type_text: {}, keyboard_type: {})",
                        e, e2
                    )),
                }
            }
        }
    }

    /// 按下键或组合键
    pub async fn press_key(&mut self, keys: &str) -> DesktopResult {
        match self.ensure_started().await {
            Err(e) => return DesktopResult::fail(e),
            Ok(()) => {}
        }

        // 解析组合键格式: "ctrl+s", "alt+f4", "enter"
        let args = if keys.contains('+') {
            let parts: Vec<&str> = keys.split('+').map(|s| s.trim()).collect();
            serde_json::json!({ "keys": parts })
        } else {
            serde_json::json!({ "key": keys })
        };

        match self.call_tool("keyboard_press", args).await {
            Ok(_) => DesktopResult::ok(format!("已按下: {}", keys)),
            Err(e) => {
                // 尝试备选工具名
                let args2 = if keys.contains('+') {
                    let parts: Vec<&str> = keys.split('+').map(|s| s.trim()).collect();
                    serde_json::json!({ "keys": parts })
                } else {
                    serde_json::json!({ "key": keys })
                };
                match self.call_tool("press_key", args2).await {
                    Ok(_) => DesktopResult::ok(format!("已按下: {}", keys)),
                    Err(e2) => DesktopResult::fail(format!(
                        "按键失败 (keyboard_press: {}, press_key: {})",
                        e, e2
                    )),
                }
            }
        }
    }

    /// 列出所有窗口
    pub async fn list_windows(&mut self, filter: Option<&str>) -> DesktopResult {
        match self.ensure_started().await {
            Err(e) => return DesktopResult::fail(e),
            Ok(()) => {}
        }

        let windows = match self.list_windows_internal().await {
            Ok(w) => w,
            Err(e) => return DesktopResult::fail(format!("列出窗口失败: {}", e)),
        };

        let filtered: Vec<WindowInfo> = if let Some(f) = filter {
            let lower = f.to_lowercase();
            windows
                .into_iter()
                .filter(|w| {
                    w.title.to_lowercase().contains(&lower)
                        || w.process_name.to_lowercase().contains(&lower)
                })
                .collect()
        } else {
            windows
        };

        let count = filtered.len();
        DesktopResult {
            success: true,
            message: format!("找到 {} 个窗口", count),
            image_base64: None,
            windows: Some(filtered),
        }
    }

    /// 内部：列出所有窗口
    async fn list_windows_internal(&mut self) -> Result<Vec<WindowInfo>, String> {
        let result = self
            .call_tool("list_windows", serde_json::json!({}))
            .await?;

        // 解析窗口列表 — 兼容多种返回格式
        let windows: Vec<WindowInfo> = if let Some(arr) = result.as_array() {
            arr.iter()
                .filter_map(|w| Self::parse_window_info(w))
                .collect()
        } else if let Some(content) = result.get("content") {
            if let Some(arr) = content.as_array() {
                arr.iter()
                    .filter_map(|item| {
                        item.get("text")
                            .and_then(|t| t.as_str())
                            .and_then(|s| serde_json::from_str::<WindowInfo>(s).ok())
                    })
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        Ok(windows)
    }

    fn parse_window_info(value: &serde_json::Value) -> Option<WindowInfo> {
        Some(WindowInfo {
            title: value
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            pid: value.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            window_id: value
                .get("window_id")
                .or_else(|| value.get("id"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            process_name: value
                .get("process_name")
                .or_else(|| value.get("app"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
    }

    /// 聚焦窗口
    pub async fn focus_window(&mut self, window_title: &str) -> DesktopResult {
        match self.ensure_started().await {
            Err(e) => return DesktopResult::fail(e),
            Ok(()) => {}
        }

        // 先查找窗口
        let windows = match self.list_windows_internal().await {
            Ok(w) => w,
            Err(e) => return DesktopResult::fail(format!("查找窗口失败: {}", e)),
        };

        let lower = window_title.to_lowercase();
        let target = windows
            .iter()
            .find(|w| w.title.to_lowercase().contains(&lower));

        let args = if let Some(win) = target {
            serde_json::json!({
                "pid": win.pid,
                "window_id": win.window_id
            })
        } else {
            // 尝试按标题匹配
            serde_json::json!({ "title": window_title })
        };

        match self.call_tool("focus_window", args).await {
            Ok(_) => DesktopResult::ok(format!("已聚焦窗口: {}", window_title)),
            Err(e) => DesktopResult::fail(format!("聚焦窗口失败: {}", e)),
        }
    }

    /// 滚动
    pub async fn scroll(
        &mut self,
        x: Option<i32>,
        y: Option<i32>,
        direction: &str,
        amount: i32,
    ) -> DesktopResult {
        match self.ensure_started().await {
            Err(e) => return DesktopResult::fail(e),
            Ok(()) => {}
        }

        let args = serde_json::json!({
            "x": x.unwrap_or(0),
            "y": y.unwrap_or(0),
            "direction": direction,
            "amount": amount
        });

        match self.call_tool("scroll", args).await {
            Ok(_) => DesktopResult::ok(format!("已滚动 {} {} 格", direction, amount)),
            Err(e) => {
                // 尝试 mouse_scroll
                let args2 = serde_json::json!({
                    "direction": direction,
                    "amount": amount
                });
                match self.call_tool("mouse_scroll", args2).await {
                    Ok(_) => DesktopResult::ok(format!("已滚动 {} {} 格", direction, amount)),
                    Err(e2) => DesktopResult::fail(format!(
                        "滚动失败 (scroll: {}, mouse_scroll: {})",
                        e, e2
                    )),
                }
            }
        }
    }

    /// 检查 cua-driver 是否可用（不启动子进程，仅检测二进制是否存在）
    pub fn is_installed() -> bool {
        let client = Self::new();
        client.check_binary_exists()
    }
}

impl DesktopResult {
    fn ok(message: String) -> Self {
        Self {
            success: true,
            message,
            image_base64: None,
            windows: None,
        }
    }

    fn fail(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            image_base64: None,
            windows: None,
        }
    }
}

// ==================== 便捷函数（供 agent.rs 调用） ====================

/// 供 agent.rs execute_tool_parallel 调用的便捷封装
pub async fn execute_desktop_tool(
    client: &Arc<Mutex<CuaDriverClient>>,
    name: &str,
    args: &serde_json::Value,
) -> serde_json::Value {
    let mut guard = client.lock().await;

    let result = match name {
        "desktop_screenshot" => {
            let window_title = args.get("window_title").and_then(|v| v.as_str());
            let monitor = args.get("monitor").and_then(|v| v.as_u64()).map(|v| v as usize);
            guard.screenshot(window_title, monitor).await
        }
        "desktop_click" => {
            let x = args.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let y = args.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let button = args
                .get("button")
                .and_then(|v| v.as_str())
                .unwrap_or("left");
            guard.click(x, y, button).await
        }
        "desktop_type" => {
            let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            guard.type_text(text).await
        }
        "desktop_key" => {
            let keys = args.get("keys").and_then(|v| v.as_str()).unwrap_or("");
            guard.press_key(keys).await
        }
        "desktop_list_windows" => {
            let filter = args.get("filter").and_then(|v| v.as_str());
            guard.list_windows(filter).await
        }
        "desktop_focus_window" => {
            let window_title = args
                .get("window_title")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            guard.focus_window(window_title).await
        }
        "desktop_scroll" => {
            let x = args.get("x").and_then(|v| v.as_i64()).map(|v| v as i32);
            let y = args.get("y").and_then(|v| v.as_i64()).map(|v| v as i32);
            let direction = args
                .get("direction")
                .and_then(|v| v.as_str())
                .unwrap_or("down");
            let amount = args.get("amount").and_then(|v| v.as_i64()).unwrap_or(3) as i32;
            guard.scroll(x, y, direction, amount).await
        }
        _ => DesktopResult::fail(format!("未知桌面工具: {}", name)),
    };

    serde_json::to_value(&result).unwrap_or_else(|e| {
        serde_json::json!({"success": false, "message": format!("序列化结果失败: {}", e)})
    })
}

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::*;

    /// 创建模拟 cua-driver.cmd，响应 MCP 协议（无需读取 stdin，直接输出响应）
    fn create_mock_cua_driver(dir: &std::path::Path) {
        let script = "@echo off\r\n\
echo {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{\"tools\":{}},\"serverInfo\":{\"name\":\"cua-driver\",\"version\":\"1.0.0\"}}}\r\n\
:loop\r\n\
echo {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"mock result\"}]}}\r\n\
goto :loop\r\n";
        std::fs::write(dir.join("cua-driver.cmd"), script).unwrap();
    }

    // ==================== 测试 1: check_binary_exists 通过 set_binary_path ====================

    #[test]
    fn test_check_binary_exists_found_in_path() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        create_mock_cua_driver(temp_dir.path());

        let client = CuaDriverClient {
            child: None,
            stdin: None,
            stdout_reader: None,
            initialized: false,
            request_id: 1,
            available: false,
            checked: false,
            binary_path_override: Some(temp_dir.path().join("cua-driver.cmd")),
        };
        assert!(
            client.check_binary_exists(),
            "应该能通过 binary_path_override 检测到 cua-driver.cmd"
        );
    }

    // ==================== 测试 2: check_binary_exists 未找到 ====================

    #[test]
    fn test_check_binary_exists_not_found() {
        let client = CuaDriverClient::new();
        // 不设置 override，且 PATH 中不应有 cua-driver（如果系统安装了则跳过）
        // 我们直接检查：没有 override 且系统未安装时的行为
        // 由于无法保证系统是否安装，这里只验证默认状态
        let has_real = {
            let home = std::env::var("USERPROFILE").unwrap_or_default();
            std::path::PathBuf::from(&home)
                .join(".cargo")
                .join("bin")
                .join("cua-driver.exe")
                .exists()
        };
        if !has_real {
            assert!(!client.check_binary_exists());
        } else {
            eprintln!("[SKIP] 系统已安装 cua-driver，跳过 '未找到' 测试");
        }
    }

    // ==================== 测试 3: 状态机 — 未安装 → 安装 → 热检测 ====================

    #[tokio::test]
    async fn test_state_machine_recheck_triggers_on_second_call() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        create_mock_cua_driver(temp_dir.path());
        let mock_path = temp_dir.path().join("cua-driver.cmd");

        let mut client = CuaDriverClient::new();

        // 阶段 1: 不设置 override，调用 ensure_started — 应失败
        let result1 = client.ensure_started().await;
        assert!(result1.is_err(), "第一次调用应失败（未找到 cua-driver）");
        assert!(client.checked, "checked 应为 true");
        assert!(!client.available, "available 应为 false");
        let err_msg = result1.as_ref().unwrap_err();
        assert!(
            err_msg.contains("安装脚本") || err_msg.contains("未安装"),
            "错误信息应包含安装提示，实际: {}",
            err_msg
        );

        // 阶段 2: "安装" cua-driver（设置 override），第二次调用 — 应触发热检测
        client.set_binary_path(mock_path);

        let result2 = client.ensure_started().await;

        assert!(
            result2.is_ok(),
            "第二次调用应成功（热检测到新安装的 cua-driver），实际: {:?}",
            result2
        );
        assert!(client.available, "available 应为 true");
        assert!(client.initialized, "initialized 应为 true");
    }

    // ==================== 测试 4: 热检测后工具调用正常 ====================

    #[tokio::test]
    async fn test_tool_call_after_hot_detection() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        create_mock_cua_driver(temp_dir.path());
        let mock_path = temp_dir.path().join("cua-driver.cmd");

        let mut client = CuaDriverClient::new();

        // 模拟：先失败一次
        let _ = client.ensure_started().await;
        assert!(client.checked && !client.available);

        // 热检测启动 + 工具调用
        client.set_binary_path(mock_path);
        client.ensure_started().await.unwrap();
        let result = client.screenshot(None, None).await;

        assert!(
            result.success,
            "热检测后截图应成功，实际: {}",
            result.message
        );
    }

    // ==================== 测试 5: 已安装 → 直接启动（无需重检） ====================

    #[tokio::test]
    async fn test_direct_start_when_installed() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        create_mock_cua_driver(temp_dir.path());
        let mock_path = temp_dir.path().join("cua-driver.cmd");

        let mut client = CuaDriverClient::new();
        client.set_binary_path(mock_path);

        let result = client.ensure_started().await;
        assert!(result.is_ok(), "首次调用应直接启动成功，错误: {:?}", result.err());
        assert!(client.available);
        assert!(client.initialized);
        assert!(client.checked);
    }

    // ==================== 测试 6: 启动失败时提示重启 ====================

    #[tokio::test]
    async fn test_startup_failure_suggests_restart() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        // 创建一个会立即退出的假 cua-driver（MCP 握手会失败）
        let bad_script = "@echo off\nexit /b 1\n";
        std::fs::write(temp_dir.path().join("cua-driver.cmd"), bad_script).unwrap();
        let bad_path = temp_dir.path().join("cua-driver.cmd");

        let mut client = CuaDriverClient::new();
        client.set_binary_path(bad_path);

        let result = client.ensure_started().await;

        assert!(result.is_err(), "恶意 cua-driver 应导致启动失败");
        assert!(!client.available, "available 应保持 false");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("重启") || msg.contains("MCP 握手失败"),
            "错误信息应提示重启或握手失败，实际: {}",
            msg
        );
    }

    // ==================== 测试 7: is_installed() 与 check_binary_exists 一致性 ====================

    #[test]
    fn test_is_installed_delegates_to_check_binary_exists() {
        let client = CuaDriverClient::new();
        assert_eq!(
            CuaDriverClient::is_installed(),
            client.check_binary_exists(),
            "is_installed() 应与 check_binary_exists() 返回一致"
        );
    }
}