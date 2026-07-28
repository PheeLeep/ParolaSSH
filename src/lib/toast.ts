/** Transient status messages, pushed from anywhere.
 *
 *  A module singleton rather than a context, for the reason `terminalStore` is
 *  one: the things worth announcing - a transfer finishing, a folder being
 *  created - happen in code that outlives whichever pane started them, and a
 *  toast raised from a component that has since unmounted must still appear.
 *
 *  Errors that a pane can show inline should stay inline. This is for outcomes
 *  the user would otherwise miss because they have navigated away.
 */

export type ToastKind = "progress" | "success" | "error";

export type Toast = {
  id: number;
  kind: ToastKind;
  title: string;
  detail?: string;
  /** Progress toasts stay until replaced or dismissed; the rest expire. */
  sticky: boolean;
};

/** Long enough to read a filename, short enough not to stack up. */
const DISMISS_AFTER = { success: 4000, error: 9000 } as const;

let toasts: Toast[] = [];
let nextId = 1;

const listeners = new Set<() => void>();
const timers = new Map<number, ReturnType<typeof setTimeout>>();

function emit() {
  for (const listener of listeners) listener();
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getSnapshot(): Toast[] {
  return toasts;
}

export function dismiss(id: number): void {
  const timer = timers.get(id);
  if (timer) {
    clearTimeout(timer);
    timers.delete(id);
  }
  toasts = toasts.filter((toast) => toast.id !== id);
  emit();
}

function push(kind: ToastKind, title: string, detail?: string): number {
  const id = nextId++;
  const sticky = kind === "progress";
  toasts = [...toasts, { id, kind, title, detail, sticky }];

  if (!sticky) {
    timers.set(
      id,
      setTimeout(() => dismiss(id), DISMISS_AFTER[kind]),
    );
  }

  emit();
  return id;
}

export const success = (title: string, detail?: string) => push("success", title, detail);
export const error = (title: string, detail?: string) => push("error", title, detail);

/** A toast that stays until you settle it. Returns a handle that replaces it
 *  in place, so "Deleting…" becomes "Deleted" rather than stacking two. */
export function progress(title: string, detail?: string) {
  const id = push("progress", title, detail);

  const replace = (kind: Exclude<ToastKind, "progress">, nextTitle: string, nextDetail?: string) => {
    dismiss(id);
    return push(kind, nextTitle, nextDetail);
  };

  return {
    id,
    succeed: (nextTitle: string, detail?: string) => replace("success", nextTitle, detail),
    fail: (nextTitle: string, detail?: string) => replace("error", nextTitle, detail),
    dismiss: () => dismiss(id),
  };
}
