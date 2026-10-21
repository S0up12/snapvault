import { confirm } from "@tauri-apps/plugin-dialog";
import { Pause, Play, X } from "lucide-react";

import { cancelMedia, pauseMedia, resumeMedia, type MediaState } from "../lib/jobStore";

async function handleCancel() {
  const ok = await confirm("Files already processed are kept; the rest stay queued for next time.", {
    title: "Cancel media processing?",
    kind: "warning",
  });
  if (ok) void cancelMedia();
}

// Shown while media processing is active (idle renders nothing).
export default function MediaJobControls({ mediaState }: { mediaState: MediaState }) {
  if (mediaState === "idle") return null;
  const cancelling = mediaState === "cancelling";
  return (
    <div className="mt-3">
      <div className="flex gap-2">
        {mediaState === "paused" ? (
          <button type="button" onClick={() => void resumeMedia()} className="btn btn-secondary">
            <Play className="h-4 w-4" /> Resume
          </button>
        ) : (
          <button type="button" onClick={() => void pauseMedia()} disabled={cancelling} className="btn btn-secondary disabled:cursor-not-allowed">
            <Pause className="h-4 w-4" /> Pause
          </button>
        )}
        <button type="button" onClick={() => void handleCancel()} disabled={cancelling} className="btn btn-secondary disabled:cursor-not-allowed">
          <X className="h-4 w-4" /> {cancelling ? "Cancelling..." : "Cancel"}
        </button>
      </div>
      <p className="mt-1.5 text-[11px] text-neutral-500">
        {mediaState === "paused" ? "Paused. " : ""}Pause and cancel take effect after the current file.
      </p>
    </div>
  );
}
