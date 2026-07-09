import { ArrowDownWideNarrow, ArrowUpWideNarrow } from "lucide-react";

import type { MemoryFilter, MemorySort } from "../../hooks/useMemories";

type MemoriesToolbarProps = {
  sort: MemorySort;
  filter: MemoryFilter;
  tag: string | null;
  availableTags: string[];
  total: number;
  isLoading: boolean;
  onSortChange: (value: MemorySort) => void;
  onFilterChange: (value: MemoryFilter) => void;
  onTagChange: (value: string | null) => void;
};

const FILTER_OPTIONS: { value: MemoryFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "favorite", label: "Favorites" },
  { value: "photo", label: "Photos" },
  { value: "video", label: "Videos" },
];

const selectClassName =
  "rounded-[1rem] border border-slate-200/80 bg-white px-3 py-2 text-sm text-slate-700 shadow-sm transition hover:border-slate-300 focus:outline-none focus:ring-2 focus:ring-sky-300/40 dark:border-white/10 dark:bg-slate-950/70 dark:text-slate-200 dark:hover:border-white/20";

export default function MemoriesToolbar({
  sort,
  filter,
  tag,
  availableTags,
  total,
  isLoading,
  onSortChange,
  onFilterChange,
  onTagChange,
}: MemoriesToolbarProps) {
  return (
    <section className="flex flex-col gap-3 rounded-[1.6rem] border border-slate-200/80 bg-white/80 px-4 py-3 shadow-[0_20px_48px_rgba(15,23,42,0.08)] dark:border-white/10 dark:bg-white/[0.045] md:flex-row md:items-center md:justify-between">
      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={() => onSortChange(sort === "desc" ? "asc" : "desc")}
          className={`${selectClassName} inline-flex items-center gap-2`}
          title={sort === "desc" ? "Newest first" : "Oldest first"}
        >
          {sort === "desc" ? <ArrowDownWideNarrow className="h-4 w-4" /> : <ArrowUpWideNarrow className="h-4 w-4" />}
          {sort === "desc" ? "Newest first" : "Oldest first"}
        </button>

        <select
          value={filter}
          onChange={(event) => onFilterChange(event.target.value as MemoryFilter)}
          className={selectClassName}
        >
          {FILTER_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>

        <select
          value={tag ?? ""}
          onChange={(event) => onTagChange(event.target.value || null)}
          className={selectClassName}
        >
          <option value="">All tags</option>
          {availableTags.map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
      </div>

      <p className="text-xs uppercase tracking-[0.22em] text-slate-500 dark:text-slate-400">
        {isLoading ? "Loading..." : `${total.toLocaleString()} ${total === 1 ? "memory" : "memories"}`}
      </p>
    </section>
  );
}
