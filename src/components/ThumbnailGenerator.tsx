import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AlertCircle, CheckCircle2, ImageIcon, Loader2 } from "lucide-react";
import { useEffect, useState } from "react";

import Stat from "./Stat";

type ProgressEvent =
  | { status: "progress"; processed: number; total: number; percent: number; message: string }
  | { status: "completed"; summary: ThumbnailSummary }
  | { status: "error"; message: string };

type ThumbnailSummary = {
  total: number;
  generated: number;
  failed: number;
  playback_transcoded: number;
  playback_failed: number;
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
    <div className="rounded-lg bg-surface p-6 ring-1 ring-divider">
      <p className="text-[10px] uppercase tracking-[0.14em] text-accent">Thumbnails</p>
      <p className="mt-2 text-lg font-medium">Reprocess media</p>
      <p className="mt-1 text-sm text-neutral-300">
        New imports generate thumbnails and convert incompatible videos (e.g. HEVC) automatically.
        Use this to backfill libraries imported before that existed, or to retry any that failed.
      </p>

      <button
        type="button"
        onClick={handleGenerate}
        disabled={status === "running"}
        className="btn btn-primary mt-4 disabled:cursor-not-allowed"
      >
        {status === "running" ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <ImageIcon className="h-4 w-4" />
        )}
        {status === "running" ? "Processing..." : "Reprocess media"}
      </button>

      {status === "running" ? (
        <div className="mt-5">
          <div className="h-[5px] w-full overflow-hidden rounded-full bg-neutral-800">
            <div className="h-full rounded-full bg-accent transition-[width] duration-200" style={{ width: `${percent}%` }} />
          </div>
          <p className="mt-1.5 truncate text-xs text-neutral-400">{message}</p>
        </div>
      ) : null}

      {status === "error" && error ? (
        <div className="mt-5 rounded-md bg-red-500/8 p-4 ring-1 ring-red-400/25">
          <div className="flex items-center gap-2">
            <AlertCircle className="h-5 w-5 text-red-400" />
            <p className="text-sm font-semibold text-red-300">Thumbnail generation failed</p>
          </div>
          <p className="mt-1.5 text-xs text-red-400">{error}</p>
        </div>
      ) : null}

      {status === "completed" && summary ? (
        <div className="mt-5 rounded-md bg-accent/6 p-4 ring-1 ring-accent/25">
          <div className="flex items-center gap-2">
            <CheckCircle2 className="h-5 w-5 text-accent" />
            <p className="text-sm font-semibold text-accent-200">
              {summary.total === 0 ? "Nothing to process" : "Media processed"}
            </p>
          </div>
          <div className="mt-3 grid grid-cols-3 gap-3 text-xs">
            <Stat label="Total" value={summary.total} />
            <Stat label="Thumbnails generated" value={summary.generated} />
            <Stat label="Thumbnail failures" value={summary.failed} />
            <Stat label="Videos converted" value={summary.playback_transcoded} />
            <Stat label="Conversion failures" value={summary.playback_failed} />
          </div>
        </div>
      ) : null}
    </div>
  );
}
