import { convertFileSrc } from "@tauri-apps/api/core";
import { ChevronLeft, ChevronRight, Star, Tags, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { formatDayGroup } from "../hooks/useMemories";
import { useViewerSettings } from "../hooks/useViewerSettings";

// Trackpads/mice fire many small wheel events per physical swipe - this
// gates how big a single event's deltaY must be to count as an intentional
// scroll, and how long to ignore further nav input after acting on one, so
// a single swipe/keypress moves exactly one item and the slide animation
// below has time to finish before the next one can start.
const WHEEL_THRESHOLD = 12;
const TOUCH_THRESHOLD = 50;
const SLIDE_ANIMATION_MS = 280;
const NAVIGATE_COOLDOWN_MS = 320;

type Axis = "vertical" | "horizontal";
type NavDirection = { axis: Axis; sign: 1 | -1 };
type MediaBox = { width: number; height: number } | null;

// The incoming item slides in from the side it "comes from"; the outgoing
// item keeps moving the same direction, off the opposite side - both moving
// together like a strip of film, rather than the outgoing item just
// vanishing in place.
const ENTER_ANIMATION: Record<Axis, Record<1 | -1, string>> = {
  vertical: { 1: "lightbox-enter-from-bottom", [-1]: "lightbox-enter-from-top" },
  horizontal: { 1: "lightbox-enter-from-right", [-1]: "lightbox-enter-from-left" },
};
const EXIT_ANIMATION: Record<Axis, Record<1 | -1, string>> = {
  vertical: { 1: "lightbox-exit-to-top", [-1]: "lightbox-exit-to-bottom" },
  horizontal: { 1: "lightbox-exit-to-left", [-1]: "lightbox-exit-to-right" },
};

// The fields Lightbox actually reads - Memories' MemoryAsset and Chats' chat
// media (adapted with the message's sent_at as taken_at) both satisfy this,
// so the same viewer works for either without either hook depending on the
// other's shape.
export type LightboxAsset = {
  id: string;
  media_type: "image" | "video" | "audio";
  original_path: string;
  overlay_path: string | null;
  playback_path: string | null;
  taken_at: string | null;
  is_favorite?: boolean;
};

type LightboxProps<T extends LightboxAsset> = {
  assets: T[];
  currentIndex: number;
  onClose: () => void;
  onNavigate: (nextIndex: number) => void;
  /** Omit where there's nothing sensible to favorite/tag (e.g. chat media) - the buttons only render when provided. */
  onToggleFavorite?: (asset: T) => void;
  onEditTags?: (asset: T) => void;
};

const VIEWPORT_MARGIN = { width: 96, height: 220 };

function sizeStyle(box: MediaBox): React.CSSProperties {
  return box
    ? { width: `${box.width}px`, height: `${box.height}px` }
    : { maxWidth: "min(96vw, 1400px)", maxHeight: "calc(100vh - 220px)" };
}

export default function Lightbox<T extends LightboxAsset>({
  assets,
  currentIndex,
  onClose,
  onNavigate,
  onToggleFavorite,
  onEditTags,
}: LightboxProps<T>) {
  const asset = assets[currentIndex];
  const [mediaBox, setMediaBox] = useState<MediaBox>(null);
  const { settings: viewerSettings, isLoading: viewerSettingsLoading } = useViewerSettings();
  // Defaults on while settings are still loading, so scroll navigation
  // behaves the same as soon as it's usable instead of waiting on an IPC
  // round-trip. Autoplay delay can't default this way - defaulting to 0
  // would autoplay instantly before the real (possibly nonzero) value
  // loads, so that effect below waits for the load instead.
  const scrollNavigationEnabled = viewerSettings?.vertical_scroll_navigation ?? true;
  const autoplayDelayMs = viewerSettings?.autoplay_delay_ms ?? 0;

  const [navDirection, setNavDirection] = useState<NavDirection | null>(null);
  const [outgoing, setOutgoing] = useState<{ asset: T; mediaBox: MediaBox; direction: NavDirection } | null>(null);
  const navLockRef = useRef(false);
  const touchStartYRef = useRef<number | null>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);

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

  // Autoplay the current video/voice message after the configured delay
  // (0 = immediately, matching the old unconditional autoPlay behavior).
  // Waits for settings to finish loading first - starting immediately on an
  // unloaded default would always autoplay instantly, ignoring whatever
  // delay is actually configured.
  useEffect(() => {
    if (viewerSettingsLoading) {
      return;
    }
    const el = asset?.media_type === "video" ? videoRef.current : asset?.media_type === "audio" ? audioRef.current : null;
    if (!el) {
      return;
    }
    if (autoplayDelayMs <= 0) {
      el.play().catch(() => {});
      return;
    }
    const timer = window.setTimeout(() => {
      el.play().catch(() => {});
    }, autoplayDelayMs);
    return () => window.clearTimeout(timer);
  }, [asset?.id, asset?.media_type, autoplayDelayMs, viewerSettingsLoading]);

  function triggerNavigate(sign: 1 | -1, axis: Axis) {
    if (navLockRef.current || !asset) {
      return;
    }
    navLockRef.current = true;
    const direction: NavDirection = { axis, sign };
    setNavDirection(direction);
    setOutgoing({ asset, mediaBox, direction });
    onNavigate(currentIndex + sign);
    window.setTimeout(() => setOutgoing(null), SLIDE_ANIMATION_MS);
    window.setTimeout(() => {
      navLockRef.current = false;
    }, NAVIGATE_COOLDOWN_MS);
  }

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onClose();
      } else if (event.key === "ArrowLeft") {
        triggerNavigate(-1, "horizontal");
      } else if (event.key === "ArrowRight") {
        triggerNavigate(1, "horizontal");
      } else if (scrollNavigationEnabled && event.key === "ArrowUp") {
        triggerNavigate(-1, "vertical");
      } else if (scrollNavigationEnabled && event.key === "ArrowDown") {
        triggerNavigate(1, "vertical");
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [currentIndex, onClose, onNavigate, scrollNavigationEnabled]);

  function handleWheel(event: React.WheelEvent) {
    if (!scrollNavigationEnabled || Math.abs(event.deltaY) < WHEEL_THRESHOLD) {
      return;
    }
    triggerNavigate(event.deltaY > 0 ? 1 : -1, "vertical");
  }

  function handleTouchStart(event: React.TouchEvent) {
    if (!scrollNavigationEnabled) {
      return;
    }
    touchStartYRef.current = event.touches[0]?.clientY ?? null;
  }

  function handleTouchEnd(event: React.TouchEvent) {
    const startY = touchStartYRef.current;
    touchStartYRef.current = null;
    if (!scrollNavigationEnabled || startY === null) {
      return;
    }
    const endY = event.changedTouches[0]?.clientY ?? startY;
    const delta = startY - endY;
    if (Math.abs(delta) < TOUCH_THRESHOLD) {
      return;
    }
    triggerNavigate(delta > 0 ? 1 : -1, "vertical");
  }

  if (!asset) {
    return null;
  }

  const isVideo = asset.media_type === "video";
  const isAudio = asset.media_type === "audio";
  const date = formatDayGroup(asset.taken_at);

  function renderMedia(item: T, live: boolean) {
    const itemIsVideo = item.media_type === "video";
    const itemIsAudio = item.media_type === "audio";
    const itemUrl = convertFileSrc(item.playback_path ?? item.original_path);
    const itemOverlayUrl = item.overlay_path ? convertFileSrc(item.overlay_path) : null;
    const itemLabel = formatDayGroup(item.taken_at).label;

    return (
      <>
        {itemIsVideo ? (
          <video
            key={item.id}
            ref={live ? videoRef : undefined}
            src={itemUrl}
            controls={live}
            playsInline
            preload="metadata"
            muted={!live}
            onLoadedMetadata={
              live
                ? (event) => {
                    const el = event.currentTarget;
                    if (el.videoWidth > 0 && el.videoHeight > 0) {
                      fitMediaBox(el.videoWidth, el.videoHeight);
                    }
                  }
                : undefined
            }
            className="col-start-1 row-start-1 block h-full w-full rounded-[1.5rem] object-contain"
          />
        ) : itemIsAudio ? (
          <div className="col-start-1 row-start-1 flex h-full w-full min-h-[10rem] min-w-[16rem] flex-col items-center justify-center gap-4 rounded-[1.5rem] bg-white/[0.03] p-8">
            {live ? (
              <audio ref={audioRef} key={item.id} src={itemUrl} controls preload="metadata" className="w-full max-w-sm" />
            ) : null}
          </div>
        ) : (
          <img
            key={item.id}
            src={itemUrl}
            alt={itemLabel}
            onLoad={
              live
                ? (event) => {
                    const el = event.currentTarget;
                    if (el.naturalWidth > 0 && el.naturalHeight > 0) {
                      fitMediaBox(el.naturalWidth, el.naturalHeight);
                    }
                  }
                : undefined
            }
            className="col-start-1 row-start-1 block h-full w-full rounded-[1.5rem] object-contain"
          />
        )}
        {itemOverlayUrl ? (
          <img
            src={itemOverlayUrl}
            alt=""
            aria-hidden="true"
            className="pointer-events-none col-start-1 row-start-1 h-full w-full rounded-[1.5rem] object-fill"
          />
        ) : null}
      </>
    );
  }

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
                {isVideo ? "Video" : isAudio ? "Voice message" : "Photo"}
              </span>
              <p className="text-sm text-slate-300">{date.label}</p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            {onEditTags ? (
              <button
                type="button"
                onClick={() => onEditTags(asset)}
                className="rounded-2xl border border-white/10 bg-white/[0.05] px-3 py-2 text-slate-200 transition hover:bg-white/[0.1]"
                title="Edit tags"
              >
                <Tags className="h-4.5 w-4.5" />
              </button>
            ) : null}
            {onToggleFavorite ? (
              <button
                type="button"
                onClick={() => onToggleFavorite(asset)}
                className="rounded-2xl border border-white/10 bg-white/[0.05] px-3 py-2 text-slate-200 transition hover:bg-white/[0.1]"
                title={asset.is_favorite ? "Unfavorite" : "Favorite"}
              >
                <Star className={asset.is_favorite ? "h-4.5 w-4.5 fill-amber-300 text-amber-300" : "h-4.5 w-4.5"} />
              </button>
            ) : null}
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

        <div
          className="relative flex min-h-0 flex-1 items-center justify-center bg-[radial-gradient(circle_at_top,_rgba(24,38,59,0.42),_rgba(4,6,10,0.96)_60%)] px-3 py-4 sm:px-6"
          onWheel={handleWheel}
          onTouchStart={handleTouchStart}
          onTouchEnd={handleTouchEnd}
        >
          <button
            type="button"
            onClick={() => triggerNavigate(-1, "horizontal")}
            disabled={currentIndex === 0}
            className="absolute left-3 top-1/2 z-10 -translate-y-1/2 rounded-full border border-white/10 bg-black/50 p-3 text-white transition hover:bg-black/70 disabled:cursor-not-allowed disabled:opacity-40 sm:left-6"
          >
            <ChevronLeft className="h-5 w-5" />
          </button>

          <button
            type="button"
            onClick={() => triggerNavigate(1, "horizontal")}
            disabled={currentIndex >= assets.length - 1}
            className="absolute right-3 top-1/2 z-10 -translate-y-1/2 rounded-full border border-white/10 bg-black/50 p-3 text-white transition hover:bg-black/70 disabled:cursor-not-allowed disabled:opacity-40 sm:right-6"
          >
            <ChevronRight className="h-5 w-5" />
          </button>

          <div className="relative flex h-full w-full min-h-0 min-w-0 max-w-[min(96vw,1400px)] items-center justify-center overflow-hidden">
            {outgoing ? (
              <div
                key={`exit-${outgoing.asset.id}`}
                className="pointer-events-none absolute inset-0 flex items-center justify-center"
                style={{ animation: `${EXIT_ANIMATION[outgoing.direction.axis][outgoing.direction.sign]} ${SLIDE_ANIMATION_MS}ms ease-in forwards` }}
              >
                <div
                  className="relative inline-grid max-h-full max-w-full min-h-0 min-w-0 place-items-center overflow-hidden rounded-[1.5rem]"
                  style={sizeStyle(outgoing.mediaBox)}
                >
                  {renderMedia(outgoing.asset, false)}
                </div>
              </div>
            ) : null}

            <div
              key={asset.id}
              className="absolute inset-0 flex items-center justify-center"
              style={navDirection ? { animation: `${ENTER_ANIMATION[navDirection.axis][navDirection.sign]} ${SLIDE_ANIMATION_MS}ms ease-out` } : undefined}
            >
              <div
                className="relative inline-grid max-h-full max-w-full min-h-0 min-w-0 place-items-center overflow-hidden rounded-[1.5rem]"
                style={sizeStyle(mediaBox)}
              >
                {renderMedia(asset, true)}
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
