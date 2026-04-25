use std::fs::File;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use tokio::sync::{watch, Notify};

use crate::database::Database;
use crate::services::vault::VaultState;

/// 应用全局状态，通过 tauri::State 注入到 Command 中
pub struct AppState {
    pub db: Database,
    /// 实例数据根目录（默认实例 = app_data_dir，多开实例 = app_data_dir/instance-N）
    /// 资产/PDF/sources/db 都基于此路径
    pub data_dir: PathBuf,
    /// 实例 ID（None = 默认实例，Some(N) = 第 N 个多开实例）
    /// 由 `get_system_info` 暴露给前端，用于在 UI 上区分多开实例
    pub instance_id: Option<u32>,
    /// AI 生成取消信号 (conversation_id -> sender)
    pub ai_cancel: Mutex<std::collections::HashMap<i64, watch::Sender<bool>>>,
    /// 自动同步调度器唤醒信号：配置变更时 notify_one 重载
    pub sync_scheduler_notify: Arc<Notify>,
    /// 启动时 argv 里的 .md 文件路径，等前端 mount 后 take 出来
    pub pending_open_md_path: Mutex<Option<String>>,
    /// T-007 笔记加密保险库：内存中的主密钥（可选），锁定时清空
    pub vault: RwLock<VaultState>,
    /// 实例锁文件句柄（保持存活以维持独占锁，进程退出时自动释放）
    _lock_file: Option<File>,
}

impl AppState {
    pub fn new(
        db: Database,
        data_dir: PathBuf,
        instance_id: Option<u32>,
        lock_file: Option<File>,
    ) -> Self {
        Self {
            db,
            data_dir,
            instance_id,
            ai_cancel: Mutex::new(std::collections::HashMap::new()),
            sync_scheduler_notify: Arc::new(Notify::new()),
            pending_open_md_path: Mutex::new(None),
            vault: RwLock::new(VaultState::default()),
            _lock_file: lock_file,
        }
    }
}
