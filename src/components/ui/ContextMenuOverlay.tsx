import { Dropdown, type MenuProps } from "antd";

interface Props {
  /** 是否显示。建议传 `!!ctx.state.payload` */
  open: boolean;
  /** viewport 坐标（来自 useContextMenu state.x / state.y） */
  x: number;
  y: number;
  /** 同位置重唤起时用的 React key（来自 useContextMenu state.ts） */
  ts: number;
  /** 菜单项；调用方根据 payload 动态构造 */
  items: MenuProps["items"];
  /** 菜单项点击回调 */
  onClick?: MenuProps["onClick"];
  /** Dropdown 关闭时回调（点菜单项 / 点别处 / Esc）；一般直传 ctx.close */
  onClose: () => void;
}

/**
 * 右键菜单浮层：1×1 fixed 幻影锚点 + Dropdown(open=true)。
 *
 * 为什么不用 `<Dropdown trigger={["contextMenu"]}>包裹真实元素</Dropdown>`：
 * - antd Tree 等已绑定原生 mousedown / dragstart 的组件被这个 Dropdown 包了之后
 *   拖拽会失效（rc-trigger 拦截事件）
 * - 用幻影锚点让 Dropdown 完全脱离真实 DOM 树，只受 open + 坐标控制，
 *   不影响目标元素任何已有交互
 */
export function ContextMenuOverlay({
  open,
  x,
  y,
  ts,
  items,
  onClick,
  onClose,
}: Props) {
  return (
    <Dropdown
      // ts 让同位置反复唤起也能强制重渲染（Dropdown 内部缓存位置）
      key={ts}
      menu={{ items, onClick }}
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
      // 不靠任何触发器，只受 open 控制
      trigger={[]}
      destroyOnHidden
    >
      <div
        style={{
          position: "fixed",
          left: x,
          top: y,
          width: 1,
          height: 1,
          pointerEvents: "none",
          zIndex: 0,
        }}
      />
    </Dropdown>
  );
}
