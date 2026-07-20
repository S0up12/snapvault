import { useState } from "react";
import { useOutletContext } from "react-router-dom";

import Lightbox from "../components/Lightbox";
import type { LayoutOutletContext } from "../components/Layout";
import TagEditorModal from "../components/memories/TagEditorModal";
import VirtualTimelineGrid from "../components/VirtualTimelineGrid";
import { useMemories, type MemoryAsset } from "../hooks/useMemories";

export default function Memories() {
  const { memoriesFilters } = useOutletContext<LayoutOutletContext>();
  const { sort, filter, tag, availableTags, refreshTags } = memoriesFilters;
  const [selectedIndex, setSelectedIndex] = useState<number | null>(null);
  const [tagEditorAsset, setTagEditorAsset] = useState<MemoryAsset | null>(null);

  const {
    assets,
    total,
    isLoading,
    isFetchingNextPage,
    hasNextPage,
    error,
    fetchNextPage,
    toggleFavorite,
    updateTags,
  } = useMemories(sort, filter, tag);

  return (
    <section className="flex h-full min-h-0 w-full flex-col gap-3 overflow-hidden">
      <p className="px-1 text-xs uppercase tracking-[0.22em] text-neutral-500">
        {isLoading ? "Loading..." : `${total.toLocaleString()} ${total === 1 ? "memory" : "memories"}`}
      </p>

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
          onToggleFavorite={(asset) => {
            void toggleFavorite(asset);
          }}
          onEditTags={setTagEditorAsset}
        />
      )}

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
          onToggleFavorite={(asset) => {
            void toggleFavorite(asset);
          }}
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
