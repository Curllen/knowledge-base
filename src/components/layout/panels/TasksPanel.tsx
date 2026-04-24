import { useState, useEffect } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { Button, Badge, Divider, theme as antdTheme } from "antd";
import {
  CheckSquare,
  Plus,
  Inbox,
  AlertTriangle,
  Sun,
  Flame,
  Check,
} from "lucide-react";
import { taskApi } from "@/lib/api";
import { useAppStore } from "@/store";
import type { TaskStats } from "@/types";

/**
 * TasksPanel —— "待办"视图的主面板（MVP 第 1 步）。
 *
 * 智能列表 + URL 驱动:
 *   · 📥 进行中 (默认 /tasks 不带参数)
 *   · ⚠️ 逾期
 *   · 📅 今天
 *   · 🔴 紧急
 *   · ✓ 已完成
 *
 * Badge 数字来自 taskApi.stats()，订阅 store.urgentTodoCount 的变化触发重拉。
 *
 * 后续迭代（未实现）：本周 / 无日期 / 按优先级 / 按关联。
 */

type FilterKey = "todo" | "overdue" | "today" | "urgent" | "done";

export function TasksPanel() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const { token } = antdTheme.useToken();

  // URL 是真相源；缺省视为 "todo"
  const currentFilter = (searchParams.get("filter") ?? "todo") as FilterKey;

  // 订阅 urgentTodoCount：主区做完任务操作后会 refreshTaskStats，
  // 这里 tick 一次就重拉 stats，保持 Badge 实时
  const urgentTodoCount = useAppStore((s) => s.urgentTodoCount);

  const [stats, setStats] = useState<TaskStats | null>(null);

  useEffect(() => {
    let cancelled = false;
    taskApi
      .stats()
      .then((s) => {
        if (!cancelled) setStats(s);
      })
      .catch(() => {
        // 静默：Panel 不是关键路径，失败就不显示 Badge
      });
    return () => {
      cancelled = true;
    };
  }, [urgentTodoCount]);

  function goTo(f: FilterKey) {
    if (f === "todo") {
      navigate("/tasks");
    } else {
      navigate(`/tasks?filter=${f}`);
    }
  }

  const items: Array<{
    key: FilterKey;
    icon: React.ReactNode;
    label: string;
    count?: number;
    /** true 时 Badge 用红色（逾期/紧急） */
    danger?: boolean;
  }> = [
    {
      key: "todo",
      icon: <Inbox size={14} />,
      label: "进行中",
      count: stats?.totalTodo,
    },
    {
      key: "overdue",
      icon: <AlertTriangle size={14} />,
      label: "逾期",
      count: stats?.overdue,
      danger: true,
    },
    {
      key: "today",
      icon: <Sun size={14} />,
      label: "今天",
      count: stats?.dueToday,
    },
    {
      key: "urgent",
      icon: <Flame size={14} />,
      label: "紧急",
      count: stats?.urgentTodo,
      danger: true,
    },
  ];

  return (
    <div className="flex flex-col h-full" style={{ overflow: "hidden" }}>
      {/* 视图标题 */}
      <div
        className="flex items-center gap-2 px-3 py-2.5 shrink-0"
        style={{ borderBottom: `1px solid ${token.colorBorderSecondary}` }}
      >
        <CheckSquare size={15} style={{ color: token.colorPrimary }} />
        <span
          style={{ fontSize: 13, fontWeight: 600, color: token.colorText }}
        >
          待办
        </span>
        <div style={{ flex: 1 }} />
        <Button
          type="text"
          size="small"
          icon={<Plus size={14} />}
          onClick={() => {
            // 触发主区"新建任务"Modal：通过一次性 URL 参数携带
            navigate("/tasks?new=1");
          }}
          style={{ width: 24, height: 24, padding: 0 }}
          title="新建任务"
        />
      </div>

      {/* 智能列表 */}
      <div
        className="flex-1 overflow-auto"
        style={{ minHeight: 0, padding: "6px 8px" }}
      >
        {items.map((item) => (
          <SmartRow
            key={item.key}
            active={currentFilter === item.key}
            icon={item.icon}
            label={item.label}
            count={item.count}
            danger={item.danger}
            onClick={() => goTo(item.key)}
            token={token}
          />
        ))}

        <Divider style={{ margin: "8px 6px" }} />

        <SmartRow
          active={currentFilter === "done"}
          icon={<Check size={14} />}
          label="已完成"
          count={stats?.totalDone}
          onClick={() => goTo("done")}
          token={token}
        />
      </div>
    </div>
  );
}

function SmartRow({
  active,
  icon,
  label,
  count,
  danger,
  onClick,
  token,
}: {
  active: boolean;
  icon: React.ReactNode;
  label: string;
  count?: number;
  danger?: boolean;
  onClick: () => void;
  token: {
    colorPrimary: string;
    colorError: string;
    colorText: string;
    colorTextSecondary: string;
    colorTextTertiary: string;
  };
}) {
  return (
    <div
      onClick={onClick}
      className="cursor-pointer"
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        padding: "6px 10px",
        borderRadius: 6,
        background: active ? `${token.colorPrimary}14` : "transparent",
        color: active ? token.colorPrimary : token.colorText,
        fontWeight: active ? 500 : undefined,
        fontSize: 13,
        transition: "background .15s",
      }}
    >
      <span
        style={{
          color: danger
            ? token.colorError
            : active
              ? token.colorPrimary
              : token.colorTextSecondary,
          display: "inline-flex",
          alignItems: "center",
          flexShrink: 0,
        }}
      >
        {icon}
      </span>
      <span style={{ flex: 1 }}>{label}</span>
      {typeof count === "number" && count > 0 && (
        <Badge
          count={count}
          overflowCount={99}
          style={{
            backgroundColor: danger
              ? token.colorError
              : active
                ? token.colorPrimary
                : "transparent",
            color: danger || active ? "#fff" : token.colorTextTertiary,
            boxShadow: "none",
            fontWeight: 500,
          }}
        />
      )}
    </div>
  );
}
