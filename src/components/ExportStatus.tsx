import { revealItemInDir } from "@tauri-apps/plugin-opener";

import {
  cancelExport,
  dismissExport,
  useExport,
  type ChatExportSummary,
  type MemoriesExportSummary,
} from "../lib/exportStore";

function summaryLine(kind: "memories" | "chats", summary: MemoriesExportSummary | ChatExportSummary): string {
  if (kind === "memories") {
    const s = summary as MemoriesExportSummary;
    const parts = [`${s.exported} exported`, `${s.skipped_existing} skipped`, `${s.failed + s.missing_source} failed`];
    if (s.overlay_failed > 0) parts.push(`${s.overlay_failed} overlay failures`);
    if (s.videos_without_overlay > 0) parts.push(`${s.videos_without_overlay} videos copied without overlay`);
    return parts.join(", ");
  }
  const s = summary as ChatExportSummary;
  const parts = [`${s.threads} chats`, `${s.messages} messages`, `${s.media_copied} media copied`];
  if (s.media_missing > 0) parts.push(`${s.media_missing} media missing`);
  if (s.failed > 0) parts.push(`${s.failed} failed`);
  return parts.join(", ");
}

export default function ExportStatus() {
  const { status, kind, processed, total, message, summary, error } = useExport();
  if (status === "idle") return null;

  const label = kind === "chats" ? "Chat export" : "Memories export";

  return (
    <div className="flex shrink-0 items-center gap-3 border-b border-divider px-5 py-2 text-xs">
      {status === "running" ? (
        <>
          <span className="shrink-0">{label}</span>
          <progress className="h-1.5 w-48" value={total > 0 ? processed : undefined} max={total > 0 ? total : undefined} />
          <span className="shrink-0 text-neutral-400">
            {processed.toLocaleString()} / {total.toLocaleString()}
          </span>
          <span className="min-w-0 flex-1 truncate text-neutral-500">{message}</span>
          <button type="button" onClick={() => void cancelExport()} className="btn btn-secondary">
            Cancel
          </button>
        </>
      ) : (
        <>
          <span className={["min-w-0 flex-1", status === "error" ? "text-red-300" : "text-neutral-300"].join(" ")}>
            {status === "error"
              ? `${label} failed: ${error ?? "unknown error"}`
              : `${label} ${status === "cancelled" ? "cancelled" : "finished"}${
                  summary && kind ? `: ${summaryLine(kind, summary)}` : ""
                }`}
          </span>
          {summary && status !== "error" ? (
            <button type="button" onClick={() => void revealItemInDir(summary.destination)} className="btn btn-secondary">
              Show in folder
            </button>
          ) : null}
          <button type="button" onClick={dismissExport} className="btn btn-secondary">
            Dismiss
          </button>
        </>
      )}
    </div>
  );
}
