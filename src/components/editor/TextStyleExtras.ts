/**
 * 在 @tiptap/extension-text-style 基础上补字号 / 行间距 / 段落缩进 attr。
 *
 * - 字号：作为 mark.attr.fontSize（写到 span style="font-size: 14px"）
 * - 行间距：作为 paragraph/heading 节点的 attr.lineHeight
 * - 缩进：作为 paragraph 节点的 attr.indent（数值，每级 24px padding-left）
 *
 * Markdown 序列化由 tiptap-markdown 走 prosemirror-markdown：
 * - mark 类型支持有限（粗体/斜体/code/link/strike），fontSize 等扩展属性会被忽略
 * - 这是有意 trade-off：字号/行距/缩进只在编辑器内可视化，导出 md 不保留
 *   （Notion/语雀 导出 md 也不保字号，业界一致）
 */
import { Extension } from "@tiptap/core";
import { TextStyle } from "@tiptap/extension-text-style";

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    fontSize: {
      setFontSize: (size: string) => ReturnType;
      unsetFontSize: () => ReturnType;
    };
    lineHeight: {
      setLineHeight: (lh: string) => ReturnType;
      unsetLineHeight: () => ReturnType;
    };
    indent: {
      indent: () => ReturnType;
      outdent: () => ReturnType;
    };
  }
}

/**
 * 字号 mark 扩展。
 *
 * 不另起新 mark name —— 直接继承 TextStyle 加 fontSize attr，复用 textStyle 这个
 * mark 名。这样 @tiptap/extension-color 默认配的 types: ["textStyle"] 仍然有效，
 * 一个 mark 同时承载 color + fontSize 两个 attr，schema 干净。
 */
export const FontSize = TextStyle.extend({
  addAttributes() {
    return {
      ...this.parent?.(),
      fontSize: {
        default: null,
        parseHTML: (el: HTMLElement) =>
          el.style.fontSize?.replace(/['"]+/g, "") || null,
        renderHTML: (attrs: { fontSize?: string | null }) =>
          attrs.fontSize ? { style: `font-size: ${attrs.fontSize}` } : {},
      },
    };
  },
  addCommands() {
    return {
      // 必须 spread 父 commands，否则会丢失父 TextStyle 的 removeEmptyTextStyle
      ...(this.parent?.() ?? {}),
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      setFontSize: (size: string) => ({ chain }: { chain: () => any }) =>
        chain().setMark("textStyle", { fontSize: size }).run(),
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      unsetFontSize: () => ({ chain }: { chain: () => any }) => {
        // 不依赖 removeEmptyTextStyle —— 单独清 fontSize attr 后让父扩展的
        // appendTransaction 自然清理空 mark；这样不论父 commands 是否齐全都安全。
        return chain().setMark("textStyle", { fontSize: null }).run();
      },
    };
  },
});

/** 行间距：节点级 attr，作用于 paragraph / heading */
export const LineHeight = Extension.create({
  name: "lineHeight",
  addOptions() {
    return {
      types: ["paragraph", "heading"] as string[],
      defaultLineHeight: null as string | null,
    };
  },
  addGlobalAttributes() {
    return [
      {
        types: this.options.types,
        attributes: {
          lineHeight: {
            default: this.options.defaultLineHeight,
            parseHTML: (el) => (el as HTMLElement).style.lineHeight || null,
            renderHTML: (attrs) =>
              attrs.lineHeight ? { style: `line-height: ${attrs.lineHeight}` } : {},
          },
        },
      },
    ];
  },
  addCommands() {
    return {
      setLineHeight:
        (lh: string) =>
        ({ commands }) => {
          // every 会要求所有 types 都返回 true，但当前 selection 只在某一种类型上
          // → 必有一种返回 false → 命令整体失败。改用 some：任一成功即视为成功。
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          return (this.options.types as string[]).some((t) =>
            (commands as any).updateAttributes(t, { lineHeight: lh }),
          );
        },
      unsetLineHeight:
        () =>
        ({ commands }) => {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          return (this.options.types as string[]).some((t) =>
            (commands as any).updateAttributes(t, { lineHeight: null }),
          );
        },
    };
  },
});

/** 段落缩进：indent 数值（0/1/2...），渲染为 padding-left * 24px */
export const Indent = Extension.create({
  name: "indent",
  addOptions() {
    return {
      types: ["paragraph", "heading"] as string[],
      maxLevel: 8,
    };
  },
  addGlobalAttributes() {
    return [
      {
        types: this.options.types,
        attributes: {
          indent: {
            default: 0,
            parseHTML: (el) => {
              const raw = (el as HTMLElement).getAttribute("data-indent");
              const n = raw ? parseInt(raw, 10) : 0;
              return Number.isFinite(n) ? n : 0;
            },
            renderHTML: (attrs) => {
              const n = Number(attrs.indent || 0);
              if (!n) return {};
              return {
                "data-indent": String(n),
                style: `padding-left: ${n * 1.5}em`,
              };
            },
          },
        },
      },
    ];
  },
  addCommands() {
    const max = this.options.maxLevel;
    const types = this.options.types as string[];
    return {
      indent:
        () =>
        ({ state, commands }) => {
          const { $from } = state.selection;
          for (let d = $from.depth; d > 0; d--) {
            const node = $from.node(d);
            if (types.includes(node.type.name)) {
              const cur = Number(node.attrs.indent || 0);
              if (cur >= max) return false;
              // eslint-disable-next-line @typescript-eslint/no-explicit-any
              return (commands as any).updateAttributes(node.type.name, {
                indent: cur + 1,
              });
            }
          }
          return false;
        },
      outdent:
        () =>
        ({ state, commands }) => {
          const { $from } = state.selection;
          for (let d = $from.depth; d > 0; d--) {
            const node = $from.node(d);
            if (types.includes(node.type.name)) {
              const cur = Number(node.attrs.indent || 0);
              if (cur <= 0) return false;
              // eslint-disable-next-line @typescript-eslint/no-explicit-any
              return (commands as any).updateAttributes(node.type.name, {
                indent: cur - 1,
              });
            }
          }
          return false;
        },
    };
  },
});
