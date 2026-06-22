import { memo } from "react";
import { AlertCircle, CheckCircle2, Loader2 } from "lucide-react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";

import { ProviderIconTile } from "@/components/dashboard/ProviderIconTile";
import { UsageBar } from "@/components/dashboard/UsageBar";
import { statusTextClass } from "@/components/dashboard/helpers";
import {
  formatPercent,
  formatResetShort,
  formatServiceError,
  percentSeverity,
  remainingPercent,
} from "@/lib/format";
import {
  getAccountAction,
  type AccountActionState,
  type AccountActionStatus,
} from "@/lib/accountActionState";
import type { AccountRow, AccountSection } from "@/lib/inspectorModel";
import { anchorSupported } from "@/lib/providers";
import { severityToStatus } from "@/lib/status";
import type { LimitWindow } from "@/lib/types";
import { cn } from "@/lib/utils";

export function AccountSections({
  sections,
  selectedId,
  nowMs,
  loadingProviders,
  accountActions,
  onSelect,
  onRefreshAccount,
  onSendAnchor,
}: {
  sections: AccountSection[];
  selectedId: string | null;
  nowMs: number;
  loadingProviders: Set<string>;
  accountActions: AccountActionState;
  onSelect: (id: string) => void;
  onRefreshAccount: (id: string) => void;
  onSendAnchor: (id: string) => void;
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
                loading={loadingProviders.has(row.id)}
                accountActions={accountActions}
                onSelect={onSelect}
                onRefreshAccount={onRefreshAccount}
                onSendAnchor={onSendAnchor}
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
  accountActions,
  onSelect,
  onRefreshAccount,
  onSendAnchor,
}: {
  row: AccountRow;
  nowMs: number;
  selected: boolean;
  loading: boolean;
  accountActions: AccountActionState;
  onSelect: (id: string) => void;
  onRefreshAccount: (id: string) => void;
  onSendAnchor: (id: string) => void;
}) {
  const { t } = useTranslation();
  const connected = row.service.connected;
  const reset = row.headline?.resets_at
    ? formatResetShort(row.headline.resets_at, nowMs, t)
    : null;
  const percent = row.headlinePercent;
  const secondary = row.service.windows
    .filter((window) => window !== row.headline)
    .slice(0, 2);

  const isAnchorSupported = anchorSupported(row.service.provider);
  const refreshAction = getAccountAction(accountActions, row.id, "refresh");
  const anchorAction = getAccountAction(accountActions, row.id, "anchor");
  const refreshPending = loading || refreshAction === "pending";
  const anchorPending = anchorAction === "pending";
  const actionBusy = refreshPending || anchorPending;
  const refreshDisabled = actionBusy;
  const anchorDisabled = actionBusy;
  const actionStatus = anchorAction ?? (refreshPending ? "pending" : refreshAction);
  const actionMessage = actionStatus
    ? accountActionMessage(actionStatus, anchorAction != null ? "anchor" : "refresh", t)
    : null;

  return (
    <div className="group relative h-full">
      <button
        type="button"
        onClick={() => onSelect(row.id)}
        className={cn(
          "relative flex h-full min-h-[188px] w-full flex-col overflow-hidden rounded-lg border p-4 text-left",
          "transition-[background-color,border-color,box-shadow,transform,opacity] duration-150 hover:-translate-y-0.5 hover:border-border-strong hover:bg-surface",
          actionBusy && "card-active-hairline opacity-70",
          selected
            ? "border-border-strong bg-surface-2 shadow-lg shadow-black/10"
            : connected
              ? "border-border bg-surface/60 hover:border-border-strong hover:bg-surface"
              : "border-border bg-surface/40 opacity-55 hover:bg-surface/60 hover:opacity-100",
        )}
        aria-busy={actionBusy}
      >
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <ProviderIconTile provider={row.service.provider} status={row.status} />
          <div className="min-w-0">
            <div className="flex min-w-0 items-center gap-2">
              <span className="truncate text-sm font-semibold" title={row.title}>
                {row.title}
              </span>
              {row.service.source === "stored" && (
                <span className="rounded border border-border bg-surface px-1.5 py-0.5 text-[10px] text-text-faint">
                  {t("card.stored")}
                </span>
              )}
            </div>
            <div
              className="num mt-0.5 truncate text-xs text-text-faint"
              title={row.subtitle}
            >
              {row.subtitle}
            </div>
          </div>
        </div>
        <div className="flex items-center gap-1.5">
          <span className="rounded-md border border-border bg-surface px-2 py-1 text-xs text-text-dim">
            {connected
              ? row.service.source === "stored"
                ? t("card.session")
                : t("card.oauth")
              : t("card.loggedOut")}
          </span>
          <span
            className={cn(
              "rounded-md border px-2 py-1 text-[10px]",
              isAnchorSupported
                ? "border-border bg-surface text-text-dim"
                : "border-border bg-surface/50 text-text-faint",
            )}
            title={isAnchorSupported ? t("card.autoAvail") : t("card.autoUnavail")}
          >
            {isAnchorSupported ? t("card.autoAvail") : t("card.autoUnavail")}
          </span>
        </div>
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
                    <span className="num shrink-0 text-[11px] text-text-faint">
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
                {formatPercent(remainingPercent(percent))}
              </div>
            </div>
            <UsageBar
              percent={remainingPercent(percent)}
              tone={row.status}
              label={row.headline?.label ?? t("card.noUsageWindow")}
            />
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

          <div className="mt-auto flex items-center justify-start pt-4 text-xs text-text-faint">
            <span className="opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100">
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
      {actionMessage && (
        <span
          role="status"
          aria-live="polite"
          aria-atomic="true"
          className="action-feedback-overlay rounded-lg"
        >
          <span className="action-feedback-indicator max-w-[calc(100%-2rem)] px-3 py-2 text-xs">
            <AccountActionIcon status={actionStatus ?? "pending"} />
            <span className="truncate">{actionMessage}</span>
          </span>
        </span>
      )}
      </button>

      <div className="absolute bottom-2 right-2 z-10 flex gap-1 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100">
        <button
          type="button"
          title={t("card.refresh")}
          aria-label={t("card.refresh")}
          disabled={refreshDisabled}
          onClick={(e) => { e.stopPropagation(); onRefreshAccount(row.id); }}
          className="rounded-md border border-border bg-surface px-2 py-1 text-xs text-text-dim hover:bg-surface-2 disabled:cursor-not-allowed disabled:opacity-60"
        >
          {t("card.refresh")}
        </button>
        {isAnchorSupported && connected && (
          <button
            type="button"
            title={t("card.sendRequest")}
            aria-label={t("card.sendRequest")}
            disabled={anchorDisabled}
            onClick={(e) => { e.stopPropagation(); onSendAnchor(row.id); }}
            className="rounded-md border border-border bg-surface px-2 py-1 text-xs text-text-dim hover:bg-surface-2 disabled:cursor-not-allowed disabled:opacity-60"
          >
            {t("card.sendRequest")}
          </button>
        )}
      </div>
    </div>
  );
});

function accountActionMessage(
  status: AccountActionStatus,
  kind: "refresh" | "anchor",
  t: TFunction,
): string {
  if (status === "pending") {
    return kind === "anchor"
      ? t("status.sendingAnchor")
      : t("status.refreshingAccount");
  }
  if (status === "success") {
    return kind === "anchor"
      ? t("status.anchorSent")
      : t("status.refreshComplete");
  }
  return kind === "anchor"
    ? t("status.anchorFailed")
    : t("status.refreshFailed");
}

function AccountActionIcon({ status }: { status: AccountActionStatus }) {
  if (status === "success") {
    return <CheckCircle2 className="size-3.5 shrink-0 text-ok" aria-hidden />;
  }
  if (status === "error") {
    return <AlertCircle className="size-3.5 shrink-0 text-crit" aria-hidden />;
  }
  return <Loader2 className="refresh-spin size-3.5 shrink-0 text-[#73b8f4]" aria-hidden />;
}

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
            <span className="num shrink-0 text-[11px] text-text-faint">
              · {t("card.resetsIn", { time: reset })}
            </span>
          )}
        </span>
        <span className={cn("num shrink-0", statusTextClass(tone))}>
          {formatPercent(remainingPercent(percent))}
        </span>
      </div>
      <UsageBar percent={remainingPercent(percent)} tone={tone} size="sm" label={window.label} />
    </div>
  );
}
