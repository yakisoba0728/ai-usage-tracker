import { Loader2, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { percentBarColor, percentColor } from "@/lib/format";

export interface HeaderProps {
  fetchedAt: number | null;
  loading: boolean;
  onRefresh: () => void;
  nowMs: number;
  peak: number | null;
  connectedCount: number;
  totalCount: number;
}

function relativeLabel(fetchedAt: number | null, nowMs: number): string {
  if (fetchedAt == null) return "Awaiting first update…";
  const elapsed = Math.max(0, Math.floor(nowMs / 1000) - fetchedAt);
  if (elapsed < 5) return "Updated just now";
  if (elapsed < 60) return `Updated ${elapsed}s ago`;
  const mins = Math.floor(elapsed / 60);
  const secs = elapsed % 60;
  return `Updated ${mins}m ${secs}s ago`;
}

function formatAbsTime(epochSeconds: number): string {
  return new Date(epochSeconds * 1000).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function Header({
  fetchedAt,
  loading,
  onRefresh,
  nowMs,
  peak,
  connectedCount,
  totalCount,
}: HeaderProps) {
  const rel = relativeLabel(fetchedAt, nowMs);

  return (
    <header className="sticky top-0 z-30 border-b border-white/[0.06] bg-background/80 backdrop-blur-xl">
      <div className="mx-auto flex w-full max-w-6xl items-center justify-between gap-3 px-5 py-3.5 sm:gap-4 sm:px-6">
        {/* Brand */}
        <div className="flex min-w-0 items-center gap-3">
          <div className="flex size-9 shrink-0 items-center justify-center rounded-xl border border-white/10 bg-brand/10 text-brand shadow-[inset_0_1px_0_0_rgba(255,255,255,0.05)]">
            <svg viewBox="0 0 24 24" fill="none" className="size-[18px]">
              <circle
                cx="12"
                cy="12"
                r="8.5"
                stroke="currentColor"
                strokeWidth="2"
                opacity="0.28"
              />
              <path
                d="M12 3.5a8.5 8.5 0 0 1 8.5 8.5"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
              />
              <circle cx="12" cy="12" r="2.4" fill="currentColor" />
            </svg>
          </div>
          <div className="min-w-0">
            <h1 className="text-[15px] font-semibold leading-tight tracking-tight">
              AI Usage Tracker
            </h1>
            <p className="truncate text-[11px] text-muted-foreground/70">
              {totalCount > 0
                ? `${connectedCount}/${totalCount} services live`
                : "No services configured"}
            </p>
          </div>
        </div>

        {/* Status + actions */}
        <div className="flex items-center gap-2 sm:gap-3">
          {/* Peak pill — highest used_percent across connected services */}
          {peak != null && (
            <div className="flex items-center gap-2 rounded-full border border-white/[0.07] bg-white/[0.03] px-3 py-1.5">
              <span
                className={cn("size-1.5 rounded-full", percentBarColor(peak))}
              />
              <span className="text-[11px] font-medium text-muted-foreground">
                Peak
              </span>
              <span
                className={cn(
                  "font-mono text-[13px] font-semibold tabular-nums",
                  percentColor(peak),
                )}
              >
                {Math.round(peak)}%
              </span>
            </div>
          )}

          {/* Live "updated Xs ago" */}
          <div className="flex flex-col items-end leading-tight">
            <span
              className="font-mono text-[11px] tabular-nums text-muted-foreground"
              title={
                fetchedAt != null ? `Fetched at ${formatAbsTime(fetchedAt)}` : undefined
              }
            >
              {rel}
            </span>
            {fetchedAt != null && (
              <span className="hidden font-mono text-[10px] tabular-nums text-muted-foreground/60 sm:block">
                {formatAbsTime(fetchedAt)}
              </span>
            )}
          </div>

          {/* Refresh */}
          <Button
            variant="outline"
            size="sm"
            onClick={onRefresh}
            disabled={loading}
            className="h-8 gap-1.5 border-white/10 bg-white/[0.03] px-3 text-muted-foreground hover:bg-white/[0.06] hover:text-foreground"
          >
            {loading ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : (
              <RefreshCw className="size-3.5" />
            )}
            <span className="hidden sm:inline">Refresh</span>
          </Button>
        </div>
      </div>
    </header>
  );
}
