import { useEffect, useState } from "react";

import { invokeLogged } from "../lib/ipc";

type Result = { id: string; path: string | null; error: string | null };

// Show the "converting" UI only if resolving takes longer than this, so an
// already-playable video never flashes a spinner.
const SPINNER_DELAY_MS = 200;

/**
 * For a video without a playback copy, asks the backend to make one (or return
 * the original if it already plays). Results for a previous asset are ignored.
 * Pass null when no resolution is needed.
 */
export function useEnsurePlayback(assetId: string | null) {
  const [result, setResult] = useState<Result | null>(null);
  const [slowId, setSlowId] = useState<string | null>(null);

  useEffect(() => {
    if (!assetId) return;
    let stale = false;
    const timer = window.setTimeout(() => setSlowId(assetId), SPINNER_DELAY_MS);
    invokeLogged<{ path: string; converted: boolean }>("ensure_playback", { assetId })
      .then((r) => !stale && setResult({ id: assetId, path: r.path, error: null }))
      .catch((err) => !stale && setResult({ id: assetId, path: null, error: String(err) }))
      .finally(() => window.clearTimeout(timer));
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [assetId]);

  const current = result && result.id === assetId ? result : null;
  return {
    path: current?.path ?? null,
    error: current?.error ?? null,
    converting: !!assetId && !current && slowId === assetId,
  };
}
