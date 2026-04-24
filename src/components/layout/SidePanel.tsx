import { useAppStore } from "@/store";
import type { ActiveView } from "@/store";
import { NotesPanel } from "./panels/NotesPanel";
import { TagsPanel } from "./panels/TagsPanel";
import { DailyPanel } from "./panels/DailyPanel";
import { SearchPanel } from "./panels/SearchPanel";
import { TasksPanel } from "./panels/TasksPanel";

/**
 * SidePanel —— Activity Bar 模式下 ActivityBar 右侧的主面板。
 *
 * 根据 activeView 分发到具体 panel 子组件。
 * 拥有面板的视图见 VIEWS_WITH_PANEL；其他视图（home/graph/ai/prompts/
 * about/trash）点图标直接展开主区,AppLayout 会把 SidePanel 宽度置 0。
 */

/** 哪些视图拥有独立 SidePanel 内容 */
const VIEWS_WITH_PANEL = new Set<ActiveView>([
  "notes",
  "tags",
  "tasks",
  "search",
  "daily",
]);

export function viewHasPanel(view: ActiveView): boolean {
  return VIEWS_WITH_PANEL.has(view);
}

export function SidePanel() {
  const activeView = useAppStore((s) => s.activeView);

  switch (activeView) {
    case "notes":
      return <NotesPanel />;
    case "tags":
      return <TagsPanel />;
    case "daily":
      return <DailyPanel />;
    case "search":
      return <SearchPanel />;
    case "tasks":
      return <TasksPanel />;
    default:
      // 无面板视图（home/daily/graph/ai/prompts/about/trash）
      // AppLayout 会基于 viewHasPanel() 把 SidePanel 宽度置 0
      return null;
  }
}
