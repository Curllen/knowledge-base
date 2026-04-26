import { Extension, InputRule, ReactRenderer } from "@tiptap/react";
import Suggestion from "@tiptap/suggestion";
import tippy, { type Instance as TippyInstance } from "tippy.js";
import { linkApi } from "@/lib/api";
import {
  WikiLinkSuggestionList,
  type WikiLinkSuggestionListRef,
  type WikiSuggestionItem,
} from "./WikiLinkSuggestionList";

export const WikiLinkSuggestion = Extension.create({
  name: "wikiLinkSuggestion",

  // 中文全角双方括号 `【【` 自动改写为 `[[`，复用同一条 Suggestion 触发链。
  // Why: 数据层只保留一种 wikilink 表达 `[[Title]]`，反链/搜索/导出零改动；
  //      用户输完第二个 `【` 即时替换，再继续敲文字就走 [[ 触发管线。
  addInputRules() {
    return [
      new InputRule({
        find: /【【$/,
        handler: ({ state, range }) => {
          state.tr.replaceWith(
            range.from,
            range.to,
            state.schema.text("[["),
          );
        },
      }),
    ];
  },

  addProseMirrorPlugins() {
    return [
      Suggestion<WikiSuggestionItem>({
        editor: this.editor,
        char: "[[",
        startOfLine: false,
        allowSpaces: true,
        // 关闭"前一字符必须是空格/行首"的限制；中英混排时常见的
        // `中文[[`、`word[[` 也应触发候选列表（对齐 Obsidian 行为）。
        // 默认 `allowedPrefixes = [' ']` 会把这些场景拒掉。
        allowedPrefixes: null,
        // 用户已经自己敲了 ]] 时应当退出
        allow: ({ state, range }) => {
          const before = state.doc.textBetween(
            Math.max(0, range.from - 2),
            range.to,
            "\n",
            "\0",
          );
          return !before.endsWith("]]");
        },
        items: async ({ query }: { query: string }) => {
          const keyword = query.trim();
          try {
            const results = await linkApi.searchTargets(keyword, 8);
            return results.map(([id, title]) => ({ id, title }));
          } catch {
            return [];
          }
        },
        command: ({ editor, range, props }) => {
          const pickedItem = props as WikiSuggestionItem;
          editor
            .chain()
            .focus()
            .insertContentAt(range, `[[${pickedItem.title}]] `)
            .run();
        },
        render: () => {
          let component: ReactRenderer<WikiLinkSuggestionListRef> | null = null;
          let popup: TippyInstance[] | null = null;

          return {
            onStart: (props) => {
              component = new ReactRenderer(WikiLinkSuggestionList, {
                props,
                editor: props.editor,
              });
              if (!props.clientRect) return;
              popup = tippy("body", {
                getReferenceClientRect: () =>
                  props.clientRect?.() ?? new DOMRect(0, 0, 0, 0),
                appendTo: () => document.body,
                content: component.element,
                showOnCreate: true,
                interactive: true,
                trigger: "manual",
                placement: "bottom-start",
                theme: "wiki-suggestion",
                arrow: false,
                offset: [0, 4],
              });
            },
            onUpdate: (props) => {
              component?.updateProps(props);
              if (!props.clientRect) return;
              popup?.[0].setProps({
                getReferenceClientRect: () =>
                  props.clientRect?.() ?? new DOMRect(0, 0, 0, 0),
              });
            },
            onKeyDown: (props) => {
              if (props.event.key === "Escape") {
                popup?.[0].hide();
                return true;
              }
              return component?.ref?.onKeyDown(props) ?? false;
            },
            onExit: () => {
              popup?.[0].destroy();
              component?.destroy();
              popup = null;
              component = null;
            },
          };
        },
      }),
    ];
  },
});
