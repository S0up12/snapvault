import { defaultRangeExtractor, useVirtualizer } from "@tanstack/react-virtual";
import { ImageIcon, LoaderCircle } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { formatDayGroup, type MemoryAsset } from "../hooks/useMemories";
import TimelineTile from "./memories/TimelineTile";

type GridRow =
  | { type: "header"; key: string; label: string; shortLabel: string; count: number }
  | { type: "assets"; key: string; items: Array<{ asset: MemoryAsset; index: number }> };

type VirtualTimelineGridProps = {
  assets: MemoryAsset[];
  total: number;
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
  isInitialLoading: boolean;
  fetchNextPage: () => Promise<void>;
  onOpenAsset: (index: number) => void;
  onToggleFavorite: (asset: MemoryAsset) => void;
  onEditTags: (asset: MemoryAsset) => void;
};

const GRID_GAP = 14;
const PORTRAIT_RATIO = 16 / 9;
const HEADER_ROW_HEIGHT = 60;
const MIN_TILE_WIDTH = 160;
const SCROLL_FETCH_THRESHOLD = 1200;

export default function VirtualTimelineGrid({
  assets,
  total,
  hasNextPage,
  isFetchingNextPage,
  isInitialLoading,
  fetchNextPage,
  onOpenAsset,
  onToggleFavorite,
  onEditTags,
}: VirtualTimelineGridProps) {
  const [scrollElement, setScrollElement] = useState<HTMLDivElement | null>(null);
  const [containerWidth, setContainerWidth] = useState(0);
  const activeStickyIndexRef = useRef(0);

  const handleScrollElementRef = useCallback((node: HTMLDivElement | null) => {
    setScrollElement(node);
  }, []);

  useEffect(() => {
    if (!scrollElement) {
      return;
    }
    const observer = new ResizeObserver(() => setContainerWidth(scrollElement.clientWidth));
    setContainerWidth(scrollElement.clientWidth);
    observer.observe(scrollElement);
    return () => observer.disconnect();
  }, [scrollElement]);

  const columnCount = Math.max(2, Math.floor((Math.max(containerWidth, 320) + GRID_GAP) / (MIN_TILE_WIDTH + GRID_GAP)));
  const tileWidth = Math.max(132, Math.floor((Math.max(containerWidth, 320) - GRID_GAP * (columnCount - 1)) / columnCount));
  const tileHeight = Math.max(180, Math.floor(tileWidth * PORTRAIT_RATIO));

  const { rows, stickyIndexes } = useMemo(() => {
    const grouped = new Map<string, { label: string; shortLabel: string; items: Array<{ asset: MemoryAsset; index: number }> }>();
    for (const [index, asset] of assets.entries()) {
      const formatted = formatDayGroup(asset.taken_at);
      const group = grouped.get(formatted.key) ?? { label: formatted.label, shortLabel: formatted.shortLabel, items: [] };
      group.items.push({ asset, index });
      grouped.set(formatted.key, group);
    }

    const nextRows: GridRow[] = [];
    const nextStickyIndexes: number[] = [];
    for (const [groupKey, group] of grouped.entries()) {
      nextStickyIndexes.push(nextRows.length);
      nextRows.push({ type: "header", key: `${groupKey}-header`, label: group.label, shortLabel: group.shortLabel, count: group.items.length });
      for (let index = 0; index < group.items.length; index += columnCount) {
        nextRows.push({ type: "assets", key: `${groupKey}-row-${index}`, items: group.items.slice(index, index + columnCount) });
      }
    }
    return { rows: nextRows, stickyIndexes: nextStickyIndexes };
  }, [assets, columnCount]);

  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollElement,
    getItemKey: (index) => rows[index]?.key ?? index,
    estimateSize: (index) => (rows[index]?.type === "header" ? HEADER_ROW_HEIGHT : tileHeight + GRID_GAP),
    overscan: 12,
    rangeExtractor: (range) => {
      const activeStickyIndex = [...stickyIndexes].reverse().find((index) => range.startIndex >= index) ?? stickyIndexes[0] ?? 0;
      activeStickyIndexRef.current = activeStickyIndex;
      return [...new Set([activeStickyIndex, ...defaultRangeExtractor(range)])].sort((a, b) => a - b);
    },
  });

  useEffect(() => {
    if (!scrollElement) {
      return;
    }
    const handleScroll = () => {
      const remaining = scrollElement.scrollHeight - scrollElement.scrollTop - scrollElement.clientHeight;
      if (hasNextPage && !isFetchingNextPage && remaining <= SCROLL_FETCH_THRESHOLD) {
        void fetchNextPage();
      }
    };
    handleScroll();
    scrollElement.addEventListener("scroll", handleScroll, { passive: true });
    return () => scrollElement.removeEventListener("scroll", handleScroll);
  }, [fetchNextPage, hasNextPage, isFetchingNextPage, scrollElement]);

  if (isInitialLoading) {
    return (
      <div className="flex h-full flex-1 items-center justify-center text-slate-400 dark:text-slate-500">
        <LoaderCircle className="h-6 w-6 animate-spin" />
      </div>
    );
  }

  if (assets.length === 0) {
    return (
      <div className="flex h-full flex-1 flex-col items-center justify-center gap-3 text-slate-400 dark:text-slate-500">
        <ImageIcon className="h-8 w-8" />
        <p className="text-sm">No memories yet. Import a Snapchat export and generate thumbnails first.</p>
      </div>
    );
  }

  return (
    <div className="min-h-0 flex-1">
      <div
        ref={handleScrollElementRef}
        className="relative h-full min-h-0 overflow-auto overscroll-contain rounded-[1.65rem] border border-slate-200/80 bg-white p-3 shadow-[0_24px_60px_rgba(15,23,42,0.08)] [scrollbar-gutter:stable] dark:border-white/10 dark:bg-slate-950/55 dark:shadow-black/20 sm:p-4"
      >
        <div style={{ height: rowVirtualizer.getTotalSize(), position: "relative", width: "100%" }}>
          {rowVirtualizer.getVirtualItems().map((virtualRow) => {
            const row = rows[virtualRow.index];
            if (!row) {
              return null;
            }

            const style =
              row.type === "header" && activeStickyIndexRef.current === virtualRow.index
                ? { position: "sticky" as const, top: 0, zIndex: 20, width: "100%" }
                : { position: "absolute" as const, left: 0, top: 0, width: "100%", willChange: "transform", transform: `translateY(${virtualRow.start}px)` };

            return (
              <div key={row.key} data-index={virtualRow.index} style={style}>
                {row.type === "header" ? (
                  <div className="pb-[14px] pt-1">
                    <div className="flex items-center justify-between rounded-2xl border border-slate-200/80 bg-[linear-gradient(180deg,rgba(255,255,255,0.96),rgba(247,250,252,0.94))] px-4 py-2.5 shadow-[0_10px_28px_rgba(15,23,42,0.08)] backdrop-blur dark:border-white/10 dark:bg-[linear-gradient(180deg,rgba(8,14,24,0.96),rgba(4,8,14,0.92))] dark:shadow-black/20">
                      <p className="text-sm font-medium text-slate-950 dark:text-white">{row.label}</p>
                      <span className="rounded-full border border-slate-200/80 bg-slate-50 px-3 py-1 text-xs uppercase tracking-[0.18em] text-slate-500 dark:border-white/10 dark:bg-white/[0.04] dark:text-slate-400">
                        {row.count}
                      </span>
                    </div>
                  </div>
                ) : (
                  <div className="grid pb-[14px]" style={{ gap: GRID_GAP, gridTemplateColumns: `repeat(${columnCount}, minmax(0, 1fr))` }}>
                    {row.items.map(({ asset, index }) => (
                      <TimelineTile
                        key={asset.id}
                        asset={asset}
                        index={index}
                        width={tileWidth}
                        height={tileHeight}
                        onOpen={onOpenAsset}
                        onToggleFavorite={onToggleFavorite}
                        onEditTags={onEditTags}
                      />
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>

        {isFetchingNextPage ? (
          <div className="pointer-events-none absolute inset-x-0 bottom-0 flex justify-center px-3 pb-3">
            <div className="inline-flex items-center gap-2 rounded-full border border-slate-200 bg-white/90 px-4 py-2 text-xs uppercase tracking-[0.22em] text-slate-700 shadow-lg shadow-slate-900/5 backdrop-blur dark:border-white/10 dark:bg-slate-950/80 dark:text-slate-300 dark:shadow-black/20">
              <LoaderCircle className="h-4 w-4 animate-spin" />
              Loading next page
            </div>
          </div>
        ) : !hasNextPage && total > 0 ? (
          <div className="pointer-events-none absolute inset-x-0 bottom-0 flex justify-center px-3 pb-3">
            <div className="inline-flex items-center gap-2 rounded-full border border-slate-200 bg-white/90 px-4 py-2 text-xs uppercase tracking-[0.22em] text-slate-500 shadow-lg shadow-slate-900/5 backdrop-blur dark:border-white/10 dark:bg-slate-950/80 dark:text-slate-500 dark:shadow-black/20">
              End of timeline
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
