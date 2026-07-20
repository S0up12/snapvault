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
    <div className="dialog-backdrop z-[60]" onClick={onClose}>
      <div className="dialog w-full max-w-md" onClick={(event) => event.stopPropagation()}>
        <div className="flex items-center justify-between">
          <div>
            <p className="text-[10px] uppercase tracking-[0.14em] text-accent">Asset Tags</p>
            <h2 className="dialog-title mt-1">Edit Tags</h2>
            <p className="mt-0.5 truncate text-xs text-neutral-500">{assetId}</p>
          </div>
          <button type="button" onClick={onClose} className="btn btn-secondary btn-icon">
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="flex gap-2">
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
            className="input flex-1"
          />
          <datalist id="memory-tag-suggestions">
            {availableTags.map((tag) => (
              <option key={tag} value={tag} />
            ))}
          </datalist>
          <button type="button" onClick={() => addTag(input)} className="btn btn-primary">
            Add
          </button>
        </div>

        <div className="flex min-h-10 flex-wrap gap-2">
          {draftTags.length === 0 ? (
            <p className="text-xs text-neutral-500">No tags yet.</p>
          ) : (
            draftTags.map((tag) => (
              <span key={tag} className="tag tag-neutral gap-1.5 py-1.5">
                <Tag className="h-3 w-3" />
                {tag}
                <button
                  type="button"
                  onClick={() => removeTag(tag)}
                  className="text-neutral-500 transition hover:text-red-400"
                  aria-label={`Remove tag ${tag}`}
                >
                  <X className="h-3 w-3" />
                </button>
              </span>
            ))
          )}
        </div>

        <div className="flex justify-end gap-2">
          <button type="button" onClick={onClose} className="btn btn-secondary">
            Cancel
          </button>
          <button type="button" onClick={handleSave} disabled={isSaving} className="btn btn-primary">
            {isSaving ? "Saving..." : "Save Tags"}
          </button>
        </div>
      </div>
    </div>
  );
}
