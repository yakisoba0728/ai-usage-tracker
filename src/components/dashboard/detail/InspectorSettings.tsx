import { useEffect, useState } from "react";
import { Cloud } from "lucide-react";
import { useTranslation } from "react-i18next";

import { allServiceWindows } from "@/components/dashboard/helpers";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { sendAnchorNow } from "@/lib/ipc";
import {
  anchorSupported,
  patchProviderConfig,
  PROVIDER_ORDER,
  providerDisplayName,
  setAutoAnchor,
} from "@/lib/providers";
import type { AppConfig, ProviderConfig, ServiceUsage } from "@/lib/types";
import { clamp } from "@/lib/utils";

export function InspectorSettings({
  service,
  config,
  onConfigChange,
  onOpenAdd,
}: {
  service: ServiceUsage;
  config: AppConfig | null;
  onConfigChange: (next: AppConfig) => void;
  onOpenAdd: () => void;
}) {
  const idx = PROVIDER_ORDER.indexOf(service.provider);
  const providerConfig: ProviderConfig | null = config?.providers[idx] ?? null;
  const allWindows = allServiceWindows(service);
  const labels = Array.from(new Set(allWindows.map((window) => window.label)));
  const { t } = useTranslation();
  const [nameDraft, setNameDraft] = useState(providerConfig?.custom_name ?? "");
  const [thresholdDraft, setThresholdDraft] = useState("");

  // Message-anchor providers (auto toggle + manual 1-token send).
  const messageAnchor = anchorSupported(service.provider);
  const autoOn = config?.auto_anchor?.[service.id] ?? false;
  const [confirmOpen, setConfirmOpen] = useState(false);

  function toggleAuto(next: boolean) {
    if (!config) return;
    onConfigChange(setAutoAnchor(config, service.id, next));
  }

  useEffect(() => {
    setNameDraft(providerConfig?.custom_name ?? "");
  }, [providerConfig?.custom_name, service.provider]);

  function patch(patchValue: Partial<ProviderConfig>) {
    if (!config) return;
    onConfigChange(patchProviderConfig(config, service.provider, patchValue));
  }

  function commitName() {
    const trimmed = nameDraft.trim();
    patch({ custom_name: trimmed.length > 0 ? trimmed : null });
  }

  function addThreshold() {
    const parsed = Number.parseInt(thresholdDraft, 10);
    if (Number.isNaN(parsed)) return;
    const next = clamp(parsed, 0, 100);
    const thresholds = providerConfig?.notify_thresholds ?? [];
    patch({ notify_thresholds: Array.from(new Set([...thresholds, next])).sort((a, b) => a - b) });
    setThresholdDraft("");
  }

  return (
    <div className="space-y-5">
      <div className="rounded-lg border border-border bg-surface/50 p-4">
        <h2 className="mb-4 text-sm font-semibold">
          {t("detail.settings.providerDisplay")}
        </h2>
        <label className="block">
          <span className="mb-1.5 block text-xs text-text-faint">
            {t("detail.settings.displayName")}
          </span>
          <input
            value={nameDraft}
            onChange={(event) => setNameDraft(event.target.value)}
            onBlur={commitName}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                commitName();
                event.currentTarget.blur();
              }
            }}
            placeholder={providerDisplayName(config, service.provider)}
            className="w-full rounded-md border border-border bg-canvas px-3 py-2 text-sm text-text outline-none focus:border-border-strong"
          />
        </label>
      </div>

      <div className="rounded-lg border border-border bg-surface/50 p-4">
        <h2 className="mb-4 text-sm font-semibold">
          {t("detail.settings.primaryWindow")}
        </h2>
        <select
          value={providerConfig?.primary_window ?? ""}
          disabled={config == null || labels.length === 0}
          onChange={(event) => patch({ primary_window: event.target.value || null })}
          aria-label={t("detail.settings.primaryWindow")}
          className="w-full rounded-md border border-border bg-canvas px-3 py-2 text-sm text-text outline-none focus:border-border-strong"
        >
          <option value="">{t("detail.settings.autoWindow")}</option>
          {labels.map((label) => (
            <option key={label} value={label}>
              {label}
            </option>
          ))}
        </select>
      </div>

      <div className="rounded-lg border border-border bg-surface/50 p-4">
        <h2 className="mb-4 text-sm font-semibold">
          {t("detail.settings.thresholds")}
        </h2>
        <div className="mb-3 flex flex-wrap gap-2">
          {(providerConfig?.notify_thresholds ?? []).map((threshold) => (
            <button
              key={threshold}
              type="button"
              onClick={() =>
                patch({
                  notify_thresholds: (providerConfig?.notify_thresholds ?? []).filter(
                    (item) => item !== threshold,
                  ),
                })
              }
              className="num rounded-md border border-border bg-surface-2 px-2 py-1 text-xs text-text-dim hover:text-text"
            >
              {threshold}% ×
            </button>
          ))}
        </div>
        <div className="flex gap-2">
          <input
            value={thresholdDraft}
            onChange={(event) => setThresholdDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") addThreshold();
            }}
            inputMode="numeric"
            placeholder="90"
            className="num w-24 rounded-md border border-border bg-canvas px-3 py-2 text-sm text-text outline-none focus:border-border-strong"
          />
          <Button variant="outline" onClick={addThreshold} disabled={thresholdDraft.trim() === ""}>
            {t("detail.settings.add")}
          </Button>
        </div>
      </div>

      <div className="rounded-lg border border-border bg-surface/50 p-4">
        <section className="space-y-2">
          <h4 className="text-xs font-semibold text-text-faint">
            {t("detail.anchor.title")}
          </h4>
          {messageAnchor ? (
            <>
              <label className="flex items-center justify-between gap-3 text-sm">
                <span>{t("detail.anchor.auto")}</span>
                <input
                  type="checkbox"
                  checked={autoOn}
                  onChange={(e) => toggleAuto(e.target.checked)}
                />
              </label>
              <Button variant="ghost" onClick={() => setConfirmOpen(true)}>
                {t("detail.anchor.sendNow")}
              </Button>
              <ConfirmDialog
                open={confirmOpen}
                title={t("detail.anchor.confirmTitle")}
                body={t("detail.anchor.confirmBody")}
                confirmLabel={t("detail.anchor.sendNow")}
                onConfirm={() => void sendAnchorNow(service.id)}
                onOpenChange={setConfirmOpen}
              />
            </>
          ) : (
            <p className="text-xs text-text-faint">{t("detail.anchor.unsupported")}</p>
          )}
          {/* Result feedback comes from the global anchor-result toast (F-10). */}
        </section>
      </div>

      <div className="rounded-lg border border-border bg-surface/50 p-4">
        <h2 className="mb-3 text-sm font-semibold text-crit">
          {t("detail.settings.dangerZone")}
        </h2>
        <Button variant="destructive" onClick={onOpenAdd}>
          <Cloud className="size-4" />
          {t("detail.settings.reauthAccount")}
        </Button>
      </div>
    </div>
  );
}
