//! MCP Commands：把同进程内的 in-memory MCP server 暴露给前端
//!
//! 这一组 IPC 走"双层"路径：
//!   前端 invoke("mcp_internal_call_tool", ...)
//!     → commands::mcp::* (本文件)
//!     → state.mcp_internal (rmcp client)
//!     → kb_core::KbServer (in-process MCP server，通过 tokio::io::duplex 通信)
//!     → SQL on shared db
//!
//! 看似绕了一圈，但好处：
//!   1) 自家 AI 对话页和外部 Claude Desktop 用完全同一份工具实现（kb-core 12 工具）
//!   2) 后续接外部 MCP server 时（GitHub / Filesystem / 高德地图…）可以走同样的 client API
//!   3) 协议统一，UI 不需要区分"原生工具"和"外部工具"

use rmcp::model::CallToolRequestParams;
use serde::Serialize;
use serde_json::Value as JsonValue;

use crate::state::AppState;

/// tools/list 返回的单条工具描述（裁剪过，前端只需要必要字段）
#[derive(Debug, Serialize)]
pub struct McpToolInfo {
    /// 工具名（如 "search_notes"）
    pub name: String,
    /// 描述（喂给 LLM 用的自然语言说明）
    pub description: Option<String>,
    /// 入参 JSON Schema（前端可用 react-jsonschema-form 自动生成表单）
    pub input_schema: JsonValue,
}

/// 列出 in-memory MCP server 暴露的所有工具（kb-core 的 12 个）
#[tauri::command]
pub async fn mcp_internal_list_tools(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<McpToolInfo>, String> {
    let client = state
        .mcp_internal
        .as_ref()
        .ok_or_else(|| "in-memory MCP server 未就绪（启动初始化失败，详见 log）".to_string())?
        .clone();

    let tools = client
        .list_all_tools()
        .await
        .map_err(|e| format!("list_tools 失败: {e}"))?;

    let infos = tools
        .into_iter()
        .map(|t| McpToolInfo {
            name: t.name.into(),
            description: t.description.map(|d| d.into()),
            // input_schema 是 Arc<JsonObject>，转成 JsonValue 给前端
            input_schema: JsonValue::Object((*t.input_schema).clone()),
        })
        .collect();

    Ok(infos)
}

/// 调用 in-memory MCP server 的工具，返回 LLM 拿到的原始 JSON 字符串
///
/// 前端传 arguments 用 JSON object（serde_json::Value::Object）；
/// 返回 content 列表里的第一个 text block（kb-core 12 工具都返回单段 text）
#[tauri::command]
pub async fn mcp_internal_call_tool(
    state: tauri::State<'_, AppState>,
    name: String,
    arguments: Option<JsonValue>,
) -> Result<String, String> {
    let client = state
        .mcp_internal
        .as_ref()
        .ok_or_else(|| "in-memory MCP server 未就绪".to_string())?
        .clone();

    // arguments 必须是 JsonObject；前端传 null 或 undefined 都映射为 None
    let args_object = match arguments {
        Some(JsonValue::Object(m)) => Some(m),
        Some(JsonValue::Null) | None => None,
        Some(other) => {
            return Err(format!(
                "arguments 必须是 JSON object 或 null，收到: {}",
                other
            ));
        }
    };

    // CallToolRequestParams 是 #[non_exhaustive]，必须用 builder
    let mut req = CallToolRequestParams::new(name.clone());
    if let Some(obj) = args_object {
        req = req.with_arguments(obj);
    }

    let result = client
        .call_tool(req)
        .await
        .map_err(|e| format!("call_tool({name}) 失败: {e}"))?;

    // 把 content 列表里的 text block 拼起来返回（12 工具都是单段 text）
    let mut out = String::new();
    for c in &result.content {
        if let Some(text) = c.as_text() {
            out.push_str(&text.text);
        }
    }
    if result.is_error.unwrap_or(false) {
        return Err(format!("工具返回错误: {out}"));
    }
    Ok(out)
}
