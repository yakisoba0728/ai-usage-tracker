import { useEffect, useState } from "react";
import { RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { ProviderMark } from "@/components/ProviderMark";
import { UsageBar } from "@/components/dashboard/UsageBar";
import { statusTextClass } from "@/components/dashboard/helpers";
import { useNow } from "@/hooks/useNow";
import { useSnapshot } from "@/hooks/useSnapshot";
import {
  formatPercent,
  formatResetShort,
  formatUpdatedAgo,
  percentSeverity,
} from "@/lib/format";
import { getConfig } from "@/lib/ipc";
import { providerDisplayName, resolveHeadlineWindow } from "@/lib/providers";
import { severityToStatus } from "@/lib/status";
import type { AppConfig, ServiceUsage } from "@/lib/types";
import { cn } from "@/lib/utils";

/**
 * Compact menu-bar summary: every connected provider with its headline window's
 * usage (percent + bar + reset). Brief by design — no search, add-account, or
 * detail modal; the full dashboard lives behind the tray menu's "Show
 * dashboard". Esc dismisses the popover.
 */
export function PopoverDashboard() {
  const { snapshot, refreshing, refresh } = useSnapshot();
  const nowMs = useNow(1000);
  const { t } = useTranslation();
  const [config, setConfig] = useState<AppConfig | null>(null);

  useEffect(() => {
    getConfig()
      .then(setConfig)
      .catch(() => {});
  }, []);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") void hideWindow();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const services = (snapshot?.services ?? []).filter((s) => s.connected);

  return (
    <div className="flex h-dvh w-dvw flex-col overflow-hidden bg-canvas text-text">
      <header className="flex items-center gap-2 px-4 pt-3.5">
        <span className="flex-1 truncate text-sm font-semibold">
          AI Usage Tracker
        </span>
        <button
          type="button"
          onClick={() => void refresh()}
          disabled={refreshing}
          aria-label={t("common.refresh")}
          title={t("common.refresh")}
          className="rounded-md p-1 text-text-faint transition-colors hover:bg-surface-2 hover:text-text disabled:opacity-50"
        >
          <RefreshCw className={refreshing ? "size-3.5 animate-spin" : "size-3.5"} />
        </button>
      </header>
      <div className="num px-4 pb-2 pt-0.5 text-[11px] text-text-faint">
        {formatUpdatedAgo(snapshot?.fetched_at ?? null, nowMs, t)}
      </div>

      <div className="h-px bg-border" />

      <div className="scroll-area flex-1 overflow-y-auto px-2 py-1.5">
        {services.length === 0 ? (
          <div className="px-2 py-12 text-center text-xs text-text-faint">
            {t("tray.noConnected")}
          </div>
        ) : (
          services.map((service) => (
            <PopoverRow key={service.id} service={service} config={config} nowMs={nowMs} />
          ))
        )}
      </div>
    </div>
  );
}

function PopoverRow({
  service,
  config,
  nowMs,
}: {
  service: ServiceUsage;
  config: AppConfig | null;
  nowMs: number;
}) {
  const { t } = useTranslation();
  const headline = resolveHeadlineWindow(service, config);
  const pct = headline?.used_percent ?? null;
  const status = severityToStatus(percentSeverity(pct));
  const reset = headline?.resets_at
    ? formatResetShort(headline.resets_at, nowMs, t)
    : null;

  return (
    <div className="rounded-lg px-3 py-2.5 transition-colors hover:bg-surface-2/60">
      <div className="flex items-center gap-2.5">
        <ProviderMark provider={service.provider} className="size-4 shrink-0 text-text-dim" />
        <span className="min-w-0 flex-1 truncate text-sm font-medium text-text">
          {providerDisplayName(config, service.provider)}
        </span>
        <span className={cn("num shrink-0 text-sm font-semibold", statusTextClass(status))}>
          {formatPercent(pct)}
        </span>
      </div>
      <div className="mt-2">
        <UsageBar
          percent={pct}
          tone={status}
          label={headline?.label ?? service.provider}
        />
      </div>
      {(headline?.label || reset) && (
        <div className="num mt-1.5 text-[11px] text-text-faint">
          {headline?.label}
          {reset && (
            <span> · {t("card.resetsIn", { time: reset })}</span>
          )}
        </div>
      )}
    </div>
  );
}

async function hideWindow() {
  try {
    await getCurrentWindow().hide();
  } catch {
    /* not in Tauri */
  }
}
