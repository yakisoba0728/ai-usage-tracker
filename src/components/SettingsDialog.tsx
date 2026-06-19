import { useEffect, useState } from "react";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ProviderMark, PROVIDER_LABEL } from "@/components/ProviderMark";
import { getConfig, setConfig } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import type { AppConfig, Provider } from "@/lib/types";

/** Display sort order — local Dashboard state, not persisted to the backend. */
export type SortBy = "usage" | "name";

/** Poll-interval segmented options — labels map to backend seconds. */
const POLL_OPTIONS: { label: string; value: number }[] = [
  { label: "1m", value: 60 },
  { label: "5m", value: 300 },
  { label: "15m", value: 900 },
  { label: "30m", value: 1800 },
];

/**
 * enabled[] index order — MUST mirror PROVIDER_ORDER in the Rust AppConfig:
 * claude / codex / gemini / copilot / cursor / zai.
 */
const PROVIDER_ORDER: Provider[] = [
  "claude",
  "codex",
  "gemini",
  "copilot",
  "cursor",
  "zai",
];

export interface SettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Display-only preferences (local state in Dashboard). */
  sortBy: SortBy;
  onSortByChange: (s: SortBy) => void;
  showOffline: boolean;
  onShowOfflineChange: (b: boolean) => void;
  /** Open the Add-account flow (closes settings first). */
  onAddAccount: () => void;
}

/**
 * Three flat-gray sections: poll interval (segmented), providers (toggle rows
 * that persist to the backend config), and display (sort + show-offline, local
 * Dashboard state). Poll + provider changes are optimistic: the UI updates
 * immediately and the poll loop picks up the new interval / provider set on its
 * next cycle.
 */
export function SettingsDialog({
  open,
  onOpenChange,
  sortBy,
  onSortByChange,
  showOffline,
  onShowOfflineChange,
  onAddAccount,
}: SettingsDialogProps) {
  const [config, setConfigState] = useState<AppConfig | null>(null);

  // Load config whenever the dialog opens.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    getConfig()
      .then((c) => {
        if (!cancelled) setConfigState(c);
      })
      .catch((e) => console.error("get_config failed:", e));
    return () => {
      cancelled = true;
    };
  }, [open]);

  // Optimistic update + persist.
  function persist(next: AppConfig) {
    setConfigState(next);
    void setConfig(next).catch((e) => console.error("set_config failed:", e));
  }

  function toggleProvider(i: number, on: boolean) {
    if (!config) return;
    const enabled = [...config.enabled] as AppConfig["enabled"];
    enabled[i] = on;
    persist({ ...config, enabled });
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="gap-0 overflow-hidden rounded-lg border-border bg-surface p-0 sm:max-w-[420px]">
        <DialogHeader className="border-b border-border px-5 py-4 pr-12">
          <DialogTitle className="text-lg font-semibold tracking-tight text-text">
            Settings
          </DialogTitle>
          <DialogDescription className="text-text-faint" style={{ fontSize: 12 }}>
            Polling, providers, and display.
          </DialogDescription>
        </DialogHeader>

        <div className="scroll-area max-h-[70vh] space-y-6 overflow-y-auto px-5 py-5">
          {/* Poll interval */}
          <Section title="Poll interval">
            <Segmented
              options={POLL_OPTIONS}
              value={config?.poll_seconds}
              onChange={(s) => config && persist({ ...config, poll_seconds: s })}
              disabled={config == null}
            />
          </Section>

          {/* Providers */}
          <Section title="Providers">
            <div className="space-y-0.5">
              {PROVIDER_ORDER.map((p, i) => (
                <Row key={p} label={PROVIDER_LABEL[p]} mark={p}>
                  <Toggle
                    checked={config?.enabled[i] ?? false}
                    disabled={config == null}
                    onChange={(on) => toggleProvider(i, on)}
                    ariaLabel={`Enable ${PROVIDER_LABEL[p]}`}
                  />
                </Row>
              ))}
            </div>
            <button
              type="button"
              onClick={onAddAccount}
              className="mt-1 rounded-md px-1.5 py-1.5 text-text-dim transition-colors hover:bg-surface-2 hover:text-text"
              style={{ fontSize: 12, fontWeight: 500 }}
            >
              + Add account
            </button>
          </Section>

          {/* Display */}
          <Section title="Display">
            <div className="space-y-0.5">
              <Row label="Sort by">
                <Segmented
                  options={[
                    { label: "Usage", value: "usage" as const },
                    { label: "Name", value: "name" as const },
                  ]}
                  value={sortBy}
                  onChange={onSortByChange}
                />
              </Row>
              <Row label="Show offline">
                <Toggle
                  checked={showOffline}
                  onChange={onShowOfflineChange}
                  ariaLabel="Show offline providers"
                />
              </Row>
            </div>
          </Section>
        </div>
      </DialogContent>
    </Dialog>
  );
}

/* ── Local primitives (flat gray, no library) ──────────────────────────────── */

function Section({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section className="space-y-2">
      <h4
        className="uppercase tracking-[0.08em] text-text-faint"
        style={{ fontSize: 10, fontWeight: 600 }}
      >
        {title}
      </h4>
      {children}
    </section>
  );
}

function Row({
  label,
  mark,
  children,
}: {
  label: string;
  mark?: Provider;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-md px-1.5 py-1.5">
      <div className="flex min-w-0 items-center gap-2.5">
        {mark && (
          <ProviderMark provider={mark} className="size-4 shrink-0 text-text-dim" />
        )}
        <span className="truncate text-text" style={{ fontSize: 13 }}>
          {label}
        </span>
      </div>
      {children}
    </div>
  );
}

interface SegmentedProps<T extends string | number> {
  options: { label: string; value: T }[];
  value: T | undefined;
  onChange: (v: T) => void;
  disabled?: boolean;
}

/** Compact segmented control — active segment inverts to surface-2 + text. */
function Segmented<T extends string | number>({
  options,
  value,
  onChange,
  disabled,
}: SegmentedProps<T>) {
  return (
    <div
      className={cn(
        "inline-flex rounded-md border border-border bg-canvas p-0.5",
        disabled && "opacity-50",
      )}
    >
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          disabled={disabled}
          onClick={() => onChange(o.value)}
          className={cn(
            "rounded px-2.5 py-1 font-medium transition-colors",
            value === o.value
              ? "bg-surface-2 text-text"
              : "text-text-faint hover:text-text-dim",
          )}
          style={{ fontSize: 12 }}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

interface ToggleProps {
  checked: boolean;
  onChange: (on: boolean) => void;
  disabled?: boolean;
  ariaLabel?: string;
}

/** Flat CSS switch — white track when on, gray when off. 150ms slide. */
function Toggle({ checked, onChange, disabled, ariaLabel }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative h-5 w-9 shrink-0 rounded-full border transition-colors duration-150",
        checked ? "border-transparent bg-text" : "border-border-strong bg-surface-2",
        disabled && "opacity-50",
      )}
    >
      <span
        className={cn(
          "absolute left-0.5 top-1/2 size-3.5 rounded-full transition-[transform,background-color] duration-150",
          checked
            ? "translate-x-[18px] -translate-y-1/2 bg-canvas"
            : "-translate-y-1/2 bg-text-faint",
        )}
      />
    </button>
  );
}
