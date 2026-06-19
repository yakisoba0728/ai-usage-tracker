import { Plus } from "lucide-react";

import { Button } from "@/components/ui/button";

/**
 * Calm, centered empty state. No apology — just the fact, one line of
 * guidance, and the primary CTA.
 */
export function EmptyState({ onAddAccount }: { onAddAccount: () => void }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-4 py-24 text-center">
      <div className="flex size-12 items-center justify-center rounded-xl border border-border bg-surface text-text-faint">
        <Plus className="size-5" />
      </div>
      <div className="space-y-1.5">
        <p className="text-text" style={{ fontSize: 16, fontWeight: 600 }}>
          No accounts connected
        </p>
        <p className="mx-auto max-w-sm leading-relaxed text-text-dim" style={{ fontSize: 13 }}>
          Add an account to start tracking usage, or sign in to a provider's CLI
          and it'll appear here automatically.
        </p>
      </div>
      <Button variant="outline" size="default" onClick={onAddAccount} className="gap-1.5">
        <Plus className="size-4 text-signal" />
        Add account
      </Button>
    </div>
  );
}
