import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  AlertCircle,
  AlertTriangle,
  CheckCircle2,
  FolderCog,
  FolderOpen,
  LoaderCircle,
  RefreshCcw,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import { useState } from "react";

import Stat from "../components/Stat";
import StorageChoiceModal from "../components/StorageChoiceModal";
import ThumbnailGenerator from "../components/ThumbnailGenerator";
import { useLibraryStats } from "../hooks/useLibraryStats";
import { usePerformanceSettings, type TranscodePreset } from "../hooks/usePerformanceSettings";
import { useStorageInfo } from "../hooks/useStorageSettings";
import { useViewerSettings } from "../hooks/useViewerSettings";

type VerifySummary = {
  checked: number;
  missing_original: number;
  missing_thumbnail: number;
  missing_playback: number;
};

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(1)} ${units[unitIndex]}`;
}

export default function Settings() {
  return (
    <div className="space-y-8">
      <SettingsSection title="Library">
        <LibraryStatsPanel />
        <VerifyLibraryPanel />
      </SettingsSection>

      <SettingsSection title="Storage">
        <StorageLocationPanel />
      </SettingsSection>

      <SettingsSection title="Media viewer">
        <ViewerSettingsPanel />
      </SettingsSection>

      <SettingsSection title="Performance">
        <PerformanceSettingsPanel />
        <ThumbnailGenerator />
      </SettingsSection>

      <SettingsSection title="Danger zone">
        <DangerZonePanel />
      </SettingsSection>
    </div>
  );
}

function SettingsSection({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section>
      <h2 className="mb-3 px-1 text-[11px] uppercase tracking-[0.28em] text-neutral-500">{title}</h2>
      <div className="flex flex-col gap-4">{children}</div>
    </section>
  );
}

function Panel({
  title,
  description,
  children,
  headerAction,
}: {
  title: string;
  description: string;
  children: React.ReactNode;
  headerAction?: React.ReactNode;
}) {
  return (
    <div className="rounded-lg bg-surface p-6 ring-1 ring-divider">
      <div className="flex items-start justify-between gap-3">
        <div>
          <p className="text-[10px] uppercase tracking-[0.14em] text-accent">{title}</p>
          <p className="mt-1 text-sm text-neutral-300">{description}</p>
        </div>
        {headerAction}
      </div>
      <div className="mt-4">{children}</div>
    </div>
  );
}

function LibraryStatsPanel() {
  const { stats, isLoading, error, refresh } = useLibraryStats();

  return (
    <Panel
      title="Library"
      description="What's actually in the database right now."
      headerAction={
        <button
          type="button"
          onClick={refresh}
          disabled={isLoading}
          className="btn btn-secondary text-xs disabled:cursor-not-allowed"
        >
          <RefreshCcw className={isLoading ? "h-3.5 w-3.5 animate-spin" : "h-3.5 w-3.5"} />
          Refresh
        </button>
      }
    >
      {error ? (
        <p className="text-sm text-red-400">Failed to load library stats: {error}</p>
      ) : !stats ? (
        <div className="flex items-center justify-center py-6 text-neutral-500">
          <LoaderCircle className="h-5 w-5 animate-spin" />
        </div>
      ) : (
        <div className="grid grid-cols-2 gap-3 text-xs sm:grid-cols-4">
          <Stat label="Total assets" value={stats.total_assets} />
          <Stat label="Photos" value={stats.images} />
          <Stat label="Videos" value={stats.videos} />
          <Stat label="Audio" value={stats.audio} />
          <Stat label="Thumbnails missing" value={stats.thumbnails_missing} />
          <Stat label="Videos pending conversion" value={stats.playback_pending} />
          <Stat label="Memory items" value={stats.memory_items} />
          <Stat label="Chat threads" value={stats.chat_threads} />
          <Stat label="Chat messages" value={stats.chat_messages} />
          <Stat label="Chat media linked" value={stats.chat_media_linked} />
          <Stat label="Profile imported" value={stats.profile_found ? "Yes" : "No"} />
          <Stat label="Database size" value={formatBytes(stats.db_size_bytes)} />
        </div>
      )}
    </Panel>
  );
}

function StorageLocationPanel() {
  const { info, error, refresh } = useStorageInfo();
  const { stats } = useLibraryStats();
  const [showChoiceModal, setShowChoiceModal] = useState(false);

  const hasAssets = (stats?.total_assets ?? 0) > 0;

  return (
    <Panel
      title="Storage"
      description="Your database (a few MB, metadata only) always stays in SnapVault's app-data folder. This only controls where your photos, videos, chat attachments, and thumbnails are stored."
    >
      {error ? (
        <p className="text-sm text-red-400">Failed to load storage settings: {error}</p>
      ) : !info ? (
        <div className="flex items-center justify-center py-6 text-neutral-500">
          <LoaderCircle className="h-5 w-5 animate-spin" />
        </div>
      ) : (
        <>
          <div className="rounded-md bg-white/[0.03] px-4 py-3 ring-1 ring-divider">
            <p className="text-[11px] uppercase tracking-wide text-neutral-500">
              Media location {info.is_default ? "(default)" : "(custom)"}
            </p>
            <p className="mt-1 truncate text-sm text-neutral-200" title={info.media_root}>
              {info.media_root}
            </p>
          </div>

          <div className="mt-4 flex flex-wrap items-center gap-2">
            <button type="button" onClick={() => revealItemInDir(info.media_root)} className="btn btn-secondary">
              <FolderOpen className="h-4 w-4" />
              Open folder
            </button>

            <button
              type="button"
              onClick={() => setShowChoiceModal(true)}
              disabled={hasAssets}
              className="btn btn-primary disabled:cursor-not-allowed"
            >
              <FolderCog className="h-4 w-4" />
              Change location
            </button>
          </div>

          {hasAssets ? (
            <p className="mt-3 text-xs text-neutral-500">
              Reset the library in the Danger Zone below before changing the storage location.
            </p>
          ) : null}
        </>
      )}

      {showChoiceModal && info ? (
        <StorageChoiceModal
          defaultPath={info.media_root}
          allowCancel
          onCancel={() => setShowChoiceModal(false)}
          onChosen={() => {
            setShowChoiceModal(false);
            void refresh();
          }}
        />
      ) : null}
    </Panel>
  );
}

const PRESET_OPTIONS: { value: TranscodePreset; label: string; description: string }[] = [
  {
    value: "fastest",
    label: "Fastest",
    description: "Lowest CPU use and quickest processing - best for older or low-power hardware. Files are a bit larger; playback quality is unaffected.",
  },
  {
    value: "balanced",
    label: "Balanced",
    description: "A middle ground between processing speed and file size.",
  },
  {
    value: "quality",
    label: "Best quality",
    description: "Smallest files, most CPU-intensive to produce. Best left for modern hardware.",
  },
];

const AUTOPLAY_DELAY_OPTIONS: { value: number; label: string }[] = [
  { value: 0, label: "Off" },
  { value: 300, label: "Short" },
  { value: 600, label: "Medium" },
  { value: 1000, label: "Long" },
];

function ViewerSettingsPanel() {
  const { settings, error, update } = useViewerSettings();
  const [isSaving, setIsSaving] = useState(false);

  async function handleToggleScrollNavigation() {
    if (!settings) {
      return;
    }
    setIsSaving(true);
    try {
      await update({ ...settings, vertical_scroll_navigation: !settings.vertical_scroll_navigation });
    } finally {
      setIsSaving(false);
    }
  }

  async function handleAutoplayDelayChange(delayMs: number) {
    if (!settings || delayMs === settings.autoplay_delay_ms) {
      return;
    }
    setIsSaving(true);
    try {
      await update({ ...settings, autoplay_delay_ms: delayMs });
    } finally {
      setIsSaving(false);
    }
  }

  return (
    <Panel
      title="Media viewer"
      description="Controls how you move between photos and videos in the fullscreen viewer."
    >
      {error ? (
        <p className="text-sm text-red-400">Failed to load viewer settings: {error}</p>
      ) : !settings ? (
        <div className="flex items-center justify-center py-6 text-neutral-500">
          <LoaderCircle className="h-5 w-5 animate-spin" />
        </div>
      ) : (
        <div className="flex flex-col gap-5">
          <label className="flex items-start gap-3 rounded-md bg-white/[0.03] px-4 py-3 ring-1 ring-divider">
            <input
              type="checkbox"
              checked={settings.vertical_scroll_navigation}
              onChange={handleToggleScrollNavigation}
              disabled={isSaving}
              className="mt-0.5 h-4 w-4 accent-accent"
            />
            <span>
              <span className="block text-sm font-medium text-neutral-200">Scroll to browse media</span>
              <span className="mt-0.5 block text-xs text-neutral-400">
                Scroll or swipe vertically in the viewer to move to the next or previous item, instead of only using
                the left/right buttons. Arrow keys and the buttons still work either way.
              </span>
            </span>
          </label>

          <div>
            <p className="text-xs uppercase tracking-wide text-neutral-500">Autoplay delay</p>
            <p className="mt-1 text-xs text-neutral-500">
              How long to wait after opening a video or voice message before it starts playing on its own.
            </p>
            <div className="mt-2 flex flex-wrap gap-2">
              {AUTOPLAY_DELAY_OPTIONS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  onClick={() => handleAutoplayDelayChange(option.value)}
                  disabled={isSaving}
                  className={[
                    "btn disabled:cursor-not-allowed",
                    settings.autoplay_delay_ms === option.value ? "btn-primary" : "btn-secondary",
                  ].join(" ")}
                >
                  {option.label}
                </button>
              ))}
            </div>
          </div>
        </div>
      )}
    </Panel>
  );
}

function PerformanceSettingsPanel() {
  const { settings, isLoading, error, update } = usePerformanceSettings();
  const [isSaving, setIsSaving] = useState(false);

  async function handlePresetChange(preset: TranscodePreset) {
    if (!settings || preset === settings.transcode_preset) {
      return;
    }
    setIsSaving(true);
    try {
      await update({ ...settings, transcode_preset: preset });
    } finally {
      setIsSaving(false);
    }
  }

  async function handleToggleCpuLimit() {
    if (!settings) {
      return;
    }
    setIsSaving(true);
    try {
      await update({ ...settings, limit_cpu_usage: !settings.limit_cpu_usage });
    } finally {
      setIsSaving(false);
    }
  }

  return (
    <Panel
      title="Media processing"
      description="Controls how much CPU generating thumbnails and converting videos for playback uses. Lower settings process slower but put a lighter load on older or lower-powered hardware - the videos and photos you see are identical either way."
    >
      {error ? (
        <p className="text-sm text-red-400">Failed to load performance settings: {error}</p>
      ) : !settings ? (
        <div className="flex items-center justify-center py-6 text-neutral-500">
          <LoaderCircle className="h-5 w-5 animate-spin" />
        </div>
      ) : (
        <div className="flex flex-col gap-5">
          <div>
            <p className="text-xs uppercase tracking-wide text-neutral-500">Video conversion speed</p>
            <div className="mt-2 flex flex-wrap gap-2">
              {PRESET_OPTIONS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  onClick={() => handlePresetChange(option.value)}
                  disabled={isSaving || isLoading}
                  className={[
                    "btn disabled:cursor-not-allowed",
                    settings.transcode_preset === option.value ? "btn-primary" : "btn-secondary",
                  ].join(" ")}
                >
                  {option.label}
                </button>
              ))}
            </div>
            <p className="mt-2 text-xs text-neutral-500">
              {PRESET_OPTIONS.find((option) => option.value === settings.transcode_preset)?.description}
            </p>
          </div>

          <label className="flex items-start gap-3 rounded-md bg-white/[0.03] px-4 py-3 ring-1 ring-divider">
            <input
              type="checkbox"
              checked={settings.limit_cpu_usage}
              onChange={handleToggleCpuLimit}
              disabled={isSaving}
              className="mt-0.5 h-4 w-4 accent-accent"
            />
            <span>
              <span className="block text-sm font-medium text-neutral-200">Limit CPU usage while processing</span>
              <span className="mt-0.5 block text-xs text-neutral-400">
                Caps media processing to about half your CPU cores, so the rest of your machine stays responsive
                during a large import or backlog run.
              </span>
            </span>
          </label>
        </div>
      )}
    </Panel>
  );
}

function VerifyLibraryPanel() {
  const [status, setStatus] = useState<"idle" | "running" | "done" | "error">("idle");
  const [summary, setSummary] = useState<VerifySummary | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function handleVerify() {
    setStatus("running");
    setError(null);
    try {
      const result = await invoke<VerifySummary>("verify_library");
      setSummary(result);
      setStatus("done");
    } catch (err) {
      setError(String(err));
      setStatus("error");
    }
  }

  const hasIssues = summary && (summary.missing_original > 0 || summary.missing_thumbnail > 0 || summary.missing_playback > 0);

  return (
    <Panel title="Verify library" description="Checks that every asset's file (original, thumbnail, playback copy) still exists on disk.">
      <button type="button" onClick={handleVerify} disabled={status === "running"} className="btn btn-primary disabled:cursor-not-allowed">
        {status === "running" ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <ShieldCheck className="h-4 w-4" />}
        {status === "running" ? "Verifying..." : "Verify library"}
      </button>

      {status === "error" && error ? (
        <div className="mt-4 rounded-md bg-red-500/8 p-4 ring-1 ring-red-400/25">
          <div className="flex items-center gap-2">
            <AlertCircle className="h-5 w-5 text-red-400" />
            <p className="text-sm font-semibold text-red-300">Verify failed</p>
          </div>
          <p className="mt-1.5 text-xs text-red-400">{error}</p>
        </div>
      ) : null}

      {status === "done" && summary ? (
        <div className={["mt-4 rounded-md p-4 ring-1", hasIssues ? "bg-amber-500/8 ring-amber-400/25" : "bg-emerald-500/6 ring-emerald-400/20"].join(" ")}>
          <div className="flex items-center gap-2">
            {hasIssues ? (
              <AlertTriangle className="h-5 w-5 text-amber-400" />
            ) : (
              <CheckCircle2 className="h-5 w-5 text-emerald-400" />
            )}
            <p className={hasIssues ? "text-sm font-semibold text-amber-300" : "text-sm font-semibold text-emerald-300"}>
              {hasIssues ? "Some files are missing" : "Everything checks out"}
            </p>
          </div>
          <div className="mt-3 grid grid-cols-2 gap-3 text-xs sm:grid-cols-4">
            <Stat label="Checked" value={summary.checked} />
            <Stat label="Missing originals" value={summary.missing_original} />
            <Stat label="Missing thumbnails" value={summary.missing_thumbnail} />
            <Stat label="Missing playback copies" value={summary.missing_playback} />
          </div>
        </div>
      ) : null}
    </Panel>
  );
}

function DangerZonePanel() {
  const [status, setStatus] = useState<"idle" | "running" | "done" | "error">("idle");
  const [error, setError] = useState<string | null>(null);

  async function handleReset() {
    const confirmed = await confirm(
      "This deletes every imported asset, chat, memory, and profile snapshot, plus all extracted files, thumbnails, and playback copies on disk. This cannot be undone.",
      { title: "Reset library and start over?", kind: "warning" },
    );
    if (!confirmed) {
      return;
    }

    setStatus("running");
    setError(null);
    try {
      await invoke("reset_library");
      setStatus("done");
    } catch (err) {
      setError(String(err));
      setStatus("error");
    }
  }

  return (
    <Panel title="Danger zone" description="Wipe the database and every extracted/generated file, so you can re-import from a clean slate.">
      <button type="button" onClick={handleReset} disabled={status === "running"} className="btn btn-danger disabled:cursor-not-allowed">
        {status === "running" ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Trash2 className="h-4 w-4" />}
        {status === "running" ? "Resetting..." : "Reset library & start over"}
      </button>

      {status === "done" ? (
        <p className="mt-3 text-sm text-emerald-300">Library reset. Go to the Dashboard to import a fresh export.</p>
      ) : null}
      {status === "error" && error ? <p className="mt-3 text-sm text-red-400">Reset failed: {error}</p> : null}
    </Panel>
  );
}
