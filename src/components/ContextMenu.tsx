import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { LucideIcon } from "lucide-react";

export type ContextMenuItem =
  | { separator: true }
  | {
      separator?: false;
      label: string;
      Icon?: LucideIcon;
      onSelect: () => void;
      disabled?: boolean;
      /** Shown on hover - the place to say *why* an entry is disabled. */
      title?: string;
      danger?: boolean;
    };

/** Where the menu was summoned, in viewport coordinates. */
export type ContextMenuAnchor = { x: number; y: number };

const EDGE_GAP = 8;

/**
 * A right-click menu anchored to a point. Portalled to `<body>` so a row
 * inside a scroll container cannot clip it, and positioned `fixed` so it does
 * not count towards that container's overflow.
 *
 * Anything that moves the anchor out from under it - scrolling, resizing,
 * losing the window - closes it rather than leaving it stranded.
 */
export function ContextMenu({
  anchor,
  items,
  onClose,
  label,
}: {
  anchor: ContextMenuAnchor | null;
  items: ContextMenuItem[];
  onClose: () => void;
  label: string;
}) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<ContextMenuAnchor | null>(null);

  // Flip into the viewport before paint, so the menu never appears offscreen
  // for a frame and then jumps.
  useLayoutEffect(() => {
    const node = menuRef.current;
    if (!anchor || !node) {
      setPosition(null);
      return;
    }
    const { width, height } = node.getBoundingClientRect();
    setPosition({
      x: Math.max(
        EDGE_GAP,
        Math.min(anchor.x, window.innerWidth - width - EDGE_GAP),
      ),
      y: Math.max(
        EDGE_GAP,
        Math.min(anchor.y, window.innerHeight - height - EDGE_GAP),
      ),
    });
  }, [anchor]);

  useEffect(() => {
    if (!anchor) return;

    const onPointerDown = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) onClose();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
      }
    };

    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown, true);
    // Capture: the anchor may live in any of several scroll containers.
    window.addEventListener("scroll", onClose, true);
    window.addEventListener("resize", onClose);
    window.addEventListener("blur", onClose);

    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("scroll", onClose, true);
      window.removeEventListener("resize", onClose);
      window.removeEventListener("blur", onClose);
    };
  }, [anchor, onClose]);

  // Give the menu the keyboard, so Escape and Tab behave and the click that
  // opened it does not leave focus behind on the row.
  useEffect(() => {
    if (anchor) menuRef.current?.focus();
  }, [anchor]);

  if (!anchor) return null;

  return createPortal(
    <div
      ref={menuRef}
      className="context-menu"
      role="menu"
      aria-label={label}
      tabIndex={-1}
      style={{
        left: position?.x ?? anchor.x,
        top: position?.y ?? anchor.y,
        // Hidden until measured, so the flip is never visible.
        visibility: position ? "visible" : "hidden",
      }}
      onContextMenu={(event) => event.preventDefault()}
    >
      {items.map((item, index) =>
        item.separator ? (
          <div key={`separator-${index}`} className="context-menu__separator" role="separator" />
        ) : (
          <button
            key={item.label}
            type="button"
            role="menuitem"
            className={`context-menu__item${item.danger ? " is-danger" : ""}`}
            disabled={item.disabled}
            title={item.title}
            onClick={() => {
              onClose();
              item.onSelect();
            }}
          >
            {item.Icon && <item.Icon className="icon-sm" aria-hidden="true" />}
            {item.label}
          </button>
        ),
      )}
    </div>,
    document.body,
  );
}
