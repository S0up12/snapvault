import { open } from "@tauri-apps/plugin-dialog";
import { AlertCircle, FolderCog, FolderOpen, HardDrive, LoaderCircle, X } from "lucide-react";
import { useState } from "react";

import { useDialogFocus } from "../hooks/useDialogFocus";
import { setMediaRoot } from "../hooks/useStorageSettings";

type StorageChoiceModalProps = {
  defaultPath: string;
  /** First-run usage has no way out except choosing - Settings' "change
   * location" usage lets the user back out without changing anything. */
  allowCancel: boolean;
  onCancel?: () => void;
  onChosen: (mediaRoot: string) => void;
};

export default function StorageChoiceModal({ defaultPath, allowCancel, onCancel, onChosen }: StorageChoiceModalProps) {
  const dialogRef = useDialogFocus<HTMLDivElement>(allowCancel ? onCancel : undefined);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function chooseDefault() {
    setIsSaving(true);
    setError(null);
    try {
      const mediaRoot = await setMediaRoot(null);
      onChosen(mediaRoot);
    } catch (err) {
      setError(String(err));
    } finally {
      setIsSaving(false);
    }
  }

  async function chooseFolder() {
    const selected = await open({
      title: "Choose a folder for Snapvault to store your memories, chats, and thumbnails",
      directory: true,
      multiple: false,
    });
    if (!selected) {
      // User cancelled the native dialog - stay on this modal rather than
      // falling through to any default.
      return;
    }

    setIsSaving(true);
    setError(null);
    try {
      const mediaRoot = await setMediaRoot(selected);
      onChosen(mediaRoot);
    } catch (err) {
      setError(String(err));
    } finally {
      setIsSaving(false);
    }
  }

  return (
    <div className="dialog-backdrop z-[60]" onClick={allowCancel ? onCancel : undefined}>
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label="Choose storage location"
        tabIndex={-1}
        className="dialog w-full max-w-lg outline-none"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-start justify-between">
          <div>
            <p className="text-[10px] uppercase tracking-[0.14em] text-accent">Storage Location</p>
            <h2 className="dialog-title mt-1">Where should Snapvault store your data?</h2>
            <p className="dialog-body mt-2">
              Your photos, videos, chat attachments, and generated thumbnails will be saved here. You
              can change this later in Settings, as long as your library is empty.
            </p>
          </div>
          {allowCancel ? (
            <button type="button" onClick={onCancel} className="btn btn-secondary btn-icon shrink-0" aria-label="Close">
              <X className="h-4 w-4" />
            </button>
          ) : null}
        </div>

        <div className="mt-2 flex flex-col gap-3">
          <button
            type="button"
            onClick={chooseDefault}
            disabled={isSaving}
            className="flex w-full items-center gap-3 rounded-md bg-white/[0.03] p-4 text-left ring-1 ring-divider transition hover:bg-accent/8 disabled:cursor-not-allowed disabled:opacity-60"
          >
            <HardDrive className="h-5 w-5 shrink-0 text-accent" />
            <span className="min-w-0 flex-1">
              <span className="block text-sm font-medium text-fg">Use the default location</span>
              <span className="mt-0.5 block truncate text-xs text-neutral-400" title={defaultPath}>
                {defaultPath}
              </span>
            </span>
          </button>

          <button
            type="button"
            onClick={chooseFolder}
            disabled={isSaving}
            className="flex w-full items-center gap-3 rounded-md bg-white/[0.03] p-4 text-left ring-1 ring-divider transition hover:bg-accent/8 disabled:cursor-not-allowed disabled:opacity-60"
          >
            <FolderCog className="h-5 w-5 shrink-0 text-accent" />
            <span className="min-w-0 flex-1">
              <span className="block text-sm font-medium text-fg">Choose a folder</span>
              <span className="mt-0.5 block text-xs text-neutral-400">
                Pick any folder on your computer, e.g. an external or secondary drive
              </span>
            </span>
            <FolderOpen className="h-4 w-4 shrink-0 text-neutral-500" />
          </button>
        </div>

        {isSaving ? (
          <div className="flex items-center gap-2 text-sm text-neutral-400">
            <LoaderCircle className="h-4 w-4 animate-spin" />
            Saving...
          </div>
        ) : null}

        {error ? (
          <div className="rounded-md bg-red-500/8 p-4 ring-1 ring-red-400/25">
            <div className="flex items-center gap-2">
              <AlertCircle className="h-5 w-5 text-red-400" />
              <p className="text-sm font-semibold text-red-300">Couldn't set storage location</p>
            </div>
            <p className="mt-1.5 text-xs text-red-400">{error}</p>
          </div>
        ) : null}
      </div>
    </div>
  );
}
