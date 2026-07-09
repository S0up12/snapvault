import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

import ImportFlow from "../components/ImportFlow";
import ThumbnailGenerator from "../components/ThumbnailGenerator";

export default function Dashboard() {
  const [assetCount, setAssetCount] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<number>("count_assets")
      .then(setAssetCount)
      .catch((err) => setError(String(err)));
  }, []);

  return (
    <div className="space-y-4">
      <ImportFlow />

      <ThumbnailGenerator />

      <div className="rounded-[1.75rem] border border-slate-200/80 bg-white/80 p-8 shadow-[0_20px_48px_rgba(15,23,42,0.08)] dark:border-white/10 dark:bg-white/[0.04]">
        <p className="text-[11px] font-semibold uppercase tracking-[0.28em] text-sky-700/70 dark:text-sky-200/65">
          Database
        </p>
        {error ? (
          <p className="mt-2 text-sm text-red-500">Failed to reach the database: {error}</p>
        ) : (
          <p className="mt-2 text-2xl font-semibold text-slate-950 dark:text-white">
            {assetCount === null ? "Loading..." : `${assetCount} assets`}
          </p>
        )}
      </div>
    </div>
  );
}
