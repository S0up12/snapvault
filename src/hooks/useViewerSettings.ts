import { invokeLogged } from "../lib/ipc";
import { useCallback, useEffect, useState } from "react";

export type ViewerSettings = {
  vertical_scroll_navigation: boolean;
  autoplay_delay_ms: number;
  /** Fixed tile columns for the Memories grid; 0 means responsive/auto. */
  grid_columns: number;
};

export function useViewerSettings() {
  const [settings, setSettings] = useState<ViewerSettings | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      setSettings(await invokeLogged<ViewerSettings>("get_viewer_settings"));
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
      await invokeLogged("set_viewer_settings", { settings: next });
    } catch (err) {
      setError(String(err));
      throw err;
    }
  }, []);

  return { settings, isLoading, error, update };
}
