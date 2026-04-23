use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use pdfium_render::prelude::*;
use serde::Serialize;

use crate::database::Database;
use crate::error::AppError;
use crate::models::{Note, NoteInput};

/// PDFium 全局实例（运行时绑定到 pdfium.dll），用作 pdf-extract 的 fallback。
///
/// pdfium-render 的 `Pdfium` 不实现 Send（内部 `Box<dyn PdfiumLibraryBindings>`），
/// 但 Tauri Command 在 worker 线程执行，需要跨线程共享。因此用 newtype 包裹并手动声明
/// Send/Sync —— 安全性由外层 `Mutex` 保证（同一时刻只有一个线程持有 PDFium 引用）。
struct PdfiumGuard(Pdfium);
// SAFETY: PDFium 底层 C API 非线程安全，但我们通过 Mutex 串行化所有访问
unsafe impl Send for PdfiumGuard {}
unsafe impl Sync for PdfiumGuard {}

static PDFIUM: OnceLock<Mutex<PdfiumGuard>> = OnceLock::new();

/// 应用启动时调用：把 PDFium 绑定到指定路径的动态库
pub fn init_pdfium(dll_path: &Path) -> Result<(), String> {
    let bindings = Pdfium::bind_to_library(dll_path).map_err(|e| e.to_string())?;
    let pdfium = Pdfium::new(bindings);
    PDFIUM
        .set(Mutex::new(PdfiumGuard(pdfium)))
        .map_err(|_| "PDFium 已被初始化过".to_string())
}

/// PDF 资产目录名（dev 模式加 dev- 前缀实现数据隔离）
const PDFS_DIR_PROD: &str = "pdfs";
const PDFS_DIR_DEV: &str = "dev-pdfs";

#[inline]
fn pdfs_dir_name() -> &'static str {
    if cfg!(debug_assertions) { PDFS_DIR_DEV } else { PDFS_DIR_PROD }
}

/// 单个 PDF 导入结果，供前端展示进度/错误清单
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfImportResult {
    pub source_path: String,
    /// 成功：对应的笔记 id；失败：None
    pub note_id: Option<i64>,
    /// 成功：笔记标题；失败：None
    pub title: Option<String>,
    /// 失败时的错误消息
    pub error: Option<String>,
}

pub struct PdfService;

impl PdfService {
    /// 获取 PDF 根目录: {app_data_dir}/{prefix}pdfs/
    pub fn pdfs_dir(app_data_dir: &Path) -> PathBuf {
        app_data_dir.join(pdfs_dir_name())
    }

    /// 确保 PDF 目录存在
    pub fn ensure_dir(app_data_dir: &Path) -> Result<PathBuf, AppError> {
        let dir = Self::pdfs_dir(app_data_dir);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// 把一个 PDF 文件导入为笔记：抽取文本 → 创建笔记 → 拷贝原文件 → 更新 pdf_path
    pub fn import_one(
        app_data_dir: &Path,
        db: &Database,
        source_path: &str,
        folder_id: Option<i64>,
    ) -> Result<Note, AppError> {
        let source = Path::new(source_path);
        if !source.exists() {
            return Err(AppError::NotFound(format!("PDF 文件不存在: {}", source_path)));
        }

        // 1. 抽取文本（扫描件 / 加密 PDF / xref 损坏会失败）
        //    对 xref 损坏类错误（如 CNKI 知网下载件）自动尝试修复后重试一次
        let raw_text = extract_text_with_repair(source)?;
        let text = normalize_text(&raw_text);

        // 2. 标题取源文件名（去后缀）
        let title = source
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "未命名 PDF".to_string());

        // 3. 创建笔记（content 先存抽出的纯文本，包在 <p> 里满足 Tiptap HTML 期望）
        let note = db.create_note(&NoteInput {
            title: title.clone(),
            content: text_to_simple_html(&text),
            folder_id,
        })?;

        // 4. 拷贝原 PDF 到 pdfs/<id>.pdf
        Self::ensure_dir(app_data_dir)?;
        let rel_path = format!("{}/{}.pdf", pdfs_dir_name(), note.id);
        let dst = app_data_dir.join(&rel_path);
        if let Err(e) = std::fs::copy(source, &dst) {
            // 拷贝失败：笔记已经建好了也算导入成功，只是不关联 PDF
            log::warn!("PDF 原文件拷贝失败（笔记已建）: {}", e);
            return Ok(note);
        }

        // 5. 更新 source_file_path 和 source_file_type
        db.set_note_source_file(note.id, Some(&rel_path), Some("pdf"))?;

        // 6. 重新取完整 note 带 source_file_path 返回
        let note = db
            .get_note(note.id)?
            .ok_or_else(|| AppError::NotFound("刚创建的笔记查询失败".into()))?;
        Ok(note)
    }

    /// 批量导入，收集每条结果（不中断整体流程）
    pub fn import_many(
        app_data_dir: &Path,
        db: &Database,
        source_paths: &[String],
        folder_id: Option<i64>,
    ) -> Vec<PdfImportResult> {
        source_paths
            .iter()
            .map(|p| match Self::import_one(app_data_dir, db, p, folder_id) {
                Ok(note) => PdfImportResult {
                    source_path: p.clone(),
                    note_id: Some(note.id),
                    title: Some(note.title),
                    error: None,
                },
                Err(e) => PdfImportResult {
                    source_path: p.clone(),
                    note_id: None,
                    title: None,
                    error: Some(e.to_string()),
                },
            })
            .collect()
    }

    /// 根据 note_id 解析出 PDF 绝对路径（不存在则返回 None）
    pub fn resolve_pdf_absolute_path(
        app_data_dir: &Path,
        pdf_path: &str,
    ) -> Option<PathBuf> {
        let abs = app_data_dir.join(pdf_path);
        if abs.exists() { Some(abs) } else { None }
    }
}

/// 抽取 PDF 文本；若首轮失败且错误属于 xref 类损坏，尝试字节级修复后重跑一次。
/// pdf_extract 0.9 对不认识的字体编码（如 CNKI 常见的 `GBK-EUC-H`）会 `panic!`，
/// 所以这里用 `catch_unwind` 全程兜底，避免整个 Tauri 后端进程被一份坏 PDF 击穿。
///
/// 完整失败链：
/// 1. `pdf-extract` 直抽
/// 2. xref 错误 → 字节级修复后重抽
/// 3. 上面两步都失败 → PDFium fallback（唯一能解中文 CMap 的路径）
/// 4. PDFium 也抽不出 → 返回友好错误
fn extract_text_with_repair(source: &Path) -> Result<String, AppError> {
    let first_err = match safe_extract_text(source) {
        Ok(t) if !t.trim().is_empty() => return Ok(t),
        Ok(_) => "pdf-extract 返回空文本".to_string(),
        Err(e) => e,
    };

    // 若是 xref 类错误，先试字节级修复
    let second_err = if is_xref_error(&first_err) {
        log::warn!("PDF 首轮抽取失败（xref 错误），尝试修复重试: {}", first_err);
        match try_extract_after_repair(source) {
            Ok(t) if !t.trim().is_empty() => {
                log::info!("PDF xref 修复成功，已抽取文本");
                return Ok(t);
            }
            Ok(_) => "修复后 pdf-extract 返回空文本".to_string(),
            Err(e) => e,
        }
    } else {
        first_err.clone()
    };

    // 最后的手段：PDFium fallback
    match extract_with_pdfium(source) {
        Ok(t) if !t.trim().is_empty() => {
            log::info!(
                "PDF 通过 PDFium fallback 抽取成功（pdf-extract 路径报错: {}）",
                first_err
            );
            Ok(t)
        }
        Ok(_) => {
            log::warn!("PDFium 抽取返回空文本（可能是扫描件 / 无文本层）");
            Err(AppError::Custom(
                "PDF 无文本层（可能是纯图片扫描件），请先 OCR 后再导入".into(),
            ))
        }
        Err(pdfium_err) => {
            log::warn!(
                "PDF 全部路径失败: pdf-extract={}, repair={}, pdfium={}",
                first_err,
                second_err,
                pdfium_err
            );
            // 友好提示基于 pdf-extract 的错误文本（用户通常装的是 pdf-extract 路径）
            Err(AppError::Custom(friendly_extract_error(&second_err)))
        }
    }
}

/// 用 PDFium 抽取 PDF 文本（逐页拼接）。PDFium 未初始化时返回 Err。
fn extract_with_pdfium(source: &Path) -> Result<String, String> {
    let mutex = PDFIUM
        .get()
        .ok_or_else(|| "PDFium 未初始化（dll 加载失败）".to_string())?;
    let guard = mutex.lock().map_err(|e| format!("PDFium 锁被毒化: {}", e))?;
    let pdfium = &guard.0;

    let doc = pdfium
        .load_pdf_from_file(source, None)
        .map_err(|e| format!("PDFium 打开 PDF 失败: {}", e))?;

    let mut pages_text = Vec::new();
    for page in doc.pages().iter() {
        let page_text = page
            .text()
            .map_err(|e| format!("PDFium 读取页面文本失败: {}", e))?;
        pages_text.push(page_text.all());
    }
    Ok(pages_text.join("\n\n"))
}

/// 用 `catch_unwind` 包裹 pdf_extract::extract_text，把 panic 也转成普通错误返回
fn safe_extract_text(path: &Path) -> Result<String, String> {
    let path = path.to_path_buf();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pdf_extract::extract_text(&path)
    }));
    match result {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(e)) => Err(e.to_string()),
        Err(panic_payload) => {
            let msg = if let Some(s) = panic_payload.downcast_ref::<&'static str>() {
                (*s).to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "PDF 解析发生未知 panic".to_string()
            };
            Err(format!("pdf-extract panic: {}", msg))
        }
    }
}

/// 读入整份 PDF → 做字节级修复 → 写到临时文件 → 重新调用 pdf_extract
fn try_extract_after_repair(source: &Path) -> Result<String, String> {
    let raw = std::fs::read(source).map_err(|e| e.to_string())?;
    let repaired = repair_pdf_bytes(&raw);

    // 临时文件放在系统临时目录，名字加 PID + 源文件 stem 防冲突
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("src");
    // 只保留 ASCII 字母数字，避免临时目录路径里混中文被某些环境拒绝
    let safe_stem: String = stem
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(32)
        .collect();
    let tmp_name = format!("kb_pdf_repair_{}_{}.pdf", std::process::id(), safe_stem);
    let tmp_path = std::env::temp_dir().join(tmp_name);

    std::fs::write(&tmp_path, &repaired).map_err(|e| e.to_string())?;
    let result = safe_extract_text(&tmp_path);
    // 清理临时文件（失败不阻断）
    let _ = std::fs::remove_file(&tmp_path);
    result
}

/// 判定 pdf_extract 的错误信息是否属于"可修复"的 xref/trailer 类
fn is_xref_error(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    lower.contains("cross reference")
        || lower.contains("xref")
        || lower.contains("invalid start value")
        || lower.contains("trailer")
}

/// 字节级修复 PDF 中常见的 CNKI（知网）/非标准工具写出的格式问题
///
/// 处理两类违规：
/// 1. xref 头同行：`xref N M\n` → `xref\nN M\n`（PDF 1.7 §7.5.4 要求 xref 单独一行）
/// 2. `%%EOF` 之后有额外字节（CNKI 的 `WebFastLoad<FileProperty>...`）→ 截断
pub(crate) fn repair_pdf_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    fix_xref_header_inline(&mut out);
    truncate_after_last_eof(&mut out);
    out
}

/// 把 `xref ` 后紧跟数字的位置的空格替换为换行符
/// 只处理 `xref` 关键字且前后无字母数字粘连（避免误伤出现在流数据里的字节串）
fn fix_xref_header_inline(data: &mut [u8]) {
    let pat = b"xref ";
    let mut i = 0;
    while i + pat.len() < data.len() {
        if &data[i..i + pat.len()] == pat {
            // 前一个字节必须是换行/回车/空白，才是真正的 xref 关键字
            let prev_ok = i == 0
                || matches!(data[i - 1], b'\n' | b'\r' | b' ' | b'\t');
            let next_byte = data[i + pat.len()];
            if prev_ok && next_byte.is_ascii_digit() {
                // i+4 是 "xref " 里的空格，替换为 \n
                data[i + 4] = b'\n';
            }
            i += pat.len();
        } else {
            i += 1;
        }
    }
}

/// 保留最后一个 `%%EOF` 及其后的一个换行符，截掉后续所有字节
fn truncate_after_last_eof(data: &mut Vec<u8>) {
    let eof = b"%%EOF";
    let pos = match data
        .windows(eof.len())
        .rposition(|w| w == eof)
    {
        Some(p) => p,
        None => return,
    };
    let after = pos + eof.len();
    let mut keep = after;
    if keep < data.len() && data[keep] == b'\r' {
        keep += 1;
        if keep < data.len() && data[keep] == b'\n' {
            keep += 1;
        }
    } else if keep < data.len() && data[keep] == b'\n' {
        keep += 1;
    }
    if keep < data.len() {
        data.truncate(keep);
    }
}

/// 把 pdf_extract 原始错误文本转成面向用户的友好提示
///
/// 常见失败类型：
/// - 字体编码 panic（如 `unsupported encoding GBK-EUC-H`）：CNKI 知网/方正等用了非标准中文 CMap
/// - xref / trailer 相关：PDF 交叉引用表损坏
/// - Encrypt / encryption：加密或带权限限制
/// - 其他：走通用提示，保留原文便于排查
fn friendly_extract_error(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    // 字体编码优先判断（CNKI PDF 修完 xref 后最常撞到的就是这个）
    if lower.contains("unsupported encoding")
        || lower.contains("cmap")
        || lower.contains("gbk-euc")
        || lower.contains("gb-euc")
        || (lower.contains("panic") && lower.contains("encoding"))
    {
        format!(
            "PDF 使用了当前版本不支持的中文字体编码（常见于中国知网下载件）。解决方案：用 Chrome/Edge 打开该 PDF，按 Ctrl+P → 目标选「另存为 PDF」→ 保存新文件后再导入即可。原始错误: {}",
            raw
        )
    } else if lower.contains("cross reference")
        || lower.contains("xref")
        || lower.contains("invalid start value")
        || lower.contains("trailer")
    {
        format!(
            "PDF 交叉引用表损坏，无法解析。请用 Chrome/Edge 打开该 PDF，然后「打印 → 另存为 PDF」生成新文件后再导入。原始错误: {}",
            raw
        )
    } else if lower.contains("encrypt") {
        format!("PDF 已加密或有权限限制，暂不支持导入。请先解除加密后再试。原始错误: {}", raw)
    } else {
        format!("PDF 文本抽取失败: {}", raw)
    }
}

/// 把抽出的纯文本转成简单 HTML（段落分隔），兼容 Tiptap StarterKit 解析
fn text_to_simple_html(text: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    // 以空行切成段落；段落内保留换行
    let paragraphs: Vec<String> = text
        .split("\n\n")
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| {
            let escaped = html_escape(p).replace('\n', "<br/>");
            format!("<p>{}</p>", escaped)
        })
        .collect();
    paragraphs.join("\n")
}

/// 规范化文本：清洗 pdf-extract 抽出的常见垃圾字符并修整结构
///
/// 处理顺序：
/// 1. 换行规范化（CRLF → LF）
/// 2. 逐行清洗：去零宽字符、行首 PUA/豆腐字符还原为 "• "、行内 PUA/替换字符删除
/// 3. 多余空行压成最多 2 个
fn normalize_text(raw: &str) -> String {
    let lf = raw.replace("\r\n", "\n").replace('\r', "\n");
    let cleaned: String = lf
        .split('\n')
        .map(clean_line)
        .collect::<Vec<_>>()
        .join("\n");
    collapse_blank_lines(&cleaned)
}

/// 单行清洗：处理零宽字符、行首项目符号字形、行内不可打印字符
fn clean_line(line: &str) -> String {
    // 1. 去零宽字符
    let no_zw: String = line.chars().filter(|c| !is_zero_width(*c)).collect();

    // 2. 行首处理：跳过前导空白，若开头是疑似项目符号字形（PUA / FFFD 等），还原成 "•"
    let leading_ws: String = no_zw.chars().take_while(|c| c.is_whitespace()).collect();
    let body = &no_zw[leading_ws.len()..];

    if let Some(first) = body.chars().next() {
        if is_likely_bullet_glyph(first) {
            // 吃掉连续多个 bullet 字形（PDF 有时一个 bullet 占多个字符）
            let bullet_end = body
                .char_indices()
                .find(|(_, c)| !is_likely_bullet_glyph(*c))
                .map(|(i, _)| i)
                .unwrap_or(body.len());
            let rest = &body[bullet_end..];
            return format!(
                "{}• {}",
                leading_ws,
                strip_unprintable(rest).trim_start()
            );
        }
    }

    // 3. 非项目符号行：仅做行内不可打印清洗
    format!("{}{}", leading_ws, strip_unprintable(body))
}

/// 删除行内的 PUA 区段字符与替换字符（这些是 pdf-extract 没解出的字形残留）
fn strip_unprintable(s: &str) -> String {
    s.chars()
        .filter(|&c| !is_pua(c) && c != '\u{FFFD}')
        .collect()
}

/// 0-宽字符（不可见但污染搜索/光标）
fn is_zero_width(c: char) -> bool {
    matches!(
        c,
        '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}' | '\u{2060}'
    )
}

/// Unicode Private Use Area（PDF 嵌入子集字体常用区段，无字形定义）
fn is_pua(c: char) -> bool {
    matches!(c as u32, 0xE000..=0xF8FF)
}

/// 判断是否疑似"被错抽的项目符号字形"
///
/// PDF 里项目符号 `•` 在很多字体（如 Wingdings、Symbol、自制嵌入字体）
/// 走的是 PUA 字形，pdf-extract 输出 \uF0B7 / \uFFFD / 各种 PUA 码点。
fn is_likely_bullet_glyph(c: char) -> bool {
    is_pua(c) || c == '\u{FFFD}'
}

/// 把连续 3+ 个换行压成 2 个，整体 trim
fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut newline_run = 0usize;
    for ch in s.chars() {
        if ch == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                out.push('\n');
            }
        } else {
            newline_run = 0;
            out.push(ch);
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pua_at_line_start_becomes_bullet() {
        let raw = "\u{E020} 将本软件作为独立产品销售\n普通段落";
        let out = normalize_text(raw);
        assert!(out.starts_with("• 将本软件作为独立产品销售"));
        assert!(out.contains("普通段落"));
    }

    #[test]
    fn fffd_at_line_start_becomes_bullet() {
        let raw = "\u{FFFD} 第一项\n\u{FFFD} 第二项";
        let out = normalize_text(raw);
        assert_eq!(out, "• 第一项\n• 第二项");
    }

    #[test]
    fn zero_width_chars_removed() {
        let raw = "正\u{200B}文\u{FEFF}内\u{200C}容";
        assert_eq!(normalize_text(raw), "正文内容");
    }

    #[test]
    fn inline_pua_stripped_normal_line_kept() {
        let raw = "正文里夹\u{E100}个 PUA";
        assert_eq!(normalize_text(raw), "正文里夹个 PUA");
    }

    #[test]
    fn excessive_blank_lines_collapsed() {
        let raw = "A\n\n\n\nB\n\n\n\n\nC";
        assert_eq!(normalize_text(raw), "A\n\nB\n\nC");
    }

    // ─── PDF 字节级修复测试 ──────────────────────────────

    #[test]
    fn is_xref_error_matches_real_lopdf_message() {
        let msg = "PDF error: failed parsing cross reference table: invalid start value";
        assert!(is_xref_error(msg));
    }

    #[test]
    fn is_xref_error_ignores_unrelated_errors() {
        assert!(!is_xref_error("Encrypted PDF is not supported"));
        assert!(!is_xref_error("Unknown font encoding"));
    }

    #[test]
    fn friendly_error_recognizes_gbk_encoding_panic() {
        let msg = "pdf-extract panic: unsupported encoding GBK-EUC-H";
        let out = friendly_extract_error(msg);
        assert!(out.contains("中文字体编码"));
        assert!(out.contains("Chrome") || out.contains("Edge"));
    }

    #[test]
    fn friendly_error_recognizes_xref_error() {
        let msg = "PDF error: failed parsing cross reference table: invalid start value";
        let out = friendly_extract_error(msg);
        assert!(out.contains("交叉引用表损坏"));
    }

    #[test]
    fn friendly_error_recognizes_encrypted_pdf() {
        let msg = "PDF error: document is encrypted";
        let out = friendly_extract_error(msg);
        assert!(out.contains("加密"));
    }

    #[test]
    fn fix_xref_header_converts_inline_to_canonical() {
        // CNKI 常见：xref 0 3\n...
        let mut data = b"header\nxref 0 3\n0000000000 65535 f\ntrailer".to_vec();
        fix_xref_header_inline(&mut data);
        assert_eq!(
            data,
            b"header\nxref\n0 3\n0000000000 65535 f\ntrailer".to_vec()
        );
    }

    #[test]
    fn fix_xref_header_leaves_canonical_form_untouched() {
        let original = b"header\nxref\n0 3\n0000000000 65535 f\ntrailer".to_vec();
        let mut data = original.clone();
        fix_xref_header_inline(&mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn fix_xref_header_does_not_touch_xref_keyword_inside_word() {
        // "xxref 0 5" 不是关键字（前字节是 'x'），不应该被改
        let original = b"xxref 0 5\n".to_vec();
        let mut data = original.clone();
        fix_xref_header_inline(&mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn truncate_after_last_eof_strips_trailing_garbage() {
        let mut data =
            b"%PDF-1.6\nsome content\n%%EOF\nWebFastLoad<FileProperty>trash</FileProperty>"
                .to_vec();
        truncate_after_last_eof(&mut data);
        assert_eq!(data, b"%PDF-1.6\nsome content\n%%EOF\n".to_vec());
    }

    #[test]
    fn truncate_after_last_eof_keeps_crlf() {
        let mut data = b"%PDF-1.6\n%%EOF\r\nGARBAGE".to_vec();
        truncate_after_last_eof(&mut data);
        assert_eq!(data, b"%PDF-1.6\n%%EOF\r\n".to_vec());
    }

    #[test]
    fn truncate_after_last_eof_noop_when_clean() {
        let original = b"%PDF-1.6\n%%EOF\n".to_vec();
        let mut data = original.clone();
        truncate_after_last_eof(&mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn repair_pdf_bytes_fixes_cnki_style_document() {
        // 模拟 CNKI 输出：xref 和头同行 + %%EOF 后附加元数据
        let input = b"%PDF-1.6\n\
                      3 0 obj\nendobj\n\
                      xref 0 2\n0000000000 65535 f\n0000000015 00000 n\n\
                      trailer\n<<>>\nstartxref\n20\n%%EOF\n\
                      WebFastLoad<FileProperty>junk</FileProperty>"
            .to_vec();
        let out = repair_pdf_bytes(&input);
        // xref 必须被换行
        assert!(out.windows(5).any(|w| w == b"xref\n"));
        assert!(!out.windows(5).any(|w| w == b"xref "));
        // %%EOF 后不允许再出现 WebFastLoad
        let tail = String::from_utf8_lossy(&out);
        assert!(!tail.contains("WebFastLoad"));
        assert!(tail.ends_with("%%EOF\n"));
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
