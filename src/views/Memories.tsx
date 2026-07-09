import { useState } from "react";

import Lightbox from "../components/Lightbox";
import MemoriesToolbar from "../components/memories/MemoriesToolbar";
import TagEditorModal from "../components/memories/TagEditorModal";
import VirtualTimelineGrid from "../components/VirtualTimelineGrid";
import { useMemories, useMemoryTags, type MemoryAsset, type MemoryFilter, type MemorySort } from "../hooks/useMemories";

export default function Memories() {
  const [sort, setSort] = useState<MemorySort>("desc");
  const [filter, setFilter] = useState<MemoryFilter>("all");
  const [tag, setTag] = useState<string | null>(null);
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
  const { tags: availableTags, refresh: refreshTags } = useMemoryTags();

  return (
    <section className="flex h-full min-h-0 w-full flex-col gap-4 overflow-hidden">
      <MemoriesToolbar
        sort={sort}
        filter={filter}
        tag={tag}
        availableTags={availableTags}
        total={total}
        isLoading={isLoading}
        onSortChange={setSort}
        onFilterChange={setFilter}
        onTagChange={setTag}
      />

      {error ? (
        <div className="rounded-[1.25rem] border border-rose-300/40 bg-rose-50 px-4 py-3 text-sm text-rose-700 dark:border-rose-400/20 dark:bg-rose-400/10 dark:text-rose-100">
          {error}
        </div>
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
