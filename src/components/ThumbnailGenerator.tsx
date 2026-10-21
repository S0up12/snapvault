import { AlertCircle, CheckCircle2, ImageIcon, Loader2 } from "lucide-react";

import MediaJobControls from "./MediaJobControls";
import Stat from "./Stat";
import { startThumbnails, useThumbnailJob } from "../lib/jobStore";

export default function ThumbnailGenerator() {
  const { status, message, percent, processed, total, summary, error, backendBusy, mediaState } =
    useThumbnailJob();
  const active = status === "running" || status === "paused";
  const busy = active || backendBusy;

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
        onClick={() => void startThumbnails()}
        disabled={busy}
        className="btn btn-primary mt-4 disabled:cursor-not-allowed"
      >
        {status === "running" ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <ImageIcon className="h-4 w-4" />
        )}
        {status === "running" ? "Processing..." : "Reprocess media"}
      </button>

      {backendBusy && !active ? (
        <p className="mt-3 text-xs text-neutral-400">Media processing is running in the background</p>
      ) : null}

      <MediaJobControls mediaState={mediaState} />

      {active ? (
        <div className="mt-5">
          <div className="h-[5px] w-full overflow-hidden rounded-full bg-neutral-800">
            <div className="h-full rounded-full bg-accent transition-[width] duration-200" style={{ width: `${percent}%` }} />
          </div>
          <p className="mt-1.5 truncate text-xs text-neutral-400">
            {status === "paused" ? "Paused - " : ""}
            {total > 0 ? `${processed} / ${total} ` : ""}
            {message}
          </p>
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

      {status === "cancelled" ? (
        <p className="mt-5 text-sm text-neutral-300">
          Cancelled - {summary ? summary.generated + summary.failed : processed} processed, remaining stay queued
        </p>
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
