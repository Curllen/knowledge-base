/**
 * 笔记创建 / 导入统一入口 —— 从旧 CreateNoteModal 拆出来的公共函数，
 * 让"+ 新建笔记"按钮能直接调用，不必再走 Tab 选择的 Modal 流程。
 */
import { List, Modal, Typography, message } from "antd";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { NavigateFunction } from "react-router-dom";

import { noteApi, importApi, pdfApi, sourceFileApi } from "./api";
import { importWordFiles } from "./wordImport";
import { useAppStore } from "@/store";

/** 未命名笔记标题，带时间戳避免同名堆叠时难区分 */
function untitledTitle(): string {
  const d = new Date();
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `未命名笔记 ${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(
    d.getDate(),
  )} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** 创建一篇空白笔记并跳转到编辑器。用户想写就写，不想保留可直接删除。 */
export async function createBlankAndOpen(
  folderId: number | null,
  navigate: NavigateFunction,
): Promise<void> {
  try {
    const note = await noteApi.create({
      title: untitledTitle(),
      content: "",
      folder_id: folderId,
    });
    useAppStore.getState().bumpNotesRefresh();
    navigate(`/notes/${note.id}`);
  } catch (e) {
    message.error(String(e));
  }
}

/** Markdown 导入流程：弹对话框 → 后端批量导入 */
export async function importMarkdownFlow(folderId: number | null): Promise<void> {
  const picked = await openDialog({
    multiple: true,
    filters: [{ name: "Markdown", extensions: ["md", "markdown"] }],
  });
  if (!picked) return;
  const paths = Array.isArray(picked) ? picked : [picked];
  if (paths.length === 0) return;
  const hide = message.loading(`正在导入 ${paths.length} 个 Markdown 文件...`, 0);
  try {
    const result = await importApi.importSelected(paths, folderId);
    hide();
    if (result.imported > 0) {
      let msg = `成功导入 ${result.imported} 篇`;
      if (result.skipped > 0) msg += `，跳过 ${result.skipped} 篇`;
      if (result.tags_attached && result.tags_attached > 0) {
        msg += `；自动关联 ${result.tags_attached} 条 frontmatter 标签`;
      }
      if (result.attachments_copied && result.attachments_copied > 0) {
        msg += `；复制 ${result.attachments_copied} 张图`;
      }
      const missCount = result.attachments_missing?.length ?? 0;
      if (missCount > 0) {
        msg += `（${missCount} 张图缺失，详见日志）`;
      }
      message.success(msg);
    } else if (result.skipped > 0) {
      message.warning(`全部 ${result.skipped} 篇已跳过`);
    }
    if (result.errors.length > 0) {
      Modal.warning({
        title: `${result.errors.length} 个文件导入失败`,
        content: (
          <List
            size="small"
            dataSource={result.errors}
            renderItem={(err) => (
              <List.Item>
                <Typography.Text type="danger" style={{ fontSize: 12 }}>
                  {err}
                </Typography.Text>
              </List.Item>
            )}
          />
        ),
      });
    }
    useAppStore.getState().bumpNotesRefresh();
    useAppStore.getState().bumpFoldersRefresh();
  } catch (e) {
    hide();
    message.error(`导入失败: ${e}`);
  }
}

/** PDF 导入流程 */
export async function importPdfsFlow(folderId: number | null): Promise<void> {
  const picked = await openDialog({
    multiple: true,
    filters: [{ name: "PDF", extensions: ["pdf"] }],
  });
  if (!picked) return;
  const paths = Array.isArray(picked) ? picked : [picked];
  if (paths.length === 0) return;
  const hide = message.loading(`正在导入 ${paths.length} 个 PDF...`, 0);
  try {
    const results = await pdfApi.importPdfs(paths, folderId);
    const ok = results.filter((r) => r.noteId !== null);
    const fail = results.filter((r) => r.noteId === null);
    hide();
    if (ok.length > 0) message.success(`成功导入 ${ok.length} 个 PDF`);
    if (fail.length > 0) {
      Modal.warning({
        title: `${fail.length} 个 PDF 导入失败`,
        content: (
          <List
            size="small"
            dataSource={fail}
            renderItem={(r) => (
              <List.Item>
                <Typography.Text type="danger" style={{ fontSize: 12 }}>
                  {r.sourcePath.split(/[\\/]/).pop()}: {r.error}
                </Typography.Text>
              </List.Item>
            )}
          />
        ),
      });
    }
    useAppStore.getState().bumpNotesRefresh();
  } catch (e) {
    hide();
    message.error(`导入失败: ${e}`);
  }
}

/** Word 导入流程：.doc 需本机装 LibreOffice / Office / WPS */
export async function importWordFlow(folderId: number | null): Promise<void> {
  const converter = await sourceFileApi
    .getConverterStatus()
    .catch(() => "none" as const);
  const exts = converter === "none" ? ["docx"] : ["docx", "doc"];
  const picked = await openDialog({
    multiple: true,
    filters: [{ name: "Word", extensions: exts }],
  });
  if (!picked) return;
  const paths = Array.isArray(picked) ? picked : [picked];
  if (paths.length === 0) return;
  if (
    converter === "none" &&
    paths.some((p) => p.toLowerCase().endsWith(".doc"))
  ) {
    Modal.warning({
      title: ".doc 暂不可用",
      content: "未检测到 LibreOffice 或 Microsoft Office / WPS。安装后可导入 .doc。",
    });
    return;
  }
  const hide = message.loading(`正在导入 ${paths.length} 个 Word 文件...`, 0);
  try {
    const results = await importWordFiles(paths, folderId);
    const ok = results.filter((r) => r.noteId !== null);
    const fail = results.filter((r) => r.noteId === null);
    hide();
    if (ok.length > 0) message.success(`成功导入 ${ok.length} 个 Word 文件`);
    if (fail.length > 0) {
      Modal.warning({
        title: `${fail.length} 个 Word 文件导入失败`,
        content: (
          <List
            size="small"
            dataSource={fail}
            renderItem={(r) => (
              <List.Item>
                <Typography.Text type="danger" style={{ fontSize: 12 }}>
                  {r.sourcePath.split(/[\\/]/).pop()}: {r.error}
                </Typography.Text>
              </List.Item>
            )}
          />
        ),
      });
    }
    useAppStore.getState().bumpNotesRefresh();
  } catch (e) {
    hide();
    message.error(`导入失败: ${e}`);
  }
}
