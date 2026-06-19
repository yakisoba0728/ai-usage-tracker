import { Loader2, Plus, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { BrandMark, PROVIDER_LABEL } from "@/components/ProviderMark";
import { cn } from "@/lib/utils";
import {
  formatClock,
  formatUpdatedAgo,
  percentSeverity,
  severityBarClass,
} from "@/lib/format";
import type { Provider } from "@/lib/types";

export interface HeaderProps {
  fetchedAt: number | null;
  refreshing: boolean;
  onRefresh: () => void;
  onAddAccount: () => void;
  nowMs: number;
  peak: number | null;
  peakProvider: Provider | null;
  connectedCount: number;
  totalCount: number;
}

export function Header({
  fetchedAt,
  refreshing,
  onRefresh,
  onAddAccount,
  nowMs,
  peak,
  peakProvider,
  connectedCount,
  totalCount,
}: HeaderProps) {
  const ago = formatUpdatedAgo(fetchedAt, nowMs);

  return (
    <header className="sticky top-0 z-30 border-b border-border bg-canvas/85 backdrop-blur-xl">
      <div className="mx-auto flex h-14 w-full max-w-[1100px] items-center justify-between gap-3 px-5">
        {/* Brand */}
        <div className="flex min-w-0 items-center gap-2.5">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-signal/30 bg-signal/10 text-signal">
            <BrandMark className="size-[18px]" pct={64} />
          </div>
          <div className="min-w-0 leading-tight">
            <h1 className="text-[15px] font-semibold tracking-tight text-text">
              AI Usage Tracker
            </h1>
            <p className="truncate text-text-faint" style={{ fontSize: 11 }}>
              {totalCount > 0
                ? `${connectedCount}/${totalCount} providers live`
                : "No providers configured"}
            </p>
          </div>
        </div>

        {/* Cluster */}
        <div className="flex items-center gap-2">
          {/* Fleet status pill — peak burn + which provider */}
          {peak != null && (
            <div
              className="hidden items-center gap-2 rounded-full border border-border bg-surface px-2.5 py-1 sm:flex"
              title={
                peakProvider != null
                  ? `Peak usage on ${PROVIDER_LABEL[peakProvider]}`
                  : "Peak usage"
              }
            >
              <span
                className={cn(
                  "size-1.5 rounded-full",
                  severityBarClass(percentSeverity(peak)),
                )}
              />
              <span className="uppercase tracking-[0.06em] text-text-faint" style={{ fontSize: 10 }}>
                Peak
              </span>
              <span
                className={cn(
                  "num font-semibold",
                  severityBarClass(percentSeverity(peak)).replace("bg-", "text-"),
                )}
              >
                {Math.round(peak)}%
              </span>
              {peakProvider != null && (
                <span className="text-text-dim" style={{ fontSize: 11 }}>
                  {PROVIDER_LABEL[peakProvider]}
                </span>
              )}
            </div>
          )}

          {/* Live "Updated Xs ago" */}
          <div
            className="num hidden items-center text-text-faint md:flex"
            style={{ fontSize: 11 }}
            title={fetchedAt != null ? `Fetched ${formatClock(fetchedAt)}` : undefined}
          >
            {ago}
          </div>

          {/* Refresh — outline, ghost-on-hover */}
          <Button
            variant="outline"
            size="icon"
            onClick={onRefresh}
            disabled={refreshing}
            aria-label="Refresh usage"
            title="Refresh"
            className="border-border text-text-dim hover:border-border-strong hover:text-text"
          >
            {refreshing ? (
              <Loader2 className="size-4 animate-spin text-signal" />
            ) : (
              <RefreshCw className="size-4" />
            )}
          </Button>

          {/* Add account — the prominent primary add action */}
          <Button variant="outline" size="sm" onClick={onAddAccount} className="gap-1.5">
            <Plus className="size-4 text-signal" />
            <span>Add account</span>
          </Button>
        </div>
      </div>
    </header>
  );
}
