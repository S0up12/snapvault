import { Archive, House, MessageSquareMore, Settings, UserRound } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { NavLink } from "react-router-dom";

type SidebarProps = {
  collapsed?: boolean;
  onNavigate?: () => void;
};

type NavItem = { to: string; label: string; icon: LucideIcon; end?: boolean };

// Stories has no route entry: Snapchat exports don't bundle actual story
// media (no stories/story_media folder, story_history.json/shared_story.json
// are activity logs only - view/reply counts and expired story.snapchat.com
// URLs, no downloadable content). The view, route, and story_collections/
// story_items schema are left in place as scaffolding for if that changes.
const navigationItems: NavItem[] = [
  { to: "/", label: "Dashboard", icon: House, end: true },
  { to: "/profile", label: "Profile", icon: UserRound },
  { to: "/chats", label: "Chats", icon: MessageSquareMore },
  { to: "/memories", label: "Memories", icon: Archive },
];

const settingsItem: NavItem = { to: "/settings", label: "Settings", icon: Settings };

function SidebarNavItem({ item, collapsed, onNavigate }: { item: NavItem; collapsed: boolean; onNavigate?: () => void }) {
  const Icon = item.icon;

  return (
    <NavLink
      to={item.to}
      end={item.end}
      onClick={onNavigate}
      className={[
        "group flex items-center gap-3 rounded-[1.2rem] px-3 py-3 text-sm font-medium transition",
        collapsed ? "justify-center" : "",
      ].join(" ")}
    >
      {({ isActive }) => (
        <>
          <span
            className={[
              "flex h-10 w-10 shrink-0 items-center justify-center rounded-[1rem] border transition",
              isActive
                ? "border-sky-300/30 bg-sky-400/[0.16] text-sky-700 dark:border-sky-300/25 dark:bg-sky-300/[0.14] dark:text-sky-200"
                : "border-slate-200 bg-white text-slate-700 group-hover:border-slate-300 group-hover:bg-slate-50 dark:border-white/10 dark:bg-white/[0.045] dark:text-slate-300 dark:group-hover:border-white/15 dark:group-hover:bg-white/[0.07]",
            ].join(" ")}
          >
            <Icon className="h-4.5 w-4.5" />
          </span>
          {!collapsed ? (
            <span
              className={[
                "truncate transition",
                isActive
                  ? "text-slate-950 dark:text-white"
                  : "text-slate-600 group-hover:text-slate-900 dark:text-slate-400 dark:group-hover:text-slate-100",
              ].join(" ")}
            >
              {item.label}
            </span>
          ) : null}
        </>
      )}
    </NavLink>
  );
}

export default function Sidebar({ collapsed = false, onNavigate }: SidebarProps) {
  return (
    <div className="flex h-full flex-col">
      <nav className="space-y-2">
        {navigationItems.map((item) => (
          <SidebarNavItem key={item.to} item={item} collapsed={collapsed} onNavigate={onNavigate} />
        ))}
      </nav>

      <div className="mt-auto">
        <SidebarNavItem item={settingsItem} collapsed={collapsed} onNavigate={onNavigate} />
      </div>
    </div>
  );
}
