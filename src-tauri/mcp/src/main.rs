//! kb-mcp · Knowledge Base MCP Server
//!
//! 把本地 SQLite 知识库以 Model Context Protocol 暴露给外部 LLM 客户端
//! （Claude Desktop / Cursor / Cherry Studio 等）。
//!
//! ## 设计要点
//! - **完全独立的 binary**：不依赖 Tauri 主应用 crate，单独 `cargo build -p kb-mcp` 就能产出
//! - **只读 SQLite**：Schema 迁移由主应用负责，sidecar 只读，避免与主应用写竞争
//! - **WAL 兼容**：主应用已开 WAL + busy_timeout=5s，多进程并发读安全
//! - **隐私默认安全**：默认过滤 is_deleted / is_hidden / is_encrypted 笔记，不向 LLM 泄露
//! - **stdio 单连接**：rmcp 1.5 + transport-io，与主流 MCP 客户端 100% 兼容

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use clap::Parser;
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
    },
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use rusqlite::{Connection, OpenFlags, params};
use serde::Serialize;

// ─── CLI ─────────────────────────────────────────────────────────

#[derive(Debug, Parser)]
#[command(
    name = "kb-mcp",
    version,
    about = "Knowledge Base MCP Server - 把本地知识库以 MCP 协议暴露给 LLM 客户端"
)]
struct Cli {
    /// 知识库 SQLite 文件路径（必填）。通常是主应用的 app.db。
    /// 例：Windows 下 C:\Users\<name>\AppData\Roaming\com.agilefr.kb\app.db
    #[arg(long, env = "KB_MCP_DB_PATH")]
    db_path: PathBuf,

    /// 启用写工具（create_note / update_note / add_tag_to_note）。
    /// 默认关闭 = 完全只读，更安全；显式打开后 LLM 可创建/修改你的笔记。
    /// 注意：开启后 sidecar 与主应用同时写 db 可能短暂阻塞（5s busy_timeout 内会自动重试）
    #[arg(long, default_value_t = false)]
    writable: bool,
}

// ─── 数据库连接（只读 + WAL 兼容） ─────────────────────────────────

/// 包装一份 rusqlite 连接。`Mutex` 因为 rusqlite::Connection 不是 Sync。
struct KbDb {
    conn: Mutex<Connection>,
}

impl KbDb {
    /// 打开 SQLite。`writable=false` 时强制只读（推荐默认）。
    /// - 只读：用 `SQLITE_OPEN_READ_ONLY`，从内核层面拦截写入
    /// - 可写：用 `SQLITE_OPEN_READ_WRITE`，依赖主应用已建好库（不创建新库）
    /// - 不调 PRAGMA journal_mode：WAL 由主应用维持，sidecar 自动跟随
    fn open(path: &PathBuf, writable: bool) -> Result<Self> {
        let mode = if writable {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        };
        let conn = Connection::open_with_flags(path, mode | OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .with_context(|| format!("打开 SQLite 失败: {}", path.display()))?;

        // busy_timeout：与主应用写事务并发时，等 5 秒再报 SQLITE_BUSY
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

// ─── 工具入参 / 出参 schema ───────────────────────────────────────

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct SearchNotesArgs {
    /// 搜索关键词，支持中英文混合；空格分隔多个词（AND 逻辑）。
    /// FTS5 优先，无结果自动降级到 LIKE 模糊匹配。
    query: String,
    /// 返回结果数上限，默认 20，最大 100
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct GetNoteArgs {
    /// 笔记 id，从 search_notes 的结果中取
    id: i64,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct SearchByTagArgs {
    /// 标签名（精确匹配，区分大小写）
    tag: String,
    /// 返回结果数上限，默认 30，最大 100
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct GetBacklinksArgs {
    /// 目标笔记 id（要查"哪些笔记链接到了我"）
    id: i64,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ListDailyArgs {
    /// 最近多少天，默认 7
    #[serde(default)]
    days: Option<i64>,
    /// 上限条数，默认 30
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ListTasksArgs {
    /// 任务状态过滤：0=todo / 1=done。不传则全部
    #[serde(default)]
    status: Option<i64>,
    /// 关键词模糊匹配 title / description
    #[serde(default)]
    keyword: Option<String>,
    /// 返回数上限，默认 50，最大 200
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct CreateNoteArgs {
    /// 笔记标题（必填，不能为空字符串）
    title: String,
    /// 笔记正文（HTML 或 Markdown 都行，主应用前端用 TipTap 渲染）
    content: String,
    /// 可选：归属文件夹 id。不传则进"未分类"
    #[serde(default)]
    folder_id: Option<i64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct UpdateNoteArgs {
    /// 笔记 id
    id: i64,
    /// 新标题（不传则保持不变）
    #[serde(default)]
    title: Option<String>,
    /// 新内容（不传则保持不变）
    #[serde(default)]
    content: Option<String>,
    /// 新文件夹 id（不传则保持不变）。
    /// 限制：M3 暂不支持把笔记"移回未分类"，需要的话请在主应用 UI 操作
    #[serde(default)]
    folder_id: Option<i64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct AddTagArgs {
    /// 笔记 id
    note_id: i64,
    /// 标签名（不存在会自动创建）
    tag: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct GetPromptArgs {
    /// 提示词模板 id（与 builtin_code 二选一）
    #[serde(default)]
    id: Option<i64>,
    /// 内置模板代码（与 id 二选一），如 "summarize" / "translate" 等
    #[serde(default)]
    builtin_code: Option<String>,
}

#[derive(Debug, Serialize)]
struct SearchHit {
    id: i64,
    title: String,
    /// 命中片段（去 HTML 标签的纯文本，长度约 140）
    snippet: String,
    /// 笔记最近更新时间（本地时区字符串）
    updated_at: String,
    folder_id: Option<i64>,
}

#[derive(Debug, Serialize)]
struct NoteDetail {
    id: i64,
    title: String,
    /// 笔记正文（如果是加密笔记则返回占位，不泄露密文）
    content: String,
    folder_id: Option<i64>,
    is_daily: bool,
    daily_date: Option<String>,
    is_pinned: bool,
    word_count: i64,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct TagInfo {
    id: i64,
    name: String,
    color: Option<String>,
    /// 该标签下未删除笔记的数量
    note_count: i64,
}

#[derive(Debug, Serialize)]
struct BacklinkRef {
    /// 反链来源笔记 id
    source_id: i64,
    source_title: String,
    /// 链接出现处的上下文片段（可能为 null）
    context: Option<String>,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct TaskRow {
    id: i64,
    title: String,
    description: Option<String>,
    /// 0=urgent / 1=normal / 2=low
    priority: i64,
    important: bool,
    /// 0=todo / 1=done
    status: i64,
    due_date: Option<String>,
    completed_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct PromptDetail {
    id: i64,
    title: String,
    description: String,
    prompt: String,
    output_mode: String,
    icon: Option<String>,
    is_builtin: bool,
    builtin_code: Option<String>,
    enabled: bool,
}

// ─── MCP Server 实现 ─────────────────────────────────────────────

#[derive(Clone)]
struct KbServer {
    db: Arc<KbDb>,
    /// 写工具是否可用（由 CLI --writable 控制）。
    /// 即使关，写工具仍在 tools/list 里可见，但调用时会立即返回错误。
    /// 这样客户端能感知到能力存在，又不会意外修改 db。
    writable: bool,
    // tool_router 由 #[tool_router] / #[tool_handler] 宏读取，编译器看不出来用法
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl KbServer {
    fn new(db: KbDb, writable: bool) -> Self {
        Self {
            db: Arc::new(db),
            writable,
            tool_router: Self::tool_router(),
        }
    }

    /// 写工具入口的统一守卫
    fn ensure_writable(&self) -> Result<(), McpError> {
        if !self.writable {
            return Err(McpError::invalid_params(
                "kb-mcp 当前是只读模式，无法写入。\
                 启动时加 --writable 开关后再调用写工具。"
                    .to_string(),
                None,
            ));
        }
        Ok(())
    }

    // ─── ping：健康检查 ────────────────────────────────────────
    #[tool(description = "健康检查。返回 'pong' + sidecar 版本。用于客户端验证 MCP server 已就绪。")]
    fn ping(&self) -> Result<CallToolResult, McpError> {
        let msg = format!("pong (kb-mcp v{})", env!("CARGO_PKG_VERSION"));
        Ok(CallToolResult::success(vec![Content::text(msg)]))
    }

    // ─── search_notes：全文搜索 ───────────────────────────────
    #[tool(description = "全文搜索本地知识库笔记（FTS5 + LIKE 兜底）。\
                          自动过滤回收站 / 隐藏 / 加密笔记。\
                          返回命中列表（id / title / snippet / updated_at）的 JSON。")]
    fn search_notes(
        &self,
        Parameters(args): Parameters<SearchNotesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = args.limit.unwrap_or(20).clamp(1, 100);
        let query = args.query.trim();
        if query.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text("[]")]));
        }

        let conn = self
            .db
            .conn
            .lock()
            .map_err(|e| McpError::internal_error(format!("db lock: {e}"), None))?;

        // 1) FTS5 路径（前缀匹配，复用主应用的 sanitize_fts_query 逻辑）
        let fts_q = sanitize_fts_query(query);
        let hits = if !fts_q.is_empty() {
            let r = search_fts(&conn, &fts_q, limit)
                .map_err(|e| McpError::internal_error(format!("fts: {e}"), None))?;
            if !r.is_empty() {
                r
            } else {
                search_like(&conn, query, limit)
                    .map_err(|e| McpError::internal_error(format!("like: {e}"), None))?
            }
        } else {
            search_like(&conn, query, limit)
                .map_err(|e| McpError::internal_error(format!("like: {e}"), None))?
        };

        let json = serde_json::to_string(&hits)
            .map_err(|e| McpError::internal_error(format!("serialize: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ─── get_note：按 id 取全文 ───────────────────────────────
    #[tool(description = "按 id 读取单条笔记的完整内容（返回 JSON）。\
                          加密笔记的 content 会被替换为占位符（不泄露密文）。\
                          隐藏笔记拒绝访问，返回错误。")]
    fn get_note(
        &self,
        Parameters(args): Parameters<GetNoteArgs>,
    ) -> Result<CallToolResult, McpError> {
        let conn = self
            .db
            .conn
            .lock()
            .map_err(|e| McpError::internal_error(format!("db lock: {e}"), None))?;

        let detail = fetch_note(&conn, args.id)
            .map_err(|e| McpError::internal_error(format!("fetch_note: {e}"), None))?
            .ok_or_else(|| McpError::invalid_params(format!("笔记 {} 不存在", args.id), None))?;

        let json = serde_json::to_string(&detail)
            .map_err(|e| McpError::internal_error(format!("serialize: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ─── list_tags：所有标签 + 笔记数 ─────────────────────────
    #[tool(description = "列出所有标签，附带每个标签下的笔记数（按笔记数降序）。\
                          已删除笔记不计入 count。")]
    fn list_tags(&self) -> Result<CallToolResult, McpError> {
        let conn = self
            .db
            .conn
            .lock()
            .map_err(|e| McpError::internal_error(format!("db lock: {e}"), None))?;
        let tags = list_tags(&conn)
            .map_err(|e| McpError::internal_error(format!("list_tags: {e}"), None))?;
        let json = serde_json::to_string(&tags)
            .map_err(|e| McpError::internal_error(format!("serialize: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ─── search_by_tag：按标签筛选笔记 ────────────────────────
    #[tool(description = "按标签名筛选笔记列表，按更新时间降序。\
                          自动过滤回收站 / 隐藏 / 加密笔记。")]
    fn search_by_tag(
        &self,
        Parameters(args): Parameters<SearchByTagArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = args.limit.unwrap_or(30).clamp(1, 100);
        let tag = args.tag.trim();
        if tag.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text("[]")]));
        }
        let conn = self
            .db
            .conn
            .lock()
            .map_err(|e| McpError::internal_error(format!("db lock: {e}"), None))?;
        let hits = search_by_tag(&conn, tag, limit)
            .map_err(|e| McpError::internal_error(format!("search_by_tag: {e}"), None))?;
        let json = serde_json::to_string(&hits)
            .map_err(|e| McpError::internal_error(format!("serialize: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ─── get_backlinks：反向链接 ─────────────────────────────
    #[tool(description = "获取目标笔记的所有反向链接（哪些笔记 [[链接]] 到了它）。\
                          隐藏 / 已删除的源笔记不计入。")]
    fn get_backlinks(
        &self,
        Parameters(args): Parameters<GetBacklinksArgs>,
    ) -> Result<CallToolResult, McpError> {
        let conn = self
            .db
            .conn
            .lock()
            .map_err(|e| McpError::internal_error(format!("db lock: {e}"), None))?;
        let links = get_backlinks(&conn, args.id)
            .map_err(|e| McpError::internal_error(format!("get_backlinks: {e}"), None))?;
        let json = serde_json::to_string(&links)
            .map_err(|e| McpError::internal_error(format!("serialize: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ─── list_daily_notes：日记列表 ──────────────────────────
    #[tool(description = "列出最近 N 天的日记（is_daily=1 的笔记），按 daily_date 降序。\
                          默认最近 7 天、最多 30 条。")]
    fn list_daily_notes(
        &self,
        Parameters(args): Parameters<ListDailyArgs>,
    ) -> Result<CallToolResult, McpError> {
        let days = args.days.unwrap_or(7).clamp(1, 365);
        let limit = args.limit.unwrap_or(30).clamp(1, 100);
        let conn = self
            .db
            .conn
            .lock()
            .map_err(|e| McpError::internal_error(format!("db lock: {e}"), None))?;
        let hits = list_daily_notes(&conn, days, limit)
            .map_err(|e| McpError::internal_error(format!("list_daily: {e}"), None))?;
        let json = serde_json::to_string(&hits)
            .map_err(|e| McpError::internal_error(format!("serialize: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ─── list_tasks：任务列表 ─────────────────────────────────
    #[tool(description = "列出主任务（不含子任务），可按 status / keyword 过滤。\
                          排序：priority ASC → due_date NULLS LAST → updated_at DESC。\
                          status: 0=todo, 1=done。")]
    fn list_tasks(
        &self,
        Parameters(args): Parameters<ListTasksArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = args.limit.unwrap_or(50).clamp(1, 200);
        let conn = self
            .db
            .conn
            .lock()
            .map_err(|e| McpError::internal_error(format!("db lock: {e}"), None))?;
        let rows = list_tasks(&conn, args.status, args.keyword.as_deref(), limit)
            .map_err(|e| McpError::internal_error(format!("list_tasks: {e}"), None))?;
        let json = serde_json::to_string(&rows)
            .map_err(|e| McpError::internal_error(format!("serialize: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ─── get_prompt：取单条提示词 ─────────────────────────────
    #[tool(description = "按 id 或 builtin_code 取一条提示词模板（Prompt Library）。\
                          二选一传参，至少一个；同时传以 id 优先。")]
    fn get_prompt(
        &self,
        Parameters(args): Parameters<GetPromptArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.id.is_none() && args.builtin_code.is_none() {
            return Err(McpError::invalid_params(
                "必须传 id 或 builtin_code 之一".to_string(),
                None,
            ));
        }
        let conn = self
            .db
            .conn
            .lock()
            .map_err(|e| McpError::internal_error(format!("db lock: {e}"), None))?;
        let detail = get_prompt(&conn, args.id, args.builtin_code.as_deref())
            .map_err(|e| McpError::internal_error(format!("get_prompt: {e}"), None))?
            .ok_or_else(|| McpError::invalid_params("提示词模板不存在".to_string(), None))?;
        let json = serde_json::to_string(&detail)
            .map_err(|e| McpError::internal_error(format!("serialize: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ─── ✏️ 写工具：create_note ─────────────────────────────────
    #[tool(description = "创建一条新笔记（仅 --writable 模式可用）。\
                          自动同步 title_normalized / content_hash / FTS5 索引。\
                          返回 {id, title} JSON。")]
    fn create_note(
        &self,
        Parameters(args): Parameters<CreateNoteArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_writable()?;
        let title = args.title.trim();
        if title.is_empty() {
            return Err(McpError::invalid_params(
                "title 不能为空".to_string(),
                None,
            ));
        }
        let conn = self
            .db
            .conn
            .lock()
            .map_err(|e| McpError::internal_error(format!("db lock: {e}"), None))?;
        let id = create_note(&conn, title, &args.content, args.folder_id)
            .map_err(|e| McpError::internal_error(format!("create_note: {e}"), None))?;
        let json = serde_json::to_string(&serde_json::json!({
            "id": id,
            "title": title,
        }))
        .map_err(|e| McpError::internal_error(format!("serialize: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ─── ✏️ 写工具：update_note ─────────────────────────────────
    #[tool(description = "按 id 更新笔记。title / content / folder_id 都是可选，\
                          只更新传入的字段。会拒绝改加密笔记。仅 --writable 模式可用。")]
    fn update_note(
        &self,
        Parameters(args): Parameters<UpdateNoteArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_writable()?;
        if args.title.is_none() && args.content.is_none() && args.folder_id.is_none() {
            return Err(McpError::invalid_params(
                "至少要传 title / content / folder_id 之一".to_string(),
                None,
            ));
        }
        let conn = self
            .db
            .conn
            .lock()
            .map_err(|e| McpError::internal_error(format!("db lock: {e}"), None))?;
        let affected = update_note(
            &conn,
            args.id,
            args.title.as_deref(),
            args.content.as_deref(),
            args.folder_id,
        )
        .map_err(|e| McpError::internal_error(format!("update_note: {e}"), None))?;
        if affected == 0 {
            return Err(McpError::invalid_params(
                format!("笔记 {} 不存在或是加密笔记/已删除", args.id),
                None,
            ));
        }
        Ok(CallToolResult::success(vec![Content::text(
            format!("{{\"id\":{},\"updated\":true}}", args.id),
        )]))
    }

    // ─── ✏️ 写工具：add_tag_to_note ─────────────────────────────
    #[tool(description = "给笔记加标签（标签不存在自动创建）。仅 --writable 模式可用。\
                          返回 {tag_id, note_id, created_tag} JSON。")]
    fn add_tag_to_note(
        &self,
        Parameters(args): Parameters<AddTagArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_writable()?;
        let tag = args.tag.trim();
        if tag.is_empty() {
            return Err(McpError::invalid_params(
                "tag 不能为空".to_string(),
                None,
            ));
        }
        let conn = self
            .db
            .conn
            .lock()
            .map_err(|e| McpError::internal_error(format!("db lock: {e}"), None))?;
        let (tag_id, created_tag) = add_tag_to_note(&conn, args.note_id, tag)
            .map_err(|e| McpError::internal_error(format!("add_tag: {e}"), None))?;
        let json = serde_json::to_string(&serde_json::json!({
            "tag_id": tag_id,
            "note_id": args.note_id,
            "created_tag": created_tag,
        }))
        .map_err(|e| McpError::internal_error(format!("serialize: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}

#[tool_handler]
impl ServerHandler for KbServer {
    fn get_info(&self) -> ServerInfo {
        // 注意：Implementation::from_build_env() 会读 rmcp 自己的 cargo env（"rmcp" / "1.5.0"），
        // 不是 kb-mcp 的；必须手动 new() 传 kb-mcp 的 name/version
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("kb-mcp", env!("CARGO_PKG_VERSION")))
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(format!(
                "本 MCP server 暴露本地知识库（笔记 / 标签 / 双链 / 任务 / 日记 / Prompt 库）。\
                 读工具：search_notes / get_note / list_tags / search_by_tag / \
                 get_backlinks / list_daily_notes / list_tasks / get_prompt。\
                 写工具：create_note / update_note / add_tag_to_note（{}）。\
                 默认过滤回收站、隐藏、加密笔记，保护隐私。",
                if self.writable { "已启用" } else { "当前禁用，启动加 --writable 开启" }
            ))
    }
}

// ─── SQL 实现（独立函数，不复用主应用代码避免循环依赖） ─────────────

/// FTS5 搜索（与主应用 src-tauri/src/database/search.rs::search_fts 保持一致）
///
/// 排序：bm25(notes_fts, 5.0, 1.0) — title 列权重 5，content 权重 1。
/// 过滤：is_deleted=0 AND is_hidden=0 AND is_encrypted=0（比主应用多过滤加密）
fn search_fts(
    conn: &Connection,
    fts_query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT n.id, n.title,
                snippet(notes_fts, 1, '', '', '...', 32) as snippet,
                n.updated_at, n.folder_id
         FROM notes_fts fts
         JOIN notes n ON fts.rowid = n.id
         WHERE notes_fts MATCH ?1
           AND n.is_deleted = 0
           AND n.is_hidden = 0
           AND n.is_encrypted = 0
         ORDER BY bm25(notes_fts, 5.0, 1.0)
         LIMIT ?2",
    )?;

    let rows = stmt
        .query_map(params![fts_query, limit as i64], |row| {
            Ok(SearchHit {
                id: row.get(0)?,
                title: row.get(1)?,
                snippet: row.get(2)?,
                updated_at: row.get(3)?,
                folder_id: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

/// LIKE 模糊搜索兜底
fn search_like(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, rusqlite::Error> {
    let pattern = format!("%{}%", query);
    let mut stmt = conn.prepare(
        "SELECT n.id, n.title, substr(n.content, 1, 140), n.updated_at, n.folder_id
         FROM notes n
         WHERE n.is_deleted = 0 AND n.is_hidden = 0 AND n.is_encrypted = 0
           AND (n.title LIKE ?1 OR n.content LIKE ?1)
         ORDER BY n.updated_at DESC
         LIMIT ?2",
    )?;

    let rows = stmt
        .query_map(params![pattern, limit as i64], |row| {
            let raw_snippet: String = row.get(2)?;
            Ok(SearchHit {
                id: row.get(0)?,
                title: row.get(1)?,
                snippet: strip_tags(&raw_snippet),
                updated_at: row.get(3)?,
                folder_id: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

/// 取单条笔记的明文。加密笔记返回占位符。隐藏笔记返回 None。
fn fetch_note(conn: &Connection, id: i64) -> Result<Option<NoteDetail>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, title, content, folder_id, is_daily, daily_date,
                is_pinned, is_hidden, is_encrypted, word_count,
                created_at, updated_at
         FROM notes WHERE id = ?1 AND is_deleted = 0",
    )?;

    let r = stmt
        .query_row(params![id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, i32>(4)? != 0,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i32>(6)? != 0,
                row.get::<_, i32>(7)? != 0,
                row.get::<_, i32>(8)? != 0,
                row.get::<_, i64>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
            ))
        })
        .ok();

    let Some((
        id,
        title,
        content,
        folder_id,
        is_daily,
        daily_date,
        is_pinned,
        is_hidden,
        is_encrypted,
        word_count,
        created_at,
        updated_at,
    )) = r
    else {
        return Ok(None);
    };

    if is_hidden {
        return Ok(None);
    }

    let safe_content = if is_encrypted {
        "🔒 已加密笔记（kb-mcp 不暴露密文，请在主应用内解锁查看）".to_string()
    } else {
        content
    };

    Ok(Some(NoteDetail {
        id,
        title,
        content: safe_content,
        folder_id,
        is_daily,
        daily_date,
        is_pinned,
        word_count,
        created_at,
        updated_at,
    }))
}

/// 列出所有标签 + 笔记数（已删除笔记不计入）
fn list_tags(conn: &Connection) -> Result<Vec<TagInfo>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, t.color, COUNT(nt.note_id) as note_count
         FROM tags t
         LEFT JOIN note_tags nt ON t.id = nt.tag_id
         LEFT JOIN notes n ON nt.note_id = n.id AND n.is_deleted = 0
         GROUP BY t.id
         ORDER BY note_count DESC, t.name",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(TagInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                note_count: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 按标签名筛选笔记（精确匹配 tag.name）
fn search_by_tag(
    conn: &Connection,
    tag: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT n.id, n.title, substr(n.content, 1, 140), n.updated_at, n.folder_id
         FROM notes n
         JOIN note_tags nt ON nt.note_id = n.id
         JOIN tags t ON t.id = nt.tag_id
         WHERE t.name = ?1
           AND n.is_deleted = 0 AND n.is_hidden = 0 AND n.is_encrypted = 0
         ORDER BY n.updated_at DESC
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![tag, limit as i64], |row| {
            let raw: String = row.get(2)?;
            Ok(SearchHit {
                id: row.get(0)?,
                title: row.get(1)?,
                snippet: strip_tags(&raw),
                updated_at: row.get(3)?,
                folder_id: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 反向链接：哪些笔记 [[链接]] 到了 target_id
/// 与主应用 database/links.rs::get_backlinks 保持一致逻辑（过滤 is_hidden / is_deleted）
fn get_backlinks(
    conn: &Connection,
    target_id: i64,
) -> Result<Vec<BacklinkRef>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT nl.source_id, n.title, nl.context, n.updated_at
         FROM note_links nl
         JOIN notes n ON n.id = nl.source_id
         WHERE nl.target_id = ?1 AND n.is_deleted = 0 AND n.is_hidden = 0
         ORDER BY n.updated_at DESC",
    )?;
    let rows = stmt
        .query_map(params![target_id], |row| {
            Ok(BacklinkRef {
                source_id: row.get(0)?,
                source_title: row.get(1)?,
                context: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 最近 N 天的日记（is_daily=1，按 daily_date 降序）
fn list_daily_notes(
    conn: &Connection,
    days: i64,
    limit: usize,
) -> Result<Vec<SearchHit>, rusqlite::Error> {
    // 用 SQLite 的 date('now', '-N days') 算下界
    let mut stmt = conn.prepare(
        "SELECT n.id, n.title, substr(n.content, 1, 140), n.updated_at, n.folder_id
         FROM notes n
         WHERE n.is_daily = 1
           AND n.is_deleted = 0 AND n.is_hidden = 0 AND n.is_encrypted = 0
           AND n.daily_date IS NOT NULL
           AND date(n.daily_date) >= date('now', 'localtime', ?1)
         ORDER BY n.daily_date DESC
         LIMIT ?2",
    )?;
    let offset = format!("-{} days", days);
    let rows = stmt
        .query_map(params![offset, limit as i64], |row| {
            let raw: String = row.get(2)?;
            Ok(SearchHit {
                id: row.get(0)?,
                title: row.get(1)?,
                snippet: strip_tags(&raw),
                updated_at: row.get(3)?,
                folder_id: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 任务列表（只主任务，可按 status / keyword 过滤）
fn list_tasks(
    conn: &Connection,
    status: Option<i64>,
    keyword: Option<&str>,
    limit: usize,
) -> Result<Vec<TaskRow>, rusqlite::Error> {
    let mut where_parts: Vec<&str> = vec!["t.parent_task_id IS NULL"];
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = status {
        where_parts.push("t.status = ?");
        binds.push(Box::new(s));
    }
    let kw_pattern = keyword
        .and_then(|k| {
            let t = k.trim();
            (!t.is_empty()).then(|| format!("%{}%", t))
        });
    if kw_pattern.is_some() {
        where_parts.push("(t.title LIKE ? OR IFNULL(t.description, '') LIKE ?)");
    }
    let where_sql = where_parts.join(" AND ");
    let sql = format!(
        "SELECT t.id, t.title, t.description, t.priority, t.important, t.status,
                t.due_date, t.completed_at, t.created_at, t.updated_at
         FROM tasks t
         WHERE {}
         ORDER BY t.priority ASC,
                  CASE WHEN t.due_date IS NULL THEN 1 ELSE 0 END,
                  t.due_date ASC,
                  t.updated_at DESC
         LIMIT ?",
        where_sql
    );

    if let Some(ref kw) = kw_pattern {
        binds.push(Box::new(kw.clone()));
        binds.push(Box::new(kw.clone()));
    }
    binds.push(Box::new(limit as i64));

    let mut stmt = conn.prepare(&sql)?;
    let bind_refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(&*bind_refs, |row| {
            Ok(TaskRow {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                priority: row.get(3)?,
                important: row.get::<_, i32>(4)? != 0,
                status: row.get(5)?,
                due_date: row.get(6)?,
                completed_at: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 取单条 prompt（按 id 优先，否则 builtin_code）
fn get_prompt(
    conn: &Connection,
    id: Option<i64>,
    builtin_code: Option<&str>,
) -> Result<Option<PromptDetail>, rusqlite::Error> {
    let cols = "id, title, description, prompt, output_mode, icon,
                is_builtin, builtin_code, enabled";
    let (sql, key): (String, Box<dyn rusqlite::ToSql>) = if let Some(id) = id {
        (
            format!("SELECT {} FROM prompt_templates WHERE id = ?1", cols),
            Box::new(id),
        )
    } else if let Some(code) = builtin_code {
        (
            format!(
                "SELECT {} FROM prompt_templates WHERE builtin_code = ?1",
                cols
            ),
            Box::new(code.to_string()),
        )
    } else {
        return Ok(None);
    };

    let mut stmt = conn.prepare(&sql)?;
    let r = stmt
        .query_row([key.as_ref()], |row| {
            Ok(PromptDetail {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                prompt: row.get(3)?,
                output_mode: row.get(4)?,
                icon: row.get(5)?,
                is_builtin: row.get::<_, i32>(6)? != 0,
                builtin_code: row.get(7)?,
                enabled: row.get::<_, i32>(8)? != 0,
            })
        })
        .ok();
    Ok(r)
}

// ─── 写操作（仅 --writable 模式调用） ─────────────────────────────

/// 创建笔记。同步维护 title_normalized + content_hash（FTS5 / word_count 由触发器自动）
/// 与主应用 database/notes.rs::create_note 保持字段一致性
fn create_note(
    conn: &Connection,
    title: &str,
    content: &str,
    folder_id: Option<i64>,
) -> Result<i64, rusqlite::Error> {
    let normalized = normalize_title(title);
    let hash = sha256_hex(content);
    conn.execute(
        "INSERT INTO notes (title, content, folder_id, title_normalized, content_hash)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![title, content, folder_id, normalized, hash],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 更新笔记。任意字段为 None 表示不变。
/// 拒绝改加密笔记 / 已删除笔记（WHERE 条件自动过滤）。
/// 返回受影响行数。
fn update_note(
    conn: &Connection,
    id: i64,
    title: Option<&str>,
    content: Option<&str>,
    folder_id: Option<i64>,
) -> Result<usize, rusqlite::Error> {
    // 动态 SET 子句：只列出实际要改的列
    let mut set_parts: Vec<String> = Vec::new();
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(t) = title {
        set_parts.push("title = ?".into());
        set_parts.push("title_normalized = ?".into());
        binds.push(Box::new(t.to_string()));
        binds.push(Box::new(normalize_title(t)));
    }
    if let Some(c) = content {
        set_parts.push("content = ?".into());
        set_parts.push("content_hash = ?".into());
        binds.push(Box::new(c.to_string()));
        binds.push(Box::new(sha256_hex(c)));
    }
    if let Some(fid) = folder_id {
        set_parts.push("folder_id = ?".into());
        binds.push(Box::new(fid));
    }
    // updated_at 永远刷新
    set_parts.push("updated_at = datetime('now', 'localtime')".into());

    let sql = format!(
        "UPDATE notes SET {} WHERE id = ? AND is_deleted = 0 AND is_encrypted = 0",
        set_parts.join(", ")
    );
    binds.push(Box::new(id));

    let bind_refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    conn.execute(&sql, &*bind_refs)
}

/// 给笔记加标签。tag 不存在则创建。返回 (tag_id, created_tag)。
fn add_tag_to_note(
    conn: &Connection,
    note_id: i64,
    tag: &str,
) -> Result<(i64, bool), rusqlite::Error> {
    // 1) 先查 tag 是否存在
    let existing: Option<i64> = conn
        .query_row("SELECT id FROM tags WHERE name = ?1", params![tag], |r| {
            r.get(0)
        })
        .ok();
    let (tag_id, created_tag) = if let Some(id) = existing {
        (id, false)
    } else {
        conn.execute("INSERT INTO tags (name) VALUES (?1)", params![tag])?;
        (conn.last_insert_rowid(), true)
    };
    // 2) 关联（INSERT OR IGNORE 防重复）
    conn.execute(
        "INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?1, ?2)",
        params![note_id, tag_id],
    )?;
    Ok((tag_id, created_tag))
}

// ─── 与主应用一致的工具函数（避免循环依赖，独立复刻） ─────────────

/// 复刻 src-tauri/src/database/links.rs::normalize_title
/// trim + 折叠空白 + 转小写 + 去 markdown 转义
fn normalize_title(s: &str) -> String {
    unescape_md(s)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn unescape_md(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                if !next.is_alphanumeric() {
                    chars.next();
                    out.push(next);
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}

/// 复刻 src-tauri/src/services/hash.rs::sha256_hex
fn sha256_hex(content: &str) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let out = hasher.finalize();
    let mut s = String::with_capacity(out.len() * 2);
    for b in out {
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}

/// 把用户查询转 FTS5 前缀匹配语法。
/// 与主应用 src-tauri/src/database/search.rs::sanitize_fts_query 保持完全一致行为。
fn sanitize_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|word| {
            let clean: String = word
                .chars()
                .filter(|c| !matches!(c, '"' | '*' | '(' | ')' | ':' | '^' | '{' | '}'))
                .collect();
            if clean.is_empty() {
                String::new()
            } else {
                format!("{}*", clean)
            }
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// 去掉 HTML 标签，给 snippet 用
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

// ─── main ────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // clap 自己处理 --help / --version（写 stdout 后 exit），先解析再打日志
    // 避免 --help 时输出多余的 starting 行
    let cli = Cli::parse();

    // 关键：日志走 stderr，stdout 是 JSON-RPC 通道，绝对不能污染
    eprintln!(
        "[kb-mcp] starting v{}, db = {}",
        env!("CARGO_PKG_VERSION"),
        cli.db_path.display()
    );

    if !cli.db_path.exists() {
        anyhow::bail!("db 文件不存在: {}", cli.db_path.display());
    }

    let db = KbDb::open(&cli.db_path, cli.writable)?;
    eprintln!(
        "[kb-mcp] mode = {}",
        if cli.writable {
            "READ-WRITE (write tools enabled)"
        } else {
            "READ-ONLY (write tools will refuse)"
        }
    );
    let server = KbServer::new(db, cli.writable);

    // serve(stdio()) 接管 stdin/stdout，按 JSON-RPC 帧收发
    let service = server
        .serve(stdio())
        .await
        .with_context(|| "rmcp serve(stdio) 启动失败")?;

    eprintln!("[kb-mcp] ready");
    service.waiting().await?;
    eprintln!("[kb-mcp] shutdown");
    Ok(())
}
