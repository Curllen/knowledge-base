import { useState, useCallback } from "react";
import type { Editor } from "@tiptap/react";
import { Button, Divider, Tooltip, Modal, Input, message, Dropdown, Select } from "antd";
import type { MenuProps } from "antd";
import {
  Bold,
  Italic,
  Underline,
  Strikethrough,
  Highlighter,
  Code,
  Heading1,
  Heading2,
  Heading3,
  List,
  ListOrdered,
  ListTodo,
  Quote,
  CodeSquare,
  Minus,
  Undo2,
  Redo2,
  Link as LinkIcon,
  Unlink,
  ImagePlus,
  Film,
  Paperclip,
  MapPin,
  Table as TableIcon,
  AlignLeft,
  AlignCenter,
  AlignRight,
  AlignJustify,
  Rows3,
  Columns3,
  Trash2,
  ChevronDown,
} from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { convertFileSrc } from "@tauri-apps/api/core";
import { attachmentApi, imageApi, videoApi } from "@/lib/api";
import { insertVideoTimestamp } from "./VideoTimestamp";

interface ToolbarProps {
  editor: Editor;
  noteId?: number;
  /** 与 TiptapEditor 的同名 prop 含义一致：noteId 缺失时用它按需建档 */
  ensureNoteId?: () => Promise<number>;
}

interface ToolItem {
  icon: React.ReactNode;
  title: string;
  /** 普通按钮的点击；带 dropdownItems 时由下拉菜单各 item 自己 onClick，可省略 */
  action?: () => void;
  isActive?: () => boolean;
  /** T-017: 提供后按钮渲染为 Dropdown trigger，菜单展示 dropdownItems */
  dropdownItems?: MenuProps["items"];
}

export function EditorToolbar({ editor, noteId, ensureNoteId }: ToolbarProps) {
  const [linkModalOpen, setLinkModalOpen] = useState(false);
  const [linkUrl, setLinkUrl] = useState("");
  /** 时间戳弹窗 state */
  const [tsModalOpen, setTsModalOpen] = useState(false);
  const [tsVideoId, setTsVideoId] = useState<string>("");
  const [tsTimeText, setTsTimeText] = useState<string>("00:00");
  async function insertImage() {
    // 与 TiptapEditor.handleImageFiles 行为对齐：优先显式 noteId，
    // 缺失时尝试 ensureNoteId（日记按需建档），仍拿不到才 warning
    let effectiveNoteId = noteId;
    if (!effectiveNoteId && ensureNoteId) {
      try {
        effectiveNoteId = await ensureNoteId();
      } catch (e) {
        message.error(`图片插入失败: ${e}`);
        return;
      }
    }
    if (!effectiveNoteId) {
      message.warning("请先保存笔记后再插入图片");
      return;
    }
    const selected = await open({
      multiple: true,
      filters: [
        {
          name: "图片",
          extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"],
        },
      ],
    });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    for (const filePath of paths) {
      try {
        const savedPath = await imageApi.saveFromPath(effectiveNoteId, filePath);
        const assetUrl = convertFileSrc(savedPath);
        editor.chain().focus().insertContent({
          type: "imageResize",
          attrs: { src: assetUrl },
        }).run();
      } catch (e) {
        message.error(`图片插入失败: ${e}`);
      }
    }
  }

  /** 与 insertImage 对称：从文件选择器导入视频走 saveFromPath（零拷贝），
   *  插入 video node。复用 TiptapEditor 已有的 VideoNode 渲染。
   *  视频文件大（GB 级），用 saveFromPath 而非 base64 上传，避免主进程内存爆。 */
  async function insertVideo() {
    let effectiveNoteId = noteId;
    if (!effectiveNoteId && ensureNoteId) {
      try {
        effectiveNoteId = await ensureNoteId();
      } catch (e) {
        message.error(`视频插入失败: ${e}`);
        return;
      }
    }
    if (!effectiveNoteId) {
      message.warning("请先保存笔记后再插入视频");
      return;
    }
    const selected = await open({
      multiple: true,
      filters: [
        {
          name: "视频",
          extensions: ["mp4", "mov", "webm", "m4v", "ogv", "mkv", "avi"],
        },
      ],
    });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    for (const filePath of paths) {
      try {
        const savedPath = await videoApi.saveFromPath(effectiveNoteId, filePath);
        const assetUrl = convertFileSrc(savedPath);
        editor.chain().focus().insertContent({
          type: "video",
          attrs: { src: assetUrl, id: Math.random().toString(36).slice(2, 10) },
        }).run();
      } catch (e) {
        message.error(`视频插入失败: ${e}`);
      }
    }
  }

  /** 与 insertVideo 对称：从文件选择器选附件 → saveFromPath 零拷贝 →
   *  插入 `📎 文件名 (大小)` Link 节点（与 TiptapEditor 拖入逻辑同款渲染，
   *  保持 markdown 序列化零改造）。
   *  PDF/Office/ZIP/音视频/通用文件都走这里；exe/bat 等被后端黑名单拦掉。 */
  async function insertAttachment() {
    let effectiveNoteId = noteId;
    if (!effectiveNoteId && ensureNoteId) {
      try {
        effectiveNoteId = await ensureNoteId();
      } catch (e) {
        message.error(`附件插入失败: ${e}`);
        return;
      }
    }
    if (!effectiveNoteId) {
      message.warning("请先保存笔记后再插入附件");
      return;
    }
    const selected = await open({
      multiple: true,
      filters: [
        {
          name: "附件",
          // 与后端 mime_for_ext 列表对齐；不含 exe/bat（后端黑名单）
          extensions: [
            "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
            "zip", "rar", "7z", "tar", "gz",
            "mp3", "wav", "ogg", "flac", "m4a",
            "csv", "json", "xml", "yaml", "yml", "txt", "md",
          ],
        },
        { name: "所有文件", extensions: ["*"] },
      ],
    });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];

    const nodes: Array<
      | { type: "text"; text: string; marks: Array<{ type: "link"; attrs: { href: string } }> }
      | { type: "text"; text: string }
    > = [];
    for (const filePath of paths) {
      try {
        const info = await attachmentApi.saveFromPath(effectiveNoteId, filePath);
        const label = `📎 ${info.fileName} (${formatSize(info.size)})`;
        const href = pathToFileUrl(info.path);
        nodes.push({ type: "text", text: label, marks: [{ type: "link", attrs: { href } }] });
        nodes.push({ type: "text", text: "\n" });
      } catch (e) {
        message.error(`附件插入失败: ${e}`);
      }
    }
    if (nodes.length > 0) {
      editor.chain().focus().insertContent(nodes).run();
    }
  }

  /** 收集当前文档所有 video 节点（含 id + 显示名 + src 文件名），给时间戳弹窗下拉用 */
  function collectVideosInDoc(): Array<{ id: string; label: string; src: string }> {
    const list: Array<{ id: string; label: string; src: string }> = [];
    let autoIdx = 0;
    editor.state.doc.descendants((n) => {
      if (n.type.name !== "video") return true;
      autoIdx += 1;
      const id = String(n.attrs.id ?? "");
      const userLabel = String(n.attrs.label ?? "");
      const src = String(n.attrs.src ?? "");
      const label = userLabel || `视频 ${autoIdx}`;
      list.push({ id, label, src });
      return true;
    });
    return list;
  }

  /** 打开"插入时间戳"弹窗：自动选中第一个视频 */
  function openTimestampModal() {
    const videos = collectVideosInDoc();
    if (videos.length === 0) {
      message.warning("当前笔记还没有视频，请先插入视频");
      return;
    }
    const valid = videos.filter((v) => v.id);
    if (valid.length === 0) {
      message.warning("视频缺少 ID。请重新打开此笔记触发自动补 ID 后再试");
      return;
    }
    setTsVideoId(valid[0].id);
    setTsTimeText("00:00");
    setTsModalOpen(true);
  }

  /** 弹窗确认：解析 mm:ss / hh:mm:ss → 秒数 → insertVideoTimestamp */
  function handleTimestampConfirm() {
    const seconds = parseTimeToSeconds(tsTimeText);
    if (seconds == null) {
      message.error("时间格式不对，请用 mm:ss 或 hh:mm:ss（如 01:40）");
      return;
    }
    const videos = collectVideosInDoc();
    const target = videos.find((v) => v.id === tsVideoId);
    if (!target) {
      message.error("未找到选中的视频");
      return;
    }
    insertVideoTimestamp(editor, {
      videoId: tsVideoId,
      seconds,
      label: `📹 ${target.label} · ${formatTimeShort(seconds)}`,
    });
    setTsModalOpen(false);
  }

  const openLinkModal = useCallback(() => {
    const previousUrl = editor.getAttributes("link").href || "";
    setLinkUrl(previousUrl);
    setLinkModalOpen(true);
  }, [editor]);

  const handleLinkConfirm = useCallback(() => {
    const url = linkUrl.trim();
    if (!url) {
      editor.chain().focus().extendMarkRange("link").unsetLink().run();
    } else {
      editor.chain().focus().extendMarkRange("link").setLink({ href: url }).run();
    }
    setLinkModalOpen(false);
    setLinkUrl("");
  }, [editor, linkUrl]);

  const groups: ToolItem[][] = [
    // 撤销/重做
    [
      {
        icon: <Undo2 size={15} />,
        title: "撤销",
        action: () => editor.chain().focus().undo().run(),
      },
      {
        icon: <Redo2 size={15} />,
        title: "重做",
        action: () => editor.chain().focus().redo().run(),
      },
    ],
    // 标题
    [
      {
        icon: <Heading1 size={15} />,
        title: "标题 1",
        action: () => editor.chain().focus().toggleHeading({ level: 1 }).run(),
        isActive: () => editor.isActive("heading", { level: 1 }),
      },
      {
        icon: <Heading2 size={15} />,
        title: "标题 2",
        action: () => editor.chain().focus().toggleHeading({ level: 2 }).run(),
        isActive: () => editor.isActive("heading", { level: 2 }),
      },
      {
        icon: <Heading3 size={15} />,
        title: "标题 3",
        action: () => editor.chain().focus().toggleHeading({ level: 3 }).run(),
        isActive: () => editor.isActive("heading", { level: 3 }),
      },
    ],
    // 文本格式
    [
      {
        icon: <Bold size={15} />,
        title: "粗体",
        action: () => editor.chain().focus().toggleBold().run(),
        isActive: () => editor.isActive("bold"),
      },
      {
        icon: <Italic size={15} />,
        title: "斜体",
        action: () => editor.chain().focus().toggleItalic().run(),
        isActive: () => editor.isActive("italic"),
      },
      {
        icon: <Underline size={15} />,
        title: "下划线",
        action: () => editor.chain().focus().toggleUnderline().run(),
        isActive: () => editor.isActive("underline"),
      },
      {
        icon: <Strikethrough size={15} />,
        title: "删除线",
        action: () => editor.chain().focus().toggleStrike().run(),
        isActive: () => editor.isActive("strike"),
      },
      {
        icon: <Highlighter size={15} />,
        title: "高亮",
        action: () => editor.chain().focus().toggleHighlight().run(),
        isActive: () => editor.isActive("highlight"),
      },
      {
        icon: <Code size={15} />,
        title: "行内代码",
        action: () => editor.chain().focus().toggleCode().run(),
        isActive: () => editor.isActive("code"),
      },
    ],
    // 列表 & 引用
    [
      {
        icon: <List size={15} />,
        title: "无序列表",
        action: () => editor.chain().focus().toggleBulletList().run(),
        isActive: () => editor.isActive("bulletList"),
      },
      {
        icon: <ListOrdered size={15} />,
        title: "有序列表",
        action: () => editor.chain().focus().toggleOrderedList().run(),
        isActive: () => editor.isActive("orderedList"),
      },
      {
        icon: <ListTodo size={15} />,
        title: "任务列表",
        action: () => editor.chain().focus().toggleTaskList().run(),
        isActive: () => editor.isActive("taskList"),
      },
      {
        icon: <Quote size={15} />,
        title: "引用",
        action: () => editor.chain().focus().toggleBlockquote().run(),
        isActive: () => editor.isActive("blockquote"),
      },
      {
        icon: (
          <span className="inline-flex items-center gap-0.5">
            <CodeSquare size={15} />
            <ChevronDown size={11} style={{ opacity: 0.6 }} />
          </span>
        ),
        title: "代码块",
        isActive: () => editor.isActive("codeBlock"),
        dropdownItems: [
          {
            key: "code-plain",
            icon: <CodeSquare size={14} />,
            label: "普通代码块",
            onClick: () => editor.chain().focus().toggleCodeBlock().run(),
          },
          {
            key: "code-mermaid",
            icon: <CodeSquare size={14} />,
            label: "Mermaid 流程图",
            onClick: () =>
              editor
                .chain()
                .focus()
                .insertContent({
                  type: "codeBlock",
                  attrs: { language: "mermaid" },
                  content: [
                    {
                      type: "text",
                      text: "flowchart TD\n  A[开始] --> B{判断}\n  B -- 是 --> C[执行]\n  B -- 否 --> D[结束]",
                    },
                  ],
                })
                .run(),
          },
        ],
      },
    ],
    // 对齐
    [
      {
        icon: <AlignLeft size={15} />,
        title: "左对齐",
        action: () => editor.chain().focus().setTextAlign("left").run(),
        isActive: () => editor.isActive({ textAlign: "left" }),
      },
      {
        icon: <AlignCenter size={15} />,
        title: "居中",
        action: () => editor.chain().focus().setTextAlign("center").run(),
        isActive: () => editor.isActive({ textAlign: "center" }),
      },
      {
        icon: <AlignRight size={15} />,
        title: "右对齐",
        action: () => editor.chain().focus().setTextAlign("right").run(),
        isActive: () => editor.isActive({ textAlign: "right" }),
      },
      {
        icon: <AlignJustify size={15} />,
        title: "两端对齐",
        action: () => editor.chain().focus().setTextAlign("justify").run(),
        isActive: () => editor.isActive({ textAlign: "justify" }),
      },
    ],
    // 表格 — T-017 全部命令折叠到 Dropdown 菜单，避免工具栏过挤
    [
      {
        icon: (
          <span className="inline-flex items-center gap-0.5">
            <TableIcon size={15} />
            <ChevronDown size={11} style={{ opacity: 0.6 }} />
          </span>
        ),
        title: "表格",
        isActive: () => editor.isActive("table"),
        dropdownItems: [
          {
            key: "insert",
            icon: <TableIcon size={14} />,
            label: "插入 3×3 表格",
            onClick: () =>
              editor
                .chain()
                .focus()
                .insertTable({ rows: 3, cols: 3, withHeaderRow: true })
                .run(),
          },
          { type: "divider" },
          {
            key: "add-col",
            icon: <Columns3 size={14} />,
            label: "在右侧加列",
            disabled: !editor.can().addColumnAfter(),
            onClick: () => editor.chain().focus().addColumnAfter().run(),
          },
          {
            key: "add-row",
            icon: <Rows3 size={14} />,
            label: "在下方加行",
            disabled: !editor.can().addRowAfter(),
            onClick: () => editor.chain().focus().addRowAfter().run(),
          },
          { type: "divider" },
          {
            key: "merge-cells",
            label: "合并单元格",
            disabled: !editor.can().mergeCells(),
            onClick: () => editor.chain().focus().mergeCells().run(),
          },
          {
            key: "split-cell",
            label: "拆分单元格",
            disabled: !editor.can().splitCell(),
            onClick: () => editor.chain().focus().splitCell().run(),
          },
          { type: "divider" },
          {
            key: "delete-row",
            label: "删除当前行",
            disabled: !editor.can().deleteRow(),
            onClick: () => editor.chain().focus().deleteRow().run(),
          },
          {
            key: "delete-col",
            label: "删除当前列",
            disabled: !editor.can().deleteColumn(),
            onClick: () => editor.chain().focus().deleteColumn().run(),
          },
          { type: "divider" },
          {
            key: "toggle-header-row",
            label: "切换首行表头",
            disabled: !editor.can().toggleHeaderRow(),
            onClick: () => editor.chain().focus().toggleHeaderRow().run(),
          },
          {
            key: "toggle-header-col",
            label: "切换首列表头",
            disabled: !editor.can().toggleHeaderColumn(),
            onClick: () => editor.chain().focus().toggleHeaderColumn().run(),
          },
          { type: "divider" },
          {
            key: "delete-table",
            icon: <Trash2 size={14} />,
            label: "删除整个表格",
            danger: true,
            disabled: !editor.can().deleteTable(),
            onClick: () => editor.chain().focus().deleteTable().run(),
          },
        ],
      },
    ],
    // 链接 & 媒体
    [
      {
        icon: <LinkIcon size={15} />,
        title: "插入链接",
        action: openLinkModal,
        isActive: () => editor.isActive("link"),
      },
      {
        icon: <Unlink size={15} />,
        title: "移除链接",
        action: () => editor.chain().focus().unsetLink().run(),
      },
      {
        icon: <ImagePlus size={15} />,
        title: "插入图片",
        action: insertImage,
      },
      {
        icon: <Film size={15} />,
        title: "插入视频",
        action: insertVideo,
      },
      {
        icon: <MapPin size={15} />,
        title: "插入视频时间戳",
        action: openTimestampModal,
      },
      {
        icon: <Paperclip size={15} />,
        title: "插入附件（PDF/Office/ZIP 等）",
        action: insertAttachment,
      },
      {
        icon: <Minus size={15} />,
        title: "分割线",
        action: () => editor.chain().focus().setHorizontalRule().run(),
      },
    ],
  ];

  return (
    <>
      <div className="tiptap-toolbar">
        {groups.map((group, gi) => (
          <span key={gi} className="inline-flex items-center">
            {gi > 0 && (
              <Divider type="vertical" style={{ height: 20, margin: "0 2px" }} />
            )}
            {group.map((item, ii) => {
              const btn = (
                <Button
                  type="text"
                  size="small"
                  icon={item.icon}
                  onClick={item.dropdownItems ? undefined : item.action}
                  className={item.isActive?.() ? "toolbar-btn-active" : ""}
                  style={{
                    minWidth: 28,
                    height: 28,
                    padding: item.dropdownItems ? "0 4px" : 0,
                  }}
                />
              );
              if (item.dropdownItems) {
                return (
                  <Tooltip
                    key={ii}
                    title={item.title}
                    mouseEnterDelay={0.5}
                  >
                    <Dropdown
                      menu={{ items: item.dropdownItems }}
                      trigger={["click"]}
                      placement="bottomLeft"
                    >
                      {btn}
                    </Dropdown>
                  </Tooltip>
                );
              }
              return (
                <Tooltip key={ii} title={item.title} mouseEnterDelay={0.5}>
                  {btn}
                </Tooltip>
              );
            })}
          </span>
        ))}
      </div>

      <Modal
        title="插入链接"
        open={linkModalOpen}
        onOk={handleLinkConfirm}
        onCancel={() => { setLinkModalOpen(false); setLinkUrl(""); }}
        okText="确定"
        cancelText="取消"
        width={420}
        destroyOnClose
      >
        <Input
          placeholder="请输入链接地址，如 https://example.com"
          value={linkUrl}
          onChange={(e) => setLinkUrl(e.target.value)}
          onPressEnter={handleLinkConfirm}
          autoFocus
        />
        <div className="mt-2 text-xs" style={{ color: "var(--ant-color-text-quaternary)" }}>
          留空并确定将移除当前链接
        </div>
      </Modal>

      {/* 插入视频时间戳弹窗 */}
      <Modal
        title="插入视频时间戳"
        open={tsModalOpen}
        onOk={handleTimestampConfirm}
        onCancel={() => setTsModalOpen(false)}
        okText="插入"
        cancelText="取消"
        width={460}
        destroyOnClose
      >
        <div className="space-y-3">
          <div>
            <div className="mb-1 text-xs" style={{ color: "var(--ant-color-text-secondary)" }}>
              选择视频
            </div>
            <Select
              style={{ width: "100%" }}
              value={tsVideoId}
              onChange={(v) => setTsVideoId(v)}
              options={collectVideosInDoc()
                .filter((v) => v.id)
                .map((v) => ({
                  value: v.id,
                  label: `${v.label} · ${shortFileName(v.src)}`,
                }))}
            />
          </div>
          <div>
            <div className="mb-1 text-xs" style={{ color: "var(--ant-color-text-secondary)" }}>
              时间（mm:ss 或 hh:mm:ss）
            </div>
            <Input
              value={tsTimeText}
              onChange={(e) => setTsTimeText(e.target.value)}
              onPressEnter={handleTimestampConfirm}
              placeholder="如 01:40 或 1:23:45"
              autoFocus
            />
            <div className="mt-1 text-xs" style={{ color: "var(--ant-color-text-quaternary)" }}>
              提示：在视频块顶部点「📍 加时间戳」可一键采用当前播放位置
            </div>
          </div>
        </div>
      </Modal>
    </>
  );
}

/** 解析 mm:ss 或 hh:mm:ss 文本为秒数；非法返回 null */
function parseTimeToSeconds(text: string): number | null {
  const trimmed = text.trim();
  if (!trimmed) return null;
  const parts = trimmed.split(":").map((p) => p.trim());
  if (parts.some((p) => !/^\d+$/.test(p))) return null;
  if (parts.length === 1) {
    return parseInt(parts[0], 10);
  }
  if (parts.length === 2) {
    const [m, s] = parts.map((p) => parseInt(p, 10));
    if (s >= 60) return null;
    return m * 60 + s;
  }
  if (parts.length === 3) {
    const [h, m, s] = parts.map((p) => parseInt(p, 10));
    if (m >= 60 || s >= 60) return null;
    return h * 3600 + m * 60 + s;
  }
  return null;
}

/** 秒数 → 短格式（mm:ss 或 h:mm:ss） */
function formatTimeShort(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = (n: number) => (n < 10 ? `0${n}` : String(n));
  if (h > 0) return `${h}:${pad(m)}:${pad(sec)}`;
  return `${pad(m)}:${pad(sec)}`;
}

/** 取 src 路径里的文件名（视频路径太长时下拉里更可读） */
function shortFileName(src: string): string {
  if (!src) return "(未命名)";
  try {
    const decoded = decodeURIComponent(src);
    const last = decoded.split(/[\\/]/).pop() || decoded;
    return last.length > 40 ? last.slice(0, 37) + "..." : last;
  } catch {
    return src.slice(-40);
  }
}

/** 字节数 → 人类可读（与 TiptapEditor.humanSize 同实现，避免跨文件 import） */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(1)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb.toFixed(1)} MB`;
  return `${(mb / 1024).toFixed(1)} GB`;
}

/** 绝对路径 → file:// URL（与 TiptapEditor.pathToFileUrl 同实现） */
function pathToFileUrl(absPath: string): string {
  const normalized = absPath.replace(/\\/g, "/");
  const encoded = normalized.split("/").map((seg) => encodeURIComponent(seg)).join("/");
  return normalized.startsWith("/") ? `file://${encoded}` : `file:///${encoded}`;
}
