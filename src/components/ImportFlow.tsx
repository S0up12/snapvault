import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertCircle, CheckCircle2, Circle, FolderOpen, Loader2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";

type Phase = "extracting" | "parsing";

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
};

type Status = "idle" | "running" | "completed" | "error";

const STEPS: { key: Phase; label: string }[] = [
  { key: "extracting", label: "Extracting files" },
  { key: "parsing", label: "Parsing metadata & repairing timestamps" },
];

export default function ImportFlow() {
  const [status, setStatus] = useState<Status>("idle");
  const [activePhase, setActivePhase] = useState<Phase | null>(null);
  const [phaseMessage, setPhaseMessage] = useState("");
  const [phasePercent, setPhasePercent] = useState(0);
  const [donePhases, setDonePhases] = useState<Set<Phase>>(new Set());
  const [summary, setSummary] = useState<IngestionSummary | null>(null);
  const [error, setError] = useState<{ phase: string; message: string } | null>(null);
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
    <div className="rounded-[1.75rem] border border-slate-200/80 bg-white/80 p-8 shadow-[0_20px_48px_rgba(15,23,42,0.08)] dark:border-white/10 dark:bg-white/[0.04]">
      <p className="text-[11px] font-semibold uppercase tracking-[0.28em] text-sky-700/70 dark:text-sky-200/65">
        Import
      </p>
      <p className="mt-2 text-lg font-semibold text-slate-950 dark:text-white">Import a Snapchat export</p>
      <p className="mt-1 text-sm text-slate-600 dark:text-slate-300">
        Select the .zip file(s) you downloaded from Snapchat's "My Data" export. If Snapchat split
        your export into multiple parts, select all of them at once.
      </p>

      <button
        type="button"
        onClick={handleSelectFile}
        disabled={status === "running"}
        className="mt-4 inline-flex items-center gap-2 rounded-[1rem] border border-sky-300/30 bg-sky-500/10 px-4 py-2 text-sm font-medium text-sky-700 transition hover:border-sky-400/45 hover:bg-sky-500/15 disabled:cursor-not-allowed disabled:opacity-50 dark:text-sky-200"
      >
        {status === "running" ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <FolderOpen className="h-4 w-4" />
        )}
        {status === "running" ? "Importing..." : "Select Snapchat export (.zip)"}
      </button>

      {status === "running" || status === "completed" || status === "error" ? (
        <div className="mt-6 space-y-3">
          {STEPS.map((step) => {
            const isDone = status !== "error" && (donePhases.has(step.key) || status === "completed");
            const isActive = !isDone && activePhase === step.key && status === "running";
            const isErrored = status === "error" && error?.phase === step.key;

            return (
              <div
                key={step.key}
                className={[
                  "rounded-[1.25rem] border p-4 transition",
                  isErrored
                    ? "border-red-300/50 bg-red-500/5 dark:border-red-400/30 dark:bg-red-500/10"
                    : isActive
                      ? "border-sky-300/50 bg-sky-500/5 dark:border-sky-400/30 dark:bg-sky-500/10"
                      : isDone
                        ? "border-emerald-300/40 bg-emerald-500/5 dark:border-emerald-400/20 dark:bg-emerald-500/5"
                        : "border-slate-200/70 bg-slate-50/50 dark:border-white/10 dark:bg-white/[0.02]",
                ].join(" ")}
              >
                <div className="flex items-center gap-3">
                  {isErrored ? (
                    <AlertCircle className="h-5 w-5 shrink-0 text-red-500" />
                  ) : isDone ? (
                    <CheckCircle2 className="h-5 w-5 shrink-0 text-emerald-500" />
                  ) : isActive ? (
                    <Loader2 className="h-5 w-5 shrink-0 animate-spin text-sky-500" />
                  ) : (
                    <Circle className="h-5 w-5 shrink-0 text-slate-300 dark:text-white/15" />
                  )}
                  <p
                    className={[
                      "text-sm font-medium",
                      isErrored
                        ? "text-red-600 dark:text-red-300"
                        : isDone
                          ? "text-emerald-700 dark:text-emerald-300"
                          : isActive
                            ? "text-sky-700 dark:text-sky-200"
                            : "text-slate-400 dark:text-slate-500",
                    ].join(" ")}
                  >
                    {step.label}
                  </p>
                </div>

                {isActive ? (
                  <div className="mt-3 pl-8">
                    <div className="h-1.5 w-full overflow-hidden rounded-full bg-sky-100 dark:bg-white/10">
                      <div
                        className="h-full rounded-full bg-sky-500 transition-[width] duration-200"
                        style={{ width: `${phasePercent}%` }}
                      />
                    </div>
                    <p className="mt-1.5 truncate text-xs text-slate-500 dark:text-slate-400">
                      {phaseMessage}
                    </p>
                  </div>
                ) : null}

                {isErrored ? (
                  <p className="mt-2 pl-8 text-xs text-red-500">{error?.message}</p>
                ) : null}
              </div>
            );
          })}
        </div>
      ) : null}

      {status === "error" && error && !STEPS.some((step) => step.key === error.phase) ? (
        <div className="mt-3 rounded-[1.25rem] border border-red-300/50 bg-red-500/5 p-4 dark:border-red-400/30 dark:bg-red-500/10">
          <div className="flex items-center gap-2">
            <AlertCircle className="h-5 w-5 text-red-500" />
            <p className="text-sm font-semibold text-red-600 dark:text-red-300">Import failed</p>
          </div>
          <p className="mt-1.5 text-xs text-red-500">{error.message}</p>
        </div>
      ) : null}

      {status === "completed" && summary ? (
        <div className="mt-5 rounded-[1.25rem] border border-emerald-300/40 bg-emerald-500/5 p-4 dark:border-emerald-400/20 dark:bg-emerald-500/5">
          <div className="flex items-center gap-2">
            <CheckCircle2 className="h-5 w-5 text-emerald-500" />
            <p className="text-sm font-semibold text-emerald-700 dark:text-emerald-300">Import complete</p>
          </div>
          <div className="mt-3 grid grid-cols-2 gap-3 text-xs sm:grid-cols-3">
            <Stat label="Files extracted" value={summary.extracted_entries} />
            <Stat label="Memories matched" value={summary.matched} />
            <Stat label="Assets inserted" value={summary.assets_inserted} />
            <Stat label="Memory items" value={summary.memory_items_inserted} />
            <Stat label="Timestamps repaired" value={summary.files_timestamp_repaired} />
            <Stat label="Unmatched files" value={summary.unmatched_files} />
          </div>
        </div>
      ) : null}
    </div>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-[0.9rem] border border-slate-200/70 bg-white/60 px-3 py-2 dark:border-white/10 dark:bg-white/[0.03]">
      <p className="text-[10px] font-medium uppercase tracking-wide text-slate-400 dark:text-slate-500">
        {label}
      </p>
      <p className="mt-0.5 text-base font-semibold text-slate-900 dark:text-white">
        {value.toLocaleString()}
      </p>
    </div>
  );
}
