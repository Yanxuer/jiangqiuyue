//! CLI-Anything AI 工具定义
//!
//! 将 CLI-Hub 的能力暴露为 AI Agent 可调用的工具函数。
//! 工具包括：list_clis、search_clis、install_cli、execute_cli 等。

use crate::cli_hub::{CliEntry, CliHub};
use serde_json::Value;
use log;

/// 生成 CLI-Anything 相关的 AI 工具定义（用于 DeepSeek function calling）
pub fn get_cli_tools() -> Vec<Value> {
    vec![
        // 1. 列出可用 CLI
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "list_clis",
                "description": "列出 CLI-Hub 中所有可用的 CLI 工具。可以按分类或来源筛选。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "category": {
                            "type": "string",
                            "description": "按分类筛选，如 '3d', 'audio', 'video', 'image', 'ai', 'devops', 'web', 'database' 等"
                        },
                        "source": {
                            "type": "string",
                            "enum": ["harness", "public", "all"],
                            "description": "按来源筛选: harness（CLI-Anything 社区构建）、public（第三方官方 CLI）、all（全部）"
                        }
                    },
                    "required": []
                }
            }
        }),
        // 2. 搜索 CLI
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "search_clis",
                "description": "在 CLI-Hub 注册表中搜索 CLI 工具。按名称、描述或分类匹配。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "搜索关键词，如 'blender', 'image', 'pdf' 等"
                        }
                    },
                    "required": ["query"]
                }
            }
        }),
        // 3. 安装 CLI
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "install_cli",
                "description": "安装指定的 CLI 工具。安装后，用户可以执行该 CLI 的命令来操作对应的软件。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "要安装的 CLI 名称，如 'blender', 'gimp', 'audacity' 等"
                        }
                    },
                    "required": ["name"]
                }
            }
        }),
        // 4. 执行 CLI 命令
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "execute_cli",
                "description": "执行已安装 CLI 工具的命令。在执行前，请先用 list_clis 或 search_clis 确认 CLI 已安装。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "cli_name": {
                            "type": "string",
                            "description": "CLI 名称，如 'blender', 'gimp'"
                        },
                        "command": {
                            "type": "string",
                            "description": "要执行的命令，如 'project new -o output.blend', 'image export --format png'"
                        }
                    },
                    "required": ["cli_name", "command"]
                }
            }
        }),
        // 5. 获取 CLI 详情
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_cli_info",
                "description": "获取指定 CLI 工具的详细信息，包括描述、安装命令、依赖要求等。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "CLI 名称"
                        }
                    },
                    "required": ["name"]
                }
            }
        }),
        // 6. 推荐 CLI
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "recommend_clis",
                "description": "根据用户本机已安装的软件，推荐可用的 CLI 工具。这些 CLI 可以让 AI 代理通过命令行操作相应的软件。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "software_names": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "用户本机已安装的软件名称列表"
                        }
                    },
                    "required": ["software_names"]
                }
            }
        }),
    ]
}

/// 执行 CLI 工具调用
pub async fn execute_cli_tool(
    hub: &CliHub,
    tool_name: &str,
    arguments: &Value,
) -> Result<String, String> {
    log::info!("[CLI-Tools] ========== AI 调用 CLI 工具 ==========");
    log::info!("[CLI-Tools] 工具名称: {}", tool_name);
    log::info!("[CLI-Tools] 参数: {}", serde_json::to_string_pretty(arguments).unwrap_or_default());

    let result = match tool_name {
        "list_clis" => {
            let category = arguments.get("category").and_then(|v| v.as_str());
            let source = arguments.get("source").and_then(|v| v.as_str());

            log::info!("[CLI-Tools] list_clis: category={:?}, source={:?}", category, source);

            let entries: Vec<&CliEntry> = match (category, source) {
                (Some(cat), Some(src)) if src != "all" => {
                    log::info!("[CLI-Tools] 筛选: 分类={}, 来源={}", cat, src);
                    hub.list_by_category(cat)
                        .into_iter()
                        .filter(|e| e._source == src)
                        .collect()
                }
                (Some(cat), _) => {
                    log::info!("[CLI-Tools] 筛选: 分类={}", cat);
                    hub.list_by_category(cat)
                }
                (None, Some(src)) if src != "all" => {
                    log::info!("[CLI-Tools] 筛选: 来源={}", src);
                    hub.by_source(src)
                }
                _ => {
                    log::info!("[CLI-Tools] 列出全部 CLI");
                    hub.all_entries().iter().collect()
                }
            };

            let result: Vec<Value> = entries
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "name": e.name,
                        "display_name": e.display_name,
                        "description": e.description,
                        "category": e.category,
                        "version": e.version,
                        "source": e._source,
                        "entry_point": e.entry_point,
                        "installed": hub.is_installed(&e.name),
                    })
                })
                .collect();

            let output = format!(
                "找到 {} 个 CLI 工具:\n{}",
                result.len(),
                serde_json::to_string_pretty(&result).unwrap_or_default()
            );
            log::info!("[CLI-Tools] list_clis 返回 {} 个结果", result.len());
            Ok(output)
        }

        "search_clis" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or("缺少 query 参数")?;

            log::info!("[CLI-Tools] search_clis: query='{}'", query);

            let results = hub.search(query);
            log::info!("[CLI-Tools] 搜索匹配 {} 个结果", results.len());

            let result: Vec<Value> = results
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "name": e.name,
                        "display_name": e.display_name,
                        "description": e.description,
                        "category": e.category,
                        "version": e.version,
                        "source": e._source,
                        "installed": hub.is_installed(&e.name),
                    })
                })
                .collect();

            Ok(format!(
                "搜索 '{}' 找到 {} 个结果:\n{}",
                query,
                result.len(),
                serde_json::to_string_pretty(&result).unwrap_or_default()
            ))
        }

        "get_cli_info" => {
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or("缺少 name 参数")?;

            log::info!("[CLI-Tools] get_cli_info: name='{}'", name);

            match hub.get_cli(name) {
                Some(cli) => {
                    let info = serde_json::json!({
                        "name": cli.name,
                        "display_name": cli.display_name,
                        "version": cli.version,
                        "description": cli.description,
                        "category": cli.category,
                        "requires": cli.requires,
                        "homepage": cli.homepage,
                        "install_cmd": cli.install_cmd,
                        "entry_point": cli.entry_point,
                        "source": cli._source,
                        "installed": hub.is_installed(&cli.name),
                    });
                    log::info!("[CLI-Tools] get_cli_info 成功: {}", cli.display_name);
                    Ok(serde_json::to_string_pretty(&info).unwrap_or_default())
                }
                None => {
                    log::warn!("[CLI-Tools] get_cli_info: CLI '{}' 未找到", name);
                    Err(format!("CLI '{}' 未找到", name))
                }
            }
        }

        "recommend_clis" => {
            let software_names: Vec<String> = arguments
                .get("software_names")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .ok_or("缺少 software_names 参数")?;

            log::info!("[CLI-Tools] recommend_clis: 软件列表={:?}", software_names);

            let recommendations = hub.recommend_for_software(&software_names);
            log::info!("[CLI-Tools] 推荐 {} 个 CLI", recommendations.len());

            let result: Vec<Value> = recommendations
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "name": e.name,
                        "display_name": e.display_name,
                        "description": e.description,
                        "category": e.category,
                        "installed": hub.is_installed(&e.name),
                    })
                })
                .collect();

            Ok(format!(
                "基于 {} 个已安装软件，推荐 {} 个 CLI 工具:\n{}",
                software_names.len(),
                result.len(),
                serde_json::to_string_pretty(&result).unwrap_or_default()
            ))
        }

        _ => {
            log::error!("[CLI-Tools] 未知工具: {}", tool_name);
            Err(format!("未知 CLI 工具: {}", tool_name))
        }
    };

    match &result {
        Ok(msg) => log::info!("[CLI-Tools] 工具 {} 执行成功 (输出 {} 字符)", tool_name, msg.len()),
        Err(e) => log::error!("[CLI-Tools] 工具 {} 执行失败: {}", tool_name, e),
    }

    result
}

/// 需要 mutable hub 引用的工具（install/execute）
pub async fn execute_cli_tool_mut(
    hub: &mut CliHub,
    tool_name: &str,
    arguments: &Value,
) -> Result<String, String> {
    log::info!("[CLI-Tools] ========== AI 调用 CLI 工具 (mut) ==========");
    log::info!("[CLI-Tools] 工具名称: {}", tool_name);
    log::info!("[CLI-Tools] 参数: {}", serde_json::to_string_pretty(arguments).unwrap_or_default());

    let result = match tool_name {
        "install_cli" => {
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or("缺少 name 参数")?;

            log::info!("[CLI-Tools] install_cli: name='{}'", name);
            hub.install(name)
        }

        "execute_cli" => {
            let cli_name = arguments
                .get("cli_name")
                .and_then(|v| v.as_str())
                .ok_or("缺少 cli_name 参数")?;

            let command = arguments
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or("缺少 command 参数")?;

            log::info!("[CLI-Tools] execute_cli: cli_name='{}', command='{}'", cli_name, command);

            // 拆分命令为参数
            let args: Vec<&str> = command.split_whitespace().collect();
            log::info!("[CLI-Tools] 拆分参数: {:?}", args);

            hub.execute_cli_command(cli_name, &args)
        }

        _ => {
            log::error!("[CLI-Tools] 未知工具 (mut): {}", tool_name);
            Err(format!("未知 CLI 工具（需要 mutable）: {}", tool_name))
        }
    };

    match &result {
        Ok(msg) => log::info!("[CLI-Tools] 工具 {} 执行成功 (输出 {} 字符)", tool_name, msg.len()),
        Err(e) => log::error!("[CLI-Tools] 工具 {} 执行失败: {}", tool_name, e),
    }

    result
}