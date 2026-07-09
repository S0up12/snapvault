import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";

export type MemoryAsset = {
  id: string;
  media_type: "image" | "video" | "audio";
  original_path: string;
  overlay_path: string | null;
  thumbnail_path: string | null;
  taken_at: string | null;
  is_favorite: boolean;
  tags: string[];
};

export type MemorySort = "desc" | "asc";
export type MemoryFilter = "all" | "photo" | "video" | "favorite";

type MemoriesPage = {
  assets: MemoryAsset[];
  total: number;
  offset: number;
};

const PAGE_SIZE = 120;

export function useMemories(sort: MemorySort, filter: MemoryFilter, tag: string | null) {
  const [assets, setAssets] = useState<MemoryAsset[]>([]);
  const [total, setTotal] = useState(0);
  const [isLoading, setIsLoading] = useState(true);
  const [isFetchingNextPage, setIsFetchingNextPage] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadPage = useCallback(
    async (offset: number) => {
      const page = await invoke<MemoriesPage>("list_memory_assets", { offset, limit: PAGE_SIZE, sort, filter, tag });
      setTotal(page.total);
      setAssets((current) => (offset === 0 ? page.assets : [...current, ...page.assets]));
    },
    [sort, filter, tag],
  );

  useEffect(() => {
    setIsLoading(true);
    setError(null);
    loadPage(0)
      .catch((err) => setError(String(err)))
      .finally(() => setIsLoading(false));
  }, [loadPage]);

  const hasNextPage = assets.length < total;

  const fetchNextPage = useCallback(async () => {
    if (isFetchingNextPage || !hasNextPage) {
      return;
    }
    setIsFetchingNextPage(true);
    try {
      await loadPage(assets.length);
    } catch (err) {
      setError(String(err));
    } finally {
      setIsFetchingNextPage(false);
    }
  }, [assets.length, hasNextPage, isFetchingNextPage, loadPage]);

  const toggleFavorite = useCallback(async (asset: MemoryAsset) => {
    const nextFavorite = !asset.is_favorite;
    setAssets((current) => current.map((a) => (a.id === asset.id ? { ...a, is_favorite: nextFavorite } : a)));
    try {
      await invoke("set_asset_favorite", { assetId: asset.id, isFavorite: nextFavorite });
    } catch (err) {
      setAssets((current) => current.map((a) => (a.id === asset.id ? { ...a, is_favorite: !nextFavorite } : a)));
      throw err;
    }
  }, []);

  const updateTags = useCallback(async (assetId: string, tags: string[]) => {
    let previous: string[] = [];
    setAssets((current) =>
      current.map((a) => {
        if (a.id !== assetId) {
          return a;
        }
        previous = a.tags;
        return { ...a, tags };
      }),
    );
    try {
      await invoke("set_asset_tags", { assetId, tags });
    } catch (err) {
      setAssets((current) => current.map((a) => (a.id === assetId ? { ...a, tags: previous } : a)));
      throw err;
    }
  }, []);

  return { assets, total, isLoading, isFetchingNextPage, hasNextPage, error, fetchNextPage, toggleFavorite, updateTags };
}

export function useMemoryTags() {
  const [tags, setTags] = useState<string[]>([]);
  const refresh = useCallback(async () => {
    const result = await invoke<string[]>("list_memory_tags");
    setTags(result);
  }, []);

  useEffect(() => {
    refresh().catch(() => {});
  }, [refresh]);

  return { tags, refresh };
}

// Groups memories by calendar day, mirroring Snapchat's own Memories grouping.
export function formatDayGroup(takenAt: string | null): { key: string; label: string; shortLabel: string } {
  if (!takenAt) {
    return { key: "undated", label: "Undated", shortLabel: "Undated" };
  }
  const date = new Date(takenAt);
  const key = takenAt.slice(0, 10);
  const label = date.toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" });
  const shortLabel = date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  return { key, label, shortLabel };
}
