import { Aperture, Filter, Settings } from "lucide-react";
import { useEffect, useState } from "react";
import { NavLink, useLocation } from "react-router-dom";

import type { ChatsScope, LayoutOutletContext } from "./Layout";
import type { MemoryFilter } from "../hooks/useMemories";

const NAV_ITEMS = [
  { to: "/", label: "Dashboard", end: true },
  { to: "/chats", label: "Chats" },
  { to: "/memories", label: "Memories" },
  { to: "/profile", label: "Profile" },
];

const MEMORY_FILTER_OPTIONS: { value: MemoryFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "favorite", label: "Favorites" },
  { value: "photo", label: "Photos" },
  { value: "video", label: "Videos" },
];

const CHATS_SCOPE_OPTIONS: { value: ChatsScope; label: string }[] = [
  { value: "all", label: "All" },
  { value: "group", label: "Group" },
  { value: "private", label: "1:1" },
];

type NavBarProps = {
  memoriesFilters: LayoutOutletContext["memoriesFilters"];
  chatsFilters: LayoutOutletContext["chatsFilters"];
};

export default function NavBar({ memoriesFilters, chatsFilters }: NavBarProps) {
  const location = useLocation();
  const [open, setOpen] = useState(false);
  const showFilter = location.pathname === "/chats" || location.pathname === "/memories";

  useEffect(() => {
    setOpen(false);
  }, [location.pathname]);

  return (
    <div className="flex h-[58px] shrink-0 items-center gap-2.5 border-b border-divider px-5">
      <span className="flex items-center gap-2.5">
        <Aperture className="h-[22px] w-[22px] text-accent" />
        <span className="text-base font-medium">SnapVault</span>
      </span>

      <nav className="mx-auto flex gap-1">
        {NAV_ITEMS.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            end={item.end}
            className={({ isActive }) =>
              [
                "rounded-lg px-3.5 py-2 text-sm transition",
                isActive ? "bg-accent/13 text-accent" : "text-neutral-400 hover:text-fg",
              ].join(" ")
            }
          >
            {item.label}
          </NavLink>
        ))}
      </nav>

      {showFilter ? (
        <div className="relative">
          <button
            type="button"
            onClick={() => setOpen((value) => !value)}
            className={[
              "flex h-9 w-9 items-center justify-center rounded-lg border transition",
              open ? "border-accent/40 text-accent" : "border-divider text-neutral-400 hover:text-fg",
            ].join(" ")}
            aria-label="Filters"
          >
            <Filter className="h-[16px] w-[16px]" />
          </button>
          {open ? (
            <>
              <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
              <div className="absolute right-0 top-[calc(100%+8px)] z-50 w-64 rounded-lg bg-surface p-3.5 ring-1 ring-divider">
                {location.pathname === "/memories" ? (
                  <MemoriesFilterPanel {...memoriesFilters} />
                ) : (
                  <ChatsFilterPanel {...chatsFilters} />
                )}
              </div>
            </>
          ) : null}
        </div>
      ) : null}

      <NavLink
        to="/settings"
        className={({ isActive }) =>
          [
            "flex h-9 w-9 items-center justify-center rounded-lg border transition",
            isActive ? "border-accent/40 text-accent" : "border-divider text-neutral-400 hover:text-fg",
          ].join(" ")
        }
        aria-label="Settings"
      >
        <Settings className="h-[17px] w-[17px]" />
      </NavLink>
    </div>
  );
}

function MemoriesFilterPanel({
  sort,
  filter,
  tag,
  setSort,
  setFilter,
  setTag,
  availableTags,
}: LayoutOutletContext["memoriesFilters"]) {
  return (
    <div className="flex flex-col gap-3">
      <div>
        <p className="mb-1.5 text-[10px] uppercase tracking-wide text-neutral-500">Sort</p>
        <button
          type="button"
          onClick={() => setSort(sort === "desc" ? "asc" : "desc")}
          className="btn btn-secondary w-full justify-start"
        >
          {sort === "desc" ? "Newest first" : "Oldest first"}
        </button>
      </div>

      <div>
        <p className="mb-1.5 text-[10px] uppercase tracking-wide text-neutral-500">Type</p>
        <select value={filter} onChange={(event) => setFilter(event.target.value as MemoryFilter)} className="input">
          {MEMORY_FILTER_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </div>

      <div>
        <p className="mb-1.5 text-[10px] uppercase tracking-wide text-neutral-500">Tag</p>
        <select value={tag ?? ""} onChange={(event) => setTag(event.target.value || null)} className="input">
          <option value="">All tags</option>
          {availableTags.map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
      </div>
    </div>
  );
}

function ChatsFilterPanel({ scope, setScope }: LayoutOutletContext["chatsFilters"]) {
  return (
    <div>
      <p className="mb-1.5 text-[10px] uppercase tracking-wide text-neutral-500">Show</p>
      <div className="flex gap-1.5">
        {CHATS_SCOPE_OPTIONS.map((option) => (
          <button
            key={option.value}
            type="button"
            onClick={() => setScope(option.value)}
            className={["btn flex-1", scope === option.value ? "btn-primary" : "btn-secondary"].join(" ")}
          >
            {option.label}
          </button>
        ))}
      </div>
    </div>
  );
}
