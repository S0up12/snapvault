import { listen } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";

import { invokeLogged } from "./ipc";

export type Phase =
  | "extracting"
  | "parsing"
  | "parsing_chats"
  | "parsing_profile"
  | "processing_media";

export const ALL_PHASES: Phase[] = [
  "extracting",
  "parsing",
  "parsing_chats",
  "parsing_profile",
  "processing_media",
];

type IngestionProgressEvent =
  | {
      status: "progress";
      job_id: string;
      phase: Phase;
      message: string;
      processed: number;
      total: number;
      percent: number;
    }
  | { status: "completed"; job_id: string; summary: IngestionSummary }
  | { status: "error"; job_id: string; phase: string; message: string };

export type IngestionSummary = {
  job_id: string;
  destination: string;
  extracted_entries: number;
  skipped_entries: number;
  json_items: number;
  files_found: number;
  matched: number;
  unmatched_files: number;
  assets_inserted: number;
  memory_items_inserted: number;
  files_timestamp_repaired: number;
  chat_threads_inserted: number;
  chat_messages_inserted: number;
  chat_media_assets_linked: number;
  profile_found: boolean;
  thumbnails_generated: number;
  thumbnails_failed: number;
  playback_transcoded: number;
  playback_failed: number;
  media_cancelled?: boolean;
};

type ThumbnailProgressEvent =
  | { status: "progress"; processed: number; total: number; percent: number; message: string }
  | { status: "paused"; processed: number; total: number }
  | { status: "resumed"; processed: number; total: number }
  | { status: "completed"; summary: ThumbnailSummary }
  | { status: "cancelled"; summary: ThumbnailSummary }
  | { status: "error"; message: string };

export type ThumbnailSummary = {
  total: number;
  generated: number;
  failed: number;
  playback_transcoded: number;
  playback_failed: number;
  cancelled?: boolean;
};

export type MediaState = "idle" | "running" | "paused" | "cancelling";

export type JobStatus = "idle" | "running" | "paused" | "cancelled" | "completed" | "error";

export type IngestionState = {
  status: JobStatus;
  activePhase: Phase | null;
  phaseMessage: string;
  phasePercent: number;
  donePhases: Set<Phase>;
  summary: IngestionSummary | null;
  error: { phase: string; message: string } | null;
};

export type ThumbnailState = {
  status: JobStatus;
  message: string;
  percent: number;
  processed: number;
  total: number;
  summary: ThumbnailSummary | null;
  error: string | null;
};

type Store = {
  ingestion: IngestionState;
  thumbnails: ThumbnailState;
  backendBusy: boolean;
  mediaState: MediaState;
};

const idleIngestion: IngestionState = {
  status: "idle",
  activePhase: null,
  phaseMessage: "",
  phasePercent: 0,
  donePhases: new Set(),
  summary: null,
  error: null,
};
const idleThumbnails: ThumbnailState = {
  status: "idle",
  message: "",
  percent: 0,
  processed: 0,
  total: 0,
  summary: null,
  error: null,
};

let state: Store = {
  ingestion: idleIngestion,
  thumbnails: idleThumbnails,
  backendBusy: false,
  mediaState: "idle",
};
const listeners = new Set<() => void>();

function subscribe(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

function setIngestion(patch: Partial<IngestionState>): void {
  state = { ...state, ingestion: { ...state.ingestion, ...patch } };
  listeners.forEach((l) => l());
}
function setThumbnails(patch: Partial<ThumbnailState>): void {
  state = { ...state, thumbnails: { ...state.thumbnails, ...patch } };
  listeners.forEach((l) => l());
}

async function refreshBusy(): Promise<void> {
  try {
    const m = await invokeLogged<{ state: MediaState; busy: boolean }>("get_media_state");
    if (m.busy !== state.backendBusy || m.state !== state.mediaState) {
      state = { ...state, backendBusy: m.busy, mediaState: m.state };
      listeners.forEach((l) => l());
    }
  } catch {
    // ponytail: already logged by invokeLogged; keep last known value
  }
}

let initialized = false;

// Call once at app start. Listeners live for the app lifetime (never unsubscribed).
export function initJobStore(): void {
  if (initialized) return;
  initialized = true;

  void listen<IngestionProgressEvent>("ingestion://progress", (event) => {
    const p = event.payload;
    if (p.status === "progress") {
      const previous = state.ingestion.activePhase;
      const donePhases =
        previous && previous !== p.phase
          ? new Set(state.ingestion.donePhases).add(previous)
          : state.ingestion.donePhases;
      setIngestion({
        status: "running",
        donePhases,
        activePhase: p.phase,
        phaseMessage: p.message,
        phasePercent: p.percent,
      });
    } else if (p.status === "completed") {
      setIngestion({ donePhases: new Set(ALL_PHASES), summary: p.summary, status: "completed" });
      void refreshBusy();
    } else if (p.status === "error") {
      setIngestion({ error: { phase: p.phase, message: p.message }, status: "error" });
      void refreshBusy();
    }
  });

  void listen<ThumbnailProgressEvent>("thumbnails://progress", (event) => {
    const p = event.payload;
    if (p.status === "progress") {
      // Also covers runs this window did not start (startup resume, window reload).
      setThumbnails({
        message: p.message,
        percent: p.percent,
        processed: p.processed,
        total: p.total,
        // In-flight files still report after Pause; stay paused until "resumed".
        status: state.thumbnails.status === "paused" ? "paused" : "running",
      });
    } else if (p.status === "paused") {
      setThumbnails({ processed: p.processed, total: p.total, status: "paused" });
      void refreshBusy();
    } else if (p.status === "resumed") {
      setThumbnails({ processed: p.processed, total: p.total, status: "running" });
      void refreshBusy();
    } else if (p.status === "cancelled") {
      setThumbnails({ summary: p.summary, status: "cancelled" });
      void refreshBusy();
    } else if (p.status === "completed") {
      setThumbnails({ summary: p.summary, percent: 100, status: "completed" });
      void refreshBusy();
    } else if (p.status === "error") {
      setThumbnails({ error: p.message, status: "error" });
      void refreshBusy();
    }
  });

  void refreshBusy();
  window.addEventListener("focus", () => void refreshBusy());
  setInterval(() => void refreshBusy(), 5000);
}

// Optimistic mediaState; reconciled from get_media_state afterwards. Invalid
// transitions are rejected by the backend and just resync quietly.
async function mediaCommand(command: string, optimistic: MediaState): Promise<void> {
  const previous = state.mediaState;
  state = { ...state, mediaState: optimistic };
  listeners.forEach((l) => l());
  try {
    await invokeLogged(command);
  } catch {
    state = { ...state, mediaState: previous };
    listeners.forEach((l) => l());
  }
  void refreshBusy();
}
export const pauseMedia = () => mediaCommand("pause_media_processing", "paused");
export const resumeMedia = () => mediaCommand("resume_media_processing", "running");
export const cancelMedia = () => mediaCommand("cancel_media_processing", "cancelling");

export async function startIngestion(archivePaths: string[]): Promise<void> {
  setIngestion({
    ...idleIngestion,
    donePhases: new Set(),
    status: "running",
    phaseMessage: "Starting...",
  });
  try {
    const result = await invokeLogged<IngestionSummary>("run_ingestion", { archivePaths });
    setIngestion({ summary: result, donePhases: new Set(ALL_PHASES), status: "completed" });
  } catch (err) {
    setIngestion({
      status: "error",
      error: state.ingestion.error ?? {
        phase: state.ingestion.activePhase ?? "ingestion",
        message: String(err),
      },
    });
  }
  void refreshBusy();
}

export async function startThumbnails(): Promise<void> {
  setThumbnails({ ...idleThumbnails, status: "running", message: "Starting..." });
  try {
    const result = await invokeLogged<ThumbnailSummary>("generate_thumbnails");
    if (result.cancelled) setThumbnails({ summary: result, status: "cancelled" });
    else setThumbnails({ summary: result, percent: 100, status: "completed" });
  } catch (err) {
    // Backend already busy (e.g. startup resume): not an error, the busy banner covers it.
    if (String(err).includes("already running")) setThumbnails({ ...idleThumbnails });
    else setThumbnails({ status: "error", error: String(err) });
  }
  void refreshBusy();
}

export function useIngestionJob(): IngestionState & { backendBusy: boolean; mediaState: MediaState } {
  const ingestion = useSyncExternalStore(subscribe, () => state.ingestion);
  const backendBusy = useSyncExternalStore(subscribe, () => state.backendBusy);
  const mediaState = useSyncExternalStore(subscribe, () => state.mediaState);
  return { ...ingestion, backendBusy, mediaState };
}

export function useThumbnailJob(): ThumbnailState & { backendBusy: boolean; mediaState: MediaState } {
  const thumbnails = useSyncExternalStore(subscribe, () => state.thumbnails);
  const backendBusy = useSyncExternalStore(subscribe, () => state.backendBusy);
  const mediaState = useSyncExternalStore(subscribe, () => state.mediaState);
  return { ...thumbnails, backendBusy, mediaState };
}
