use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tauri::{Emitter, Runtime};
use walkdir::WalkDir;

use crate::database::Database;
use crate::error::AppError;
use crate::models::{
    ImportConflictPolicy, ImportProgress, ImportResult, NoteInput, OpenMarkdownResult,
    ScannedFile,
};
use crate::services::hash::sha256_hex;

pub struct ImportService;

impl ImportService {
    /// 扫描文件夹，返回所有 Markdown 文件列表（不导入）
    ///
    /// 每条带 `relative_dir`（相对扫描根的父目录，斜杠统一 '/'，根层为空串），
    /// 以及 `match_kind` + `existing_note_id` —— 扫描阶段就告诉前端哪些文件
    /// 已导入过（path 主判 / title+content_hash 兜底），便于弹窗展示分桶统计。
    pub fn scan_markdown_folder(
        db: &Database,
        folder_path: &str,
    ) -> Result<Vec<ScannedFile>, AppError> {
        let root = Path::new(folder_path);
        if !root.is_dir() {
            return Err(AppError::InvalidInput(format!(
                "路径不是文件夹: {}",
                folder_path
            )));
        }

        // 规范化根路径：后续要和每条文件的 parent 做 strip_prefix，统一到一套表示
        let root_canonical: PathBuf =
            std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

        let mut files: Vec<ScannedFile> = WalkDir::new(root)
            .sort_by_file_name() // 同层按字母序稳定排序
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().is_file()
                    && e.path()
                        .extension()
                        .map(|ext| ext == "md" || ext == "markdown")
                        .unwrap_or(false)
            })
            .filter_map(|entry| {
                let path = entry.path();
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("未命名")
                    .to_string();
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);

                // relative_dir：相对根的父目录，使用正斜杠统一
                let parent = path.parent().unwrap_or(Path::new(""));
                let parent_canonical: PathBuf = std::fs::canonicalize(parent)
                    .unwrap_or_else(|_| parent.to_path_buf());
                let relative_dir = parent_canonical
                    .strip_prefix(&root_canonical)
                    .ok()
                    .map(|p| {
                        p.components()
                            .filter_map(|c| c.as_os_str().to_str())
                            .collect::<Vec<_>>()
                            .join("/")
                    })
                    .unwrap_or_default();

                // 扫描阶段就做去重判定 —— 前端预览需要
                let (match_kind, existing_id) =
                    detect_existing_match(db, path, &name).unwrap_or_else(|e| {
                        log::warn!(
                            "[scan] 检测重复失败（当成 new 处理）: {} -> {}",
                            path.display(),
                            e
                        );
                        ("new".to_string(), None)
                    });

                Some(ScannedFile {
                    path: path.to_string_lossy().to_string(),
                    relative_dir,
                    name,
                    size,
                    match_kind,
                    existing_note_id: existing_id,
                })
            })
            .collect();

        // 二次排序：先按相对目录，再按文件名，确保前端展示稳定
        files.sort_by(|a, b| {
            a.relative_dir
                .cmp(&b.relative_dir)
                .then_with(|| a.name.cmp(&b.name))
        });

        Ok(files)
    }

    /// 按指定文件路径列表导入 Markdown 文件
    ///
    /// - `base_folder_id`: 导入到哪个文件夹下。None = 根
    /// - `root_path`: 扫描的根路径。传了才能按相对路径重建目录树；不传则全部平铺到 base
    /// - `preserve_root`: 是否在 base 下多套一层"源文件夹名"。需要 root_path 存在
    /// - `policy`: 已存在的文件怎么办（Skip / Duplicate）
    ///
    /// 同名文件夹按 (parent_id, name) 复用已有记录，避免重复创建。
    /// 每条成功导入的笔记都会写入 canonical `source_file_path`，方便下次导入时去重。
    pub fn import_selected_files<R: Runtime, E: Emitter<R>>(
        db: &Database,
        file_paths: &[String],
        base_folder_id: Option<i64>,
        root_path: Option<&str>,
        preserve_root: bool,
        policy: ImportConflictPolicy,
        emitter: &E,
    ) -> Result<ImportResult, AppError> {
        let total = file_paths.len();
        let mut imported = 0usize;
        let mut skipped = 0usize;
        let mut duplicated = 0usize;
        let mut errors = Vec::new();

        // 预先算好根扫描路径（用于对每个文件算相对目录）+ 预先建"保留根"文件夹
        let root_canonical: Option<PathBuf> = root_path
            .map(Path::new)
            .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()));

        // 缓存：rel_path ("子A/子B") -> folder_id。空串键对应批次根 folder_id
        let mut folder_cache: HashMap<String, Option<i64>> = HashMap::new();

        // 若 preserve_root，在 base 下先建一个以 root basename 命名的文件夹作为批次根
        let batch_root_id = if preserve_root {
            if let Some(root_c) = root_canonical.as_ref() {
                let root_name = root_c
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("导入");
                match get_or_create_folder(db, base_folder_id, root_name) {
                    Ok(id) => Some(id),
                    Err(e) => {
                        errors.push(format!("创建根文件夹 {} 失败: {}", root_name, e));
                        base_folder_id
                    }
                }
            } else {
                base_folder_id
            }
        } else {
            base_folder_id
        };
        folder_cache.insert(String::new(), batch_root_id);

        for (i, file_path_str) in file_paths.iter().enumerate() {
            let file_path = Path::new(file_path_str);
            let file_name = file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("未命名")
                .to_string();

            // 发送进度事件
            let _ = emitter.emit(
                "import:progress",
                ImportProgress {
                    current: i + 1,
                    total,
                    file_name: file_name.clone(),
                },
            );

            // 读取文件内容
            let content = match std::fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(e) => {
                    errors.push(format!("{}: 读取失败 - {}", file_name, e));
                    continue;
                }
            };

            // 跳过空文件
            if content.trim().is_empty() {
                skipped += 1;
                continue;
            }

            // ─── 去重判定（与 scan 阶段用同一套逻辑，避免扫描后文件被改动造成不一致）
            let canonical_path = canonicalize_path(file_path);
            let title = extract_title(&content).unwrap_or_else(|| file_name.clone());
            let (match_kind, _existing_id) =
                match detect_existing_match_with_content(db, &canonical_path, &title, &content) {
                    Ok(v) => v,
                    Err(e) => {
                        errors.push(format!("{}: 去重检测失败 - {}", file_name, e));
                        continue;
                    }
                };

            // 冲突策略分支
            let final_title = match (match_kind.as_str(), policy) {
                ("new", _) => title,
                (_, ImportConflictPolicy::Skip) => {
                    skipped += 1;
                    continue;
                }
                (_, ImportConflictPolicy::Duplicate) => {
                    // 副本直接加 " (2)" 后缀。重复多次导入会累积为 (2) (2)...
                    // 不查 DB 严格唯一化 —— 用户选副本就是明确要一条独立记录，不追求唯一命名
                    duplicated += 1;
                    format!("{} (2)", title)
                }
            };

            // ─── 定位这条笔记要挂的文件夹 ───
            let target_folder_id = match root_canonical.as_ref() {
                Some(root_c) => {
                    let rel_dir = compute_relative_dir(file_path, root_c);
                    match ensure_folder_path(db, &rel_dir, batch_root_id, &mut folder_cache) {
                        Ok(id) => id,
                        Err(e) => {
                            errors.push(format!("{}: 创建目录失败 - {}", file_name, e));
                            continue;
                        }
                    }
                }
                None => batch_root_id,
            };

            let input = NoteInput {
                title: final_title.clone(),
                content,
                folder_id: target_folder_id,
            };

            match db.create_note(&input) {
                Ok(note) => {
                    // 写入 canonical path，下次导入同一文件即可按 path 去重命中
                    // 注意：Duplicate 策略新建的副本也挂 canonical_path —— 这样下次
                    // 再导入同文件仍会命中 path，按用户当时选的策略处理，不会无限新建
                    let _ = db.set_note_source_file(
                        note.id,
                        Some(&canonical_path),
                        Some("md"),
                    );
                    if match_kind == "new" {
                        imported += 1;
                    }
                    // duplicate 计数已在上面分支累计，不重复加
                }
                Err(e) => {
                    errors.push(format!("{}: 导入失败 - {}", final_title, e));
                }
            }
        }

        let result = ImportResult {
            imported,
            skipped,
            duplicated,
            errors,
        };

        let _ = emitter.emit("import:done", &result);

        Ok(result)
    }

    /// 打开单个 Markdown 文件：
    /// - 首次：创建新笔记并记录 source_file_path
    /// - 重复打开同一文件：复用已有笔记；若文件内容变化则同步回笔记
    ///
    /// 返回 (note_id, was_synced)：was_synced=true 表示发生了内容同步，
    /// 前端可据此显示轻量 toast。
    pub fn import_single_markdown(
        db: &Database,
        file_path: &str,
    ) -> Result<OpenMarkdownResult, AppError> {
        let path = Path::new(file_path);

        // 路径规范化（绝对路径 + 大小写/斜杠统一），保证"同文件多种写法"去重
        let canonical: String = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| file_path.to_string());

        let file_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("未命名")
            .to_string();

        let content = std::fs::read_to_string(path).map_err(|e| {
            AppError::Custom(format!("读取文件失败: {} ({})", file_path, e))
        })?;

        if content.trim().is_empty() {
            return Err(AppError::InvalidInput(format!("文件内容为空: {}", file_path)));
        }

        // 去重：已有同 source_file_path 的活跃笔记 → 复用
        if let Some((existing_id, existing_content)) =
            db.find_active_note_by_source_path(&canonical)?
        {
            // 外部修改过文件 → 同步最新内容到笔记
            let was_synced = existing_content != content;
            if was_synced {
                db.update_note_content(existing_id, &content)?;
                log::info!(
                    "[open-md] 检测到 {} 内容变化，已同步到笔记 #{}",
                    canonical, existing_id
                );
            }
            return Ok(OpenMarkdownResult {
                note_id: existing_id,
                was_synced,
            });
        }

        // 首次打开：创建笔记并记录来源
        let title = extract_title(&content).unwrap_or(file_name);
        let input = NoteInput {
            title,
            content,
            folder_id: None,
        };
        let note = db.create_note(&input)?;
        let _ = db.set_note_source_file(note.id, Some(&canonical), Some("md"));
        Ok(OpenMarkdownResult {
            note_id: note.id,
            was_synced: false,
        })
    }
}

/// 计算某文件相对扫描根的父目录（斜杠统一为 '/'，根层为空串）
fn compute_relative_dir(file_path: &Path, root_canonical: &Path) -> String {
    let parent = file_path.parent().unwrap_or(Path::new(""));
    let parent_canonical: PathBuf =
        std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    parent_canonical
        .strip_prefix(root_canonical)
        .ok()
        .map(|p| {
            p.components()
                .filter_map(|c| c.as_os_str().to_str())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default()
}

/// 确保相对路径 "子A/子B" 对应的 folder 链存在；返回最深那层的 folder_id
/// （根层 rel_path="" 直接返回 batch_root_id）。
fn ensure_folder_path(
    db: &Database,
    rel_path: &str,
    batch_root: Option<i64>,
    cache: &mut HashMap<String, Option<i64>>,
) -> Result<Option<i64>, AppError> {
    if let Some(&cached) = cache.get(rel_path) {
        return Ok(cached);
    }

    let parts: Vec<&str> = rel_path.split('/').filter(|s| !s.is_empty()).collect();
    let mut current_parent: Option<i64> = batch_root;
    let mut accumulated = String::new();

    for part in parts {
        if !accumulated.is_empty() {
            accumulated.push('/');
        }
        accumulated.push_str(part);

        if let Some(&cached) = cache.get(&accumulated) {
            current_parent = cached;
            continue;
        }

        let folder_id = get_or_create_folder(db, current_parent, part)?;
        cache.insert(accumulated.clone(), Some(folder_id));
        current_parent = Some(folder_id);
    }

    Ok(current_parent)
}

/// 查找同层同名文件夹；存在则复用，否则创建
fn get_or_create_folder(
    db: &Database,
    parent_id: Option<i64>,
    name: &str,
) -> Result<i64, AppError> {
    if let Some(id) = db.find_folder_by_name(parent_id, name)? {
        return Ok(id);
    }
    let folder = db.create_folder(name, parent_id)?;
    Ok(folder.id)
}

/// 从 Markdown 内容提取标题（第一个 # 开头的行）
fn extract_title(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            let title = trimmed.trim_start_matches('#').trim().to_string();
            if !title.is_empty() {
                return Some(title);
            }
        }
        // 跳过空行和 frontmatter
        if trimmed.is_empty() || trimmed == "---" {
            continue;
        }
        // 非标题非空行，停止查找
        if !trimmed.starts_with('#') && !trimmed.starts_with("---") {
            break;
        }
    }
    None
}

/// 文件路径规范化字符串（大小写+斜杠统一），用于与 DB source_file_path 精确比对
fn canonicalize_path(file_path: &Path) -> String {
    std::fs::canonicalize(file_path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| file_path.to_string_lossy().into_owned())
}

/// 扫描阶段的去重判定：先读文件内容算 hash，再按 (path) / (title, hash) 查 DB
///
/// 扫描大目录时每个文件都要读一遍；几 KB MD 文件 SHA-256 是毫秒级，可接受。
/// 大文件或海量文件场景再考虑跳过 hash 走仅 path 匹配。
fn detect_existing_match(
    db: &Database,
    file_path: &Path,
    file_stem: &str,
) -> Result<(String, Option<i64>), AppError> {
    let canonical = canonicalize_path(file_path);

    // 先按 path 匹配（最精确，不用读文件）
    if let Some((id, _)) = db.find_active_note_by_source_path(&canonical)? {
        return Ok(("path".to_string(), Some(id)));
    }

    // 按 title + content_hash 兜底
    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(_) => return Ok(("new".to_string(), None)),
    };
    let title = extract_title(&content).unwrap_or_else(|| file_stem.to_string());
    let hash = sha256_hex(&content);
    if let Some(id) = db.find_active_note_by_title_and_hash(&title, &hash)? {
        return Ok(("fuzzy".to_string(), Some(id)));
    }

    Ok(("new".to_string(), None))
}

/// import 阶段的去重判定：content 已读入内存，避免再读文件
fn detect_existing_match_with_content(
    db: &Database,
    canonical_path: &str,
    title: &str,
    content: &str,
) -> Result<(String, Option<i64>), AppError> {
    if let Some((id, _)) = db.find_active_note_by_source_path(canonical_path)? {
        return Ok(("path".to_string(), Some(id)));
    }
    let hash = sha256_hex(content);
    if let Some(id) = db.find_active_note_by_title_and_hash(title, &hash)? {
        return Ok(("fuzzy".to_string(), Some(id)));
    }
    Ok(("new".to_string(), None))
}
