import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";

export type TranscodePreset = "fastest" | "balanced" | "quality";

export type PerformanceSettings = {
  transcode_preset: TranscodePreset;
  limit_cpu_usage: boolean;
};

export function usePerformanceSettings() {
  const [settings, setSettings] = useState<PerformanceSettings | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      setSettings(await invoke<PerformanceSettings>("get_performance_settings"));
    } catch (err) {
      setError(String(err));
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const update = useCallback(async (next: PerformanceSettings) => {
    setSettings(next);
    try {
      await invoke("set_performance_settings", { settings: next });
    } catch (err) {
      setError(String(err));
      throw err;
    }
  }, []);

  return { settings, isLoading, error, update };
}
