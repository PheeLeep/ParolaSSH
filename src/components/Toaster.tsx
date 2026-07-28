import { useSyncExternalStore } from "react";
import { Spinner, Toast, ToastContainer } from "react-bootstrap";
import { CircleAlert, CircleCheck } from "lucide-react";

import * as toasts from "../lib/toast";

/** Renders whatever the toast store is holding.
 *
 *  Mounted once at the shell so a message survives the pane that raised it -
 *  a transfer that fails after you have navigated to another host still says so.
 */
export function Toaster() {
  const items = useSyncExternalStore(toasts.subscribe, toasts.getSnapshot);
  if (items.length === 0) return null;

  return (
    <ToastContainer className="toaster" position="bottom-end">
      {items.map((toast) => (
        <Toast
          key={toast.id}
          onClose={() => toasts.dismiss(toast.id)}
          className={`toaster__item toaster__item--${toast.kind}`}
        >
          <Toast.Header closeButton>
            <Glyph kind={toast.kind} />
            <strong className="me-auto">{toast.title}</strong>
          </Toast.Header>
          {toast.detail && (
            <Toast.Body className="text-prewrap small">{toast.detail}</Toast.Body>
          )}
        </Toast>
      ))}
    </ToastContainer>
  );
}

function Glyph({ kind }: { kind: toasts.ToastKind }) {
  if (kind === "progress") {
    return <Spinner animation="border" size="sm" className="me-2" aria-hidden="true" />;
  }
  if (kind === "success") {
    return <CircleCheck className="icon-sm me-2 text-success" aria-hidden="true" />;
  }
  return <CircleAlert className="icon-sm me-2 text-danger" aria-hidden="true" />;
}
