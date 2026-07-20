import { useState } from "react";
import { Outlet, useLocation } from "react-router-dom";

import NavBar from "./NavBar";
import TitleBar from "./TitleBar";
import { useMemoryTags } from "../hooks/useMemories";
import type { MemoryFilter, MemorySort } from "../hooks/useMemories";

export type ChatsScope = "all" | "group" | "private";

export type LayoutOutletContext = {
  memoriesFilters: {
    sort: MemorySort;
    filter: MemoryFilter;
    tag: string | null;
    setSort: (value: MemorySort) => void;
    setFilter: (value: MemoryFilter) => void;
    setTag: (value: string | null) => void;
    availableTags: string[];
    refreshTags: () => Promise<void>;
  };
  chatsFilters: {
    scope: ChatsScope;
    setScope: (value: ChatsScope) => void;
  };
};

export default function Layout() {
  const location = useLocation();
  const isContainedScrollRoute = location.pathname === "/chats" || location.pathname === "/memories";

  const [sort, setSort] = useState<MemorySort>("desc");
  const [filter, setFilter] = useState<MemoryFilter>("all");
  const [tag, setTag] = useState<string | null>(null);
  const { tags: availableTags, refresh: refreshTags } = useMemoryTags();

  const [scope, setScope] = useState<ChatsScope>("all");

  const context: LayoutOutletContext = {
    memoriesFilters: { sort, filter, tag, setSort, setFilter, setTag, availableTags, refreshTags },
    chatsFilters: { scope, setScope },
  };

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-bg text-fg">
      <TitleBar />
      <NavBar memoriesFilters={context.memoriesFilters} chatsFilters={context.chatsFilters} />

      <main
        className={[
          "min-h-0 flex-1 px-4 py-4 md:px-6 md:py-6 xl:px-8 xl:py-8",
          isContainedScrollRoute ? "overflow-hidden" : "overflow-y-auto",
        ].join(" ")}
      >
        <Outlet context={context} />
      </main>
    </div>
  );
}
