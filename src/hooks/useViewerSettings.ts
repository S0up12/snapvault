import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";

export type ViewerSettings = {
  vertical_scroll_navigation: boolean;
  autoplay_delay_ms: number;
};

export function useViewerSettings() {
  const [settings, setSettings] = useState<ViewerSettings | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      setSettings(await invoke<ViewerSettings>("get_viewer_settings"));
    } catch (err) {
      setError(String(err));
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const update = useCallback(async (next: ViewerSettings) => {
    setSettings(next);
    try {
      await invoke("set_viewer_settings", { settings: next });
    } catch (err) {
      setError(String(err));
      throw err;
    }
  }, []);

  return { settings, isLoading, error, update };
}
