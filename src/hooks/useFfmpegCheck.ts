import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

/** `error` is non-null when ffmpeg is missing/unrunnable; `version` when OK. */
export function useFfmpegCheck() {
  const [version, setVersion] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<string>("check_ffmpeg").then(setVersion).catch((err) => setError(String(err)));
  }, []);

  return { version, error };
}
