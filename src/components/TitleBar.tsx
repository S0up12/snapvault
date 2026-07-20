import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { useEffect, useState } from "react";

const appWindow = getCurrentWindow();

// Replaces the native OS title bar (tauri.conf.json sets decorations: false) -
// dragging the empty bar area moves the window, and these three buttons are
// the only way to minimize/maximize/close it now.
export default function TitleBar() {
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    appWindow.isMaximized().then(setIsMaximized);
    const unlisten = appWindow.onResized(() => {
      void appWindow.isMaximized().then(setIsMaximized);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <div
      data-tauri-drag-region
      onDoubleClick={() => void appWindow.toggleMaximize()}
      className="flex h-[38px] shrink-0 select-none items-center border-b border-divider"
    >
      <p data-tauri-drag-region className="flex-1" />

      <div className="flex h-full shrink-0 text-neutral-500">
        <button
          type="button"
          onClick={() => void appWindow.minimize()}
          className="flex h-full w-11 items-center justify-center transition hover:bg-white/5 hover:text-fg"
          aria-label="Minimize"
        >
          <Minus className="h-4 w-4" />
        </button>
        <button
          type="button"
          onClick={() => void appWindow.toggleMaximize()}
          className="flex h-full w-11 items-center justify-center transition hover:bg-white/5 hover:text-fg"
          aria-label={isMaximized ? "Restore" : "Maximize"}
        >
          {isMaximized ? <Copy className="h-3.5 w-3.5 -scale-x-100" /> : <Square className="h-3.5 w-3.5" />}
        </button>
        <button
          type="button"
          onClick={() => void appWindow.close()}
          className="flex h-full w-11 items-center justify-center transition hover:bg-red-500 hover:text-white"
          aria-label="Close"
        >
          <X className="h-4 w-4" />
        </button>
      </div>
    </div>
  );
}
