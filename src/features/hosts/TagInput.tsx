import { useMemo, useRef, useState, type KeyboardEvent } from "react";
import { Form } from "react-bootstrap";
import { X } from "lucide-react";

/**
 * Chip-style tag entry.
 *
 * Enter, Tab, and comma all commit — people type all three, and losing a tag
 * because the wrong key was pressed is the kind of small friction that stops
 * anyone tagging anything. Backspace on an empty field removes the last chip,
 * which is what every other tag input does.
 */
export function TagInput({
  value,
  onChange,
  suggestions = [],
  id,
}: {
  value: string[];
  onChange: (tags: string[]) => void;
  suggestions?: string[];
  id?: string;
}) {
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const listId = `${id ?? "tags"}-suggestions`;

  // Only offer tags that are not already on this host.
  const available = useMemo(() => {
    const taken = new Set(value.map((tag) => tag.toLowerCase()));
    return suggestions.filter((tag) => !taken.has(tag.toLowerCase()));
  }, [suggestions, value]);

  const commit = (raw: string) => {
    const tag = raw.trim().replace(/,+$/, "").trim();
    if (!tag) return;
    // Case-insensitive, so `Web` and `web` cannot both end up on one host.
    if (value.some((existing) => existing.toLowerCase() === tag.toLowerCase())) {
      setDraft("");
      return;
    }
    onChange([...value, tag]);
    setDraft("");
  };

  const removeAt = (index: number) =>
    onChange(value.filter((_, position) => position !== index));

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter" || event.key === ",") {
      // Enter would otherwise submit the surrounding form.
      event.preventDefault();
      commit(draft);
      return;
    }
    if (event.key === "Tab" && draft.trim()) {
      event.preventDefault();
      commit(draft);
      return;
    }
    if (event.key === "Backspace" && draft === "" && value.length > 0) {
      removeAt(value.length - 1);
    }
  };

  return (
    <div
      className="tag-input"
      onClick={() => inputRef.current?.focus()}
      role="presentation"
    >
      {value.map((tag, index) => (
        <span key={tag} className="tag-chip tag-chip--removable">
          {tag}
          <button
            type="button"
            className="tag-chip__remove"
            onClick={(event) => {
              event.stopPropagation();
              removeAt(index);
            }}
            aria-label={`Remove tag ${tag}`}
          >
            <X aria-hidden="true" />
          </button>
        </span>
      ))}

      <Form.Control
        ref={inputRef}
        id={id}
        className="tag-input__field"
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={handleKeyDown}
        // Committing on blur saves the tag someone typed before clicking Save.
        onBlur={() => commit(draft)}
        placeholder={value.length === 0 ? "web, database, critical…" : ""}
        list={available.length > 0 ? listId : undefined}
        spellCheck={false}
        autoComplete="off"
        aria-label="Tags"
      />

      {available.length > 0 && (
        <datalist id={listId}>
          {available.map((tag) => (
            <option key={tag} value={tag} />
          ))}
        </datalist>
      )}
    </div>
  );
}
