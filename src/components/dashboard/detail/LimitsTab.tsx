import { useTranslation } from "react-i18next";

import { UsageBar } from "@/components/dashboard/UsageBar";
import { statusTextClass } from "@/components/dashboard/helpers";
import {
  formatPercent,
  formatResetShort,
  formatUsedLimit,
  percentSeverity,
  remainingPercent,
} from "@/lib/format";
import { severityToStatus } from "@/lib/status";
import type { LimitWindow } from "@/lib/types";
import { cn } from "@/lib/utils";

export function LimitsTab({
  windows,
  nowMs,
}: {
  windows: LimitWindow[];
  nowMs: number;
}) {
  const { t } = useTranslation();
  if (windows.length === 0) {
    return (
      <div className="rounded-lg border border-border bg-surface/50 p-5 text-sm text-text-faint">
        {t("detail.limits.noWindows")}
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {windows.map((window, index) => (
        <WindowRow key={`${window.label}-${index}`} window={window} nowMs={nowMs} />
      ))}
    </div>
  );
}

function WindowRow({
  window,
  nowMs,
}: {
  window: LimitWindow;
  nowMs: number;
}) {
  const { t } = useTranslation();
  const percent = window.used_percent;
  const tone = severityToStatus(percentSeverity(percent));
  return (
    <div className="rounded-lg border border-border bg-surface/50 p-4">
      <div className="mb-3 flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h3 className="truncate text-sm font-semibold">{window.label}</h3>
          <p className="num mt-1 text-xs text-text-faint">
            {formatUsedLimit(window) ?? t("detail.limits.noUsedLimit")}
          </p>
        </div>
        <span className={cn("num text-lg font-semibold", statusTextClass(tone))}>
          {formatPercent(remainingPercent(percent))}
        </span>
      </div>
      <UsageBar percent={remainingPercent(percent)} tone={tone} label={window.label} />
      <p className="num mt-3 text-xs text-text-faint">
        {window.resets_at
          ? t("detail.limits.resetsIn", {
              time: formatResetShort(window.resets_at, nowMs, t),
            })
          : t("detail.limits.noResetReported")}
      </p>
    </div>
  );
}
