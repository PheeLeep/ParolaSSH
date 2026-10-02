import { hintText, useKeybindings } from "../lib/keybindings";

/** The shortcut for an action, shown inline. Hidden when hints are off. */
export function KeyBadge({ id, slot }: { id: string; slot?: number }) {
  useKeybindings();
  const text = hintText(id, slot);
  return text ? <kbd className="key-badge">{text}</kbd> : null;
}
