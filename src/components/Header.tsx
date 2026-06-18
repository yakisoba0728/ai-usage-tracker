import { useEffect, useState } from "react";
import { Loader2, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";

export interface HeaderProps {
  fetchedAt: number | null;
  loading: boolean;
  onRefresh: () => void;
}

function formatTime(epochSeconds: number): string {
  return new Date(epochSeconds * 1000).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function relativeLabel(fetchedAt: number | null): string {
  if (fetchedAt == null) return "Awaiting first update…";
  const elapsed = Math.max(0, Math.floor(Date.now() / 1000) - fetchedAt);
  if (elapsed < 5) return "Updated just now";
  if (elapsed < 60) return `Updated ${elapsed}s ago`;
  const mins = Math.floor(elapsed / 60);
  const secs = elapsed % 60;
  return `Updated ${mins}m ${secs}s ago`;
}

export function Header({ fetchedAt, loading, onRefresh }: HeaderProps) {
  // Re-render every second so the relative "x ago" label stays fresh without
  // depending on backend pushes.
  const [, setTick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setTick((t) => t + 1), 1000);
    return () => clearInterval(id);
  }, []);

  const label = relativeLabel(fetchedAt);

  return (
    <header className="flex items-center justify-between gap-4 border-b border-border/60 pb-4">
      <div className="min-w-0">
        <h1 className="text-lg font-semibold leading-tight tracking-tight">
          AI Usage Tracker
        </h1>
        <p className="truncate text-xs text-muted-foreground" title={label}>
          {label}
          {fetchedAt != null && (
            <span className="ml-2 tabular-nums text-muted-foreground/70">
              {formatTime(fetchedAt)}
            </span>
          )}
        </p>
      </div>

      <Button
        variant="outline"
        size="sm"
        onClick={onRefresh}
        disabled={loading}
        className="shrink-0"
      >
        {loading ? (
          <Loader2 className="animate-spin" />
        ) : (
          <RefreshCw />
        )}
        Refresh
      </Button>
    </header>
  );
}
