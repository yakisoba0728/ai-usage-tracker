import { RefreshCw } from "lucide-react";

import { cn } from "@/lib/utils";

export function ActionFeedbackOverlay({
  message,
  className,
}: {
  message: string;
  className?: string;
}) {
  return (
    <div
      role="status"
      aria-live="polite"
      aria-atomic="true"
      className={cn("action-feedback-overlay pointer-events-none", className)}
    >
      <div className="action-feedback-indicator max-w-[calc(100%-2rem)] px-3 py-2 text-xs">
        <RefreshCw className="refresh-spin size-3.5 shrink-0 text-[#73b8f4]" aria-hidden />
        <span className="truncate">{message}</span>
      </div>
    </div>
  );
}
