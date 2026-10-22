import { listen } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";

import { invokeLogged } from "./ipc";

export type ExportKind = "memories" | "chats";

export type MemoriesExportOptions = {
  destination: string;
  filter?: string | null;
  tag?: string | null;
  year?: string | null;
  apply_overlays: boolean;
  organize: "flat" | "year" | "year_month";
  set_file_times: boolean;
};

export type ChatExportOptions = {
  destination: string;
  thread_id?: string | null;
  format: "html" | "txt" | "json";
  include_media: boolean;
};

export type MemoriesExportSummary = {
  total: number;
  exported: number;
  composited: number;
  overlay_failed: number;
  skipped_existing: number;
  missing_source: number;
  failed: number;
  videos_without_overlay: number;
  cancelled: boolean;
  bytes_written: number;
  destination: string;
};

export type ChatExportSummary = {
  threads: number;
  messages: number;
  media_copied: number;
  media_missing: number;
  failed: number;
  cancelled: boolean;
  destination: string;
};

export type ExportSummary = MemoriesExportSummary | ChatExportSummary;

type ExportEvent =
  | { kind: ExportKind; status: "progress"; processed: number; total: number; message: string }
  | { kind: "memories"; status: "completed" | "cancelled"; summary: MemoriesExportSummary }
  | { kind: "chats"; status: "completed" | "cancelled"; summary: ChatExportSummary }
  | { kind: ExportKind; status: "error"; message: string };

export type ExportState = {
  status: "idle" | "running" | "completed" | "cancelled" | "error";
  kind: ExportKind | null;
  processed: number;
  total: number;
  message: string;
  summary: ExportSummary | null;
  error: string | null;
  busy: boolean;
};

const idle: ExportState = {
  status: "idle",
  kind: null,
  processed: 0,
  total: 0,
  message: "",
  summary: null,
  error: null,
  busy: false,
};

let state: ExportState = idle;
const listeners = new Set<() => void>();

function subscribe(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

function set(patch: Partial<ExportState>): void {
  state = { ...state, ...patch };
  listeners.forEach((l) => l());
}

async function refreshBusy(): Promise<void> {
  try {
    const busy = await invokeLogged<boolean>("get_export_state");
    if (busy !== state.busy) {
      // Running in the backend but unknown here (window reload): show as running.
      set({ busy, ...(busy && state.status !== "running" ? { status: "running" as const } : {}) });
    }
  } catch {
    // ponytail: already logged by invokeLogged; keep last known value
  }
}

let initialized = false;

// Call once at app start. Listeners live for the app lifetime.
export function initExportStore(): void {
  if (initialized) return;
  initialized = true;

  void listen<ExportEvent>("export://progress", (event) => {
    const p = event.payload;
    if (p.status === "progress") {
      set({ status: "running", busy: true, kind: p.kind, processed: p.processed, total: p.total, message: p.message });
    } else if (p.status === "error") {
      set({ status: "error", busy: false, kind: p.kind, error: p.message });
    } else {
      set({ status: p.status, busy: false, kind: p.kind, summary: p.summary, error: null });
    }
  });

  void refreshBusy();
  window.addEventListener("focus", () => void refreshBusy());
}

async function start(kind: ExportKind, command: string, options: object): Promise<void> {
  set({ ...idle, status: "running", kind, busy: true, message: "Starting..." });
  try {
    const summary = await invokeLogged<ExportSummary>(command, { options });
    set({ status: summary.cancelled ? "cancelled" : "completed", busy: false, summary, error: null });
  } catch (err) {
    if (String(err).includes("already running")) {
      // Another export owns the slot; the progress events will drive the strip.
      set({ ...state, busy: true });
      void refreshBusy();
    } else {
      set({ status: "error", busy: false, error: String(err) });
    }
  }
}

export const startMemoriesExport = (options: MemoriesExportOptions) =>
  start("memories", "export_memories", options);
export const startChatExport = (options: ChatExportOptions) => start("chats", "export_chats", options);

export async function cancelExport(): Promise<void> {
  try {
    await invokeLogged("cancel_export");
  } catch {
    // ponytail: "No export is running." just means it already finished
  }
  void refreshBusy();
}

export function dismissExport(): void {
  if (state.busy) return;
  set({ ...idle });
}

export function useExport(): ExportState {
  return useSyncExternalStore(subscribe, () => state);
}
