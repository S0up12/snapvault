import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";

export type StorageInfo = {
  media_root: string;
  is_default: boolean;
  needs_first_run_choice: boolean;
};

export function useStorageInfo() {
  const [info, setInfo] = useState<StorageInfo | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      setInfo(await invoke<StorageInfo>("get_storage_info"));
    } catch (err) {
      setError(String(err));
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  return { info, isLoading, error, refresh };
}

export async function setMediaRoot(path: string | null): Promise<string> {
  return invoke<string>("set_media_root", { path });
}
