import { useState, useEffect, useMemo } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { Button, Badge, theme as antdTheme } from "antd";
import {
  CheckSquare,
  Plus,
  ListChecks,
  AlertTriangle,
  Sun,
  CalendarRange,
  CalendarClock,
  Flame,
  Circle,
  Check,
  Repeat,
  Link as LinkIcon,
} from "lucide-react";
import { taskApi } from "@/lib/api";
import { useAppStore } from "@/store";
import type { Task, TaskStats } from "@/types";

/**
 * TasksPanel —— "待办"视图的主面板（方案 C MVP）。
 *
 * 维度：
 *   智能  : 进行中 / 逾期 / 今天 / 本周 / 无日期
 *   优先级: 紧急 / 普通 / 低
 *   归档  : 已完成
 *
 * 计数策略：拉一次 taskApi.list({status:0}) 拿所有未完成任务，本地派生
 * 所有未完成维度的计数；已完成数从 taskApi.stats().totalDone 取（避免
 * 把庞大的历史完成记录都拉回前端）。
 *
 * 订阅 urgentTodoCount：主区做任务操作后会 refreshTaskStats，这里 tick
 * 一次就重拉两份数据，Badge 保持实时。
 */

type FilterKey =
  | "todo"
  | "overdue"
  | "today"
  | "week"
  | "no-date"
  | "urgent"
  | "normal"
  | "low"
  | "recurring"
  | "linked"
  | "done";

/** YYYY-MM-DD（本地时区） */
function ymd(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function dueDay(t: Task): string | null {
  return t.due_date ? t.due_date.slice(0, 10) : null;
}

/** 从 Task[] 派生各维度计数 */
function deriveCounts(todoTasks: Task[]): Record<FilterKey, number> {
  const today = ymd(new Date());
  const weekEnd = ymd(new Date(Date.now() + 7 * 86400000));
  let overdue = 0;
  let dueToday = 0;
  let week = 0;
  let noDate = 0;
  let urgent = 0;
  let normal = 0;
  let low = 0;
  let recurring = 0;
  let linked = 0;
  for (const t of todoTasks) {
    if (t.priority === 0) urgent++;
    else if (t.priority === 1) normal++;
    else if (t.priority === 2) low++;
    if (t.repeat_kind && t.repeat_kind !== "none") recurring++;
    if (t.links && t.links.length > 0) linked++;
    const day = dueDay(t);
    if (!day) {
      noDate++;
    } else if (day < today) {
      overdue++;
    } else if (day === today) {
      dueToday++;
    } else if (day <= weekEnd) {
      week++;
    }
  }
  return {
    todo: todoTasks.length,
    overdue,
    today: dueToday,
    week,
    "no-date": noDate,
    urgent,
    normal,
    low,
    recurring,
    linked,
    done: 0, // 由 stats 覆盖
  };
}

export function TasksPanel() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const { token } = antdTheme.useToken();

  // URL 是真相源；缺省视为 "todo"
  const currentFilter = (searchParams.get("filter") ?? "todo") as FilterKey;

  // 订阅：主区任务增删改后 bump urgentTodoCount → 这里重拉
  const urgentTodoCount = useAppStore((s) => s.urgentTodoCount);

  const [todoTasks, setTodoTasks] = useState<Task[]>([]);
  const [stats, setStats] = useState<TaskStats | null>(null);

  useEffect(() => {
    let cancelled = false;
    // 并发：未完成任务列表 + 统计（为拿 totalDone）
    Promise.all([
      taskApi.list({ status: 0 }).catch(() => [] as Task[]),
      taskApi.stats().catch(() => null),
    ]).then(([list, s]) => {
      if (cancelled) return;
      setTodoTasks(list);
      setStats(s);
    });
    return () => {
      cancelled = true;
    };
  }, [urgentTodoCount]);

  const counts = useMemo(() => {
    const c = deriveCounts(todoTasks);
    c.done = stats?.totalDone ?? 0;
    return c;
  }, [todoTasks, stats]);

  function goTo(f: FilterKey) {
    if (f === "todo") {
      navigate("/tasks");
    } else {
      navigate(`/tasks?filter=${f}`);
    }
  }

  return (
    <div className="flex flex-col h-full" style={{ overflow: "hidden" }}>
      {/* 视图标题 */}
      <div
        className="flex items-center gap-2 px-3 py-2.5 shrink-0"
        style={{ borderBottom: `1px solid ${token.colorBorderSecondary}` }}
      >
        <CheckSquare size={15} style={{ color: token.colorPrimary }} />
        <span style={{ fontSize: 13, fontWeight: 600, color: token.colorText }}>
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

      <div
        className="flex-1 overflow-auto"
        style={{ minHeight: 0, padding: "6px 8px" }}
      >
        {/* 智能列表 */}
        {/*
          "全部任务" = 默认入口视图；列表视图下展示进行中任务 + 底部"已完成"折叠区，
          日历视图下进行中正常显示、已完成置灰显示。Badge 数仍只数未完成（与
          urgentTodoCount 等其他维度保持一致）。
        */}
        <SmartRow
          active={currentFilter === "todo"}
          icon={<ListChecks size={14} />}
          label="全部任务"
          count={counts.todo}
          onClick={() => goTo("todo")}
          token={token}
        />
        <SmartRow
          active={currentFilter === "overdue"}
          icon={<AlertTriangle size={14} />}
          label="逾期"
          count={counts.overdue}
          danger
          onClick={() => goTo("overdue")}
          token={token}
        />
        <SmartRow
          active={currentFilter === "today"}
          icon={<Sun size={14} />}
          label="今天"
          count={counts.today}
          onClick={() => goTo("today")}
          token={token}
        />
        <SmartRow
          active={currentFilter === "week"}
          icon={<CalendarRange size={14} />}
          label="本周"
          count={counts.week}
          onClick={() => goTo("week")}
          token={token}
        />
        <SmartRow
          active={currentFilter === "no-date"}
          icon={<CalendarClock size={14} />}
          label="无日期"
          count={counts["no-date"]}
          onClick={() => goTo("no-date")}
          token={token}
        />

        <GroupLabel token={token}>优先级</GroupLabel>

        <SmartRow
          active={currentFilter === "urgent"}
          icon={<Flame size={14} />}
          label="紧急"
          count={counts.urgent}
          danger
          onClick={() => goTo("urgent")}
          token={token}
        />
        <SmartRow
          active={currentFilter === "normal"}
          icon={<Circle size={14} fill={token.colorPrimary} stroke="none" />}
          label="普通"
          count={counts.normal}
          onClick={() => goTo("normal")}
          token={token}
        />
        <SmartRow
          active={currentFilter === "low"}
          icon={<Circle size={14} fill={token.colorTextQuaternary} stroke="none" />}
          label="低"
          count={counts.low}
          onClick={() => goTo("low")}
          token={token}
        />

        <GroupLabel token={token}>属性</GroupLabel>

        <SmartRow
          active={currentFilter === "recurring"}
          icon={<Repeat size={14} />}
          label="循环任务"
          count={counts.recurring}
          onClick={() => goTo("recurring")}
          token={token}
        />
        <SmartRow
          active={currentFilter === "linked"}
          icon={<LinkIcon size={14} />}
          label="有关联"
          count={counts.linked}
          onClick={() => goTo("linked")}
          token={token}
        />

        <GroupLabel token={token}>归档</GroupLabel>

        <SmartRow
          active={currentFilter === "done"}
          icon={<Check size={14} />}
          label="已完成"
          count={counts.done}
          onClick={() => goTo("done")}
          token={token}
        />
      </div>
    </div>
  );
}

/** 分组小标题（UPPERCASE 灰字） */
function GroupLabel({
  children,
  token,
}: {
  children: React.ReactNode;
  token: { colorTextTertiary: string };
}) {
  return (
    <div
      style={{
        color: token.colorTextTertiary,
        fontSize: 11,
        textTransform: "uppercase",
        letterSpacing: 0.5,
        padding: "12px 10px 6px",
      }}
    >
      {children}
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
