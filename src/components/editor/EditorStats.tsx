/**
 * 编辑器右上角字数统计 popover
 *
 * 触发器：显示当前字数（"1234 字"），hover/click 弹 Popover
 * Popover 内容：字数、字符（含/不含空格）、段落数、阅读时长
 *
 * 实现要点：
 * - 用 editor.state 计算（textContent + 节点遍历），不依赖后端
 * - useEffect 订阅 editor.on("update")，但 throttle 到 300ms 避免每次按键算
 *   （ProseMirror state 变化频繁，不防抖会卡）
 * - 中文按字数（每个 CJK 字符 1 字），英文按空格分词
 */
import { useEffect, useState } from "react";
import { Popover, Typography } from "antd";
import type { Editor } from "@tiptap/react";

const { Text } = Typography;

interface Stats {
  words: number;
  charsWithSpace: number;
  charsWithoutSpace: number;
  paragraphs: number;
  /** 阅读时长（分钟，向上取整，最少 1） */
  readMinutes: number;
}

/** 中文：每个 CJK 字符算 1 字；英文：空白分词 */
function calcStats(editor: Editor): Stats {
  const text = editor.state.doc.textContent;
  const cjkMatches = text.match(/[\u4e00-\u9fff\u3040-\u309f\u30a0-\u30ff]/g);
  const cjkCount = cjkMatches?.length ?? 0;
  // 移除 CJK 字符再按空白分词数英文单词
  const nonCjk = text.replace(/[\u4e00-\u9fff\u3040-\u309f\u30a0-\u30ff]/g, " ");
  const englishWords = nonCjk.split(/\s+/).filter(Boolean).length;
  const words = cjkCount + englishWords;

  const charsWithSpace = [...text].length;
  const charsWithoutSpace = [...text.replace(/\s+/g, "")].length;

  let paragraphs = 0;
  editor.state.doc.descendants((node) => {
    if (node.type.name === "paragraph" && node.textContent.trim().length > 0) {
      paragraphs += 1;
    }
    return true;
  });

  // 250 字/分钟（中文阅读速度），英文按 200 词/分钟，简化用 250 统一
  const readMinutes = Math.max(1, Math.ceil(words / 250));

  return { words, charsWithSpace, charsWithoutSpace, paragraphs, readMinutes };
}

export function EditorStats({ editor }: { editor: Editor }) {
  const [stats, setStats] = useState<Stats>(() => calcStats(editor));

  useEffect(() => {
    let timer: number | null = null;
    const update = () => {
      if (timer != null) return;
      timer = window.setTimeout(() => {
        timer = null;
        setStats(calcStats(editor));
      }, 300);
    };
    editor.on("update", update);
    editor.on("create", update);
    return () => {
      editor.off("update", update);
      editor.off("create", update);
      if (timer != null) window.clearTimeout(timer);
    };
  }, [editor]);

  return (
    <Popover
      placement="bottomRight"
      mouseEnterDelay={0.3}
      content={
        <div className="space-y-1.5 text-sm" style={{ minWidth: 180 }}>
          <Row label="字数" value={`${stats.words} 字`} />
          <Row label="字符（含空格）" value={`${stats.charsWithSpace}`} />
          <Row label="字符（不含空格）" value={`${stats.charsWithoutSpace}`} />
          <Row label="段落数" value={`${stats.paragraphs}`} />
          <Row label="阅读时长" value={`约 ${stats.readMinutes} 分钟`} />
        </div>
      }
    >
      <span
        className="tiptap-toolbar-stats"
        style={{ cursor: "default", padding: "0 8px" }}
      >
        <Text type="secondary" style={{ fontSize: 12 }}>
          {stats.words} 字
        </Text>
      </span>
    </Popover>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between items-center" style={{ gap: 12 }}>
      <Text type="secondary" style={{ fontSize: 12 }}>{label}</Text>
      <Text strong style={{ fontSize: 12 }}>{value}</Text>
    </div>
  );
}
