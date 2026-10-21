import { invokeLogged } from "../lib/ipc";
import { useCallback, useEffect, useState } from "react";

export type TranscodePreset = "fastest" | "balanced" | "quality";

export type PerformanceSettings = {
  transcode_preset: TranscodePreset;
  limit_cpu_usage: boolean;
  process_on_startup: boolean;
  transcode_mode: "background" | "on_open";
  hw_accel: "auto" | "off" | "nvenc" | "qsv" | "amf";
};

export type HwEncoder = { id: "nvenc" | "qsv" | "amf"; label: string; available: boolean; works: boolean };

export function useHwEncoders() {
  const [encoders, setEncoders] = useState<HwEncoder[] | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const detect = useCallback(async (forceRefresh: boolean) => {
    setIsLoading(true);
    setError(null);
    try {
      setEncoders(await invokeLogged<HwEncoder[]>("detect_hw_encoders", { forceRefresh }));
    } catch (err) {
      setError(String(err));
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    void detect(false);
  }, [detect]);

  return { encoders, isLoading, error, recheck: () => detect(true) };
}

export function usePerformanceSettings() {
  const [settings, setSettings] = useState<PerformanceSettings | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      setSettings(await invokeLogged<PerformanceSettings>("get_performance_settings"));
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
    let previous: PerformanceSettings | null = null;
    setSettings((current) => {
      previous = current;
      return next;
    });
    try {
      await invokeLogged("set_performance_settings", { settings: next });
    } catch (err) {
      setSettings(previous);
      setError(String(err));
      throw err;
    }
  }, []);

  return { settings, isLoading, error, update };
}
