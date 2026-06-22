import type { ReactNode } from "react";

export function Panel({ children }: { children: ReactNode }) {
  return (
    <div className="space-y-1 rounded-lg border border-border bg-surface-2/60 p-3">
      {children}
    </div>
  );
}
