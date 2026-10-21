import { invokeLogged } from "../lib/ipc";
import { useEffect, useState } from "react";

/** `error` is non-null when ffmpeg is missing/unrunnable; `version` when OK. */
export function useFfmpegCheck() {
  const [version, setVersion] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invokeLogged<string>("check_ffmpeg").then(setVersion).catch((err) => setError(String(err)));
  }, []);

  return { version, error };
}
