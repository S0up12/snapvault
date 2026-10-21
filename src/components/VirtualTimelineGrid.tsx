import { defaultRangeExtractor, useVirtualizer } from "@tanstack/react-virtual";
import { CalendarDays, ImageIcon, LoaderCircle } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { formatDayGroup, yearFromTakenAt, type MemoryAsset } from "../hooks/useMemories";
import TimelineTile from "./memories/TimelineTile";

type GridRow =
  | { type: "header"; key: string; groupKey: string; label: string; shortLabel: string; count: number }
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
  /** Fixed tile columns from Settings; 0/undefined falls back to the responsive auto-fit. */
  fixedColumns?: number;
  /** Fired as the topmost visible row changes, so a timeline scrubber can track "where am I". */
  onVisibleYearChange?: (year: number | null) => void;
  /** Bump this after jumping the grid to a new offset, to scroll back to the top. */
  resetScrollKey?: number;
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
  fixedColumns,
  onVisibleYearChange,
  resetScrollKey,
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

  const columnCount =
    fixedColumns && fixedColumns > 0
      ? fixedColumns
      : Math.max(2, Math.floor((Math.max(containerWidth, 320) + GRID_GAP) / (MIN_TILE_WIDTH + GRID_GAP)));
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
      nextRows.push({
        type: "header",
        key: `${groupKey}-header`,
        groupKey,
        label: group.label,
        shortLabel: group.shortLabel,
        count: group.items.length,
      });
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

  // Latest values live in a ref so the scroll listener attaches once per element.
  const scrollStateRef = useRef({ check: () => {} });
  const lastYearRef = useRef<number | null | undefined>(undefined);

  scrollStateRef.current.check = () => {
    if (!scrollElement) {
      return;
    }
    {
      const remaining = scrollElement.scrollHeight - scrollElement.scrollTop - scrollElement.clientHeight;
      if (hasNextPage && !isFetchingNextPage && remaining <= SCROLL_FETCH_THRESHOLD) {
        void fetchNextPage();
      }

      if (onVisibleYearChange) {
        const topItem = rowVirtualizer
          .getVirtualItems()
          .find((item) => item.start + item.size > scrollElement.scrollTop);
        const row = topItem ? rows[topItem.index] : undefined;
        const year = !row
          ? null
          : row.type === "header"
            ? yearFromTakenAt(row.groupKey)
            : yearFromTakenAt(row.items[0]?.asset.taken_at ?? null);
        if (year !== lastYearRef.current) {
          lastYearRef.current = year;
          onVisibleYearChange(year);
        }
      }
    }
  };

  // Re-check when data/layout changes (may need next page or a new year label)...
  useEffect(() => {
    scrollStateRef.current.check();
  }, [fetchNextPage, hasNextPage, isFetchingNextPage, onVisibleYearChange, rowVirtualizer, rows, scrollElement]);

  // ...and on scroll, rAF-throttled.
  useEffect(() => {
    if (!scrollElement) {
      return;
    }
    let frame = 0;
    const handleScroll = () => {
      if (frame) {
        return;
      }
      frame = requestAnimationFrame(() => {
        frame = 0;
        scrollStateRef.current.check();
      });
    };
    scrollElement.addEventListener("scroll", handleScroll, { passive: true });
    return () => {
      cancelAnimationFrame(frame);
      scrollElement.removeEventListener("scroll", handleScroll);
    };
  }, [scrollElement]);

  useEffect(() => {
    scrollElement?.scrollTo({ top: 0 });
  }, [resetScrollKey, scrollElement]);

  if (isInitialLoading) {
    return (
      <div className="flex h-full flex-1 items-center justify-center text-neutral-500">
        <LoaderCircle className="h-6 w-6 animate-spin" />
      </div>
    );
  }

  if (assets.length === 0) {
    return (
      <div className="flex h-full flex-1 flex-col items-center justify-center gap-3 text-neutral-500">
        <ImageIcon className="h-8 w-8" />
        <p className="text-sm">No memories yet. Import a Snapchat export and generate thumbnails first.</p>
      </div>
    );
  }

  return (
    <div className="relative min-h-0 flex-1">
      <div
        ref={handleScrollElementRef}
        className="relative h-full min-h-0 overflow-auto overscroll-contain [scrollbar-gutter:stable]"
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
                  <div className="flex justify-center pb-[14px] pt-1">
                    <div className="flex items-center gap-2 rounded-full bg-surface/82 px-4 py-2 text-xs shadow-[0_0_0_1px_var(--color-divider),0_8px_24px_rgba(0,0,0,0.4)] backdrop-blur">
                      <CalendarDays className="h-3.5 w-3.5 text-accent" />
                      <p className="font-medium text-fg">{row.label}</p>
                      <span className="text-neutral-500">· {row.count}</span>
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
            <div className="inline-flex items-center gap-2 rounded-full bg-bg/85 px-4 py-2 text-xs uppercase tracking-[0.22em] text-neutral-300 ring-1 ring-divider backdrop-blur">
              <LoaderCircle className="h-4 w-4 animate-spin" />
              Loading next page
            </div>
          </div>
        ) : !hasNextPage && total > 0 ? (
          <div className="pointer-events-none absolute inset-x-0 bottom-0 flex justify-center px-3 pb-3">
            <div className="inline-flex items-center gap-2 rounded-full bg-bg/85 px-4 py-2 text-xs uppercase tracking-[0.22em] text-neutral-500 ring-1 ring-divider backdrop-blur">
              End of timeline
            </div>
          </div>
        ) : null}
      </div>

      <div className="pointer-events-none absolute inset-x-0 top-0 h-[60px] bg-gradient-to-b from-bg to-transparent" />
      <div className="pointer-events-none absolute inset-x-0 bottom-0 h-[120px] bg-gradient-to-t from-bg to-transparent" />
    </div>
  );
}
