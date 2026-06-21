import { memo } from "react";
import { useTranslation } from "react-i18next";

import { ProviderIconTile } from "@/components/dashboard/ProviderIconTile";
import { statusFillClass, statusTextClass } from "@/components/dashboard/helpers";
import {
  formatPercent,
  formatResetShort,
  formatServiceError,
  percentSeverity,
} from "@/lib/format";
import type { AccountRow, AccountSection } from "@/lib/inspectorModel";
import { severityToStatus } from "@/lib/status";
import type { LimitWindow, Provider } from "@/lib/types";
import { cn, clamp } from "@/lib/utils";

export function AccountSections({
  sections,
  selectedId,
  nowMs,
  loadingProviders,
  onSelect,
}: {
  sections: AccountSection[];
  selectedId: string | null;
  nowMs: number;
  loadingProviders: Set<Provider>;
  onSelect: (id: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-5">
      {sections.map((section) => (
        <section key={section.key}>
          <div className="mb-3 flex items-center gap-2 px-1">
            <h2 className="text-xs font-semibold uppercase text-text-faint">
              {t(`section.${section.key}`)}
            </h2>
            <span className="num rounded-md bg-surface-2 px-1.5 py-0.5 text-xs text-text-faint">
              {section.count}
            </span>
          </div>
          <div className="grid grid-cols-1 gap-3 md:grid-cols-2 2xl:grid-cols-3">
            {section.rows.map((row) => (
              <AccountCardButton
                key={row.id}
                row={row}
                nowMs={nowMs}
                selected={selectedId === row.id}
                loading={loadingProviders.has(row.service.provider)}
                onSelect={onSelect}
              />
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

const AccountCardButton = memo(function AccountCardButton({
  row,
  nowMs,
  selected,
  loading,
  onSelect,
}: {
  row: AccountRow;
  nowMs: number;
  selected: boolean;
  loading: boolean;
  onSelect: (id: string) => void;
}) {
  const { t } = useTranslation();
  const connected = row.service.connected;
  const reset = row.headline?.resets_at
    ? formatResetShort(row.headline.resets_at, nowMs, t)
    : null;
  const percent = row.headlinePercent;
  const width = `${clamp(percent ?? 0, 0, 100)}%`;
  const secondary = row.service.windows
    .filter((window) => window !== row.headline)
    .slice(0, 2);

  return (
    <button
      type="button"
      onClick={() => onSelect(row.id)}
      className={cn(
        "group relative flex min-h-[188px] w-full flex-col overflow-hidden rounded-lg border p-4 text-left",
        "transition-[background-color,border-color,box-shadow,transform,opacity] duration-150 hover:-translate-y-0.5 hover:border-border-strong hover:bg-surface",
        selected
          ? "border-border-strong bg-surface-2 shadow-lg shadow-black/10"
          : connected
            ? "border-border bg-surface/60 hover:border-border-strong hover:bg-surface"
            : "border-border bg-surface/40 opacity-55 hover:bg-surface/60 hover:opacity-100",
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <ProviderIconTile provider={row.service.provider} status={row.status} />
          <div className="min-w-0">
            <div className="flex min-w-0 items-center gap-2">
              <span className="truncate text-sm font-semibold">{row.title}</span>
              {row.service.source === "stored" && (
                <span className="rounded border border-border bg-surface px-1.5 py-0.5 text-[10px] text-text-faint">
                  {t("card.stored")}
                </span>
              )}
            </div>
            <div className="num mt-0.5 truncate text-xs text-text-faint">
              {row.subtitle ?? row.service.id.replace("auto:", "").replace("stored:", "")}
            </div>
          </div>
        </div>
        <span className="rounded-md border border-border bg-surface px-2 py-1 text-xs text-text-dim">
          {connected
            ? row.service.source === "stored"
              ? t("card.session")
              : t("card.oauth")
            : t("card.loggedOut")}
        </span>
      </div>

      {connected ? (
        <>
          <div className="mt-5">
            <div className="mb-2 flex items-end justify-between gap-3">
              <div className="min-w-0">
                <div className="flex items-baseline gap-1.5">
                  <span className="truncate text-xs font-medium text-text-faint">
                    {row.headline?.label ?? t("card.noUsageWindow")}
                  </span>
                  {reset && (
                    <span className="num shrink-0 text-[11px] text-text-faint/70">
                      · {t("card.resetsIn", { time: reset })}
                    </span>
                  )}
                </div>
                {row.usedLimit && (
                  <div className="num mt-1 truncate text-xs text-text-faint">
                    {row.usedLimit}
                  </div>
                )}
              </div>
              <div className={cn("num text-lg font-semibold", statusTextClass(row.status))}>
                {formatPercent(percent)}
              </div>
            </div>
            <div className="h-1.5 overflow-hidden rounded-full bg-white/[0.10]">
              {percent != null && (
                <div
                  className={cn("h-full rounded-full transition-[width] duration-300", statusFillClass(row.status))}
                  style={{ width }}
                />
              )}
            </div>
          </div>

          {secondary.length > 0 && (
            <div className="mt-4 grid gap-2">
              {secondary.map((window, index) => (
                <CompactWindowLine
                  key={`${window.label}-${index}`}
                  window={window}
                  nowMs={nowMs}
                />
              ))}
            </div>
          )}

          <div className="mt-auto flex items-center justify-end pt-4 text-xs text-text-faint">
            <span className="opacity-0 transition-opacity group-hover:opacity-100">
              {t("card.viewDetails")}
            </span>
          </div>
        </>
      ) : (
        <div className="mt-6 flex flex-1 flex-col justify-center gap-1">
          <span className="text-sm font-medium text-text-dim">{t("card.loggedOut")}</span>
          {row.service.error && (
            <span className="text-xs text-text-faint">
              {formatServiceError(row.service.error, t)}
            </span>
          )}
        </div>
      )}

      {loading && (
        <span className="provider-fetch-dot absolute right-2 top-2 size-2 rounded-full bg-[#73b8f4]" />
      )}
    </button>
  );
});

function CompactWindowLine({
  window,
  nowMs,
}: {
  window: LimitWindow;
  nowMs: number;
}) {
  const { t } = useTranslation();
  const percent = window.used_percent;
  const reset = window.resets_at
    ? formatResetShort(window.resets_at, nowMs, t)
    : null;
  const tone = severityToStatus(percentSeverity(percent));
  return (
    <div>
      <div className="mb-1 flex items-center justify-between gap-2 text-xs">
        <span className="flex min-w-0 items-baseline gap-1.5 text-text-faint">
          <span className="truncate">{window.label}</span>
          {reset && (
            <span className="num shrink-0 text-[11px] text-text-faint/70">
              · {t("card.resetsIn", { time: reset })}
            </span>
          )}
        </span>
        <span className={cn("num shrink-0", statusTextClass(tone))}>
          {formatPercent(percent)}
        </span>
      </div>
      <div className="h-1 overflow-hidden rounded-full bg-white/[0.09]">
        {percent != null && (
          <div
            className={cn("h-full rounded-full", statusFillClass(tone))}
            style={{ width: `${clamp(percent, 0, 100)}%` }}
          />
        )}
      </div>
    </div>
  );
}
