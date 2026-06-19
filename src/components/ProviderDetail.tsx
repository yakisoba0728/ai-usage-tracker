import { useEffect, useState, type ReactNode } from "react";
import { Trash2, X } from "lucide-react";

import { BarGauge } from "@/components/BarGauge";
import { ProviderMark } from "@/components/ProviderMark";
import { removeAccount } from "@/lib/ipc";
import { PROVIDER_ORDER, providerDisplayName } from "@/lib/providers";
import { clamp } from "@/lib/utils";
import { DialogDescription, DialogTitle } from "@/components/ui/dialog";
import type {
  AppConfig,
  LimitWindow,
  Provider,
  ProviderConfig,
  ServiceUsage,
} from "@/lib/types";

/**
 * Source notes per provider — shown in the modal footer. Generic fallback so a
 * new provider needs no change here.
 */
const SOURCE_NOTE: Partial<Record<Provider, string>> = {
  claude: "Claude Code · api.anthropic.com",
  codex: "Codex CLI · chatgpt.com/backend-api",
  gemini: "Gemini CLI · Google Code Assist",
  copilot: "Copilot CLI · api.github.com/copilot_internal",
  cursor: "Cursor · api2.cursor.sh (experimental)",
  zai: "z.ai Coding Plan · glm coding plan",
};

type Tab = "usage" | "raw" | "settings";

export interface ProviderDetailProps {
  service: ServiceUsage;
  nowMs: number;
  /** Stored credential id, if this account was user-added (enables Remove). */
  accountId: string | null;
  onRemoved: () => void;
  /** App config — powers the Settings tab. */
  config: AppConfig | null;
  onConfigChange: (next: AppConfig) => void;
}

export function ProviderDetail({
  service,
  nowMs,
  accountId,
  onRemoved,
  config,
  onConfigChange,
}: ProviderDetailProps) {
  const [tab, setTab] = useState<Tab>("usage");
  const windows = service.windows ?? [];
  const detail = service.detail_windows ?? [];
  const hasRaw = Boolean(service.raw_response);
  const hasWindows = service.connected && windows.length > 0;
  const title = providerDisplayName(config, service.provider);
  const source = SOURCE_NOTE[service.provider] ?? `${title} usage`;

  async function handleRemove() {
    if (!accountId) return;
    try {
      await removeAccount(accountId);
    } catch (e) {
      console.error("remove_account failed:", e);
    } finally {
      onRemoved();
    }
  }

  const tabs: readonly Tab[] = hasRaw
    ? (["usage", "raw", "settings"] as const)
    : (["usage", "settings"] as const);

  return (
    <div className="flex flex-col">
      {/* Header */}
      <div className="flex items-start justify-between gap-3 border-b border-border px-5 py-4 pr-12">
        <div className="flex min-w-0 items-center gap-2.5">
          <span className="flex size-8 shrink-0 items-center justify-center rounded-md border border-border bg-surface-2 text-text-dim">
            <ProviderMark provider={service.provider} className="size-[18px]" />
          </span>
          <div className="min-w-0 leading-tight">
            <DialogTitle className="text-lg font-semibold tracking-tight text-text">
              {title}
            </DialogTitle>
            <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1">
              {service.plan && (
                <span
                  className="rounded border border-border-strong bg-surface-2 px-1.5 py-0.5 font-medium text-text-dim"
                  style={{ fontSize: 11 }}
                >
                  {service.plan}
                </span>
              )}
              <span
                className={
                  service.connected ? "font-medium text-text-dim" : "font-medium text-text-faint"
                }
                style={{ fontSize: 11 }}
              >
                {service.connected ? "Connected" : "Offline"}
              </span>
              {service.account && (
                <span
                  className="num truncate text-text-faint"
                  style={{ fontSize: 11 }}
                  title={service.account}
                >
                  {service.account}
                </span>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* Tab bar — Usage + Settings always; Raw when available. */}
      <div className="flex gap-1 border-b border-border px-5 pt-2">
        {tabs.map((t) => (
          <button
            key={t}
            type="button"
            onClick={() => setTab(t)}
            className={
              "border-b-2 px-3 py-2 transition-colors " +
              (tab === t
                ? "border-text text-text"
                : "border-transparent text-text-faint hover:text-text-dim")
            }
            style={{ fontSize: 12, fontWeight: 500 }}
          >
            {t === "usage" ? "Usage" : t === "raw" ? "Raw Response" : "Settings"}
          </button>
        ))}
      </div>

      {/* Body */}
      <div className="scroll-area max-h-[60vh] overflow-y-auto px-5 py-4">
        {tab === "raw" && hasRaw ? (
          <pre
            className="num overflow-x-auto whitespace-pre-wrap break-words rounded-md border border-border bg-canvas p-3 text-text-dim"
            style={{ fontSize: 11, lineHeight: 1.6 }}
          >
            {service.raw_response}
          </pre>
        ) : tab === "settings" ? (
          <ProviderSettings
            service={service}
            config={config}
            onConfigChange={onConfigChange}
          />
        ) : hasWindows ? (
          <div className="space-y-5">
            <Section title="Usage">
              <div className="divide-y divide-border">
                {windows.map((w, i) => (
                  <div key={`${w.label}-${i}`} className="py-3 first:pt-0 last:pb-0">
                    <BarGauge window={w} nowMs={nowMs} variant="primary" showUsedLimit />
                  </div>
                ))}
              </div>
            </Section>

            {detail.length > 0 && (
              <Section title="More detail">
                <div className="divide-y divide-border">
                  {detail.map((w, i) => (
                    <div key={`${w.label}-${i}`} className="py-3 first:pt-0 last:pb-0">
                      <BarGauge window={w} nowMs={nowMs} variant="primary" showUsedLimit />
                    </div>
                  ))}
                </div>
              </Section>
            )}
          </div>
        ) : (
          <div
            className="rounded-md border border-border bg-surface-2 px-3.5 py-3 text-text-dim"
            style={{ fontSize: 13 }}
          >
            {service.error?.trim() ??
              (service.connected ? "No usage windows reported." : "Not connected.")}
          </div>
        )}
      </div>

      {/* Footer */}
      <div className="flex items-center justify-between gap-3 border-t border-border px-5 py-3">
        <DialogDescription className="text-text-faint" style={{ fontSize: 11 }}>
          {source}
        </DialogDescription>
        {accountId && (
          <button
            type="button"
            onClick={handleRemove}
            className="flex shrink-0 items-center gap-1.5 rounded-md px-2 py-1 text-text-dim transition-colors hover:bg-surface-2 hover:text-text"
            style={{ fontSize: 12, fontWeight: 500 }}
          >
            <Trash2 className="size-3.5" />
            Remove
          </button>
        )}
      </div>
    </div>
  );
}

/* ── Settings tab ───────────────────────────────────────────────────────────── */

function ProviderSettings({
  service,
  config,
  onConfigChange,
}: {
  service: ServiceUsage;
  config: AppConfig | null;
  onConfigChange: (next: AppConfig) => void;
}) {
  const idx = PROVIDER_ORDER.indexOf(service.provider);
  const pc: ProviderConfig | null = config?.providers[idx] ?? null;

  const allWindows: LimitWindow[] = [
    ...(service.windows ?? []),
    ...(service.detail_windows ?? []),
  ];
  const windowLabels = Array.from(new Set(allWindows.map((w) => w.label)));
  const primaryWindow = pc?.primary_window ?? null;
  const selectValue =
    primaryWindow && windowLabels.includes(primaryWindow) ? primaryWindow : "";

  const [nameDraft, setNameDraft] = useState(pc?.custom_name ?? "");
  const [thresholdDraft, setThresholdDraft] = useState("");

  // Keep the local draft in sync if the underlying config changes externally
  // (e.g. another tab edits it). Editing locally does not write through until
  // blur / Enter, so this never fights typing.
  useEffect(() => {
    setNameDraft(pc?.custom_name ?? "");
  }, [service.provider, pc?.custom_name]);

  const thresholds = pc?.notify_thresholds ?? [];

  function patch(patch: Partial<ProviderConfig>) {
    if (!config) return;
    const providers = [...config.providers] as AppConfig["providers"];
    providers[idx] = { ...providers[idx], ...patch };
    onConfigChange({ ...config, providers });
  }

  function commitName() {
    const trimmed = nameDraft.trim();
    const next = trimmed.length > 0 ? trimmed : null;
    if (next !== (pc?.custom_name ?? null)) patch({ custom_name: next });
  }

  function removeThreshold(t: number) {
    patch({ notify_thresholds: thresholds.filter((x) => x !== t) });
  }

  function addThreshold() {
    const parsed = parseInt(thresholdDraft, 10);
    if (Number.isNaN(parsed)) return;
    const t = clamp(parsed, 0, 100);
    if (thresholds.includes(t)) {
      setThresholdDraft("");
      return;
    }
    patch({
      notify_thresholds: [...thresholds, t].sort((a, b) => a - b),
    });
    setThresholdDraft("");
  }

  const disabled = config == null;

  return (
    <div className="space-y-5">
      {/* Display name */}
      <Section title="Display name">
        <input
          type="text"
          value={nameDraft}
          disabled={disabled}
          placeholder={providerDisplayName(config, service.provider)}
          onChange={(e) => setNameDraft(e.target.value)}
          onBlur={commitName}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              commitName();
              (e.target as HTMLInputElement).blur();
            }
          }}
          className="num min-w-0 flex-1 rounded-md border border-border-strong bg-canvas px-2.5 py-1.5 text-text placeholder:text-text-faint focus:border-border-strong disabled:opacity-50"
          style={{ fontSize: 12 }}
        />
        <p className="mt-1.5 text-text-faint" style={{ fontSize: 11 }}>
          Leave empty to use the default name.
        </p>
      </Section>

      {/* Primary window */}
      <Section title="Primary window">
        <select
          value={selectValue}
          disabled={disabled || windowLabels.length === 0}
          onChange={(e) => patch({ primary_window: e.target.value || null })}
          className="min-w-0 flex-1 rounded-md border border-border-strong bg-canvas px-2.5 py-1.5 text-text focus:border-border-strong disabled:opacity-50"
          style={{ fontSize: 12 }}
        >
          <option value="">Auto (highest usage)</option>
          {windowLabels.map((label) => (
            <option key={label} value={label}>
              {label}
            </option>
          ))}
        </select>
        <p className="mt-1.5 text-text-faint" style={{ fontSize: 11 }}>
          Shown as the card headline. Falls back to auto when unavailable.
        </p>
      </Section>

      {/* Notification thresholds */}
      <Section title="Notification thresholds">
        {thresholds.length > 0 ? (
          <div className="flex flex-wrap gap-1.5">
            {thresholds.map((t) => (
              <span
                key={t}
                className="num inline-flex items-center gap-1 rounded border border-border-strong bg-surface-2 px-1.5 py-0.5 text-text"
                style={{ fontSize: 11 }}
              >
                {t}%
                <button
                  type="button"
                  onClick={() => removeThreshold(t)}
                  aria-label={`Remove ${t}% threshold`}
                  className="rounded-sm text-text-faint transition-colors hover:text-text"
                >
                  <X className="size-3" />
                </button>
              </span>
            ))}
          </div>
        ) : (
          <p className="text-text-faint" style={{ fontSize: 11 }}>
            No thresholds. Add one below to get notified on usage crossings.
          </p>
        )}
        <div className="mt-2 flex gap-2">
          <input
            type="number"
            min={0}
            max={100}
            inputMode="numeric"
            value={thresholdDraft}
            disabled={disabled}
            placeholder="e.g. 80"
            onChange={(e) => setThresholdDraft(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && addThreshold()}
            className="num w-24 rounded-md border border-border-strong bg-canvas px-2.5 py-1.5 text-text placeholder:text-text-faint focus:border-border-strong disabled:opacity-50"
            style={{ fontSize: 12 }}
          />
          <button
            type="button"
            onClick={addThreshold}
            disabled={disabled || thresholdDraft.trim() === ""}
            className="rounded-md border border-border-strong bg-surface-2 px-3 py-1.5 text-text transition-colors hover:border-text-faint disabled:opacity-50"
            style={{ fontSize: 12, fontWeight: 500 }}
          >
            Add
          </button>
        </div>
      </Section>
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section>
      <h4
        className="mb-2 uppercase tracking-[0.08em] text-text-faint"
        style={{ fontSize: 10, fontWeight: 600 }}
      >
        {title}
      </h4>
      {children}
    </section>
  );
}
