import { open } from "@tauri-apps/plugin-dialog";
import { AlertCircle, FolderCog, FolderOpen, HardDrive, LoaderCircle, X } from "lucide-react";
import { useState } from "react";

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
      title: "Choose a folder for SnapVault to store your memories, chats, and thumbnails",
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
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-slate-950/55 p-4 backdrop-blur-sm"
      onClick={allowCancel ? onCancel : undefined}
    >
      <div
        className="w-full max-w-lg rounded-[1.8rem] border border-slate-200/80 bg-white/96 p-6 shadow-2xl shadow-slate-900/20 dark:border-white/10 dark:bg-slate-950/95"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-start justify-between">
          <div>
            <p className="text-[11px] font-semibold uppercase tracking-[0.28em] text-sky-700/70 dark:text-sky-200/65">
              Storage Location
            </p>
            <h2 className="mt-1 text-lg font-semibold text-slate-950 dark:text-white">
              Where should SnapVault store your data?
            </h2>
            <p className="mt-2 text-sm text-slate-600 dark:text-slate-300">
              Your photos, videos, chat attachments, and generated thumbnails will be saved here. You
              can change this later in Settings, as long as your library is empty.
            </p>
          </div>
          {allowCancel ? (
            <button
              type="button"
              onClick={onCancel}
              className="shrink-0 rounded-xl border border-slate-200 bg-white p-2 text-slate-600 transition hover:border-slate-300 hover:bg-slate-50 dark:border-white/10 dark:bg-white/5 dark:text-slate-300 dark:hover:bg-white/10"
            >
              <X className="h-4 w-4" />
            </button>
          ) : null}
        </div>

        <div className="mt-5 space-y-3">
          <button
            type="button"
            onClick={chooseDefault}
            disabled={isSaving}
            className="flex w-full items-center gap-3 rounded-[1.25rem] border border-slate-200/80 bg-slate-50/85 p-4 text-left transition hover:border-sky-300/40 hover:bg-sky-50/60 disabled:cursor-not-allowed disabled:opacity-60 dark:border-white/10 dark:bg-white/[0.035] dark:hover:bg-white/[0.06]"
          >
            <HardDrive className="h-5 w-5 shrink-0 text-sky-600 dark:text-sky-300" />
            <span className="min-w-0 flex-1">
              <span className="block text-sm font-medium text-slate-900 dark:text-white">
                Use the default location
              </span>
              <span className="mt-0.5 block truncate text-xs text-slate-500 dark:text-slate-400" title={defaultPath}>
                {defaultPath}
              </span>
            </span>
          </button>

          <button
            type="button"
            onClick={chooseFolder}
            disabled={isSaving}
            className="flex w-full items-center gap-3 rounded-[1.25rem] border border-slate-200/80 bg-slate-50/85 p-4 text-left transition hover:border-sky-300/40 hover:bg-sky-50/60 disabled:cursor-not-allowed disabled:opacity-60 dark:border-white/10 dark:bg-white/[0.035] dark:hover:bg-white/[0.06]"
          >
            <FolderCog className="h-5 w-5 shrink-0 text-sky-600 dark:text-sky-300" />
            <span className="min-w-0 flex-1">
              <span className="block text-sm font-medium text-slate-900 dark:text-white">Choose a folder</span>
              <span className="mt-0.5 block text-xs text-slate-500 dark:text-slate-400">
                Pick any folder on your computer, e.g. an external or secondary drive
              </span>
            </span>
            <FolderOpen className="h-4 w-4 shrink-0 text-slate-400 dark:text-slate-500" />
          </button>
        </div>

        {isSaving ? (
          <div className="mt-4 flex items-center gap-2 text-sm text-slate-500 dark:text-slate-400">
            <LoaderCircle className="h-4 w-4 animate-spin" />
            Saving...
          </div>
        ) : null}

        {error ? (
          <div className="mt-4 rounded-[1.25rem] border border-red-300/50 bg-red-500/5 p-4 dark:border-red-400/30 dark:bg-red-500/10">
            <div className="flex items-center gap-2">
              <AlertCircle className="h-5 w-5 text-red-500" />
              <p className="text-sm font-semibold text-red-600 dark:text-red-300">Couldn't set storage location</p>
            </div>
            <p className="mt-1.5 text-xs text-red-500">{error}</p>
          </div>
        ) : null}
      </div>
    </div>
  );
}
