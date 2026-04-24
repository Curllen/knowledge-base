import { useNavigate, useSearchParams } from "react-router-dom";
import { Button, Popconfirm, theme as antdTheme } from "antd";
import { Search as SearchIcon, Clock, X, Trash2 } from "lucide-react";
import { useAppStore } from "@/store";

/**
 * SearchPanel —— Activity Bar 模式下"搜索"视图的主面板。
 *
 * 职责：
 *   · 顶部：视图标题 + "清空历史"按钮
 *   · 最近搜索列表：点击重用（navigate 到 /search?q=xxx）；
 *     每条支持单条删除（hover 显示 X）；空态引导用户先搜一下
 *
 * 进阶（未实现，占位给未来迭代）：
 *   · 按文件夹 / 标签 / 时间范围过滤：需要 Rust searchApi 支持 filter 参数
 *   · 保存的查询
 */
export function SearchPanel() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const currentQ = searchParams.get("q") ?? "";
  const { token } = antdTheme.useToken();

  const recentSearches = useAppStore((s) => s.recentSearches);
  const removeRecentSearch = useAppStore((s) => s.removeRecentSearch);
  const clearRecentSearches = useAppStore((s) => s.clearRecentSearches);

  function goToQuery(q: string) {
    navigate(`/search?q=${encodeURIComponent(q)}`);
  }

  return (
    <div
      className="flex flex-col h-full"
      style={{ overflow: "hidden" }}
    >
      {/* 视图标题 */}
      <div
        className="flex items-center gap-2 px-3 py-2.5 shrink-0"
        style={{ borderBottom: `1px solid ${token.colorBorderSecondary}` }}
      >
        <SearchIcon size={15} style={{ color: token.colorPrimary }} />
        <span style={{ fontSize: 13, fontWeight: 600, color: token.colorText }}>
          搜索
        </span>
        <div style={{ flex: 1 }} />
        {recentSearches.length > 0 && (
          <Popconfirm
            title="清空所有搜索历史？"
            okText="清空"
            cancelText="取消"
            onConfirm={clearRecentSearches}
          >
            <Button
              type="text"
              size="small"
              icon={<Trash2 size={14} />}
              style={{ width: 24, height: 24, padding: 0 }}
              title="清空历史"
            />
          </Popconfirm>
        )}
      </div>

      {/* 历史分组标题 */}
      <div
        className="flex items-center gap-1 shrink-0"
        style={{
          color: token.colorTextSecondary,
          fontSize: 11,
          textTransform: "uppercase",
          letterSpacing: 0.5,
          padding: "10px 16px 6px",
        }}
      >
        <Clock size={12} />
        最近搜索
      </div>

      {/* 历史列表 */}
      <div
        className="flex-1 overflow-auto"
        style={{ minHeight: 0, padding: "0 8px 8px" }}
      >
        {recentSearches.length === 0 ? (
          <div
            className="text-center py-6"
            style={{ color: token.colorTextQuaternary, fontSize: 12 }}
          >
            暂无搜索历史
            <br />
            <span style={{ fontSize: 11 }}>在右侧输入关键词搜索</span>
          </div>
        ) : (
          recentSearches.map((q) => {
            const active = q === currentQ;
            return (
              <div
                key={q}
                onClick={() => goToQuery(q)}
                className="cursor-pointer group"
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  padding: "6px 10px",
                  borderRadius: 6,
                  background: active
                    ? `${token.colorPrimary}14`
                    : "transparent",
                  color: active ? token.colorPrimary : token.colorText,
                  fontSize: 13,
                  transition: "background .15s",
                }}
              >
                <SearchIcon
                  size={12}
                  style={{
                    color: active ? token.colorPrimary : token.colorTextTertiary,
                    flexShrink: 0,
                  }}
                />
                <span className="truncate" style={{ flex: 1 }}>
                  {q}
                </span>
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    removeRecentSearch(q);
                  }}
                  aria-label="删除此条历史"
                  className="opacity-0 group-hover:opacity-100"
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    width: 18,
                    height: 18,
                    border: "none",
                    background: "transparent",
                    cursor: "pointer",
                    color: token.colorTextTertiary,
                    borderRadius: 3,
                    transition: "opacity .15s, background .15s",
                  }}
                >
                  <X size={12} />
                </button>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
