import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";

export type LibraryStats = {
  total_assets: number;
  images: number;
  videos: number;
  audio: number;
  thumbnails_missing: number;
  playback_pending: number;
  memory_items: number;
  chat_threads: number;
  chat_messages: number;
  chat_media_linked: number;
  profile_found: boolean;
  db_size_bytes: number;
};

export function useLibraryStats() {
  const [stats, setStats] = useState<LibraryStats | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      setStats(await invoke<LibraryStats>("get_library_stats"));
    } catch (err) {
      setError(String(err));
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  return { stats, isLoading, error, refresh };
}
