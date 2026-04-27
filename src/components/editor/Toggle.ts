/**
 * Toggle 折叠块（Notion 风 ▶ 标题 + 可折叠内容）
 *
 * 结构：toggle node 包含两个子节点
 *   - toggleSummary: 标题行（inline content，类似段落）
 *   - toggleContent: 折叠内容（block+，可包多段落/列表/嵌套 callout 等）
 *
 * 折叠状态用 attrs.open 管理；NodeView 控制 ▶ 图标和内容显隐。
 *
 * markdown 兼容：渲染为标准 HTML `<details><summary>...</summary>...</details>`
 * （tiptap-markdown html:true 透传；GFM/Obsidian 都识别原生 details 标签）。
 */
import { Node, mergeAttributes } from "@tiptap/core";
import { ReactNodeViewRenderer } from "@tiptap/react";
import { ToggleNodeView } from "./ToggleNodeView";

export interface ToggleOptions {
  HTMLAttributes: Record<string, unknown>;
}

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    toggleBlock: {
      setToggle: () => ReturnType;
    };
  }
}

export const ToggleSummary = Node.create({
  name: "toggleSummary",
  content: "inline*",
  defining: true,

  parseHTML() {
    return [{ tag: "summary" }];
  },
  renderHTML({ HTMLAttributes }) {
    return ["summary", mergeAttributes(HTMLAttributes), 0];
  },
});

export const ToggleContent = Node.create({
  name: "toggleContent",
  content: "block+",
  defining: true,

  parseHTML() {
    return [
      {
        tag: "div[data-toggle-content]",
      },
    ];
  },
  renderHTML({ HTMLAttributes }) {
    return [
      "div",
      mergeAttributes(HTMLAttributes, { "data-toggle-content": "true" }),
      0,
    ];
  },
});

export const Toggle = Node.create<ToggleOptions>({
  name: "toggle",
  group: "block",
  content: "toggleSummary toggleContent",
  defining: true,

  addOptions() {
    return {
      HTMLAttributes: { class: "tiptap-toggle" },
    };
  },

  addAttributes() {
    return {
      open: {
        default: true,
        parseHTML: (el) => (el as HTMLElement).hasAttribute("open"),
        renderHTML: (attrs) => (attrs.open ? { open: "true" } : {}),
      },
    };
  },

  parseHTML() {
    return [{ tag: "details" }];
  },

  renderHTML({ node, HTMLAttributes }) {
    return [
      "details",
      mergeAttributes(this.options.HTMLAttributes, HTMLAttributes, {
        "data-open": node.attrs.open ? "true" : "false",
      }),
      0,
    ];
  },

  addNodeView() {
    return ReactNodeViewRenderer(ToggleNodeView);
  },

  addCommands() {
    return {
      setToggle:
        () =>
        ({ commands }) =>
          commands.insertContent({
            type: "toggle",
            attrs: { open: true },
            content: [
              {
                type: "toggleSummary",
                content: [{ type: "text", text: "折叠标题" }],
              },
              {
                type: "toggleContent",
                content: [{ type: "paragraph" }],
              },
            ],
          }),
    };
  },
});
