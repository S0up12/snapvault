import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertCircle, CheckCircle2, Circle, FolderOpen, Loader2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import Stat from "./Stat";
import StorageChoiceModal from "./StorageChoiceModal";
import type { StorageInfo } from "../hooks/useStorageSettings";

type Phase = "extracting" | "parsing" | "parsing_chats" | "parsing_profile" | "processing_media";

type ProgressEvent =
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

type IngestionSummary = {
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
};

type Status = "idle" | "running" | "completed" | "error";

const STEPS: { key: Phase; label: string }[] = [
  { key: "extracting", label: "Extracting files" },
  { key: "parsing", label: "Parsing metadata & repairing timestamps" },
  { key: "parsing_chats", label: "Importing chat history" },
  { key: "parsing_profile", label: "Importing profile metadata" },
  { key: "processing_media", label: "Generating thumbnails & converting videos" },
];

export default function ImportFlow() {
  const [status, setStatus] = useState<Status>("idle");
  const [activePhase, setActivePhase] = useState<Phase | null>(null);
  const [phaseMessage, setPhaseMessage] = useState("");
  const [phasePercent, setPhasePercent] = useState(0);
  const [donePhases, setDonePhases] = useState<Set<Phase>>(new Set());
  const [summary, setSummary] = useState<IngestionSummary | null>(null);
  const [error, setError] = useState<{ phase: string; message: string } | null>(null);
  const [storageChoice, setStorageChoice] = useState<{ defaultPath: string } | null>(null);
  const activePhaseRef = useRef<Phase | null>(null);

  useEffect(() => {
    const unlisten = listen<ProgressEvent>("ingestion://progress", (event) => {
      const payload = event.payload;
      if (payload.status === "progress") {
        const previous = activePhaseRef.current;
        if (previous && previous !== payload.phase) {
          setDonePhases((done) => new Set(done).add(previous));
        }
        activePhaseRef.current = payload.phase;
        setActivePhase(payload.phase);
        setPhaseMessage(payload.message);
        setPhasePercent(payload.percent);
      } else if (payload.status === "completed") {
        setDonePhases(new Set(STEPS.map((step) => step.key)));
        setSummary(payload.summary);
        setStatus("completed");
      } else if (payload.status === "error") {
        setError({ phase: payload.phase, message: payload.message });
        setStatus("error");
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  async function handleSelectFile() {
    const storageInfo = await invoke<StorageInfo>("get_storage_info");
    if (storageInfo.needs_first_run_choice) {
      setStorageChoice({ defaultPath: storageInfo.media_root });
      return;
    }
    await openArchivePicker();
  }

  async function openArchivePicker() {
    const selected = await open({
      title: "Select Snapchat export (select all parts if it was split into several zips)",
      filters: [{ name: "Snapchat export", extensions: ["zip"] }],
      multiple: true,
    });

    if (!selected) {
      return;
    }
    const archivePaths = Array.isArray(selected) ? selected : [selected];

    activePhaseRef.current = null;
    setStatus("running");
    setActivePhase(null);
    setPhaseMessage("Starting...");
    setPhasePercent(0);
    setDonePhases(new Set());
    setSummary(null);
    setError(null);

    try {
      const result = await invoke<IngestionSummary>("run_ingestion", { archivePaths });
      setSummary(result);
      setDonePhases(new Set(STEPS.map((step) => step.key)));
      setStatus("completed");
    } catch (err) {
      setStatus("error");
      setError((prev) => prev ?? { phase: activePhaseRef.current ?? "ingestion", message: String(err) });
    }
  }

  return (
    <div className="rounded-lg bg-surface p-6 ring-1 ring-divider">
      <p className="text-[10px] uppercase tracking-[0.14em] text-accent">Import</p>
      <p className="mt-2 text-[19px]">Import a Snapchat export</p>
      <p className="mt-1 text-[13px] text-neutral-400">
        Select the .zip file(s) you downloaded from Snapchat's "My Data" export. If Snapchat split
        your export into multiple parts, select all of them at once.
      </p>

      <button
        type="button"
        onClick={handleSelectFile}
        disabled={status === "running"}
        className="btn btn-primary mt-4 disabled:cursor-not-allowed"
      >
        {status === "running" ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <FolderOpen className="h-4 w-4" />
        )}
        {status === "running" ? "Importing..." : "Select Snapchat export (.zip)"}
      </button>

      {status === "running" || status === "completed" || status === "error" ? (
        <div className="mt-6 flex flex-col gap-2">
          {STEPS.map((step) => {
            const isDone = status !== "error" && (donePhases.has(step.key) || status === "completed");
            const isActive = !isDone && activePhase === step.key && status === "running";
            const isErrored = status === "error" && error?.phase === step.key;

            return (
              <div
                key={step.key}
                className={[
                  "rounded-md p-3 transition",
                  isErrored
                    ? "bg-red-500/7"
                    : isActive
                      ? "bg-accent/12 shadow-[inset_2px_0_0_var(--color-accent)]"
                      : isDone
                        ? "bg-accent/7"
                        : "bg-white/[0.02]",
                ].join(" ")}
              >
                <div className="flex items-center gap-3">
                  {isErrored ? (
                    <AlertCircle className="h-5 w-5 shrink-0 text-red-400" />
                  ) : isDone ? (
                    <CheckCircle2 className="h-5 w-5 shrink-0 text-accent-200" />
                  ) : isActive ? (
                    <Loader2 className="h-5 w-5 shrink-0 animate-spin text-accent" />
                  ) : (
                    <Circle className="h-5 w-5 shrink-0 text-white/15" />
                  )}
                  <p
                    className={[
                      "text-[13.5px]",
                      isErrored
                        ? "text-red-300"
                        : isDone
                          ? "text-accent-200"
                          : isActive
                            ? "font-medium text-accent"
                            : "text-neutral-500",
                    ].join(" ")}
                  >
                    {step.label}
                  </p>
                </div>

                {isActive ? (
                  <div className="mt-2.5 pl-8">
                    <div className="h-[5px] w-full overflow-hidden rounded-full bg-neutral-800">
                      <div
                        className="h-full rounded-full bg-accent shadow-[0_0_12px_1px_color-mix(in_srgb,var(--color-accent)_55%,transparent)] transition-[width] duration-200"
                        style={{ width: `${phasePercent}%` }}
                      />
                    </div>
                    <p className="mt-1.5 truncate text-[11px] text-neutral-400">
                      {phaseMessage}
                    </p>
                  </div>
                ) : null}

                {isErrored ? (
                  <p className="mt-2 pl-8 text-xs text-red-400">{error?.message}</p>
                ) : null}
              </div>
            );
          })}
        </div>
      ) : null}

      {status === "error" && error && !STEPS.some((step) => step.key === error.phase) ? (
        <div className="mt-3 rounded-lg bg-red-500/8 p-4 ring-1 ring-red-400/25">
          <div className="flex items-center gap-2">
            <AlertCircle className="h-5 w-5 text-red-400" />
            <p className="text-sm font-semibold text-red-300">Import failed</p>
          </div>
          <p className="mt-1.5 text-xs text-red-400">{error.message}</p>
        </div>
      ) : null}

      {status === "completed" && summary ? (
        <div className="mt-5 rounded-lg bg-accent/6 p-4 ring-1 ring-accent/25">
          <div className="flex items-center gap-2">
            <CheckCircle2 className="h-5 w-5 text-accent" />
            <p className="text-sm font-semibold text-accent-200">Import complete</p>
          </div>
          <div className="mt-3 grid grid-cols-2 gap-3 text-xs sm:grid-cols-3">
            <Stat label="Files extracted" value={summary.extracted_entries} />
            <Stat label="Memories matched" value={summary.matched} />
            <Stat label="Assets inserted" value={summary.assets_inserted} />
            <Stat label="Memory items" value={summary.memory_items_inserted} />
            <Stat label="Timestamps repaired" value={summary.files_timestamp_repaired} />
            <Stat label="Unmatched files" value={summary.unmatched_files} />
            <Stat label="Chat threads" value={summary.chat_threads_inserted} />
            <Stat label="Chat messages" value={summary.chat_messages_inserted} />
            <Stat label="Chat media linked" value={summary.chat_media_assets_linked} />
            <Stat label="Thumbnails generated" value={summary.thumbnails_generated} />
            <Stat label="Videos converted" value={summary.playback_transcoded} />
          </div>
        </div>
      ) : null}

      {storageChoice ? (
        <StorageChoiceModal
          defaultPath={storageChoice.defaultPath}
          allowCancel={false}
          onChosen={() => {
            setStorageChoice(null);
            void openArchivePicker();
          }}
        />
      ) : null}
    </div>
  );
}
