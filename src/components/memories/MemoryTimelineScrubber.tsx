import { useRef, useState } from "react";

const TICK_SPACING = 34;

// Floating year-jump control for the Memories grid, styled as a vertical
// flagpole on the right edge: newest year at the top, oldest at the bottom.
// Click a tick to jump straight there, or drag the flag to scrub.
export default function MemoryTimelineScrubber({
  years,
  activeYear,
  onJump,
}: {
  years: number[];
  activeYear: number | null;
  onJump: (year: number) => void;
}) {
  const trackRef = useRef<HTMLDivElement | null>(null);
  const [dragYear, setDragYear] = useState<number | null>(null);

  if (years.length < 2) {
    // ponytail: not worth a scrubber when there's only one year to jump between.
    return null;
  }

  const sortedYears = [...years].sort((a, b) => b - a); // newest first = top of the pole
  const trackHeight = (sortedYears.length - 1) * TICK_SPACING;
  const displayYear = dragYear ?? activeYear;
  const activeIndex = displayYear !== null ? sortedYears.indexOf(displayYear) : -1;
  const progressPercent = activeIndex >= 0 ? (activeIndex / (sortedYears.length - 1)) * 100 : 0;

  function yearAtClientY(clientY: number): number {
    const rect = trackRef.current!.getBoundingClientRect();
    const ratio = rect.height === 0 ? 0 : Math.min(1, Math.max(0, (clientY - rect.top) / rect.height));
    return sortedYears[Math.round(ratio * (sortedYears.length - 1))];
  }

  function handleFlagPointerDown(event: React.PointerEvent) {
    event.currentTarget.setPointerCapture(event.pointerId);
    setDragYear(yearAtClientY(event.clientY));
  }

  function handleFlagPointerMove(event: React.PointerEvent) {
    if (dragYear === null) {
      return;
    }
    setDragYear(yearAtClientY(event.clientY));
  }

  function handleFlagPointerUp(event: React.PointerEvent) {
    if (dragYear === null) {
      return;
    }
    const year = yearAtClientY(event.clientY);
    setDragYear(null);
    onJump(year);
  }

  return (
    <div className="pointer-events-none absolute right-6 top-1/2 z-20 -translate-y-1/2">
      <div className="pointer-events-auto rounded-[20px] bg-surface/88 px-4 py-5 shadow-[0_0_0_1px_var(--color-divider),0_16px_40px_rgba(0,0,0,0.55)] backdrop-blur-md">
        <div ref={trackRef} className="relative w-1" style={{ height: trackHeight }}>
          <div className="absolute inset-x-0 inset-y-0 left-1/2 w-[2px] -translate-x-1/2 rounded-full bg-neutral-700" />
          <div
            className="absolute bottom-0 left-1/2 w-[2px] -translate-x-1/2 rounded-full bg-gradient-to-t from-accent/25 to-accent"
            style={{ height: `${progressPercent}%` }}
          />

          {sortedYears.map((year, index) => (
            <button
              key={year}
              type="button"
              onClick={() => onJump(year)}
              aria-label={`Jump to ${year}`}
              className="absolute left-1/2 flex -translate-x-1/2 -translate-y-1/2 items-center justify-center p-1.5"
              style={{ top: `${(index / (sortedYears.length - 1)) * 100}%` }}
            >
              <span
                className={
                  year === displayYear
                    ? "block h-4 w-4 rounded-full bg-accent shadow-[0_0_0_3px_var(--color-surface),0_0_16px_3px_color-mix(in_srgb,var(--color-accent)_65%,transparent)]"
                    : "block h-2.5 w-2.5 rounded-full bg-neutral-600 transition hover:bg-neutral-400"
                }
              />
            </button>
          ))}

          <button
            type="button"
            aria-label={`Currently at ${displayYear ?? sortedYears[0]}, drag to jump to another year`}
            onPointerDown={handleFlagPointerDown}
            onPointerMove={handleFlagPointerMove}
            onPointerUp={handleFlagPointerUp}
            className="absolute right-full flex -translate-y-1/2 cursor-grab touch-none items-center gap-1.5 pr-2.5 active:cursor-grabbing"
            style={{ top: `${progressPercent}%` }}
          >
            <span className="whitespace-nowrap rounded-md bg-accent px-2 py-0.5 text-[11px] font-semibold text-white shadow-[0_4px_10px_rgba(0,0,0,0.4)]">
              {displayYear}
            </span>
            <span className="h-[2px] w-2.5 bg-accent" />
          </button>
        </div>
      </div>
    </div>
  );
}
