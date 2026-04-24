use crate::models::{Note, NoteInput, NoteQuery, PageResult};
use crate::services::note::NoteService;
use crate::services::trash::TrashService;
use crate::state::AppState;

/// 创建笔记
#[tauri::command]
pub fn create_note(
    state: tauri::State<'_, AppState>,
    input: NoteInput,
) -> Result<Note, String> {
    NoteService::create(&state.db, &input).map_err(|e| e.to_string())
}

/// 更新笔记
#[tauri::command]
pub fn update_note(
    state: tauri::State<'_, AppState>,
    id: i64,
    input: NoteInput,
) -> Result<Note, String> {
    NoteService::update(&state.db, id, &input).map_err(|e| e.to_string())
}

/// 删除笔记（软删除，移入回收站）
#[tauri::command]
pub fn delete_note(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    TrashService::soft_delete(&state.db, id).map_err(|e| e.to_string())
}

/// 获取单个笔记
#[tauri::command]
pub fn get_note(state: tauri::State<'_, AppState>, id: i64) -> Result<Note, String> {
    NoteService::get(&state.db, id).map_err(|e| e.to_string())
}

/// 切换笔记置顶状态
#[tauri::command]
pub fn toggle_pin(state: tauri::State<'_, AppState>, id: i64) -> Result<bool, String> {
    NoteService::toggle_pin(&state.db, id).map_err(|e| e.to_string())
}

/// 移动笔记到文件夹
#[tauri::command]
pub fn move_note_to_folder(
    state: tauri::State<'_, AppState>,
    note_id: i64,
    folder_id: Option<i64>,
) -> Result<(), String> {
    NoteService::move_to_folder(&state.db, note_id, folder_id).map_err(|e| e.to_string())
}

/// 全部移到回收站（软删）
#[tauri::command]
pub fn trash_all_notes(state: tauri::State<'_, AppState>) -> Result<usize, String> {
    NoteService::trash_all(&state.db).map_err(|e| e.to_string())
}

/// 查询笔记列表（分页）
#[tauri::command]
pub fn list_notes(
    state: tauri::State<'_, AppState>,
    query: NoteQuery,
) -> Result<PageResult<Note>, String> {
    NoteService::list(&state.db, &query).map_err(|e| e.to_string())
}

// ─── T-003 隐藏笔记 Commands ────────────────────

/// 切换笔记"隐藏"状态；返回切换后的新状态
///
/// 隐藏后主列表 / 搜索 / 反链 / 图谱 / RAG 全部不显示；取消隐藏立刻恢复可见。
#[tauri::command]
pub fn set_note_hidden(
    state: tauri::State<'_, AppState>,
    id: i64,
    hidden: bool,
) -> Result<bool, String> {
    NoteService::set_hidden(&state.db, id, hidden).map_err(|e| e.to_string())
}

/// 列出所有隐藏笔记（分页）—— 用于 /hidden 专用页
#[tauri::command]
pub fn list_hidden_notes(
    state: tauri::State<'_, AppState>,
    page: Option<usize>,
    page_size: Option<usize>,
) -> Result<PageResult<Note>, String> {
    NoteService::list_hidden(&state.db, page, page_size).map_err(|e| e.to_string())
}
