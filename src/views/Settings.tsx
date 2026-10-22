import { getVersion } from "@tauri-apps/api/app";
import { invokeLogged } from "../lib/ipc";
import { confirm } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
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
import { useEffect, useState } from "react";

import Stat from "../components/Stat";
import StorageChoiceModal from "../components/StorageChoiceModal";
import ThumbnailGenerator from "../components/ThumbnailGenerator";
import { useLibraryStats } from "../hooks/useLibraryStats";
import { useHwEncoders, usePerformanceSettings, type PerformanceSettings, type TranscodePreset } from "../hooks/usePerformanceSettings";
import { useProfile } from "../hooks/useProfile";
import { useStorageInfo } from "../hooks/useStorageSettings";
import { useViewerSettings } from "../hooks/useViewerSettings";

type VerifySummary = {
  checked: number;
  missing_original: number;
  missing_thumbnail: number;
  missing_playback: number;
};

type SectionKey = "library" | "storage" | "viewer" | "performance" | "about" | "danger";

const SECTIONS: { key: SectionKey; label: string }[] = [
  { key: "library", label: "Library" },
  { key: "storage", label: "Storage" },
  { key: "viewer", label: "Media viewer" },
  { key: "performance", label: "Performance" },
  { key: "about", label: "About" },
  { key: "danger", label: "Danger zone" },
];

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
  const [section, setSection] = useState<SectionKey>("library");
  const { profile } = useProfile();
  const vaultName = profile?.snapshot.account.display_name || profile?.snapshot.account.username || "No profile imported";

  return (
    <div>
      <div className="mb-[22px] flex items-end gap-3">
        <div>
          <h2 className="m-0 text-[26px] tracking-[-0.02em]">Settings</h2>
          <div className="mt-1.5 text-[11px] uppercase tracking-[0.16em] text-neutral-500">Vault · {vaultName}</div>
        </div>
      </div>

      <div className="grid grid-cols-[200px_1fr] gap-6">
        <nav className="flex flex-col gap-0.5">
          {SECTIONS.map((s) => (
            <button
              key={s.key}
              type="button"
              onClick={() => setSection(s.key)}
              className={[
                "rounded-lg px-3.5 py-2.5 text-left text-sm transition",
                section === s.key
                  ? "bg-accent/13 text-accent shadow-[inset_2px_0_0_var(--color-accent)]"
                  : "text-neutral-400 hover:bg-surface hover:text-fg",
              ].join(" ")}
            >
              {s.label}
            </button>
          ))}
        </nav>

        <div className="flex min-w-0 flex-col gap-4">
          {section === "library" ? (
            <>
              <LibraryStatsPanel />
              <VerifyLibraryPanel />
            </>
          ) : null}
          {section === "storage" ? <StorageLocationPanel /> : null}
          {section === "viewer" ? <ViewerSettingsPanel /> : null}
          {section === "performance" ? (
            <>
              <PerformanceSettingsPanel />
              <ThumbnailGenerator />
            </>
          ) : null}
          {section === "about" ? <AboutPanel /> : null}
          {section === "danger" ? <DangerZonePanel /> : null}
        </div>
      </div>
    </div>
  );
}

function Panel({
  kicker,
  title,
  description,
  children,
  headerAction,
}: {
  kicker: string;
  title: string;
  description?: string;
  children: React.ReactNode;
  headerAction?: React.ReactNode;
}) {
  return (
    <div className="rounded-lg bg-surface p-[22px] ring-1 ring-divider">
      <div className="flex items-start justify-between gap-3">
        <div>
          <p className="text-[10px] uppercase tracking-[0.14em] text-accent">{kicker}</p>
          <h3 className="mt-1 text-lg">{title}</h3>
          {description ? <p className="mt-1.5 text-sm text-neutral-400">{description}</p> : null}
        </div>
        {headerAction}
      </div>
      <div className="mt-4">{children}</div>
    </div>
  );
}

/** A label/hint row with a control on the right, matching the redesign's settings-list rows. */
function SettingsRow({ label, hint, children, last }: { label: string; hint: string; children: React.ReactNode; last?: boolean }) {
  return (
    <div>
      <div className="flex items-center gap-4 py-3">
        <div className="min-w-0 flex-1">
          <p className="text-sm">{label}</p>
          <p className="mt-0.5 text-xs text-neutral-500">{hint}</p>
        </div>
        {children}
      </div>
      {last ? null : <div className="h-px bg-divider" />}
    </div>
  );
}

function Toggle({ checked, onChange, disabled }: { checked: boolean; onChange: () => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={onChange}
      disabled={disabled}
      className={[
        "relative h-[22px] w-10 shrink-0 rounded-full transition disabled:cursor-not-allowed disabled:opacity-45",
        checked ? "bg-accent" : "bg-neutral-700",
      ].join(" ")}
    >
      <span
        className="absolute left-0.5 top-0.5 h-4 w-4 rounded-full bg-white transition-transform"
        style={{ transform: checked ? "translateX(18px)" : "translateX(0)" }}
      />
    </button>
  );
}

function LibraryStatsPanel() {
  const { stats, isLoading, error, refresh } = useLibraryStats();

  return (
    <Panel
      kicker="Library"
      title="What's in your vault"
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
          <Stat label="Videos not yet checked for playback" value={stats.playback_pending} />
          {stats.playback_failed > 0 ? <Stat label="Unplayable videos" value={stats.playback_failed} /> : null}
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
      kicker="Storage"
      title="Media location"
      description="Your database (a few MB, metadata only) always stays in Snapvault's app-data folder. This only controls where your photos, videos, chat attachments, and thumbnails are stored."
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

const GRID_COLUMNS_OPTIONS: { value: number; label: string }[] = [
  { value: 0, label: "Auto" },
  { value: 4, label: "4" },
  { value: 5, label: "5" },
  { value: 6, label: "6" },
  { value: 7, label: "7" },
  { value: 8, label: "8" },
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

  async function handleGridColumnsChange(columns: number) {
    if (!settings || columns === settings.grid_columns) {
      return;
    }
    setIsSaving(true);
    try {
      await update({ ...settings, grid_columns: columns });
    } finally {
      setIsSaving(false);
    }
  }

  return (
    <Panel
      kicker="Media viewer"
      title="Media viewer"
      description="Controls how you move between photos and videos in the fullscreen viewer, and how the Memories grid lays out tiles."
    >
      {error ? (
        <p className="text-sm text-red-400">Failed to load viewer settings: {error}</p>
      ) : !settings ? (
        <div className="flex items-center justify-center py-6 text-neutral-500">
          <LoaderCircle className="h-5 w-5 animate-spin" />
        </div>
      ) : (
        <div className="flex flex-col">
          <SettingsRow label="Scroll to browse media" hint="Scroll or swipe vertically in the viewer to move to the next or previous item. Arrow keys and the buttons still work either way.">
            <Toggle checked={settings.vertical_scroll_navigation} onChange={handleToggleScrollNavigation} disabled={isSaving} />
          </SettingsRow>

          <SettingsRow label="Autoplay delay" hint="How long to wait after opening a video or voice message before it starts playing on its own.">
            <div className="flex flex-wrap justify-end gap-2">
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
          </SettingsRow>

          <SettingsRow label="Grid density" hint="Tiles per row in Memories. Auto fits as many as the window width allows." last>
            <div className="flex flex-wrap justify-end gap-2">
              {GRID_COLUMNS_OPTIONS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  onClick={() => handleGridColumnsChange(option.value)}
                  disabled={isSaving}
                  className={[
                    "btn disabled:cursor-not-allowed",
                    settings.grid_columns === option.value ? "btn-primary" : "btn-secondary",
                  ].join(" ")}
                >
                  {option.label}
                </button>
              ))}
            </div>
          </SettingsRow>
        </div>
      )}
    </Panel>
  );
}

const TRANSCODE_MODE_OPTIONS: { value: PerformanceSettings["transcode_mode"]; label: string }[] = [
  { value: "background", label: "In the background" },
  { value: "on_open", label: "When I open a video" },
];

function PerformanceSettingsPanel() {
  const { settings, isLoading, error, update } = usePerformanceSettings();
  const hw = useHwEncoders();
  const [isSaving, setIsSaving] = useState(false);

  async function handlePatch(patch: Partial<PerformanceSettings>) {
    if (!settings) {
      return;
    }
    setIsSaving(true);
    try {
      await update({ ...settings, ...patch });
    } finally {
      setIsSaving(false);
    }
  }

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
      kicker="Performance"
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
        <div className="flex flex-col">
          <SettingsRow label="Video conversion speed" hint={PRESET_OPTIONS.find((option) => option.value === settings.transcode_preset)?.description ?? ""}>
            <div className="flex flex-wrap justify-end gap-2">
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
          </SettingsRow>

          <SettingsRow label="Limit CPU usage while processing" hint="Caps media processing to about half your CPU cores, so the rest of your machine stays responsive during a large import or backlog run.">
            <Toggle checked={settings.limit_cpu_usage} onChange={handleToggleCpuLimit} disabled={isSaving} />
          </SettingsRow>

          <SettingsRow label="Video conversion" hint="Converting when you open a video saves CPU, but the first play of each HEVC video waits for a conversion.">
            <div className="flex flex-wrap justify-end gap-2">
              {TRANSCODE_MODE_OPTIONS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  onClick={() => settings.transcode_mode !== option.value && handlePatch({ transcode_mode: option.value })}
                  disabled={isSaving}
                  className={["btn disabled:cursor-not-allowed", settings.transcode_mode === option.value ? "btn-primary" : "btn-secondary"].join(" ")}
                >
                  {option.label}
                </button>
              ))}
            </div>
          </SettingsRow>

          <SettingsRow label="Hardware acceleration" hint={hw.error ? `Could not detect encoders: ${hw.error}` : "Use your GPU to convert videos faster. Auto picks the best working encoder."}>
            <div className="flex items-center justify-end gap-2">
              {hw.isLoading ? <LoaderCircle className="h-4 w-4 animate-spin text-neutral-500" /> : null}
              <select
                value={settings.hw_accel}
                onChange={(event) => void handlePatch({ hw_accel: event.target.value as PerformanceSettings["hw_accel"] })}
                disabled={isSaving || hw.isLoading}
                className="rounded-md bg-surface px-3 py-2 text-sm ring-1 ring-divider"
              >
                <option value="auto">Auto</option>
                <option value="off">Off</option>
                {settings.hw_accel !== "auto" &&
                settings.hw_accel !== "off" &&
                !(hw.encoders ?? []).some((enc) => enc.available && enc.id === settings.hw_accel) ? (
                  <option value={settings.hw_accel} disabled>
                    {settings.hw_accel} (not available)
                  </option>
                ) : null}
                {(hw.encoders ?? []).filter((enc) => enc.available).map((enc) => {
                  const broken = enc.available && !enc.works;
                  return (
                    <option key={enc.id} value={enc.id} disabled={broken}>
                      {enc.label}
                      {broken ? " (not working on this machine)" : ""}
                    </option>
                  );
                })}
              </select>
              <button type="button" onClick={() => void hw.recheck()} disabled={hw.isLoading} className="btn btn-secondary disabled:cursor-not-allowed">
                Re-check
              </button>
            </div>
          </SettingsRow>

          <SettingsRow label="Process media when Snapvault starts" hint="Resume any unfinished thumbnails and video conversions each time the app opens." last>
            <Toggle checked={settings.process_on_startup} onChange={() => handlePatch({ process_on_startup: !settings.process_on_startup })} disabled={isSaving} />
          </SettingsRow>
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
      const result = await invokeLogged<VerifySummary>("verify_library");
      setSummary(result);
      setStatus("done");
    } catch (err) {
      setError(String(err));
      setStatus("error");
    }
  }

  const hasIssues = summary && (summary.missing_original > 0 || summary.missing_thumbnail > 0 || summary.missing_playback > 0);

  return (
    <Panel kicker="Library" title="Verify library" description="Checks that every asset's file (original, thumbnail, playback copy) still exists on disk.">
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

function AboutPanel() {
  const [version, setVersion] = useState("");

  useEffect(() => {
    getVersion().then(setVersion).catch(() => setVersion(""));
  }, []);

  return (
    <Panel kicker="About" title="Snapvault" description="A self-hosted, offline-first viewer for exported Snapchat data. Everything stays on your device.">
      <SettingsRow label="Version" hint="Installed build">
        <span className="text-sm text-neutral-300">{version || "-"}</span>
      </SettingsRow>
      <SettingsRow label="Author" hint="Built by S0up12">
        <button type="button" onClick={() => openUrl("https://github.com/S0up12/snapvault")} className="btn">
          GitHub
        </button>
      </SettingsRow>
      <SettingsRow label="License" hint="Free and open source" last>
        <span className="text-sm text-neutral-300">MIT</span>
      </SettingsRow>
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
      await invokeLogged("reset_library");
      setStatus("done");
    } catch (err) {
      setError(String(err));
      setStatus("error");
    }
  }

  return (
    <Panel kicker="Danger zone" title="Reset everything" description="Wipe the database and every extracted/generated file, so you can re-import from a clean slate.">
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
