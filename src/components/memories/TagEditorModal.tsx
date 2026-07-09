import { Tag, X } from "lucide-react";
import { useState } from "react";

type TagEditorModalProps = {
  assetId: string;
  initialTags: string[];
  availableTags: string[];
  onClose: () => void;
  onSave: (tags: string[]) => Promise<void>;
};

export default function TagEditorModal({ assetId, initialTags, availableTags, onClose, onSave }: TagEditorModalProps) {
  const [draftTags, setDraftTags] = useState<string[]>(initialTags);
  const [input, setInput] = useState("");
  const [isSaving, setIsSaving] = useState(false);

  function addTag(value: string) {
    const trimmed = value.trim();
    if (!trimmed) {
      return;
    }
    const exists = draftTags.some((tag) => tag.toLocaleLowerCase() === trimmed.toLocaleLowerCase());
    if (!exists) {
      setDraftTags((current) => [...current, trimmed]);
    }
    setInput("");
  }

  function removeTag(tag: string) {
    setDraftTags((current) => current.filter((t) => t !== tag));
  }

  async function handleSave() {
    setIsSaving(true);
    try {
      await onSave(draftTags);
      onClose();
    } finally {
      setIsSaving(false);
    }
  }

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-slate-950/55 p-4 backdrop-blur-sm" onClick={onClose}>
      <div
        className="w-full max-w-md rounded-[1.8rem] border border-slate-200/80 bg-white/96 p-6 shadow-2xl shadow-slate-900/20 dark:border-white/10 dark:bg-slate-950/95"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <div>
            <p className="text-[11px] font-semibold uppercase tracking-[0.28em] text-sky-700/70 dark:text-sky-200/65">
              Asset Tags
            </p>
            <h2 className="mt-1 text-lg font-semibold text-slate-950 dark:text-white">Edit Tags</h2>
            <p className="mt-0.5 truncate text-xs text-slate-400 dark:text-slate-500">{assetId}</p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded-xl border border-slate-200 bg-white p-2 text-slate-600 transition hover:border-slate-300 hover:bg-slate-50 dark:border-white/10 dark:bg-white/5 dark:text-slate-300 dark:hover:bg-white/10"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="mt-5 flex gap-2">
          <input
            type="text"
            list="memory-tag-suggestions"
            value={input}
            onChange={(event) => setInput(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                addTag(input);
              }
            }}
            placeholder="Add a tag..."
            className="flex-1 rounded-[1rem] border border-slate-200/80 bg-white px-3 py-2 text-sm text-slate-900 focus:outline-none focus:ring-2 focus:ring-sky-300/40 dark:border-white/10 dark:bg-white/[0.04] dark:text-white"
          />
          <datalist id="memory-tag-suggestions">
            {availableTags.map((tag) => (
              <option key={tag} value={tag} />
            ))}
          </datalist>
          <button
            type="button"
            onClick={() => addTag(input)}
            className="rounded-[1rem] border border-sky-300/30 bg-sky-500/10 px-4 py-2 text-sm font-medium text-sky-700 transition hover:bg-sky-500/15 dark:text-sky-200"
          >
            Add
          </button>
        </div>

        <div className="mt-4 flex min-h-[2.5rem] flex-wrap gap-2">
          {draftTags.length === 0 ? (
            <p className="text-xs text-slate-400 dark:text-slate-500">No tags yet.</p>
          ) : (
            draftTags.map((tag) => (
              <span
                key={tag}
                className="inline-flex items-center gap-1.5 rounded-full border border-slate-200/80 bg-slate-50 px-3 py-1.5 text-xs font-medium text-slate-700 dark:border-white/10 dark:bg-white/[0.06] dark:text-slate-200"
              >
                <Tag className="h-3 w-3" />
                {tag}
                <button
                  type="button"
                  onClick={() => removeTag(tag)}
                  className="text-slate-400 transition hover:text-red-500 dark:text-slate-500"
                  aria-label={`Remove tag ${tag}`}
                >
                  <X className="h-3 w-3" />
                </button>
              </span>
            ))
          )}
        </div>

        <div className="mt-6 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-[1rem] border border-slate-200 bg-white px-4 py-2 text-sm font-medium text-slate-700 transition hover:bg-slate-50 dark:border-white/10 dark:bg-white/5 dark:text-slate-200 dark:hover:bg-white/10"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={handleSave}
            disabled={isSaving}
            className="rounded-[1rem] border border-sky-300/30 bg-sky-500 px-4 py-2 text-sm font-medium text-white transition hover:bg-sky-600 disabled:cursor-not-allowed disabled:opacity-60"
          >
            {isSaving ? "Saving..." : "Save Tags"}
          </button>
        </div>
      </div>
    </div>
  );
}
