import { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import {
  Alert,
  Button,
  Popconfirm,
  Space,
  Table,
  Typography,
  message,
} from "antd";
import type { ColumnsType, TablePaginationConfig } from "antd/es/table";
import { Eye, EyeOff } from "lucide-react";
import { hiddenApi, noteApi } from "@/lib/api";
import { EmptyState } from "@/components/ui/EmptyState";
import type { Note, PageResult } from "@/types";

const { Title, Text } = Typography;

function relativeTime(dateStr: string): string {
  const date = new Date(dateStr);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 1) return "刚刚";
  if (diffMin < 60) return `${diffMin}分钟前`;
  const diffHour = Math.floor(diffMin / 60);
  if (diffHour < 24) return `${diffHour}小时前`;
  const diffDay = Math.floor(diffHour / 24);
  if (diffDay < 7) return `${diffDay}天前`;
  return dateStr.slice(0, 10);
}

/**
 * 隐藏笔记页（T-003）
 *
 * 语义与 `/trash` 类似，但独立维度：回收站是"已删除"，这里是"主动隐藏"。
 * 主界面（列表 / 搜索 / 反链 / 图谱 / RAG）完全不显示隐藏笔记；
 * 只在这个专用页面能看到、取消隐藏或点击打开。
 */
export default function HiddenPage() {
  const navigate = useNavigate();
  const [data, setData] = useState<PageResult<Note>>({
    items: [],
    total: 0,
    page: 1,
    page_size: 20,
  });
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    void loadList(1);
  }, []);

  async function loadList(page: number) {
    setLoading(true);
    try {
      const result = await hiddenApi.list(page, 20);
      setData(result);
    } catch (e) {
      message.error(String(e));
    } finally {
      setLoading(false);
    }
  }

  async function handleUnhide(id: number) {
    try {
      await noteApi.setHidden(id, false);
      message.success("已取消隐藏");
      // 删除当前行即可，保持分页状态
      setData((prev) => ({
        ...prev,
        items: prev.items.filter((n) => n.id !== id),
        total: Math.max(0, prev.total - 1),
      }));
    } catch (e) {
      message.error(String(e));
    }
  }

  function handleTableChange(pagination: TablePaginationConfig) {
    void loadList(pagination.current ?? 1);
  }

  const columns: ColumnsType<Note> = [
    {
      title: "标题",
      dataIndex: "title",
      key: "title",
      ellipsis: true,
      render: (val: string, record: Note) => (
        <a
          onClick={(e) => {
            e.preventDefault();
            navigate(`/notes/${record.id}`);
          }}
          style={{ cursor: "pointer" }}
        >
          {val}
        </a>
      ),
    },
    {
      title: "更新时间",
      dataIndex: "updated_at",
      key: "updated_at",
      width: 120,
      render: (val: string) => (
        <Text type="secondary" style={{ fontSize: 13 }}>
          {relativeTime(val)}
        </Text>
      ),
    },
    {
      title: "操作",
      key: "action",
      width: 140,
      render: (_: unknown, record: Note) => (
        <Space size="small">
          <Popconfirm
            title="取消隐藏？"
            description="这条笔记会重新出现在主列表 / 搜索 / 图谱中"
            okText="取消隐藏"
            cancelText="保留隐藏"
            onConfirm={() => handleUnhide(record.id)}
          >
            <Button type="link" size="small" icon={<Eye size={14} />}>
              取消隐藏
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div className="max-w-4xl mx-auto">
      <div className="flex items-center justify-between mb-4">
        <Title level={3} style={{ margin: 0, lineHeight: "32px" }}>
          <span className="flex items-center gap-2">
            <EyeOff size={22} />
            隐藏笔记
          </span>
        </Title>
      </div>

      <Alert
        type="info"
        showIcon
        message='这里只显示被标记为"隐藏"的笔记'
        description="隐藏是弱保护：主界面 / 搜索 / 反链 / 图谱 / AI 问答都不会显示这些笔记，但数据库里仍是明文。需要强保护（加密）等待 T-007。"
        style={{ marginBottom: 16 }}
      />

      {data.total > 0 || loading ? (
        <Table
          columns={columns}
          dataSource={data.items}
          rowKey="id"
          loading={loading}
          onChange={handleTableChange}
          pagination={{
            current: data.page,
            pageSize: data.page_size,
            total: data.total,
            showTotal: (total) => `共 ${total} 篇`,
            showSizeChanger: false,
          }}
        />
      ) : (
        <EmptyState description="还没有被隐藏的笔记" />
      )}
    </div>
  );
}
