import { convertFileSrc } from "@tauri-apps/api/core";
import { Clapperboard, ImageIcon, Star, Tags } from "lucide-react";
import { memo } from "react";

import type { MemoryAsset } from "../../hooks/useMemories";

const MEDIA_TYPE_ICONS = {
  image: ImageIcon,
  video: Clapperboard,
  audio: ImageIcon,
} as const;

export default memo(function TimelineTile({
  asset,
  index,
  width,
  height,
  onOpen,
  onToggleFavorite,
  onEditTags,
}: {
  asset: MemoryAsset;
  index: number;
  width: number;
  height: number;
  onOpen: (index: number) => void;
  onToggleFavorite: (asset: MemoryAsset) => void;
  onEditTags: (asset: MemoryAsset) => void;
}) {
  const MediaTypeIcon = MEDIA_TYPE_ICONS[asset.media_type];
  const thumbnailUrl = asset.thumbnail_path ? convertFileSrc(asset.thumbnail_path) : null;

  return (
    <div
      className="group relative overflow-hidden rounded-[1.35rem] border border-slate-200/70 bg-white shadow-[0_18px_40px_rgba(15,23,42,0.08)] transition hover:-translate-y-0.5 hover:border-sky-300/30 hover:shadow-[0_24px_60px_rgba(15,23,42,0.14)] dark:border-white/10 dark:bg-slate-950 dark:shadow-black/25"
      style={{ width, height }}
    >
      <button type="button" onClick={() => onOpen(index)} className="block h-full w-full" aria-label={`Open ${asset.media_type}`}>
        {thumbnailUrl ? (
          <img src={thumbnailUrl} alt="" loading="lazy" decoding="async" className="h-full w-full object-cover" />
        ) : (
          <div className="flex h-full w-full items-center justify-center bg-slate-100 text-slate-400 dark:bg-white/[0.04] dark:text-slate-600">
            <MediaTypeIcon className="h-8 w-8" />
          </div>
        )}
        <div className="pointer-events-none absolute inset-0 bg-gradient-to-t from-black/70 via-black/10 to-transparent opacity-0 transition duration-200 group-hover:opacity-100" />
      </button>

      <div className="pointer-events-none absolute inset-x-0 bottom-0 z-10 flex items-center justify-between px-3 pb-3 opacity-0 transition duration-150 group-hover:opacity-100">
        <button
          type="button"
          onClick={(event) => {
            event.stopPropagation();
            onToggleFavorite(asset);
          }}
          className="pointer-events-auto p-1 text-white drop-shadow-[0_1px_2px_rgba(0,0,0,0.5)] transition"
          aria-label={asset.is_favorite ? "Remove from favorites" : "Add to favorites"}
          title={asset.is_favorite ? "Unfavorite" : "Favorite"}
        >
          <Star className={asset.is_favorite ? "h-4 w-4 fill-amber-300 text-amber-300" : "h-4 w-4 text-white/92"} />
        </button>

        <MediaTypeIcon className="h-4 w-4 text-white/92 drop-shadow-[0_1px_2px_rgba(0,0,0,0.45)]" />

        <button
          type="button"
          onClick={(event) => {
            event.stopPropagation();
            onEditTags(asset);
          }}
          className="pointer-events-auto p-1 text-white drop-shadow-[0_1px_2px_rgba(0,0,0,0.5)] transition"
          aria-label="Edit tags"
          title="Edit tags"
        >
          <Tags className="h-4 w-4 text-white/92" />
        </button>
      </div>
    </div>
  );
});
