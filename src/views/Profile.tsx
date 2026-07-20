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
      <div className="flex h-full items-center justify-center text-neutral-500">
        <LoaderCircle className="h-6 w-6 animate-spin" />
      </div>
    );
  }

  if (error) {
    return <div className="rounded-md bg-red-500/8 px-4 py-3 text-sm text-red-300 ring-1 ring-red-400/25">{error}</div>;
  }

  if (!profile) {
    return (
      <div className="rounded-lg bg-surface p-8 text-sm text-neutral-400 ring-1 ring-divider">
        No profile data yet. Import a Snapchat export from the Dashboard first.
      </div>
    );
  }

  const { account, friends, ranking, engagement, bitmoji } = profile.snapshot;
  const displayName = account.display_name || account.username || "Snapchat Profile";

  return (
    <section className="mx-auto flex w-full max-w-[1400px] flex-col gap-5">
      <div className="relative overflow-hidden rounded-lg bg-surface p-6.5 ring-1 ring-divider">
        <div className="pointer-events-none absolute -right-8 -top-16 h-56 w-80 rounded-full bg-[radial-gradient(circle,color-mix(in_srgb,var(--color-accent)_26%,transparent),transparent_68%)]" />
        <div className="relative flex flex-wrap items-center gap-5">
          <span className="flex h-19 w-19 shrink-0 items-center justify-center rounded-[18px] bg-gradient-to-br from-accent-500 to-accent-700 text-[26px] font-semibold text-white shadow-[0_0_0_1px_color-mix(in_srgb,var(--color-accent)_40%,transparent)]">
            {avatarInitials(displayName)}
          </span>
          <div className="min-w-0">
            <p className="text-[10px] uppercase tracking-[0.14em] text-accent">Imported profile</p>
            <h1 className="mt-1 text-[27px] tracking-[-0.02em]">{displayName}</h1>
            <div className="mt-3 flex flex-wrap gap-2">
              {account.username ? <span className="tag tag-neutral">@{account.username}</span> : null}
              <span className="tag tag-neutral">Joined {formatDate(account.created_at)}</span>
              {account.country ? <span className="tag tag-neutral">{account.country}</span> : null}
              <span className="tag tag-outline">Snapscore {formatNumber(ranking.snapscore)}</span>
            </div>
          </div>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-3.5 sm:grid-cols-4">
        <StatCard label="Friends" value={formatNumber(friends.friends_count)} Icon={Users} accent />
        <StatCard label="Snapscore" value={formatNumber(ranking.snapscore)} Icon={Trophy} />
        <StatCard label="Memories" value={formatNumber(profile.memory_count)} Icon={Archive} />
        <StatCard label="Bitmoji opens" value={formatNumber(bitmoji.app_open_count)} Icon={Smile} />
      </div>

      <div className="grid gap-5 xl:grid-cols-2">
        <div className="flex flex-col gap-5">
          <DetailCard title="Account">
            <InfoList
              items={[
                { label: "Display name", value: account.display_name },
                { label: "Username", value: account.username },
                { label: "Created", value: formatDate(account.created_at) },
                { label: "Country", value: account.country },
                { label: "Language", value: account.in_app_language },
                { label: "Registration IP", value: account.registration_ip },
              ]}
            />
          </DetailCard>

          <DetailCard title="Friends">
            <div className="grid grid-cols-3 gap-2.5">
              <MiniStat label="Current" value={formatNumber(friends.friends_count)} />
              <MiniStat label="Blocked" value={formatNumber(friends.blocked_count)} />
              <MiniStat label="Deleted" value={formatNumber(friends.deleted_count)} />
            </div>
            {friends.top_friends.length > 0 ? (
              <div className="mt-4 flex flex-col gap-1.5">
                {friends.top_friends.map((friend) => (
                  <div
                    key={friend.Username}
                    className="flex items-center justify-between rounded-md bg-white/[0.04] px-3.5 py-2.25"
                  >
                    <span className="text-[13.5px] text-neutral-200">
                      {friend["Display Name"] || friend.Username}
                    </span>
                    <span className="text-xs text-neutral-500">@{friend.Username}</span>
                  </div>
                ))}
              </div>
            ) : null}
          </DetailCard>
        </div>

        <div className="flex flex-col gap-5">
          <DetailCard title="Engagement">
            <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-3">
              <EngagementStat label="App opens" value={formatNumber(engagement.application_opens)} />
              <EngagementStat label="Story views" value={formatNumber(engagement.story_views)} />
              <EngagementStat label="Snap views" value={formatNumber(engagement.snap_views)} />
              <EngagementStat label="Chats sent" value={formatNumber(engagement.chats_sent)} />
              <EngagementStat label="Chats viewed" value={formatNumber(engagement.chats_viewed)} />
              <EngagementStat label="Direct snaps" value={formatNumber(engagement.direct_snaps_created)} />
            </div>
          </DetailCard>

          <DetailCard title="Bitmoji">
            <div className="flex flex-col gap-px">
              <BitmojiRow label="Avatar gender" value={bitmoji.avatar_gender ?? "—"} />
              <BitmojiRow label="Outfit saves" value={formatNumber(bitmoji.outfit_save_count)} />
              <BitmojiRow label="Shares" value={formatNumber(bitmoji.share_count)} />
              <BitmojiRow label="Created" value={formatDate(bitmoji.account_created_at)} last />
            </div>
          </DetailCard>
        </div>
      </div>
    </section>
  );
}

function DetailCard({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="rounded-lg bg-surface p-5.5 ring-1 ring-divider">
      <p className="mb-3.5 text-[10px] uppercase tracking-[0.14em] text-neutral-500">{title}</p>
      {children}
    </div>
  );
}

function StatCard({ label, value, Icon, accent }: { label: string; value: string; Icon: LucideIcon; accent?: boolean }) {
  return (
    <div className="relative rounded-md bg-surface p-4.5 ring-1 ring-divider">
      <Icon className="absolute right-4 top-4 h-5 w-5 text-neutral-600" />
      <p className={["text-[10px] tracking-[0.12em] uppercase", accent ? "text-accent" : "text-neutral-500"].join(" ")}>
        {label}
      </p>
      <p className="mt-2 text-[28px] font-medium tracking-[-0.02em]">{value}</p>
    </div>
  );
}

function MiniStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md bg-white/[0.04] px-2 py-3 text-center">
      <div className="text-[22px] font-medium">{value}</div>
      <div className="mt-1 text-[10px] uppercase tracking-wide text-neutral-500">{label}</div>
    </div>
  );
}

function EngagementStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md bg-white/[0.04] p-3.25">
      <div className="text-[19px] font-medium">{value}</div>
      <div className="mt-1 text-[10px] uppercase tracking-wide text-neutral-500">{label}</div>
    </div>
  );
}

function BitmojiRow({ label, value, last }: { label: string; value: string; last?: boolean }) {
  return (
    <>
      <div className="flex justify-between px-0.5 py-2.25 text-[13.5px]">
        <span className="text-neutral-400">{label}</span>
        <span>{value}</span>
      </div>
      {last ? null : (
        <div className="h-px bg-[linear-gradient(to_right,transparent,var(--color-divider)_24px,var(--color-divider)_calc(100%-24px),transparent)]" />
      )}
    </>
  );
}

function InfoList({ items }: { items: { label: string; value: string | null }[] }) {
  return (
    <dl className="grid grid-cols-2 gap-2.5">
      {items.map((item) => (
        <div key={item.label} className="rounded-md bg-white/[0.04] px-3.5 py-2.75">
          <dt className="text-[10px] uppercase tracking-[0.08em] text-neutral-500">{item.label}</dt>
          <dd className="mt-1 text-sm text-neutral-200">{item.value || "—"}</dd>
        </div>
      ))}
    </dl>
  );
}
