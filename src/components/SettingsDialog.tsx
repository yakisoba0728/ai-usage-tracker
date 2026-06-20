import { useMemo, useState, type ReactNode } from "react";
import {
  Bell,
  Check,
  ChevronDown,
  Cloud,
  Code2,
  Database,
  Download,
  Monitor,
  RefreshCw,
  Settings2,
  Shield,
  SlidersHorizontal,
} from "lucide-react";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { ProviderMark, PROVIDER_LABEL } from "@/components/ProviderMark";
import { PROVIDER_ORDER } from "@/lib/providers";
import { cn } from "@/lib/utils";
import type { AppConfig, Provider } from "@/lib/types";

export type SortBy = "custom" | "usage" | "name";

type SettingsSection =
  | "general"
  | "providers"
  | "notifications"
  | "sessions"
  | "advanced";

const POLL_OPTIONS: { label: string; value: number }[] = [
  { label: "1 minute", value: 60 },
  { label: "5 minutes", value: 300 },
  { label: "15 minutes", value: 900 },
  { label: "30 minutes", value: 1800 },
];

const SECTION_NAV: {
  id: SettingsSection;
  label: string;
  icon: ReactNode;
}[] = [
  { id: "general", label: "General", icon: <Settings2 className="size-4" /> },
  { id: "providers", label: "Providers", icon: <Cloud className="size-4" /> },
  { id: "notifications", label: "Notifications", icon: <Bell className="size-4" /> },
  { id: "sessions", label: "Sessions", icon: <Database className="size-4" /> },
  { id: "advanced", label: "Advanced", icon: <Code2 className="size-4" /> },
];

export interface SettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  config: AppConfig | null;
  onConfigChange: (next: AppConfig) => void;
  sortBy: SortBy;
  onSortByChange: (s: SortBy) => void;
  showOffline: boolean;
  onShowOfflineChange: (b: boolean) => void;
  onReuseLocalSession?: () => void;
  onOpenAddAccount?: () => void;
}

export function SettingsDialog({
  open,
  onOpenChange,
  config,
  onConfigChange,
  sortBy,
  onSortByChange,
  showOffline,
  onShowOfflineChange,
  onReuseLocalSession,
  onOpenAddAccount,
}: SettingsDialogProps) {
  const [section, setSection] = useState<SettingsSection>("general");
  const [launchAtLogin, setLaunchAtLogin] = useState(true);
  const [autoUpdates, setAutoUpdates] = useState(true);
  const [compactMode, setCompactMode] = useState(false);
  const [crossingAlerts, setCrossingAlerts] = useState(true);
  const [refreshAlerts, setRefreshAlerts] = useState(false);
  const [localParsing, setLocalParsing] = useState(true);
  const enabledCount = useMemo(
    () => config?.providers.filter((provider) => provider.enabled).length ?? 0,
    [config],
  );

  function patchConfig(patch: Partial<AppConfig>) {
    if (!config) return;
    onConfigChange({ ...config, ...patch });
  }

  function toggleProvider(provider: Provider, enabled: boolean) {
    if (!config) return;
    const index = PROVIDER_ORDER.indexOf(provider);
    const providers = [...config.providers] as AppConfig["providers"];
    providers[index] = { ...providers[index], enabled };
    onConfigChange({ ...config, providers });
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="grid h-[min(640px,88dvh)] w-[min(780px,94vw)] max-w-none grid-cols-[200px_minmax(0,1fr)] gap-0 overflow-hidden rounded-lg border-border bg-[#202225] p-0 shadow-2xl shadow-black/50 max-md:h-[min(680px,94dvh)] max-md:grid-cols-1 max-md:grid-rows-[auto_minmax(0,1fr)]">
        <div className="min-h-0 overflow-y-auto border-r border-border bg-[#1a1d20] max-md:border-b max-md:border-r-0">
          <div className="flex h-14 items-center border-b border-border px-4">
            <DialogTitle className="text-sm font-semibold text-text">
              Settings
            </DialogTitle>
          </div>
          <nav className="grid gap-1 p-3 max-md:grid-cols-3 max-sm:grid-cols-2">
            {SECTION_NAV.map((item) => (
              <button
                key={item.id}
                type="button"
                onClick={() => setSection(item.id)}
                className={cn(
                  "flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-xs transition-colors",
                  section === item.id
                    ? "bg-[#256297] text-text"
                    : "text-text-dim hover:bg-surface hover:text-text",
                )}
              >
                <span className={section === item.id ? "text-text" : "text-text-faint"}>
                  {item.icon}
                </span>
                {item.label}
              </button>
            ))}
          </nav>
        </div>

        <div className="scroll-area min-h-0 min-w-0 overflow-y-auto">
          <div className="sticky top-0 z-10 flex h-14 items-center justify-between border-b border-border bg-[#202225]/95 px-5 backdrop-blur">
            <DialogDescription className="text-xs text-text-faint">
              Preferences are applied immediately.
            </DialogDescription>
          </div>

          <div className="p-5">
            {section === "general" && (
              <Section title="General" description="Display, refresh, and account visibility.">
                <SettingsRow
                  icon={<Monitor className="size-4" />}
                  label="Launch AI Usage Tracker at login"
                  description="Start the app automatically when you sign in."
                >
                  <Toggle checked={launchAtLogin} onChange={setLaunchAtLogin} />
                </SettingsRow>
                <SettingsRow
                  icon={<RefreshCw className="size-4" />}
                  label="Check for updates"
                  description="Automatically check for app updates."
                >
                  <Toggle checked={autoUpdates} onChange={setAutoUpdates} />
                </SettingsRow>
                <SettingsRow
                  icon={<SlidersHorizontal className="size-4" />}
                  label="Default refresh interval"
                  description="How often accounts are refreshed."
                >
                  <SelectValue
                    value={config?.poll_seconds ?? 300}
                    options={POLL_OPTIONS}
                    onChange={(value) => patchConfig({ poll_seconds: value })}
                    disabled={config == null}
                  />
                </SettingsRow>
                <SettingsRow
                  icon={<LayoutIcon />}
                  label="Sort account list"
                  description="Controls ordering inside Online and Offline groups."
                >
                  <Segmented
                    options={[
                      { label: "Custom", value: "custom" as const },
                      { label: "Usage", value: "usage" as const },
                      { label: "Name", value: "name" as const },
                    ]}
                    value={sortBy}
                    onChange={onSortByChange}
                  />
                </SettingsRow>
                <SettingsRow
                  icon={<Shield className="size-4" />}
                  label="Show offline accounts"
                  description="Keep disconnected providers visible in the monitor."
                >
                  <Toggle checked={showOffline} onChange={onShowOfflineChange} />
                </SettingsRow>
                <SettingsRow
                  icon={<Monitor className="size-4" />}
                  label="Compact mode"
                  description="Reduce spacing for smaller displays."
                >
                  <Toggle checked={compactMode} onChange={setCompactMode} />
                </SettingsRow>
              </Section>
            )}

            {section === "providers" && (
              <Section title="Providers" description={`${enabledCount} providers enabled.`}>
                <div className="space-y-1">
                  {PROVIDER_ORDER.map((provider, index) => (
                    <SettingsRow
                      key={provider}
                      icon={<ProviderMark provider={provider} className="size-4" />}
                      label={PROVIDER_LABEL[provider]}
                      description={
                        config?.providers[index].custom_name
                          ? `Shown as ${config.providers[index].custom_name}`
                          : "Default provider name"
                      }
                    >
                      <Toggle
                        checked={config?.providers[index].enabled ?? false}
                        disabled={config == null}
                        onChange={(enabled) => toggleProvider(provider, enabled)}
                      />
                    </SettingsRow>
                  ))}
                </div>
                <div className="mt-5 flex flex-wrap gap-2">
                  <ButtonLike onClick={() => onOpenAddAccount?.()}>
                    <Cloud className="size-4" />
                    Add provider account
                  </ButtonLike>
                  <ButtonLike onClick={() => onReuseLocalSession?.()}>
                    <RefreshCw className="size-4" />
                    Scan local sessions
                  </ButtonLike>
                </div>
              </Section>
            )}

            {section === "notifications" && (
              <Section title="Notifications" description="Alert behavior for quota crossings.">
                <SettingsRow
                  icon={<Bell className="size-4" />}
                  label="Usage crossing alerts"
                  description="Notify when a provider crosses configured thresholds."
                >
                  <Toggle checked={crossingAlerts} onChange={setCrossingAlerts} />
                </SettingsRow>
                <SettingsRow
                  icon={<RefreshCw className="size-4" />}
                  label="Refresh completion alerts"
                  description="Show a notification after manual refresh finishes."
                >
                  <Toggle checked={refreshAlerts} onChange={setRefreshAlerts} />
                </SettingsRow>
                <div className="mt-5 rounded-lg border border-border bg-canvas/50 p-4">
                  <div className="mb-3 text-sm font-semibold">Usage Thresholds</div>
                  <div className="grid gap-3 sm:grid-cols-2">
                    <ThresholdField label="Warning" value={70} />
                    <ThresholdField label="Critical" value={90} />
                  </div>
                  <p className="mt-3 text-xs text-text-faint">
                    Provider-specific thresholds can be changed from each account inspector.
                  </p>
                </div>
              </Section>
            )}

            {section === "sessions" && (
              <Section title="Sessions" description="Local parsing, OAuth, and stored credential behavior.">
                <SettingsRow
                  icon={<Database className="size-4" />}
                  label="Local session reuse"
                  description="Prefer existing CLI/browser sessions when available."
                >
                  <Toggle checked={localParsing} onChange={setLocalParsing} />
                </SettingsRow>
                <SettingsRow
                  icon={<Cloud className="size-4" />}
                  label="OAuth callback listener"
                  description="Use the desktop loopback callback for providers that support it."
                >
                  <StatusBadge label="Ready" />
                </SettingsRow>
                <div className="mt-5 flex flex-wrap gap-2">
                  <ButtonLike onClick={() => onReuseLocalSession?.()}>
                    <RefreshCw className="size-4" />
                    Reuse local sessions
                  </ButtonLike>
                  <ButtonLike onClick={() => onOpenAddAccount?.()}>
                    <Cloud className="size-4" />
                    Start sign-in
                  </ButtonLike>
                </div>
              </Section>
            )}

            {section === "advanced" && (
              <Section title="Advanced" description="Export, diagnostics, and reset actions.">
                <SettingsRow
                  icon={<Download className="size-4" />}
                  label="API & export"
                  description="Prepare provider usage data for external workflows."
                >
                  <ButtonLike onClick={() => undefined}>Export...</ButtonLike>
                </SettingsRow>
                <SettingsRow
                  icon={<Code2 className="size-4" />}
                  label="Diagnostic logs"
                  description="Open local log files and provider parse reports."
                >
                  <ButtonLike onClick={() => undefined}>Open logs</ButtonLike>
                </SettingsRow>
                <div className="mt-6 rounded-lg border border-crit/30 bg-crit/10 p-4">
                  <div className="text-sm font-semibold text-crit">Danger Zone</div>
                  <p className="mt-1 text-xs text-text-dim">
                    Reset display-only preferences without touching stored credentials.
                  </p>
                  <ButtonLike
                    danger
                    onClick={() => {
                      onSortByChange("custom");
                      onShowOfflineChange(true);
                    }}
                    className="mt-4"
                  >
                    Reset display preferences
                  </ButtonLike>
                </div>
              </Section>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function Section({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <section>
      {title && (
        <div className="mb-3">
          <h2 className="text-sm font-semibold text-text">{title}</h2>
          <p className="mt-1 text-xs text-text-faint">{description}</p>
        </div>
      )}
      <div className="space-y-1">{children}</div>
    </section>
  );
}

function SettingsRow({
  icon,
  label,
  description,
  children,
}: {
  icon: ReactNode;
  label: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-4 rounded-lg px-3 py-3 transition-colors hover:bg-white/[0.03] max-sm:grid-cols-1">
      <div className="flex min-w-0 gap-2.5">
        <span className="mt-0.5 text-text-faint">{icon}</span>
        <div className="min-w-0">
          <div className="text-sm font-medium leading-5 text-text">{label}</div>
          <div className="mt-0.5 text-xs leading-5 text-text-faint">{description}</div>
        </div>
      </div>
      <div className="flex items-center justify-end">{children}</div>
    </div>
  );
}

function Toggle({
  checked,
  onChange,
  disabled,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative h-5 w-9 rounded-full border transition-colors disabled:opacity-50",
        checked ? "border-[#4b9bea] bg-[#2c84d8]" : "border-border-strong bg-surface-2",
      )}
    >
      <span
        className={cn(
          "absolute left-0.5 top-1/2 size-4 -translate-y-1/2 rounded-full bg-text shadow-sm transition-transform",
          checked && "translate-x-4",
        )}
      />
    </button>
  );
}

function SelectValue({
  value,
  options,
  onChange,
  disabled,
}: {
  value: number;
  options: { label: string; value: number }[];
  onChange: (value: number) => void;
  disabled?: boolean;
}) {
  return (
    <label className="relative block">
      <select
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(Number(event.target.value))}
        className="h-8 appearance-none rounded-md border border-border bg-surface px-2.5 pr-7 text-xs text-text outline-none transition-colors focus:border-border-strong disabled:opacity-50"
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
      <ChevronDown className="pointer-events-none absolute right-2 top-1/2 size-4 -translate-y-1/2 text-text-faint" />
    </label>
  );
}

function Segmented<T extends string>({
  options,
  value,
  onChange,
}: {
  options: { label: string; value: T }[];
  value: T;
  onChange: (value: T) => void;
}) {
  return (
    <div className="inline-flex rounded-md border border-border bg-canvas p-0.5">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          onClick={() => onChange(option.value)}
          className={cn(
            "rounded px-2 py-1.5 text-[11px] font-medium transition-colors",
            value === option.value
              ? "bg-[#2c84d8] text-text"
              : "text-text-faint hover:text-text",
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

function StatusBadge({ label }: { label: string }) {
  return (
    <span className="inline-flex items-center gap-1.5 rounded-md border border-ok/30 bg-ok/10 px-2 py-1 text-xs font-medium text-ok">
      <Check className="size-3.5" />
      {label}
    </span>
  );
}

function ThresholdField({ label, value }: { label: string; value: number }) {
  return (
    <label>
      <span className="mb-1 block text-xs text-text-faint">{label}</span>
      <div className="flex h-9 items-center rounded-md border border-border bg-surface px-3">
        <input
          readOnly
          value={value}
          className="num min-w-0 flex-1 bg-transparent text-sm text-text outline-none"
        />
        <span className="num text-xs text-text-faint">%</span>
      </div>
    </label>
  );
}

function ButtonLike({
  children,
  onClick,
  danger = false,
  className,
}: {
  children: ReactNode;
  onClick: () => void;
  danger?: boolean;
  className?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "inline-flex h-9 items-center gap-2 rounded-md border px-3 text-sm font-medium transition-colors",
        danger
          ? "border-crit/40 bg-crit/20 text-crit hover:bg-crit/25"
          : "border-border bg-surface text-text-dim hover:border-border-strong hover:text-text",
        className,
      )}
    >
      {children}
    </button>
  );
}

function LayoutIcon() {
  return <SlidersHorizontal className="size-4" />;
}
