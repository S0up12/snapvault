import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";

type ProgressEvent =
  | { status: "started"; job_id: string; total_entries: number; destination: string }
  | {
      status: "progress";
      job_id: string;
      processed_entries: number;
      total_entries: number;
      current_entry: string;
      percent: number;
    }
  | {
      status: "completed";
      job_id: string;
      destination: string;
      extracted_entries: number;
      skipped_entries: number;
    }
  | { status: "error"; job_id: string; message: string };

type ExtractionSummary = {
  job_id: string;
  destination: string;
  extracted_entries: number;
  skipped_entries: number;
};

type Phase = "idle" | "extracting" | "completed" | "error";

export default function ImportFlow() {
  const [phase, setPhase] = useState<Phase>("idle");
  const [percent, setPercent] = useState(0);
  const [currentEntry, setCurrentEntry] = useState("");
  const [summary, setSummary] = useState<ExtractionSummary | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const unlisten = listen<ProgressEvent>("ingestion://progress", (event) => {
      const payload = event.payload;
      switch (payload.status) {
        case "started":
          setPercent(0);
          setCurrentEntry("");
          break;
        case "progress":
          setPercent(payload.percent);
          setCurrentEntry(payload.current_entry);
          break;
        case "completed":
          setPercent(100);
          setPhase("completed");
          setSummary({
            job_id: payload.job_id,
            destination: payload.destination,
            extracted_entries: payload.extracted_entries,
            skipped_entries: payload.skipped_entries,
          });
          break;
        case "error":
          setPhase("error");
          setError(payload.message);
          break;
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  async function handleSelectFile() {
    const selected = await open({
      title: "Select Snapchat export",
      filters: [{ name: "Snapchat export", extensions: ["zip"] }],
      multiple: false,
    });

    if (!selected || Array.isArray(selected)) {
      return;
    }

    setPhase("extracting");
    setPercent(0);
    setCurrentEntry("");
    setSummary(null);
    setError(null);

    try {
      await invoke<ExtractionSummary>("extract_snapchat_export", {
        archivePath: selected,
      });
    } catch (err) {
      setPhase("error");
      setError(String(err));
    }
  }

  return (
    <div className="rounded-[1.75rem] border border-slate-200/80 bg-white/80 p-8 shadow-[0_20px_48px_rgba(15,23,42,0.08)] dark:border-white/10 dark:bg-white/[0.04]">
      <p className="text-[11px] font-semibold uppercase tracking-[0.28em] text-sky-700/70 dark:text-sky-200/65">
        Import
      </p>
      <p className="mt-2 text-lg font-semibold text-slate-950 dark:text-white">Import a Snapchat export</p>
      <p className="mt-1 text-sm text-slate-600 dark:text-slate-300">
        Select the .zip file you downloaded from Snapchat's "My Data" export.
      </p>

      <button
        type="button"
        onClick={handleSelectFile}
        disabled={phase === "extracting"}
        className="mt-4 inline-flex items-center rounded-[1rem] border border-sky-300/30 bg-sky-500/10 px-4 py-2 text-sm font-medium text-sky-700 transition hover:border-sky-400/45 hover:bg-sky-500/15 disabled:cursor-not-allowed disabled:opacity-50 dark:text-sky-200"
      >
        {phase === "extracting" ? "Extracting..." : "Select Snapchat export (.zip)"}
      </button>

      {phase === "extracting" ? (
        <div className="mt-4">
          <div className="h-2 w-full overflow-hidden rounded-full bg-slate-200 dark:bg-white/10">
            <div
              className="h-full rounded-full bg-sky-500 transition-[width]"
              style={{ width: `${percent}%` }}
            />
          </div>
          <p className="mt-2 truncate text-xs text-slate-500 dark:text-slate-400">
            {percent.toFixed(0)}%{currentEntry ? ` - ${currentEntry}` : ""}
          </p>
        </div>
      ) : null}

      {phase === "completed" && summary ? (
        <p className="mt-4 text-sm text-emerald-600 dark:text-emerald-400">
          Extracted {summary.extracted_entries} files
          {summary.skipped_entries > 0 ? ` (skipped ${summary.skipped_entries} unsafe entries)` : ""} to{" "}
          {summary.destination}
        </p>
      ) : null}

      {phase === "error" && error ? (
        <p className="mt-4 text-sm text-red-500">Extraction failed: {error}</p>
      ) : null}
    </div>
  );
}
