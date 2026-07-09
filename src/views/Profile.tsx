import { Archive, LoaderCircle, Smile, Trophy, Users } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import { avatarInitials } from "../hooks/useChats";
import { useProfile } from "../hooks/useProfile";

function formatDate(value: string | null): string {
  if (!value) {
    return "Unknown";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" });
}

function formatNumber(value: number | null): string {
  return value === null || value === undefined ? "—" : value.toLocaleString();
}

export default function Profile() {
  const { profile, isLoading, error } = useProfile();

  if (isLoading) {
    return (
      <div className="flex h-full items-center justify-center text-slate-400 dark:text-slate-500">
        <LoaderCircle className="h-6 w-6 animate-spin" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="rounded-[1.25rem] border border-rose-300/40 bg-rose-50 px-4 py-3 text-sm text-rose-700 dark:border-rose-400/20 dark:bg-rose-400/10 dark:text-rose-100">
        {error}
      </div>
    );
  }

  if (!profile) {
    return (
      <div className="rounded-[1.75rem] border border-slate-200/80 bg-white/80 p-8 text-sm text-slate-500 shadow-[0_20px_48px_rgba(15,23,42,0.08)] dark:border-white/10 dark:bg-white/[0.04] dark:text-slate-400">
        No profile data yet. Import a Snapchat export from the Dashboard first.
      </div>
    );
  }

  const { account, friends, ranking, engagement, bitmoji } = profile.snapshot;
  const displayName = account.display_name || account.username || "Snapchat Profile";

  return (
    <section className="mx-auto flex w-full max-w-[1600px] flex-col gap-6">
      <div className="overflow-hidden rounded-[2rem] border border-slate-200/70 bg-[radial-gradient(circle_at_top,_rgba(56,189,248,0.14),_transparent_40%),linear-gradient(135deg,_rgba(255,255,255,0.95),_rgba(238,246,255,0.94))] p-6 shadow-[0_28px_70px_rgba(15,23,42,0.08)] dark:border-white/10 dark:bg-[radial-gradient(circle_at_top,_rgba(56,189,248,0.18),_transparent_42%),linear-gradient(135deg,_rgba(10,18,28,0.92),_rgba(4,9,15,0.98))]">
        <div className="flex flex-wrap items-center gap-5">
          <div className="flex h-20 w-20 shrink-0 items-center justify-center rounded-[1.7rem] border border-slate-200/80 bg-white/85 text-2xl font-semibold text-slate-900 shadow-inner shadow-white/50 dark:border-white/10 dark:bg-white/[0.07] dark:text-white">
            {avatarInitials(displayName)}
          </div>
          <div className="min-w-0">
            <p className="text-[11px] font-semibold uppercase tracking-[0.28em] text-sky-700/70 dark:text-sky-200/65">
              Imported Profile
            </p>
            <h1 className="mt-1 text-2xl font-semibold tracking-tight text-slate-950 dark:text-white">
              {displayName}
            </h1>
            <div className="mt-2 flex flex-wrap items-center gap-2 text-xs">
              {account.username ? (
                <span className="rounded-full border border-slate-200/80 bg-white/70 px-3 py-1 text-slate-600 dark:border-white/10 dark:bg-white/[0.05] dark:text-slate-300">
                  @{account.username}
                </span>
              ) : null}
              <span className="rounded-full border border-slate-200/80 bg-white/70 px-3 py-1 text-slate-600 dark:border-white/10 dark:bg-white/[0.05] dark:text-slate-300">
                Joined {formatDate(account.created_at)}
              </span>
              {account.country ? (
                <span className="rounded-full border border-slate-200/80 bg-white/70 px-3 py-1 text-slate-600 dark:border-white/10 dark:bg-white/[0.05] dark:text-slate-300">
                  {account.country}
                </span>
              ) : null}
            </div>
          </div>
        </div>

        <div className="mt-6 grid grid-cols-2 gap-3 sm:grid-cols-4">
          <StatCard label="Friends" value={formatNumber(friends.friends_count)} Icon={Users} />
          <StatCard label="Snapscore" value={formatNumber(ranking.snapscore)} Icon={Trophy} />
          <StatCard label="Memories" value={formatNumber(profile.memory_count)} Icon={Archive} />
          <StatCard label="Bitmoji Opens" value={formatNumber(bitmoji.app_open_count)} Icon={Smile} />
        </div>
      </div>

      <div className="grid gap-6 xl:grid-cols-2">
        <div className="space-y-6">
          <SettingsCard title="Account">
            <InfoList
              items={[
                { label: "Display Name", value: account.display_name },
                { label: "Username", value: account.username },
                { label: "Created", value: formatDate(account.created_at) },
                { label: "Country", value: account.country },
                { label: "Language", value: account.in_app_language },
                { label: "Registration IP", value: account.registration_ip },
              ]}
            />
          </SettingsCard>

          <SettingsCard title="Friends">
            <div className="grid grid-cols-3 gap-3">
              <StatCard label="Current" value={formatNumber(friends.friends_count)} Icon={Users} />
              <StatCard label="Blocked" value={formatNumber(friends.blocked_count)} Icon={Users} />
              <StatCard label="Deleted" value={formatNumber(friends.deleted_count)} Icon={Users} />
            </div>
            {friends.top_friends.length > 0 ? (
              <div className="mt-4 space-y-2">
                <p className="text-xs font-semibold uppercase tracking-[0.2em] text-slate-500 dark:text-slate-400">
                  Friend Preview
                </p>
                {friends.top_friends.map((friend) => (
                  <div
                    key={friend.Username}
                    className="flex items-center justify-between rounded-[1.2rem] border border-slate-200/70 bg-slate-50/85 px-4 py-2.5 dark:border-white/10 dark:bg-white/[0.035]"
                  >
                    <span className="text-sm text-slate-700 dark:text-slate-200">
                      {friend["Display Name"] || friend.Username}
                    </span>
                    <span className="text-xs text-slate-400 dark:text-slate-500">@{friend.Username}</span>
                  </div>
                ))}
              </div>
            ) : null}
          </SettingsCard>
        </div>

        <div className="space-y-6">
          <SettingsCard title="Engagement">
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
              <StatCard label="App Opens" value={formatNumber(engagement.application_opens)} Icon={Smile} />
              <StatCard label="Story Views" value={formatNumber(engagement.story_views)} Icon={Smile} />
              <StatCard label="Snap Views" value={formatNumber(engagement.snap_views)} Icon={Smile} />
              <StatCard label="Chats Sent" value={formatNumber(engagement.chats_sent)} Icon={Smile} />
              <StatCard label="Chats Viewed" value={formatNumber(engagement.chats_viewed)} Icon={Smile} />
              <StatCard label="Direct Snaps" value={formatNumber(engagement.direct_snaps_created)} Icon={Smile} />
            </div>
          </SettingsCard>

          <SettingsCard title="Bitmoji">
            <InfoList
              items={[
                { label: "Avatar Gender", value: bitmoji.avatar_gender },
                { label: "App Opens", value: formatNumber(bitmoji.app_open_count) },
                { label: "Outfit Saves", value: formatNumber(bitmoji.outfit_save_count) },
                { label: "Shares", value: formatNumber(bitmoji.share_count) },
                { label: "Created", value: formatDate(bitmoji.account_created_at) },
              ]}
            />
          </SettingsCard>
        </div>
      </div>
    </section>
  );
}

function SettingsCard({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="rounded-[1.75rem] border border-slate-200/70 bg-white/85 p-5 shadow-[0_24px_50px_rgba(15,23,42,0.08)] backdrop-blur dark:border-white/10 dark:bg-white/[0.045] dark:shadow-black/20 md:p-6">
      <p className="text-xs font-semibold uppercase tracking-[0.24em] text-slate-500 dark:text-slate-400">{title}</p>
      <div className="mt-4">{children}</div>
    </div>
  );
}

function StatCard({ label, value, Icon }: { label: string; value: string; Icon: LucideIcon }) {
  return (
    <div className="relative rounded-[1.5rem] border border-slate-200/70 bg-white/80 p-4 shadow-[0_18px_40px_rgba(15,23,42,0.06)] dark:border-white/10 dark:bg-white/[0.04]">
      <Icon className="absolute right-4 top-4 h-5 w-5 text-slate-500 dark:text-slate-300" />
      <div className="pr-8">
        <p className="text-xs font-semibold uppercase tracking-[0.24em] text-slate-500 dark:text-slate-400">{label}</p>
        <p className="mt-3 text-2xl font-semibold tracking-tight text-slate-950 dark:text-white">{value}</p>
      </div>
    </div>
  );
}

function InfoList({ items }: { items: { label: string; value: string | null }[] }) {
  return (
    <dl className="grid gap-2 sm:grid-cols-2">
      {items.map((item) => (
        <div
          key={item.label}
          className="rounded-[1.2rem] border border-slate-200/70 bg-slate-50/80 px-4 py-3 dark:border-white/10 dark:bg-white/[0.035]"
        >
          <dt className="text-[11px] font-medium uppercase tracking-wide text-slate-400 dark:text-slate-500">
            {item.label}
          </dt>
          <dd className="mt-1 text-sm text-slate-800 dark:text-slate-200">{item.value || "—"}</dd>
        </div>
      ))}
    </dl>
  );
}
