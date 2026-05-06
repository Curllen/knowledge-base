import { useEffect, useRef, useState } from "react";
import { Modal, Tabs, message, Alert } from "antd";
import { Copy, AlertTriangle } from "lucide-react";
import QRCode from "qrcode";
import {
  KIND_LABELS,
  stringifyEnvelope,
  type Envelope,
} from "@/lib/configShare";

/**
 * 配置导出弹窗。
 * 两个 tab：
 *   - JSON：可复制粘贴的文本
 *   - QR 码：手机端扫一下即可导入
 *
 * 顶部红色 banner：只发给信任的人 — envelope 含 API Key / 密码明文。
 */
export function ShareConfigModal({
  open,
  onClose,
  envelope,
  hasSecret = true,
}: {
  open: boolean;
  onClose: () => void;
  envelope: Envelope | null;
  /** 此 envelope 是否含敏感字段（API Key / 密码）— 决定是否显示警告 */
  hasSecret?: boolean;
}) {
  const [text, setText] = useState("");
  const [qrDataUrl, setQrDataUrl] = useState("");
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    if (!envelope) {
      setText("");
      setQrDataUrl("");
      return;
    }
    const json = stringifyEnvelope(envelope);
    setText(json);
    // QR 码用紧凑（无缩进）的 JSON 减小图像复杂度
    const compact = stringifyEnvelope(envelope, false);
    void QRCode.toDataURL(compact, {
      errorCorrectionLevel: "M",
      width: 320,
      margin: 1,
    })
      .then(setQrDataUrl)
      .catch((e) => {
        console.warn("[ShareConfigModal] QR generation failed:", e);
        setQrDataUrl("");
      });
  }, [envelope]);

  async function copyJson() {
    try {
      await navigator.clipboard.writeText(text);
      message.success("已复制到剪贴板");
    } catch {
      // fallback：选中 textarea 让用户手动复制
      message.error("自动复制失败，请手动选中复制");
    }
  }

  const title = envelope
    ? `分享 ${KIND_LABELS[envelope.kind]}`
    : "分享配置";

  return (
    <Modal
      open={open}
      onCancel={onClose}
      title={title}
      footer={null}
      destroyOnClose
      width={420}
    >
      {hasSecret && (
        <Alert
          type="warning"
          showIcon
          icon={<AlertTriangle size={16} className="text-amber-600" />}
          message="包含敏感字段"
          description="此配置含 API Key / 密码（明文），请只发给信任的人。"
          className="!mb-3"
        />
      )}

      <Tabs
        items={[
          {
            key: "json",
            label: "JSON 文本",
            children: (
              <div>
                <textarea
                  readOnly
                  value={text}
                  onClick={(e) => (e.target as HTMLTextAreaElement).select()}
                  className="w-full h-48 rounded-lg border border-slate-200 bg-slate-50 p-3 font-mono text-xs leading-relaxed"
                />
                <div className="mt-3 flex gap-2">
                  <button
                    onClick={copyJson}
                    className="flex flex-1 items-center justify-center gap-1.5 rounded-lg bg-[#1677FF] py-2 text-sm font-medium text-white active:scale-95 transition-transform"
                  >
                    <Copy size={14} /> 复制 JSON
                  </button>
                </div>
              </div>
            ),
          },
          {
            key: "qr",
            label: "扫码导入",
            children: (
              <div className="flex flex-col items-center gap-3">
                {qrDataUrl ? (
                  <img
                    src={qrDataUrl}
                    alt="config QR code"
                    className="h-72 w-72 rounded-lg border border-slate-200 bg-white p-2"
                  />
                ) : (
                  <div className="h-72 w-72 flex items-center justify-center rounded-lg border border-dashed border-slate-300 text-sm text-slate-400">
                    QR 生成中…
                  </div>
                )}
                <p className="text-center text-xs text-slate-500">
                  在另一台设备的 <strong>导入配置</strong> → <strong>扫码</strong>
                  <br />
                  对准二维码即可
                </p>
              </div>
            ),
          },
        ]}
      />
      <canvas ref={canvasRef} className="hidden" />
    </Modal>
  );
}
