import { useCallback, useState } from "react";
import { useOutletContext } from "react-router-dom";

import ExportDialog from "../components/ExportDialog";
import Lightbox from "../components/Lightbox";
import type { LayoutOutletContext } from "../components/Layout";
import MemoryTimelineScrubber from "../components/memories/MemoryTimelineScrubber";
import TagEditorModal from "../components/memories/TagEditorModal";
import VirtualTimelineGrid from "../components/VirtualTimelineGrid";
import { useMemories, useMemoryYears, type MemoryAsset } from "../hooks/useMemories";
import { useViewerSettings } from "../hooks/useViewerSettings";

export default function Memories() {
  const { memoriesFilters } = useOutletContext<LayoutOutletContext>();
  const { sort, filter, tag, year, availableTags, refreshTags } = memoriesFilters;
  const [selectedIndex, setSelectedIndex] = useState<number | null>(null);
  const [tagEditorAsset, setTagEditorAsset] = useState<MemoryAsset | null>(null);
  const [visibleYear, setVisibleYear] = useState<number | null>(null);
  const [exportOpen, setExportOpen] = useState(false);

  const {
    assets,
    total,
    isLoading,
    isFetchingNextPage,
    hasNextPage,
    error,
    fetchNextPage,
    jumpToYear,
    jumpToken,
    toggleFavorite,
    updateTags,
  } = useMemories(sort, filter, tag, year);
  // Distinct from Layout's availableYears (which ignores the date filter so
  // the dropdown can always switch years) - the scrubber should reflect
  // whatever the grid is actually showing right now, filtered or not.
  const scrubberYears = useMemoryYears(filter, tag, year);
  const { settings: viewerSettings } = useViewerSettings();
  // Stable identity so memo(TimelineTile) survives Memories re-renders (e.g. visibleYear changes).
  const handleToggleFavorite = useCallback(
    (asset: MemoryAsset) => {
      void toggleFavorite(asset);
    },
    [toggleFavorite],
  );

  return (
    <section className="relative flex h-full min-h-0 w-full flex-col gap-3 overflow-hidden">
      <div className="flex items-center justify-between gap-3">
        <p className="px-1 text-xs uppercase tracking-[0.22em] text-neutral-500">
          {isLoading ? "Loading..." : `${total.toLocaleString()} ${total === 1 ? "memory" : "memories"}`}
        </p>
        <button type="button" onClick={() => setExportOpen(true)} className="btn btn-secondary">
          Export
        </button>
      </div>
      {exportOpen ? (
        <ExportDialog
          kind="memories"
          filter={filter}
          tag={tag}
          year={year}
          count={isLoading ? undefined : total}
          onClose={() => setExportOpen(false)}
        />
      ) : null}

      {error ? (
        <div className="rounded-md bg-red-500/8 px-4 py-3 text-sm text-red-300 ring-1 ring-red-400/25">{error}</div>
      ) : (
        <VirtualTimelineGrid
          assets={assets}
          total={total}
          hasNextPage={hasNextPage}
          isFetchingNextPage={isFetchingNextPage}
          isInitialLoading={isLoading}
          fetchNextPage={fetchNextPage}
          onOpenAsset={setSelectedIndex}
          onToggleFavorite={handleToggleFavorite}
          onEditTags={setTagEditorAsset}
          fixedColumns={viewerSettings?.grid_columns}
          onVisibleYearChange={setVisibleYear}
          resetScrollKey={jumpToken}
        />
      )}

      {!error ? (
        <MemoryTimelineScrubber
          years={scrubberYears}
          activeYear={visibleYear}
          onJump={(targetYear) => {
            void jumpToYear(targetYear);
          }}
        />
      ) : null}

      {selectedIndex !== null && assets[selectedIndex] ? (
        <Lightbox
          assets={assets}
          currentIndex={selectedIndex}
          onClose={() => setSelectedIndex(null)}
          onNavigate={(nextIndex) => {
            if (nextIndex >= 0 && nextIndex < assets.length) {
              setSelectedIndex(nextIndex);
            }
          }}
          onToggleFavorite={handleToggleFavorite}
          onEditTags={setTagEditorAsset}
        />
      ) : null}

      {tagEditorAsset ? (
        <TagEditorModal
          assetId={tagEditorAsset.id}
          initialTags={tagEditorAsset.tags}
          availableTags={availableTags}
          onClose={() => setTagEditorAsset(null)}
          onSave={async (tags) => {
            await updateTags(tagEditorAsset.id, tags);
            await refreshTags();
          }}
        />
      ) : null}
    </section>
  );
}
