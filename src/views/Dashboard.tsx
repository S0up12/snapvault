import { convertFileSrc } from "@tauri-apps/api/core";
import { Clapperboard } from "lucide-react";
import { Link } from "react-router-dom";

import ImportFlow from "../components/ImportFlow";
import { useLibraryStats } from "../hooks/useLibraryStats";
import { useMemories } from "../hooks/useMemories";
import { useProfile } from "../hooks/useProfile";

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(1)} ${units[unitIndex]}`;
}

function StatTile({ label, value, accent }: { label: string; value: string; accent?: boolean }) {
  return (
    <div className="rounded-md bg-surface px-4.5 py-4 ring-1 ring-divider">
      <div className={["text-[10px] tracking-[0.12em] uppercase", accent ? "text-accent" : "text-neutral-500"].join(" ")}>
        {label}
      </div>
      <div className="mt-2 text-[26px] font-medium tracking-[-0.02em]">{value}</div>
    </div>
  );
}

export default function Dashboard() {
  const { stats } = useLibraryStats();
  const { profile } = useProfile();
  const { assets: recent } = useMemories("desc", "all", null);

  return (
    <div className="mx-auto flex w-full max-w-[1400px] flex-col gap-6">
      <div>
        <h2 className="text-[26px] tracking-[-0.02em]">Welcome back</h2>
        <div className="mt-1.5 text-[11px] uppercase tracking-[0.16em] text-neutral-500">
          {stats ? `${stats.total_assets.toLocaleString()} assets imported` : "Loading library…"}
        </div>
      </div>

      <div className="grid grid-cols-2 gap-3.5 sm:grid-cols-3 xl:grid-cols-6">
        <StatTile label="Memories" value={stats ? stats.memory_items.toLocaleString() : "—"} accent />
        <StatTile label="Photos" value={stats ? stats.images.toLocaleString() : "—"} />
        <StatTile label="Videos" value={stats ? stats.videos.toLocaleString() : "—"} />
        <StatTile label="Chat threads" value={stats ? stats.chat_threads.toLocaleString() : "—"} />
        <StatTile
          label="Friends"
          value={profile ? profile.snapshot.friends.friends_count.toLocaleString() : "—"}
        />
        <StatTile label="Database" value={stats ? formatBytes(stats.db_size_bytes) : "—"} />
      </div>

      <div className="grid gap-5 xl:grid-cols-[1.15fr_1fr]">
        <ImportFlow />
        <RecentCard assets={recent.slice(0, 8)} />
      </div>
    </div>
  );
}

function RecentCard({ assets }: { assets: ReturnType<typeof useMemories>["assets"] }) {
  return (
    <div className="rounded-lg bg-surface p-5.5 ring-1 ring-divider">
      <div className="mb-3.5 flex items-baseline gap-2.5">
        <h3 className="text-[17px]">Recent</h3>
        <span className="flex-1" />
        <Link to="/memories" className="text-[12.5px] text-accent hover:underline">
          Open Memories →
        </Link>
      </div>
      {assets.length === 0 ? (
        <p className="text-sm text-neutral-500">No memories imported yet.</p>
      ) : (
        <div className="grid grid-cols-4 gap-2.5">
          {assets.map((asset) => (
            <div
              key={asset.id}
              className="relative aspect-3/4 overflow-hidden rounded-md bg-neutral-900 ring-1 ring-white/5"
            >
              {asset.thumbnail_path ? (
                <img
                  src={convertFileSrc(asset.thumbnail_path)}
                  alt=""
                  loading="lazy"
                  className="h-full w-full object-cover"
                />
              ) : null}
              <span className="pointer-events-none absolute inset-0 bg-gradient-to-t from-bg/40 to-transparent to-[42%]" />
              {asset.media_type === "video" ? (
                <span className="pointer-events-none absolute inset-0 flex items-center justify-center">
                  <span className="flex h-7.5 w-7.5 items-center justify-center rounded-full bg-bg/50 text-white">
                    <Clapperboard className="h-3.5 w-3.5" />
                  </span>
                </span>
              ) : null}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
