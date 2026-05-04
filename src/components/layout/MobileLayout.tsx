import { useEffect } from "react";
import { Outlet, useNavigate, useLocation } from "react-router-dom";
import {
  Home,
  FileText,
  Sparkles,
  CheckSquare,
  User,
  Plus,
} from "lucide-react";

/**
 * 移动端主布局。设计稿位于 output/UI原型/2026-05-04_知识库移动端App/。
 *
 * - 顶部：让出 Android 状态栏（safe-area-inset-top），由系统控制状态栏样式
 * - 中间：全屏 Outlet 容器，可滚动；overflow-y-auto 让长页面（笔记列表）能滑
 * - 底部：5 格 Tab（主页 / 笔记 / AI / 待办 / 我的）+ 浮动 FAB（右下角）
 *   - Tab 高亮规则：当前路由匹配 Tab 路径前缀（笔记列表/编辑都高亮"笔记"）
 *   - FAB 全局指向 /quick-create（暂未实现 → 跳到 /notes 让用户用右上 + 新建）
 *
 * 与桌面 AppLayout 完全隔离：移动端不渲染 ActivityBar / SidePanel / WindowControls，
 * 因为这些是桌面专属（多窗口 / 标签页 / 系统控制按钮等）。
 */

interface TabItem {
  key: string;
  path: string;
  icon: typeof Home;
  label: string;
  /** 当前路径以哪些前缀开头时高亮该 Tab */
  matchPrefixes: string[];
  /** 高亮颜色 token，AI 用 accent，其它用 primary */
  activeColor?: "primary" | "accent";
}

const TABS: TabItem[] = [
  {
    key: "home",
    path: "/",
    icon: Home,
    label: "主页",
    matchPrefixes: ["/"],
  },
  {
    key: "notes",
    path: "/notes",
    icon: FileText,
    label: "笔记",
    matchPrefixes: ["/notes", "/search", "/tags", "/daily", "/trash", "/hidden"],
  },
  {
    key: "ai",
    path: "/ai",
    icon: Sparkles,
    label: "AI",
    matchPrefixes: ["/ai", "/prompts"],
    activeColor: "accent",
  },
  {
    key: "tasks",
    path: "/tasks",
    icon: CheckSquare,
    label: "待办",
    matchPrefixes: ["/tasks", "/cards"],
  },
  {
    key: "me",
    path: "/settings",
    icon: User,
    label: "我的",
    matchPrefixes: ["/settings", "/about", "/graph"],
  },
];

function isTabActive(item: TabItem, pathname: string): boolean {
  // "/" 单独判（其它前缀都以 "/" 开头会误命中）
  if (item.path === "/") return pathname === "/";
  return item.matchPrefixes.some(
    (p) => p !== "/" && (pathname === p || pathname.startsWith(`${p}/`)),
  );
}

export function MobileLayout() {
  const navigate = useNavigate();
  const location = useLocation();

  // Android 物理返回键 / 手势：让 history back 优先（路由内导航），
  // 而不是让 WebView 直接关闭应用。
  useEffect(() => {
    function onPopState() {
      // React Router 自己处理；这里仅占位，未来需要拦截"已在根路由还按返回"时退出 app 可在此扩展
    }
    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  }, []);

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-slate-50">
      {/* Android 状态栏让出 padding（系统会在这块绘制信号/电量） */}
      <div style={{ height: "env(safe-area-inset-top, 0px)" }} />

      {/* 主内容：可滚动；底部留出 Tab(64) + FAB 间距 + 安全区 */}
      <main className="relative flex-1 overflow-y-auto overflow-x-hidden">
        <div className="min-h-full pb-20">
          <Outlet />
        </div>

        {/* 浮动 FAB（右下，悬浮在 Tab 上方） */}
        <button
          aria-label="新建"
          onClick={() => navigate("/notes")}
          className="fixed right-5 z-30 flex h-14 w-14 items-center justify-center rounded-full bg-[#1677FF] text-white shadow-[0_8px_24px_rgba(22,119,255,0.4)] active:scale-95 transition-transform"
          style={{
            bottom: `calc(64px + env(safe-area-inset-bottom, 0px) + 16px)`,
          }}
        >
          <Plus size={28} strokeWidth={2.5} />
        </button>
      </main>

      {/* 底部 5 Tab + 安全区 */}
      <nav
        className="border-t border-slate-200 bg-white"
        style={{ paddingBottom: "env(safe-area-inset-bottom, 0px)" }}
      >
        <div className="flex h-16 items-stretch">
          {TABS.map((tab) => {
            const active = isTabActive(tab, location.pathname);
            const Icon = tab.icon;
            const activeColor =
              tab.activeColor === "accent" ? "#FA8C16" : "#1677FF";
            return (
              <button
                key={tab.key}
                onClick={() => navigate(tab.path)}
                className="flex flex-1 flex-col items-center justify-center gap-0.5 py-1 active:bg-slate-100"
                style={{
                  color: active ? activeColor : "#94A3B8",
                  minHeight: 44,
                }}
              >
                <Icon size={22} strokeWidth={active ? 2.5 : 2} />
                <span
                  className="text-[10px] leading-tight"
                  style={{ fontWeight: active ? 600 : 400 }}
                >
                  {tab.label}
                </span>
              </button>
            );
          })}
        </div>
      </nav>
    </div>
  );
}
