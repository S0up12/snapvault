import { convertFileSrc } from "@tauri-apps/api/core";
import { ChevronLeft, ChevronRight, Star, Tags, X } from "lucide-react";
import { useEffect, useState } from "react";

import type { MemoryAsset } from "../hooks/useMemories";
import { formatDayGroup } from "../hooks/useMemories";

type LightboxProps = {
  assets: MemoryAsset[];
  currentIndex: number;
  onClose: () => void;
  onNavigate: (nextIndex: number) => void;
  onToggleFavorite: (asset: MemoryAsset) => void;
  onEditTags: (asset: MemoryAsset) => void;
};

const VIEWPORT_MARGIN = { width: 96, height: 220 };

export default function Lightbox({ assets, currentIndex, onClose, onNavigate, onToggleFavorite, onEditTags }: LightboxProps) {
  const asset = assets[currentIndex];
  const [mediaBox, setMediaBox] = useState<{ width: number; height: number } | null>(null);

  function fitMediaBox(naturalWidth: number, naturalHeight: number) {
    const maxWidth = Math.min(window.innerWidth * 0.96, 1400);
    const maxHeight = window.innerHeight - VIEWPORT_MARGIN.height;
    const scale = Math.min(maxWidth / naturalWidth, maxHeight / naturalHeight, 1);
    setMediaBox({ width: naturalWidth * scale, height: naturalHeight * scale });
  }

  useEffect(() => {
    setMediaBox(null);
  }, [asset?.id]);

  useEffect(() => {
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = previousOverflow;
    };
  }, []);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onClose();
      } else if (event.key === "ArrowLeft") {
        onNavigate(currentIndex - 1);
      } else if (event.key === "ArrowRight") {
        onNavigate(currentIndex + 1);
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [currentIndex, onClose, onNavigate]);

  if (!asset) {
    return null;
  }

  const isVideo = asset.media_type === "video";
  const mediaUrl = convertFileSrc(asset.playback_path ?? asset.original_path);
  const overlayUrl = asset.overlay_path ? convertFileSrc(asset.overlay_path) : null;
  const date = formatDayGroup(asset.taken_at);

  return (
    <div className="fixed inset-0 z-50 bg-black/95 p-3 sm:p-5" onClick={onClose}>
      <div
        className="relative flex h-full w-full flex-col overflow-hidden rounded-[2rem] border border-white/10 bg-[#060c14] shadow-2xl shadow-black/50"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-white/10 px-4 py-4 sm:px-6">
          <div>
            <p className="text-xs font-semibold uppercase tracking-[0.25em] text-slate-500">Viewer</p>
            <div className="mt-2 flex items-center gap-3">
              <span className="rounded-full border border-white/10 bg-white/[0.05] px-3 py-1 text-xs font-medium uppercase tracking-[0.18em] text-slate-100">
                {isVideo ? "Video" : "Photo"}
              </span>
              <p className="text-sm text-slate-300">{date.label}</p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => onEditTags(asset)}
              className="rounded-2xl border border-white/10 bg-white/[0.05] px-3 py-2 text-slate-200 transition hover:bg-white/[0.1]"
              title="Edit tags"
            >
              <Tags className="h-4.5 w-4.5" />
            </button>
            <button
              type="button"
              onClick={() => onToggleFavorite(asset)}
              className="rounded-2xl border border-white/10 bg-white/[0.05] px-3 py-2 text-slate-200 transition hover:bg-white/[0.1]"
              title={asset.is_favorite ? "Unfavorite" : "Favorite"}
            >
              <Star className={asset.is_favorite ? "h-4.5 w-4.5 fill-amber-300 text-amber-300" : "h-4.5 w-4.5"} />
            </button>
            <span className="hidden rounded-full border border-white/10 bg-white/[0.04] px-3 py-2 text-xs uppercase tracking-[0.2em] text-slate-400 sm:inline-flex">
              {currentIndex + 1} / {assets.length}
            </span>
            <button
              type="button"
              onClick={onClose}
              className="rounded-2xl border border-white/10 bg-white/[0.05] p-3 text-slate-200 transition hover:bg-white/[0.1]"
            >
              <X className="h-5 w-5" />
            </button>
          </div>
        </div>

        <div className="relative flex min-h-0 flex-1 items-center justify-center bg-[radial-gradient(circle_at_top,_rgba(24,38,59,0.42),_rgba(4,6,10,0.96)_60%)] px-3 py-4 sm:px-6">
          <button
            type="button"
            onClick={() => onNavigate(currentIndex - 1)}
            disabled={currentIndex === 0}
            className="absolute left-3 top-1/2 z-10 -translate-y-1/2 rounded-full border border-white/10 bg-black/50 p-3 text-white transition hover:bg-black/70 disabled:cursor-not-allowed disabled:opacity-40 sm:left-6"
          >
            <ChevronLeft className="h-5 w-5" />
          </button>

          <button
            type="button"
            onClick={() => onNavigate(currentIndex + 1)}
            disabled={currentIndex >= assets.length - 1}
            className="absolute right-3 top-1/2 z-10 -translate-y-1/2 rounded-full border border-white/10 bg-black/50 p-3 text-white transition hover:bg-black/70 disabled:cursor-not-allowed disabled:opacity-40 sm:right-6"
          >
            <ChevronRight className="h-5 w-5" />
          </button>

          <div className="relative flex h-full w-full min-h-0 min-w-0 max-w-[min(96vw,1400px)] items-center justify-center overflow-hidden">
            <div
              className="relative inline-grid max-h-full max-w-full min-h-0 min-w-0 place-items-center overflow-hidden rounded-[1.5rem]"
              style={
                mediaBox
                  ? { width: `${mediaBox.width}px`, height: `${mediaBox.height}px` }
                  : { maxWidth: "min(96vw, 1400px)", maxHeight: "calc(100vh - 220px)" }
              }
            >
              {isVideo ? (
                <video
                  key={asset.id}
                  src={mediaUrl}
                  controls
                  playsInline
                  preload="metadata"
                  autoPlay
                  onLoadedMetadata={(event) => {
                    const el = event.currentTarget;
                    if (el.videoWidth > 0 && el.videoHeight > 0) {
                      fitMediaBox(el.videoWidth, el.videoHeight);
                    }
                  }}
                  className="col-start-1 row-start-1 block h-full w-full rounded-[1.5rem] object-contain"
                />
              ) : (
                <img
                  key={asset.id}
                  src={mediaUrl}
                  alt={date.label}
                  onLoad={(event) => {
                    const el = event.currentTarget;
                    if (el.naturalWidth > 0 && el.naturalHeight > 0) {
                      fitMediaBox(el.naturalWidth, el.naturalHeight);
                    }
                  }}
                  className="col-start-1 row-start-1 block h-full w-full rounded-[1.5rem] object-contain"
                />
              )}
              {overlayUrl ? (
                <img
                  src={overlayUrl}
                  alt=""
                  aria-hidden="true"
                  className="pointer-events-none col-start-1 row-start-1 h-full w-full rounded-[1.5rem] object-fill"
                />
              ) : null}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
