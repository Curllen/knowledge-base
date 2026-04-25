//! T-009 OB 整库导入：附件目录扫描 + 笔记正文图片路径重写
//!
//! 流程概要：
//! 1. 扫描 vault 根下的 OB 约定附件目录（`attachments/` `assets/` `images/` `_resources/`），
//!    建一个"basename → 源文件绝对路径"的索引（仅图片扩展名）
//! 2. 对每篇导入的 .md 笔记，解析其 body 中两类图片引用：
//!    - 标准 markdown：`![alt](path)`（路径相对当前 .md / 相对 vault 根 / 绝对路径）
//!    - OB wiki 嵌入：`![[name|alt|width]]`（按 basename 全局检索）
//! 3. 找到本地源文件后，调 `ImageService::save_from_path` 复制到
//!    `kb_assets/images/<note_id>/`，再把 body 中的引用改写成 asset URL，
//!    供 Tiptap 编辑器直接渲染
//! 4. 缺失的图片记入 `RewriteResult::missing`，让前端提示用户

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use walkdir::WalkDir;

use crate::error::AppError;
use crate::services::image::ImageService;

/// 受支持的图片扩展名（小写比对）
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "avif", "ico"];

/// OB 约定的附件目录名（vault 根下的同层目录，不递归）
const ATTACHMENT_DIR_NAMES: &[&str] = &["attachments", "assets", "images", "_resources"];

/// vault 根下扫到的附件索引
///
/// `by_basename` 的 key 是**小写化**的 basename；多个目录里同名文件存在时，
/// **第一个找到的胜出**（与 OB 行为一致），后续的记 warn 日志
pub struct AttachmentIndex {
    pub by_basename: HashMap<String, PathBuf>,
    /// 仅给单元测试 / 日志用，运行时业务不读
    #[allow(dead_code)]
    pub total_indexed: usize,
}

impl AttachmentIndex {
    pub fn empty() -> Self {
        Self {
            by_basename: HashMap::new(),
            total_indexed: 0,
        }
    }

    /// 扫描 vault 根下约定的附件目录，仅收图片文件
    ///
    /// 仅处理直接子目录中那几个名字（attachments / assets / images / _resources）；
    /// 不在 vault 根的图片不入索引（避免把无关图片也带进来）
    pub fn build(vault_root: &Path) -> Self {
        let mut by_basename: HashMap<String, PathBuf> = HashMap::new();
        let mut total = 0usize;

        for dir_name in ATTACHMENT_DIR_NAMES {
            let dir = vault_root.join(dir_name);
            if !dir.is_dir() {
                continue;
            }
            for entry in WalkDir::new(&dir).into_iter().filter_map(|e| e.ok()) {
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                let ext = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                if !IMAGE_EXTS.contains(&ext.as_str()) {
                    continue;
                }
                let key = match path.file_name().and_then(|s| s.to_str()) {
                    Some(n) => n.to_ascii_lowercase(),
                    None => continue,
                };
                total += 1;
                if let Some(existing) = by_basename.get(&key) {
                    log::warn!(
                        "[import-attach] 同名附件：'{}' 已索引 {}, 忽略 {}（采用先到先得策略）",
                        key,
                        existing.display(),
                        path.display()
                    );
                    continue;
                }
                by_basename.insert(key, path.to_path_buf());
            }
        }

        Self {
            by_basename,
            total_indexed: total,
        }
    }
}

/// 单篇笔记 body 重写结果
pub struct RewriteResult {
    pub new_body: String,
    /// 这条笔记里**新复制成功**的图片张数（每个引用都计 1，即使源文件相同）
    pub copied: usize,
    /// 缺失的图片（用户原始引用文本，去重后展示给用户）
    pub missing: Vec<String>,
}

impl RewriteResult {
    /// 构造一个"未改动"的结果
    fn unchanged(body: String) -> Self {
        Self {
            new_body: body,
            copied: 0,
            missing: Vec::new(),
        }
    }
}

fn md_image_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // 标准 markdown 图片：`![alt](url)`，url 不含右括号；alt 可能有方括号转义
        // 这个 regex 不处理嵌套 `()`，与 markdown 标准基本一致
        Regex::new(r#"!\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)"#).unwrap()
    })
}

fn ob_wiki_embed_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // OB 嵌入：`![[name]]` / `![[name|alt]]` / `![[name|alt|300]]`
        // name 不含 `|` 和 `]`
        Regex::new(r"!\[\[([^\]\|]+?)(\|[^\]]*)?\]\]").unwrap()
    })
}

/// 把绝对路径转成 Tauri asset 协议 URL
///
/// Win:  http://asset.localhost/<encoded>
/// 其他: asset://localhost/<encoded>
///
/// 与前端 `convertFileSrc(absPath)` 行为一致；CSP 已在 tauri.conf.json 放行。
pub fn path_to_asset_url(abs: &Path) -> String {
    let s = abs.to_string_lossy().replace('\\', "/");
    let encoded = urlencoding::encode(&s);
    if cfg!(target_os = "windows") {
        format!("http://asset.localhost/{}", encoded)
    } else {
        format!("asset://localhost/{}", encoded)
    }
}

/// 在 body 中重写所有图片引用为 asset URL
///
/// `note_file_dir` 用于解析 `./xxx` / `xxx.png` 之类相对当前 .md 的路径
/// `vault_root` 用于解析 `attachments/foo.png` 之类相对 vault 根的路径
/// `index` 是预建的全局 basename 索引（OB wiki / fallback）
/// `app_data_dir` 给 `ImageService` 用
pub fn rewrite_image_paths(
    body: &str,
    note_id: i64,
    note_file_dir: &Path,
    vault_root: &Path,
    index: &AttachmentIndex,
    app_data_dir: &Path,
) -> Result<RewriteResult, AppError> {
    if body.is_empty() {
        return Ok(RewriteResult::unchanged(String::new()));
    }

    let mut copied = 0usize;
    let mut missing: Vec<String> = Vec::new();

    // ─── pass 1：替换标准 markdown `![alt](path)` ───
    let md_re = md_image_regex();
    let after_md = md_re
        .replace_all(body, |caps: &regex::Captures| {
            let full_match = caps.get(0).unwrap().as_str().to_string();
            let alt = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let raw_url = caps.get(2).map(|m| m.as_str()).unwrap_or("").trim();

            // 跳过外链 / data URL / 已经是 asset URL（重复运行幂等）
            if is_external_or_asset_url(raw_url) {
                return full_match;
            }

            let decoded = urlencoding::decode(raw_url)
                .map(|s| s.into_owned())
                .unwrap_or_else(|_| raw_url.to_string());

            // 解析候选源路径：先按当前 .md 目录，再按 vault 根，再按 basename 索引
            let resolved = resolve_local_image(&decoded, note_file_dir, vault_root, index);
            match resolved {
                Some(src) => {
                    match ImageService::save_from_path(
                        app_data_dir,
                        note_id,
                        &src.to_string_lossy(),
                    ) {
                        Ok(new_abs) => {
                            copied += 1;
                            let url = path_to_asset_url(Path::new(&new_abs));
                            format!("![{}]({})", alt, url)
                        }
                        Err(e) => {
                            log::warn!(
                                "[import-attach] 笔记 {} 图片复制失败 ({}): {}",
                                note_id,
                                src.display(),
                                e
                            );
                            missing.push(raw_url.to_string());
                            full_match
                        }
                    }
                }
                None => {
                    missing.push(raw_url.to_string());
                    full_match
                }
            }
        })
        .into_owned();

    // ─── pass 2：替换 OB wiki 嵌入 `![[name|alt|w]]` ───
    let wiki_re = ob_wiki_embed_regex();
    let after_wiki = wiki_re
        .replace_all(&after_md, |caps: &regex::Captures| {
            let full_match = caps.get(0).unwrap().as_str().to_string();
            let raw_name = caps.get(1).map(|m| m.as_str()).unwrap_or("").trim();
            let extra = caps.get(2).map(|m| m.as_str()).unwrap_or(""); // 含前导 |

            // 仅处理图片扩展名；非图片（如 ![[some-note]] 嵌入笔记）保持原样
            if !is_image_filename(raw_name) {
                return full_match;
            }

            let key = Path::new(raw_name)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(raw_name)
                .to_ascii_lowercase();

            match index.by_basename.get(&key) {
                Some(src) => {
                    match ImageService::save_from_path(
                        app_data_dir,
                        note_id,
                        &src.to_string_lossy(),
                    ) {
                        Ok(new_abs) => {
                            copied += 1;
                            let url = path_to_asset_url(Path::new(&new_abs));
                            // 保留 wiki 的 alt（| 之后的第一段），舍弃宽度（v1 不处理）
                            let alt_text = parse_wiki_alt(extra);
                            format!("![{}]({})", alt_text, url)
                        }
                        Err(e) => {
                            log::warn!(
                                "[import-attach] 笔记 {} OB-wiki 图片复制失败 ({}): {}",
                                note_id,
                                src.display(),
                                e
                            );
                            missing.push(raw_name.to_string());
                            full_match
                        }
                    }
                }
                None => {
                    missing.push(raw_name.to_string());
                    full_match
                }
            }
        })
        .into_owned();

    // missing 去重（保持插入顺序）
    let mut seen: HashMap<String, ()> = HashMap::new();
    let dedup_missing: Vec<String> = missing
        .into_iter()
        .filter(|m| seen.insert(m.clone(), ()).is_none())
        .collect();

    Ok(RewriteResult {
        new_body: after_wiki,
        copied,
        missing: dedup_missing,
    })
}

fn is_external_or_asset_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://asset.localhost/")
        || lower.starts_with("asset://")
        || lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("data:")
        || lower.starts_with("file://")
        || lower.starts_with("kb-image://")
}

fn is_image_filename(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| IMAGE_EXTS.contains(&s.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// 解析 wiki 嵌入的 `|alt|w` 后缀；返回纯 alt（无宽度）
fn parse_wiki_alt(extra: &str) -> &str {
    if extra.is_empty() {
        return "";
    }
    // 去掉前导 |
    let s = extra.strip_prefix('|').unwrap_or(extra);
    // 取第一段（| 切分）；若它纯是数字（即 OB 写宽度的形式 `![[a.png|300]]`），返回空 alt
    let first = s.split('|').next().unwrap_or("");
    if first.trim().chars().all(|c| c.is_ascii_digit()) {
        ""
    } else {
        first
    }
}

/// 给定一个原始引用，依次尝试：当前 .md 目录 → vault 根 → 全局 basename 索引
fn resolve_local_image(
    raw_url: &str,
    note_file_dir: &Path,
    vault_root: &Path,
    index: &AttachmentIndex,
) -> Option<PathBuf> {
    // 绝对路径直接判定
    let p = Path::new(raw_url);
    if p.is_absolute() && p.is_file() {
        return Some(p.to_path_buf());
    }

    // 相对当前 .md 目录
    let rel_to_note = note_file_dir.join(raw_url);
    if rel_to_note.is_file() {
        return Some(rel_to_note);
    }

    // 相对 vault 根
    let rel_to_vault = vault_root.join(raw_url);
    if rel_to_vault.is_file() {
        return Some(rel_to_vault);
    }

    // 兜底：按 basename 在全局索引里查
    let basename = Path::new(raw_url)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());
    if let Some(key) = basename {
        if let Some(p) = index.by_basename.get(&key) {
            return Some(p.clone());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kb-import-attach-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 构造一个最小 vault：vault_root/attachments/foo.png + bar.png；返回 (vault, app_data)
    fn make_vault() -> (PathBuf, PathBuf) {
        let root = temp_root();
        let vault = root.join("vault");
        let app_data = root.join("app_data");
        std::fs::create_dir_all(vault.join("attachments")).unwrap();
        std::fs::create_dir_all(vault.join("images")).unwrap();
        std::fs::create_dir_all(&app_data).unwrap();
        // 写 1px PNG header — 内容不重要，只要文件存在
        let png_bytes: &[u8] = b"\x89PNG\r\n\x1a\n";
        std::fs::write(vault.join("attachments/foo.png"), png_bytes).unwrap();
        std::fs::write(vault.join("attachments/bar.png"), png_bytes).unwrap();
        std::fs::write(vault.join("images/space pic.jpg"), png_bytes).unwrap();
        (vault, app_data)
    }

    #[test]
    fn build_index_collects_images_by_basename() {
        let (vault, _) = make_vault();
        let idx = AttachmentIndex::build(&vault);
        assert_eq!(idx.total_indexed, 3);
        assert!(idx.by_basename.contains_key("foo.png"));
        assert!(idx.by_basename.contains_key("bar.png"));
        assert!(idx.by_basename.contains_key("space pic.jpg"));
    }

    #[test]
    fn rewrite_standard_md_link() {
        let (vault, app_data) = make_vault();
        let idx = AttachmentIndex::build(&vault);
        let body = "# T\n\n![描述](attachments/foo.png)\n";
        let r = rewrite_image_paths(body, 42, &vault, &vault, &idx, &app_data).unwrap();
        assert_eq!(r.copied, 1);
        assert!(r.missing.is_empty());
        assert!(
            r.new_body.contains("asset.localhost") || r.new_body.contains("asset://localhost"),
            "应被改写成 asset URL，实际：{}",
            r.new_body
        );
        // alt 文本保留
        assert!(r.new_body.contains("![描述]"));
    }

    #[test]
    fn rewrite_obsidian_wiki_embed_with_alt_and_width() {
        let (vault, app_data) = make_vault();
        let idx = AttachmentIndex::build(&vault);
        let body = "前文 ![[bar.png|示例图|400]] 后文";
        let r = rewrite_image_paths(body, 7, &vault, &vault, &idx, &app_data).unwrap();
        assert_eq!(r.copied, 1);
        // alt 取第一段 "示例图"，宽度 400 被丢弃
        assert!(r.new_body.contains("![示例图]"), "得到：{}", r.new_body);
    }

    #[test]
    fn rewrite_obsidian_wiki_embed_pure_width() {
        let (vault, app_data) = make_vault();
        let idx = AttachmentIndex::build(&vault);
        let body = "![[foo.png|300]]";
        let r = rewrite_image_paths(body, 7, &vault, &vault, &idx, &app_data).unwrap();
        // 纯数字宽度 → alt 留空
        assert!(r.new_body.starts_with("![]("), "得到：{}", r.new_body);
    }

    #[test]
    fn external_urls_not_touched() {
        let (vault, app_data) = make_vault();
        let idx = AttachmentIndex::build(&vault);
        let body = "![remote](https://cdn.example.com/x.png) ![data](data:image/png;base64,AAA)";
        let r = rewrite_image_paths(body, 1, &vault, &vault, &idx, &app_data).unwrap();
        assert_eq!(r.copied, 0);
        assert!(r.missing.is_empty());
        assert_eq!(r.new_body, body); // 一模一样
    }

    #[test]
    fn missing_image_recorded_and_body_preserved() {
        let (vault, app_data) = make_vault();
        let idx = AttachmentIndex::build(&vault);
        let body = "![](attachments/notexist.png)";
        let r = rewrite_image_paths(body, 1, &vault, &vault, &idx, &app_data).unwrap();
        assert_eq!(r.copied, 0);
        assert_eq!(r.missing, vec!["attachments/notexist.png".to_string()]);
        // body 保留原引用，便于用户后续手动修
        assert_eq!(r.new_body, body);
    }

    #[test]
    fn wiki_embed_falls_back_to_basename_index() {
        let (vault, app_data) = make_vault();
        let idx = AttachmentIndex::build(&vault);
        // 写法没带目录，OB 风格按 basename 全局查
        let body = "![[foo.png]]";
        let r = rewrite_image_paths(body, 1, &vault, &vault, &idx, &app_data).unwrap();
        assert_eq!(r.copied, 1);
        assert!(r.missing.is_empty());
    }

    #[test]
    fn idempotent_when_already_asset_url() {
        let (vault, app_data) = make_vault();
        let idx = AttachmentIndex::build(&vault);
        // 二次跑（用户重导入同一个被改写过的笔记）不会再次复制
        let body = "![](http://asset.localhost/some/path.png)";
        let r = rewrite_image_paths(body, 1, &vault, &vault, &idx, &app_data).unwrap();
        assert_eq!(r.copied, 0);
        assert!(r.missing.is_empty());
        assert_eq!(r.new_body, body);
    }

    #[test]
    fn missing_dedup() {
        let (vault, app_data) = make_vault();
        let idx = AttachmentIndex::build(&vault);
        let body = "![](a.png) ![](a.png) ![[b.png]] ![[b.png]]";
        let r = rewrite_image_paths(body, 1, &vault, &vault, &idx, &app_data).unwrap();
        assert_eq!(r.missing.len(), 2);
    }
}
