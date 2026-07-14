import { Menu, X } from "lucide-react";
import { useState } from "react";
import { Outlet, useLocation } from "react-router-dom";

import Sidebar from "./Sidebar";
import TitleBar from "./TitleBar";

export default function Layout() {
  const [mobileOpen, setMobileOpen] = useState(false);
  const [sidebarHovered, setSidebarHovered] = useState(false);
  const location = useLocation();
  const isContainedScrollRoute = location.pathname === "/chats" || location.pathname === "/memories";

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-[radial-gradient(circle_at_top,_rgba(14,165,233,0.08),_transparent_26%),linear-gradient(180deg,_#edf4fb,_#e6eef8_48%,_#dbe7f2)] text-slate-900 dark:bg-[radial-gradient(circle_at_top,_rgba(14,165,233,0.08),_transparent_26%),linear-gradient(180deg,_#06101a,_#04070c_48%,_#020407)] dark:text-slate-100">
      <TitleBar />

      <div className="flex min-h-0 flex-1">
        {/* Fixed-width slot reserves layout space for the collapsed sidebar so
            main content never reflows; the expanded panel on hover overlays on
            top of it instead (absolute), which is why it's a group. */}
        <div
          className="relative hidden w-24 shrink-0 xl:block"
          onMouseEnter={() => setSidebarHovered(true)}
          onMouseLeave={() => setSidebarHovered(false)}
        >
          <aside
            className={[
              "absolute inset-y-0 left-0 z-40 border-r border-slate-200/70 bg-white/90 p-5 shadow-2xl shadow-slate-900/10 backdrop-blur transition-[width] duration-200 ease-out dark:border-white/10 dark:bg-slate-950/85 dark:shadow-black/40",
              sidebarHovered ? "w-[16.5rem]" : "w-24",
            ].join(" ")}
          >
            <Sidebar collapsed={!sidebarHovered} />
          </aside>
        </div>

        <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          <div className="flex items-center gap-3 px-4 py-4 md:px-6 xl:hidden">
            <button
              type="button"
              onClick={() => setMobileOpen(true)}
              className="inline-flex rounded-[1.1rem] border border-slate-200 bg-white/80 p-3 text-slate-700 transition hover:border-slate-300 hover:bg-white dark:border-white/10 dark:bg-white/5 dark:text-slate-200 dark:hover:border-white/15 dark:hover:bg-white/10"
              aria-label="Open navigation"
            >
              <Menu className="h-5 w-5" />
            </button>
          </div>

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
