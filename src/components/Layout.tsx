import { Menu, X } from "lucide-react";
import { useState } from "react";
import { Outlet, useLocation } from "react-router-dom";

import Sidebar from "./Sidebar";

const routeMeta: Record<string, { eyebrow: string; title: string }> = {
  "/": {
    eyebrow: "Overview",
    title: "Dashboard",
  },
  "/profile": {
    eyebrow: "Workspace",
    title: "Profile",
  },
  "/chats": {
    eyebrow: "Conversations",
    title: "Chats",
  },
  "/memories": {
    eyebrow: "Archive",
    title: "Memories",
  },
  "/stories": {
    eyebrow: "Archive",
    title: "Stories",
  },
  "/settings": {
    eyebrow: "Workspace",
    title: "Settings",
  },
};

export default function Layout() {
  const [mobileOpen, setMobileOpen] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const location = useLocation();
  const isContainedScrollRoute = location.pathname === "/chats" || location.pathname === "/memories";

  const header = routeMeta[location.pathname] ?? routeMeta["/"];

  return (
    <div className="h-screen overflow-hidden bg-[radial-gradient(circle_at_top,_rgba(14,165,233,0.08),_transparent_26%),linear-gradient(180deg,_#edf4fb,_#e6eef8_48%,_#dbe7f2)] text-slate-900 dark:bg-[radial-gradient(circle_at_top,_rgba(14,165,233,0.08),_transparent_26%),linear-gradient(180deg,_#06101a,_#04070c_48%,_#020407)] dark:text-slate-100">
      <div className="flex h-full min-h-0">
        <aside
          className={[
            "hidden shrink-0 border-r border-slate-200/70 bg-white/75 p-5 backdrop-blur dark:border-white/10 dark:bg-slate-950/70 xl:block",
            sidebarCollapsed ? "w-24" : "w-[16.5rem]",
          ].join(" ")}
        >
          <Sidebar
            collapsed={sidebarCollapsed}
            onToggleCollapse={() => setSidebarCollapsed((value) => !value)}
          />
        </aside>

        <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          <header className="sticky top-0 z-30 border-b border-slate-200/70 bg-white/70 backdrop-blur dark:border-white/10 dark:bg-slate-950/55">
            <div className="flex items-center justify-between gap-4 px-4 py-4 md:px-6 xl:px-8">
              <div className="flex items-center gap-3 xl:hidden">
                <button
                  type="button"
                  onClick={() => setMobileOpen(true)}
                  className="inline-flex rounded-[1.1rem] border border-slate-200 bg-white/80 p-3 text-slate-700 transition hover:border-slate-300 hover:bg-white dark:border-white/10 dark:bg-white/5 dark:text-slate-200 dark:hover:border-white/15 dark:hover:bg-white/10"
                  aria-label="Open navigation"
                >
                  <Menu className="h-5 w-5" />
                </button>
              </div>

              <div className="min-w-0 flex-1">
                <p className="text-[11px] font-semibold uppercase tracking-[0.32em] text-sky-700/70 dark:text-sky-200/65">
                  {header.eyebrow}
                </p>
                <div className="mt-2 flex flex-col gap-2 md:flex-row md:items-end md:justify-between">
                  <div className="min-w-0">
                    <h1 className="truncate text-2xl font-semibold tracking-tight text-slate-950 dark:text-white md:text-3xl">
                      {header.title}
                    </h1>
                  </div>
                </div>
              </div>
            </div>
          </header>

          <main
            className={[
              "min-h-0 flex-1 px-4 py-4 md:px-6 md:py-6 xl:px-8 xl:py-8",
              isContainedScrollRoute ? "overflow-hidden" : "overflow-y-auto",
            ].join(" ")}
          >
            <Outlet />
          </main>
        </div>
      </div>

      {mobileOpen ? (
        <div className="fixed inset-0 z-50 bg-slate-950/40 backdrop-blur-sm dark:bg-slate-950/70 xl:hidden">
          <button
            type="button"
            className="absolute inset-0"
            aria-label="Close navigation"
            onClick={() => setMobileOpen(false)}
          />
          <div className="absolute inset-y-0 left-0 w-[19rem] max-w-[86vw] border-r border-slate-200/70 bg-white/96 p-5 shadow-2xl shadow-slate-900/10 dark:border-white/10 dark:bg-slate-950/96 dark:shadow-black/40">
            <div className="mb-4 flex justify-end">
              <button
                type="button"
                onClick={() => setMobileOpen(false)}
                className="inline-flex rounded-[1rem] border border-slate-200 bg-white p-2.5 text-slate-700 transition hover:border-slate-300 hover:bg-slate-50 dark:border-white/10 dark:bg-white/5 dark:text-slate-200 dark:hover:border-white/15 dark:hover:bg-white/10"
                aria-label="Close navigation"
              >
                <X className="h-4.5 w-4.5" />
              </button>
            </div>
            <Sidebar onNavigate={() => setMobileOpen(false)} />
          </div>
        </div>
      ) : null}
    </div>
  );
}
