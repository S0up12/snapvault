export default function Stat({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="rounded-md bg-white/[0.03] px-3 py-2 ring-1 ring-divider">
      <p className="text-[10px] uppercase tracking-wide text-neutral-500">{label}</p>
      <p className="mt-0.5 text-base font-medium text-fg">
        {typeof value === "number" ? value.toLocaleString() : value}
      </p>
    </div>
  );
}
