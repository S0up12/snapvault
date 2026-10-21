import { invoke } from "@tauri-apps/api/core";

const MAX_LEN = 500;
const MAX_PER_WINDOW = 20;
const WINDOW_MS = 10_000;
let windowStart = 0;
let sent = 0;

// Mirrors a line to the Rust log. Fire-and-forget; uses raw invoke so a
// failure here can never recurse into invokeLogged. Callers pass no user content.
export function mirrorToRust(level: "warn" | "error", message: string): void {
  try {
    const now = Date.now();
    if (now - windowStart > WINDOW_MS) {
      windowStart = now;
      sent = 0;
    }
    if (sent++ >= MAX_PER_WINDOW) {
      return;
    }
    invoke("log_frontend", { level, message: message.slice(0, MAX_LEN) }).catch(() => {});
  } catch {
    // ponytail: swallow; logging must never throw
  }
}

// Logs command name + arg keys (never values) and duration; errors are mirrored to Rust.
export async function invokeLogged<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const keys = args ? Object.keys(args).join(",") : "";
  const start = performance.now();
  try {
    const result = await invoke<T>(cmd, args);
    console.debug(`[ipc] ${cmd}(${keys}) ok ${Math.round(performance.now() - start)}ms`);
    return result;
  } catch (err) {
    const msg = `ipc ${cmd}(${keys}) failed: ${String(err)}`;
    console.error(`[ipc] ${msg}`);
    mirrorToRust("error", msg);
    throw err;
  }
}
