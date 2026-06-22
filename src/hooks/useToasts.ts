import { useCallback, useRef, useState } from "react";

import type { Toast } from "@/components/Toaster";

export interface UseToastsResult {
  toasts: Toast[];
  /** Queue a toast; it auto-dismisses after 5s. */
  pushToast: (message: string) => void;
  dismissToast: (id: number) => void;
}

/**
 * The dashboard toast queue: append + auto-dismiss after 5s, plus manual
 * dismiss. `pushToast` is stable (`[]` deps) so its many consumers — handlers
 * and event effects — don't churn their own callbacks/subscriptions.
 */
export function useToasts(): UseToastsResult {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const toastIdRef = useRef(0);

  const pushToast = useCallback((message: string) => {
    const id = ++toastIdRef.current;
    setToasts((t) => [...t, { id, message }]);
    window.setTimeout(() => {
      setToasts((t) => t.filter((x) => x.id !== id));
    }, 5000);
  }, []);

  const dismissToast = useCallback((id: number) => {
    setToasts((t) => t.filter((x) => x.id !== id));
  }, []);

  return { toasts, pushToast, dismissToast };
}
