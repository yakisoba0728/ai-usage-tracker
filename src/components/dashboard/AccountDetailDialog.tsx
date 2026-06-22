import { useEffect, useRef, type KeyboardEvent } from "react";
import {
  Cloud,
  Edit3,
  MoreHorizontal,
  RefreshCw,
  Settings,
  Trash2,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import { ActionFeedbackOverlay } from "@/components/ActionFeedbackOverlay";
import { ProviderIconTile } from "@/components/dashboard/ProviderIconTile";
import { InspectorSettings } from "@/components/dashboard/detail/InspectorSettings";
import { LimitsTab } from "@/components/dashboard/detail/LimitsTab";
import { MenuItem } from "@/components/dashboard/detail/primitives";
import { RawTab } from "@/components/dashboard/detail/RawTab";
import { SessionsTab } from "@/components/dashboard/detail/SessionsTab";
import {
  allServiceWindows,
  storedAccountId,
} from "@/components/dashboard/helpers";
import {
  INSPECTOR_TABS,
  type InspectorTab,
} from "@/components/dashboard/inspectorTabs";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { formatUpdatedAgo } from "@/lib/format";
import {
  getAccountAction,
  type AccountActionState,
  type AccountActionStatus,
} from "@/lib/accountActionState";
import { buildInspectorSummary } from "@/lib/inspectorModel";
import { providerDisplayName } from "@/lib/providers";
import { serviceStatus } from "@/lib/status";
import type { AppConfig, ServiceUsage } from "@/lib/types";
import { cn } from "@/lib/utils";

export function AccountDetailDialog({
  service,
  open,
  onOpenChange,
  config,
  nowMs,
  fetchedAt,
  refreshing,
  tab,
  onTabChange,
  moreOpen,
  onMoreOpenChange,
  accountActions,
  onRefresh,
  onSendAnchor,
  onOpenAdd,
  onOpenSettings,
  onConfigChange,
  onRenameAccount,
  onRemove,
}: {
  service: ServiceUsage | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  config: AppConfig | null;
  nowMs: number;
  fetchedAt: number | null;
  refreshing: boolean;
  tab: InspectorTab;
  onTabChange: (tab: InspectorTab) => void;
  moreOpen: boolean;
  onMoreOpenChange: (open: boolean) => void;
  accountActions: AccountActionState;
  onRefresh: () => void;
  onSendAnchor: (id: string) => void;
  onOpenAdd: () => void;
  onOpenSettings: () => void;
  onConfigChange: (next: AppConfig) => void;
  onRenameAccount: (serviceId: string, name: string | null) => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Dialog open={open && service != null} onOpenChange={onOpenChange}>
      <DialogContent className="h-[min(760px,86dvh)] w-[min(860px,94vw)] max-w-none gap-0 overflow-hidden rounded-xl border-border bg-[#1b1d20] p-0 shadow-2xl shadow-black/50">
        {service && (
          <>
            <DialogTitle className="sr-only">
              {t("detail.srTitle", {
                provider: providerDisplayName(config, service.id, service.provider),
              })}
            </DialogTitle>
            <DialogDescription className="sr-only">
              {t("detail.srDesc")}
            </DialogDescription>
            <DetailPanelContent
              service={service}
              config={config}
              nowMs={nowMs}
              fetchedAt={fetchedAt}
              refreshing={refreshing}
              tab={tab}
              onTabChange={onTabChange}
              moreOpen={moreOpen}
              onMoreOpenChange={onMoreOpenChange}
              refreshAction={getAccountAction(accountActions, service.id, "refresh")}
              anchorAction={getAccountAction(accountActions, service.id, "anchor")}
              onRefresh={onRefresh}
              onSendAnchor={onSendAnchor}
              onOpenAdd={onOpenAdd}
              onOpenSettings={onOpenSettings}
              onConfigChange={onConfigChange}
              onRenameAccount={onRenameAccount}
              onRemove={onRemove}
            />
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

function DetailPanelContent({
  service,
  config,
  nowMs,
  fetchedAt,
  refreshing,
  tab,
  onTabChange,
  moreOpen,
  onMoreOpenChange,
  refreshAction,
  anchorAction,
  onRefresh,
  onSendAnchor,
  onOpenAdd,
  onOpenSettings,
  onConfigChange,
  onRenameAccount,
  onRemove,
}: {
  service: ServiceUsage;
  config: AppConfig | null;
  nowMs: number;
  fetchedAt: number | null;
  refreshing: boolean;
  tab: InspectorTab;
  onTabChange: (tab: InspectorTab) => void;
  moreOpen: boolean;
  onMoreOpenChange: (open: boolean) => void;
  refreshAction: AccountActionStatus | null;
  anchorAction: AccountActionStatus | null;
  onRefresh: () => void;
  onSendAnchor: (id: string) => void;
  onOpenAdd: () => void;
  onOpenSettings: () => void;
  onConfigChange: (next: AppConfig) => void;
  onRenameAccount: (serviceId: string, name: string | null) => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation();
  const summary = buildInspectorSummary(service, config);
  const accountId = storedAccountId(service);
  const allWindows = allServiceWindows(service);
  const tabBaseId = `detail-${service.id.replace(/[^A-Za-z0-9_-]/g, "-")}`;
  const refreshFeedbackMessage =
    refreshAction === "success"
      ? t("status.refreshComplete")
      : refreshAction === "error"
        ? t("status.refreshFailed")
        : refreshing
          ? t("status.refreshingUsage")
          : null;

  const moreRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!moreOpen) return;
    // Move focus into the menu so the Arrow keys drive it immediately.
    moreRef.current
      ?.querySelector<HTMLButtonElement>('[role="menuitem"]')
      ?.focus();
    function onPointerDown(e: PointerEvent) {
      if (moreRef.current && !moreRef.current.contains(e.target as Node)) {
        onMoreOpenChange(false);
      }
    }
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [moreOpen, onMoreOpenChange]);

  function onMenuKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    const items = Array.from(
      e.currentTarget.querySelectorAll<HTMLButtonElement>(
        '[role="menuitem"]:not([disabled])',
      ),
    );
    if (items.length === 0) return;
    const idx = items.findIndex((el) => el === document.activeElement);
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        items[(idx + 1) % items.length]?.focus();
        break;
      case "ArrowUp":
        e.preventDefault();
        items[(idx - 1 + items.length) % items.length]?.focus();
        break;
      case "Home":
        e.preventDefault();
        items[0]?.focus();
        break;
      case "End":
        e.preventDefault();
        items[items.length - 1]?.focus();
        break;
      case "Escape":
        e.preventDefault();
        e.stopPropagation();
        onMoreOpenChange(false);
        triggerRef.current?.focus();
        break;
    }
  }

  function onTabsKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    const tabs = Array.from(
      e.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]'),
    );
    if (tabs.length === 0) return;
    const idx = tabs.findIndex((el) => el === document.activeElement);
    let nextIndex: number;
    switch (e.key) {
      case "ArrowRight":
        nextIndex = (idx + 1) % tabs.length;
        break;
      case "ArrowLeft":
        nextIndex = (idx - 1 + tabs.length) % tabs.length;
        break;
      case "Home":
        nextIndex = 0;
        break;
      case "End":
        nextIndex = tabs.length - 1;
        break;
      default:
        return;
    }
    e.preventDefault();
    const next = tabs[nextIndex];
    next?.focus();
    const nextTab = next?.dataset.tabId as InspectorTab | undefined;
    if (nextTab) onTabChange(nextTab);
  }

  function openAddFromMenu() {
    onMoreOpenChange(false);
    onOpenAdd();
  }

  function openSettingsFromMenu() {
    onMoreOpenChange(false);
    onOpenSettings();
  }

  return (
    <section className="scroll-area h-full min-h-0 overflow-y-auto bg-[#1b1d20]">
      <div className="sticky top-0 z-20 border-b border-border bg-[#1b1d20]/95 px-5 py-5 backdrop-blur">
        <div className="flex items-start justify-between gap-4">
          <div className="flex min-w-0 items-center gap-3">
            <ProviderIconTile provider={service.provider} status={serviceStatus(service, config)} large />
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <h1 className="truncate text-xl font-semibold">{summary.title}</h1>
                <span className={cn("size-2 rounded-full", service.connected ? "bg-ok" : "bg-text-faint")} />
                <span className="text-xs text-text-dim">
                  {service.connected ? t("detail.connected") : t("detail.offline")}
                </span>
              </div>
              <div className="num mt-2 grid gap-x-5 gap-y-1 text-xs text-text-faint sm:grid-cols-2">
                <span>{t("detail.accountId")}&nbsp;&nbsp;{summary.accountId}</span>
                <span>
                  {t("detail.source")}&nbsp;&nbsp;
                  {service.source === "stored"
                    ? t("detail.sessions.storedCredential")
                    : t("detail.sessions.autoDetected")}
                </span>
                <span>
                  {t("detail.lastRefresh")}&nbsp;&nbsp;
                  {formatUpdatedAgo(fetchedAt, nowMs, t, { prefix: false })}
                </span>
                <span>{t("detail.plan")}&nbsp;&nbsp;{service.plan ?? "—"}</span>
              </div>
            </div>
          </div>

          <div className="relative flex shrink-0 items-center gap-2" ref={moreRef}>
            <Button variant="ghost" size="sm" onClick={() => onTabChange("settings")}>
              <Edit3 className="size-4" />
              {t("detail.edit")}
            </Button>
            <Button
              ref={triggerRef}
              variant="ghost"
              size="icon"
              onClick={() => onMoreOpenChange(!moreOpen)}
              aria-label={t("detail.moreActions")}
              aria-haspopup="menu"
              aria-expanded={moreOpen}
            >
              <MoreHorizontal className="size-4" />
            </Button>
            {moreOpen && (
              <div
                role="menu"
                aria-label={t("detail.moreActions")}
                onKeyDown={onMenuKeyDown}
                className="menu-pop absolute right-0 top-10 z-40 w-56 rounded-lg border border-border-strong bg-[#25272b]/95 p-1.5 shadow-2xl shadow-black/40"
              >
                <MenuItem
                  icon={<RefreshCw className={refreshing ? "size-4 animate-spin" : "size-4"} />}
                  disabled={refreshing}
                  onClick={onRefresh}
                >
                  {t("detail.menu.refresh")}
                </MenuItem>
                <MenuItem icon={<Cloud className="size-4" />} onClick={openAddFromMenu}>
                  {t("detail.menu.reauth")}
                </MenuItem>
                <MenuItem icon={<Settings className="size-4" />} onClick={openSettingsFromMenu}>
                  {t("detail.menu.settings")}
                </MenuItem>
                <MenuItem
                  icon={<Trash2 className="size-4" />}
                  disabled={accountId == null}
                  destructive
                  onClick={onRemove}
                >
                  {t("detail.menu.remove")}
                </MenuItem>
              </div>
            )}
          </div>
        </div>

        <div
          role="tablist"
          aria-label={t("detail.srDesc")}
          onKeyDown={onTabsKeyDown}
          className="mt-5 flex gap-5 overflow-x-auto"
        >
          {INSPECTOR_TABS.map((id) => (
            <button
              key={id}
              type="button"
              id={`${tabBaseId}-tab-${id}`}
              role="tab"
              aria-selected={tab === id}
              aria-controls={`${tabBaseId}-panel-${id}`}
              tabIndex={tab === id ? 0 : -1}
              data-tab-id={id}
              onClick={() => onTabChange(id)}
              className={cn(
                "border-b-2 pb-3 text-sm font-medium transition-colors",
                tab === id
                  ? "border-[#4b9bea] text-text"
                  : "border-transparent text-text-dim hover:text-text",
              )}
            >
              {t(`detail.tab.${id}`)}
            </button>
          ))}
        </div>
      </div>

      <div
        id={`${tabBaseId}-panel-${tab}`}
        role="tabpanel"
        aria-labelledby={`${tabBaseId}-tab-${tab}`}
        aria-busy={refreshing}
        className="relative min-h-[220px] px-5 py-5"
      >
        {tab === "limits" && (
          <LimitsTab windows={allWindows} nowMs={nowMs} />
        )}
        {tab === "sessions" && (
          <SessionsTab
            service={service}
            fetchedAt={fetchedAt}
            nowMs={nowMs}
            refreshing={refreshing}
            onRefresh={onRefresh}
            onOpenAdd={onOpenAdd}
          />
        )}
        {tab === "raw" && <RawTab service={service} />}
        {tab === "settings" && (
          <InspectorSettings
            service={service}
            config={config}
            anchorAction={anchorAction}
            onSendAnchor={onSendAnchor}
            onConfigChange={onConfigChange}
            onRenameAccount={onRenameAccount}
            onOpenAdd={onOpenAdd}
          />
        )}
        {refreshFeedbackMessage && (
          <ActionFeedbackOverlay message={refreshFeedbackMessage} />
        )}
      </div>
    </section>
  );
}
