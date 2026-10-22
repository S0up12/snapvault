import { open } from "@tauri-apps/plugin-dialog";
import { X } from "lucide-react";
import { useState } from "react";

import { useDialogFocus } from "../hooks/useDialogFocus";
import { startChatExport, startMemoriesExport, useExport, type ChatExportOptions, type MemoriesExportOptions } from "../lib/exportStore";

export type ExportDialogProps = { onClose: () => void } & (
  | { kind: "memories"; filter: string; tag: string | null; year: number | null; count?: number }
  | { kind: "chats"; currentThreadId: string | null; currentLabel: string | null; threadCount: number }
);

export default function ExportDialog(props: ExportDialogProps) {
  const { onClose } = props;
  const dialogRef = useDialogFocus<HTMLDivElement>(onClose);
  const { busy } = useExport();
  const [destination, setDestination] = useState<string | null>(null);
  const [applyOverlays, setApplyOverlays] = useState(true);
  const [organize, setOrganize] = useState<MemoriesExportOptions["organize"]>("year_month");
  const [setFileTimes, setSetFileTimes] = useState(true);
  const [format, setFormat] = useState<ChatExportOptions["format"]>("html");
  const [includeMedia, setIncludeMedia] = useState(true);
  const hasCurrent = props.kind === "chats" && props.currentThreadId !== null;
  const [chatScope, setChatScope] = useState<"this" | "all">(hasCurrent ? "this" : "all");

  async function chooseFolder() {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked === "string") setDestination(picked);
  }

  function handleExport() {
    if (!destination) return;
    if (props.kind === "memories") {
      void startMemoriesExport({
        destination,
        filter: props.filter,
        tag: props.tag,
        year: props.year === null ? null : String(props.year),
        apply_overlays: applyOverlays,
        organize,
        set_file_times: setFileTimes,
      });
    } else {
      void startChatExport({
        destination,
        thread_id: chatScope === "this" ? props.currentThreadId : null,
        format,
        include_media: includeMedia,
      });
    }
    onClose();
  }

  const scope =
    props.kind === "memories"
      ? `Exporting: ${[props.filter, props.tag, props.year].filter((v) => v !== null && v !== "").join(" / ")}${
          props.count !== undefined ? ` (${props.count.toLocaleString()} items)` : ""
        }`
      : null;
  const rowLabel = "flex flex-col gap-1.5 text-sm";
  const rowTitle = "text-xs font-medium text-neutral-400";

  return (
    <div className="dialog-backdrop z-[60]" onClick={onClose}>
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label={props.kind === "memories" ? "Export memories" : "Export chats"}
        tabIndex={-1}
        className="dialog w-full max-w-md outline-none"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <h2 className="dialog-title">{props.kind === "memories" ? "Export memories" : "Export chats"}</h2>
          <button type="button" onClick={onClose} className="btn btn-secondary btn-icon" aria-label="Close">
            <X className="h-4 w-4" />
          </button>
        </div>

        {scope ? <p className="text-xs text-neutral-400">{scope}</p> : null}

        {props.kind === "chats" ? (
          <fieldset className="flex flex-col gap-1.5 text-sm">
            <legend className={`${rowTitle} mb-1.5`}>What to export</legend>
            <label className={`flex items-center gap-2 ${hasCurrent ? "" : "opacity-45"}`}>
              <input
                type="radio"
                name="chat-scope"
                checked={chatScope === "this"}
                disabled={!hasCurrent}
                onChange={() => setChatScope("this")}
              />
              <span className="min-w-0 truncate">This chat{props.currentLabel ? ` - ${props.currentLabel}` : ""}</span>
            </label>
            <label className="flex items-center gap-2">
              <input type="radio" name="chat-scope" checked={chatScope === "all"} onChange={() => setChatScope("all")} />
              All chats ({props.threadCount.toLocaleString()})
            </label>
          </fieldset>
        ) : null}

        <div className={rowLabel}>
          <span className={rowTitle}>Save to</span>
          <div className="flex items-center gap-2">
            <button type="button" data-autofocus onClick={() => void chooseFolder()} className="btn btn-secondary">
              Choose folder...
            </button>
            <p className="min-w-0 flex-1 truncate text-xs text-neutral-400" title={destination ?? undefined}>
              {destination ?? "No folder chosen"}
            </p>
          </div>
        </div>

        {props.kind === "memories" ? (
          <>
            <label className="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={applyOverlays} onChange={(e) => setApplyOverlays(e.target.checked)} />
              Apply caption/drawing overlays to photos
            </label>
            <label className={rowLabel}>
              <span className={rowTitle}>Organize into</span>
              <select
                value={organize}
                onChange={(e) => setOrganize(e.target.value as MemoriesExportOptions["organize"])}
                className="input"
              >
                <option value="flat">One folder</option>
                <option value="year">By year</option>
                <option value="year_month">By year and month</option>
              </select>
            </label>
            <label className="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={setFileTimes} onChange={(e) => setSetFileTimes(e.target.checked)} />
              Set original dates on files
            </label>
            <p className="text-xs text-neutral-500">Videos are copied without overlays.</p>
          </>
        ) : (
          <>
            <label className={rowLabel}>
              <span className={rowTitle}>Format</span>
              <select
                value={format}
                onChange={(e) => setFormat(e.target.value as ChatExportOptions["format"])}
                className="input"
              >
                <option value="html">HTML (readable)</option>
                <option value="txt">Text</option>
                <option value="json">JSON</option>
              </select>
            </label>
            <label className="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={includeMedia} onChange={(e) => setIncludeMedia(e.target.checked)} />
              Include photos and videos
            </label>
          </>
        )}

        {busy ? <p className="text-xs text-neutral-500">Another export is running.</p> : null}

        <div className="flex justify-end gap-2">
          <button type="button" onClick={onClose} className="btn btn-secondary">
            Cancel
          </button>
          <button type="button" onClick={handleExport} disabled={!destination || busy} className="btn btn-primary">
            Export
          </button>
        </div>
      </div>
    </div>
  );
}
