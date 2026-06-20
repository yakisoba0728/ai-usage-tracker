import { useEffect, useState } from "react";
import { RotateCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  getAllWindows,
  getCurrentWindow,
} from "@tauri-apps/api/window";

import { ProviderMark } from "@/components/ProviderMark";
import { useSnapshot } from "@/hooks/useSnapshot";
import { useNow } from "@/hooks/useNow";
import { getConfig } from "@/lib/ipc";
import {
  providerDisplayName,
  highestBurnWindow,
} from "@/lib/providers";
import {
  formatPercent,
  formatUpdatedAgo,
  percentSeverity,
  severityBarClass,
} from "@/lib/format";
import { clamp } from "@/lib/utils";
import type { AppConfig, LimitWindow, ServiceUsage } from "@/lib/types";

/**
 * Compact 340×440 borderless popover for the tray. Lists every connected
 * provider with its highest-burn window, plus a "Open Dashboard" action that
 * surfaces the main window. The backend hides this window on focus loss and on
 * Escape (we also hide on Esc here for parity / keyboard users).
 */
export function TrayPopover() {
  const { snapshot, refreshing, refresh } = useSnapshot();
  const nowMs = useNow(1000);
  const { t } = useTranslation();
  const [config, setConfig] = useState<AppConfig | null>(null);

  useEffect(() => {
    getConfig().then(setConfig).catch(() => {});
  }, []);

  // Esc dismisses the popover (hides the window).
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") void hideWindow();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const services = (snapshot?.services ?? []).filter((s) => s.connected);

  return (
    <div className="flex h-dvh w-dvw flex-col overflow-hidden rounded-xl bg-surface text-text">
      {/* Header */}
      <header className="px-4 pt-3">
        <div className="flex items-center gap-2">
          <span className="flex-1 truncate text-sm font-medium text-text-dim">
            AI Usage Tracker
          </span>
          <button
            type="button"
            onClick={() => void refresh()}
            disabled={refreshing}
            aria-label={t("tray.refreshUsage")}
            title={t("common.refresh")}
            className="rounded-md p-1 text-text-faint transition-colors hover:bg-surface-2 hover:text-text disabled:opacity-50"
          >
            <RotateCw className={refreshing ? "size-3.5 animate-spin" : "size-3.5"} />
          </button>
        </div>
        <div className="num mt-0.5 text-text-faint" style={{ fontSize: 11 }}>
          {formatUpdatedAgo(snapshot?.fetched_at ?? null, nowMs)}
        </div>
      </header>

      <div className="my-2 h-px bg-border" />

      {/* Body — connected providers, highest-burn window each */}
      <div className="scroll-area flex-1 overflow-y-auto px-2 py-1">
        {services.length === 0 ? (
          <div className="px-2 py-10 text-center text-text-faint" style={{ fontSize: 12 }}>
            {t("tray.noConnected")}
          </div>
        ) : (
          services.map((s) => (
            <PopoverRow
              key={s.provider}
              service={s}
              config={config}
            />
          ))
        )}
      </div>

      {/* Footer — surface the dashboard */}
      <div className="border-t border-border p-2">
        <button
          type="button"
          onClick={() => void openDashboard()}
          className="w-full rounded-md bg-surface-2 px-3 py-2 text-text transition-colors hover:bg-white/[0.06]"
          style={{ fontSize: 13, fontWeight: 500 }}
        >
          {t("tray.openDashboard")}
        </button>
      </div>
    </div>
  );
}

function PopoverRow({
  service,
  config,
}: {
  service: ServiceUsage;
  config: AppConfig | null;
}) {
  const window: LimitWindow | null = highestBurnWindow([
    ...(service.windows ?? []),
    ...(service.detail_windows ?? []),
  ]);
  const pct = window?.used_percent ?? null;
  const sev = percentSeverity(pct);
  const width = `${clamp(pct ?? 0, 0, 100)}%`;

  return (
    <div className="px-2 py-2">
      <div className="flex items-center gap-2">
        <ProviderMark provider={service.provider} className="size-3.5 shrink-0 text-text-dim" />
        <span
          className="min-w-0 flex-1 truncate text-text"
          style={{ fontSize: 12, fontWeight: 500 }}
        >
          {providerDisplayName(config, service.provider)}
        </span>
        <span className="num shrink-0 font-semibold text-text" style={{ fontSize: 12 }}>
          {formatPercent(pct)}
        </span>
      </div>
      <div
        className="mt-1.5 w-full overflow-hidden rounded-full bg-surface-2"
        style={{ height: 3 }}
        role="progressbar"
        aria-label={window?.label ?? service.provider}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct != null ? Math.round(pct) : undefined}
      >
        <div
          className={`h-full rounded-full transition-[width] duration-200 ease-out ${severityBarClass(sev)}`}
          style={{ width }}
        />
      </div>
    </div>
  );
}

/** Surface the main window, then hide this popover. No-op outside Tauri. */
async function openDashboard() {
  try {
    const all = await getAllWindows();
    const main = all.find((w) => w.label === "main");
    if (main) {
      await main.show();
      await main.setFocus();
    }
    await getCurrentWindow().hide();
  } catch (e) {
    console.error("openDashboard failed:", e);
  }
}

async function hideWindow() {
  try {
    await getCurrentWindow().hide();
  } catch {
    /* not in Tauri */
  }
}
