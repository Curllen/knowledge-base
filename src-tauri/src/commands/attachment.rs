//! 附件相关 Command（薄包装 → AttachmentService）
//!
//! 与 commands/image.rs 对称设计。前端拖放非图片/非文本文件时调用。

use tauri::Manager;

use crate::models::AttachmentInfo;
use crate::services::attachment::AttachmentService;

/// 保存附件（base64 数据，用于前端拖放）
///
/// 返回附件信息（含绝对路径、字节数、MIME 类型）
#[tauri::command]
pub fn save_note_attachment(
    app: tauri::AppHandle,
    note_id: i64,
    file_name: String,
    base64_data: String,
) -> Result<AttachmentInfo, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    AttachmentService::save_from_base64(&data_dir, note_id, &file_name, &base64_data)
        .map_err(|e| e.to_string())
}

/// 从本地文件路径零拷贝保存附件（用于工具栏"插入附件"按钮）
#[tauri::command]
pub fn save_note_attachment_from_path(
    app: tauri::AppHandle,
    note_id: i64,
    source_path: String,
) -> Result<AttachmentInfo, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    AttachmentService::save_from_path(&data_dir, note_id, &source_path)
        .map_err(|e| e.to_string())
}

/// 删除笔记的所有附件
#[tauri::command]
pub fn delete_note_attachments(app: tauri::AppHandle, note_id: i64) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    AttachmentService::delete_note_attachments(&data_dir, note_id).map_err(|e| e.to_string())
}

/// 获取附件存储目录路径（设置页"打开目录"入口用）
#[tauri::command]
pub fn get_attachments_dir(app: tauri::AppHandle) -> Result<String, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    let dir = AttachmentService::ensure_dir(&data_dir).map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().into_owned())
}
