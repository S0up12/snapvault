import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AlertCircle, CheckCircle2, ImageIcon, Loader2 } from "lucide-react";
import { useEffect, useState } from "react";

type ProgressEvent =
  | { status: "progress"; processed: number; total: number; percent: number; message: string }
  | { status: "completed"; summary: ThumbnailSummary }
  | { status: "error"; message: string };

type ThumbnailSummary = {
  total: number;
  generated: number;
  failed: number;
};

type Status = "idle" | "running" | "completed" | "error";

export default function ThumbnailGenerator() {
  const [status, setStatus] = useState<Status>("idle");
  const [message, setMessage] = useState("");
  const [percent, setPercent] = useState(0);
  const [summary, setSummary] = useState<ThumbnailSummary | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const unlisten = listen<ProgressEvent>("thumbnails://progress", (event) => {
      const payload = event.payload;
      if (payload.status === "progress") {
        setMessage(payload.message);
        setPercent(payload.percent);
      } else if (payload.status === "completed") {
        setSummary(payload.summary);
        setPercent(100);
        setStatus("completed");
      } else if (payload.status === "error") {
        setError(payload.message);
        setStatus("error");
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  async function handleGenerate() {
    setStatus("running");
    setMessage("Starting...");
    setPercent(0);
    setSummary(null);
    setError(null);

    try {
      const result = await invoke<ThumbnailSummary>("generate_thumbnails");
      setSummary(result);
      setPercent(100);
      setStatus("completed");
    } catch (err) {
      setStatus("error");
      setError(String(err));
    }
  }

  return (
    <div className="rounded-[1.75rem] border border-slate-200/80 bg-white/80 p-8 shadow-[0_20px_48px_rgba(15,23,42,0.08)] dark:border-white/10 dark:bg-white/[0.04]">
      <p className="text-[11px] font-semibold uppercase tracking-[0.28em] text-sky-700/70 dark:text-sky-200/65">
        Thumbnails
      </p>
      <p className="mt-2 text-lg font-semibold text-slate-950 dark:text-white">Generate thumbnails</p>
      <p className="mt-1 text-sm text-slate-600 dark:text-slate-300">
        Creates optimized previews for memories that don't have one yet. Required before the
        Memories grid can render images.
      </p>

      <button
        type="button"
        onClick={handleGenerate}
        disabled={status === "running"}
        className="mt-4 inline-flex items-center gap-2 rounded-[1rem] border border-sky-300/30 bg-sky-500/10 px-4 py-2 text-sm font-medium text-sky-700 transition hover:border-sky-400/45 hover:bg-sky-500/15 disabled:cursor-not-allowed disabled:opacity-50 dark:text-sky-200"
      >
        {status === "running" ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <ImageIcon className="h-4 w-4" />
        )}
        {status === "running" ? "Generating..." : "Generate thumbnails"}
      </button>

      {status === "running" ? (
        <div className="mt-5">
          <div className="h-1.5 w-full overflow-hidden rounded-full bg-sky-100 dark:bg-white/10">
            <div
              className="h-full rounded-full bg-sky-500 transition-[width] duration-200"
              style={{ width: `${percent}%` }}
            />
          </div>
          <p className="mt-1.5 truncate text-xs text-slate-500 dark:text-slate-400">{message}</p>
        </div>
      ) : null}

      {status === "error" && error ? (
        <div className="mt-5 rounded-[1.25rem] border border-red-300/50 bg-red-500/5 p-4 dark:border-red-400/30 dark:bg-red-500/10">
          <div className="flex items-center gap-2">
            <AlertCircle className="h-5 w-5 text-red-500" />
            <p className="text-sm font-semibold text-red-600 dark:text-red-300">Thumbnail generation failed</p>
          </div>
          <p className="mt-1.5 text-xs text-red-500">{error}</p>
        </div>
      ) : null}

      {status === "completed" && summary ? (
        <div className="mt-5 rounded-[1.25rem] border border-emerald-300/40 bg-emerald-500/5 p-4 dark:border-emerald-400/20 dark:bg-emerald-500/5">
          <div className="flex items-center gap-2">
            <CheckCircle2 className="h-5 w-5 text-emerald-500" />
            <p className="text-sm font-semibold text-emerald-700 dark:text-emerald-300">
              {summary.total === 0 ? "Nothing to generate" : "Thumbnails generated"}
            </p>
          </div>
          <div className="mt-3 grid grid-cols-3 gap-3 text-xs">
            <Stat label="Total" value={summary.total} />
            <Stat label="Generated" value={summary.generated} />
            <Stat label="Failed" value={summary.failed} />
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
