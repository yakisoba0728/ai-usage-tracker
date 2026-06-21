import { X } from "lucide-react";
import { useTranslation } from "react-i18next";

export interface Toast {
  id: number;
  message: string;
}

/**
 * Fixed top-right stack of in-app toasts (threshold crossings, etc.). Purely
 * presentational — the parent owns the list and each toast's auto-dismiss
 * timer. Pointer-events pass through the container but not the toasts
 * themselves, so the area below stays clickable.
 */
export function Toaster({
  toasts,
  onDismiss,
}: {
  toasts: Toast[];
  onDismiss: (id: number) => void;
}) {
  const { t } = useTranslation();
  if (toasts.length === 0) return null;
  return (
    <div className="pointer-events-none fixed right-4 top-4 z-[60] flex w-72 flex-col gap-2">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          role="status"
          className="pointer-events-auto flex items-start gap-2.5 rounded-md border border-border-strong bg-surface px-3 py-2.5 shadow-lg"
        >
          <p className="min-w-0 flex-1 leading-relaxed text-text" style={{ fontSize: 12 }}>
            {toast.message}
          </p>
          <button
            type="button"
            onClick={() => onDismiss(toast.id)}
            aria-label={t("toast.dismiss")}
            className="-mr-1 -mt-0.5 shrink-0 rounded-sm p-0.5 text-text-faint transition-colors hover:text-text"
          >
            <X className="size-3.5" />
          </button>
        </div>
      ))}
    </div>
  );
}
