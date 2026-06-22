import { useMemo, useState, type ReactNode } from "react";
import {
  ChevronDown,
  Cloud,
  Download,
  Globe,
  Power,
  RefreshCw,
  Settings2,
  Shield,
  SlidersHorizontal,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { ProviderMark } from "@/components/ProviderMark";
import { PROVIDER_LABEL } from "@/lib/providerMetadata";
import { patchProviderConfig, PROVIDER_ORDER } from "@/lib/providers";
import { toggleThreshold } from "@/lib/thresholds";
import { cn } from "@/lib/utils";
import type { AppConfig, Provider } from "@/lib/types";

export type SortBy = "custom" | "usage" | "name";

type SettingsSection = "general" | "providers";

/** Notification threshold chips offered per provider (percent). */
const THRESHOLD_CHIPS = [50, 75, 90, 95, 100] as const;

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
  /** Toggle launch-at-login — calls `set_launch_at_login` (OS item + persist). */
  onLaunchAtLoginChange?: (enable: boolean) => void;
  /** Manual "Check for updates" — calls `check_update_now` and toasts result. */
  onCheckForUpdates?: () => void;
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
  onLaunchAtLoginChange,
  onCheckForUpdates,
}: SettingsDialogProps) {
  const { t, i18n } = useTranslation();
  // The rail is now real navigation: only the active section renders on the
  // right (previously both rendered at once on this small surface).
  const [activeSection, setActiveSection] = useState<SettingsSection>("general");
  // One provider's threshold chips open at a time (keeps the dialog compact).
  const [expandedProvider, setExpandedProvider] = useState<Provider | null>(null);
  const enabledCount = useMemo(
    () => config?.providers.filter((provider) => provider.enabled).length ?? 0,
    [config],
  );

  const pollOptions = [
    { label: t("settings.poll.1m"), value: 60 },
    { label: t("settings.poll.5m"), value: 300 },
    { label: t("settings.poll.15m"), value: 900 },
    { label: t("settings.poll.30m"), value: 1800 },
  ];

  const sectionNav: { id: SettingsSection; label: string; icon: ReactNode }[] = [
    { id: "general", label: t("settings.nav.general"), icon: <Settings2 className="size-4" /> },
    { id: "providers", label: t("settings.nav.providers"), icon: <Cloud className="size-4" /> },
  ];

  const currentLang = i18n.resolvedLanguage === "ko" ? "ko" : "en";

  function patchConfig(patch: Partial<AppConfig>) {
    if (!config) return;
    onConfigChange({ ...config, ...patch });
  }

  function toggleProvider(provider: Provider, enabled: boolean) {
    if (!config) return;
    onConfigChange(patchProviderConfig(config, provider, { enabled }));
  }

  function toggleProviderThreshold(provider: Provider, value: number) {
    if (!config) return;
    const index = PROVIDER_ORDER.indexOf(provider);
    const current = config.providers[index]?.notify_thresholds ?? [];
    onConfigChange(
      patchProviderConfig(config, provider, {
        notify_thresholds: toggleThreshold(current, value),
      }),
    );
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="grid h-[min(560px,88dvh)] w-[min(720px,94vw)] max-w-none grid-cols-[180px_minmax(0,1fr)] gap-0 overflow-hidden rounded-lg border-border bg-[#202225] p-0 shadow-2xl shadow-black/50 max-md:h-[min(620px,94dvh)] max-md:grid-cols-1 max-md:grid-rows-[auto_minmax(0,1fr)]">
        <div className="min-h-0 overflow-y-auto border-r border-border bg-[#1a1d20] max-md:border-b max-md:border-r-0">
          <div className="flex h-14 items-center border-b border-border px-4">
            <DialogTitle className="text-sm font-semibold text-text">
              {t("settings.title")}
            </DialogTitle>
          </div>
          <div className="grid gap-1 p-3 max-md:grid-cols-2">
            {sectionNav.map((item) => {
              const active = item.id === activeSection;
              return (
                <button
                  key={item.id}
                  type="button"
                  aria-current={active ? "page" : undefined}
                  onClick={() => setActiveSection(item.id)}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-xs transition-colors",
                    active
                      ? "bg-[#2c84d8] font-medium text-text"
                      : "text-text-dim hover:bg-white/[0.04] hover:text-text",
                  )}
                >
                  <span className={active ? "text-text" : "text-text-faint"}>
                    {item.icon}
                  </span>
                  {item.label}
                </button>
              );
            })}
          </div>
        </div>

        <div className="scroll-area min-h-0 min-w-0 overflow-y-auto">
          <div className="sticky top-0 z-10 flex h-14 items-center justify-between border-b border-border bg-[#202225]/95 px-5 backdrop-blur">
            <DialogDescription className="text-xs text-text-faint">
              {t("settings.applied")}
            </DialogDescription>
          </div>

          <div className="space-y-6 p-5">
            {activeSection === "general" ? (
              <>
                <Section title={t("settings.group.display")} description="">
                  <SettingsRow
                    icon={<SlidersHorizontal className="size-4" />}
                    label={t("settings.refreshInterval")}
                    description={t("settings.refreshIntervalDesc")}
                  >
                    <SelectValue
                      value={config?.poll_seconds ?? 300}
                      options={pollOptions}
                      onChange={(value) => patchConfig({ poll_seconds: value })}
                      disabled={config == null}
                      ariaLabel={t("settings.refreshInterval")}
                    />
                  </SettingsRow>
                  <SettingsRow
                    icon={<SlidersHorizontal className="size-4" />}
                    label={t("settings.sortList")}
                    description={t("settings.sortListDesc")}
                  >
                    <Segmented
                      options={[
                        { label: t("settings.sort.custom"), value: "custom" as const },
                        { label: t("settings.sort.usage"), value: "usage" as const },
                        { label: t("settings.sort.name"), value: "name" as const },
                      ]}
                      value={sortBy}
                      onChange={onSortByChange}
                      ariaLabel={t("settings.sortList")}
                    />
                  </SettingsRow>
                  <SettingsRow
                    icon={<Shield className="size-4" />}
                    label={t("settings.showOffline")}
                    description={t("settings.showOfflineDesc")}
                  >
                    <Toggle
                      checked={showOffline}
                      onChange={onShowOfflineChange}
                      ariaLabel={t("settings.showOffline")}
                    />
                  </SettingsRow>
                  <SettingsRow
                    icon={<Globe className="size-4" />}
                    label={t("settings.language")}
                    description={t("settings.languageDesc")}
                  >
                    <Segmented
                      options={[
                        { label: t("language.english"), value: "en" as const },
                        { label: t("language.korean"), value: "ko" as const },
                      ]}
                      value={currentLang}
                      onChange={(lang) => void i18n.changeLanguage(lang)}
                      ariaLabel={t("settings.language")}
                    />
                  </SettingsRow>
                </Section>

                <Section title={t("settings.group.system")} description="">
                  <SettingsRow
                    icon={<Power className="size-4" />}
                    label={t("settings.launchAtLogin")}
                    description={t("settings.launchAtLoginDesc")}
                  >
                    <Toggle
                      checked={config?.launch_at_login ?? false}
                      disabled={config == null}
                      onChange={(enable) => onLaunchAtLoginChange?.(enable)}
                      ariaLabel={t("settings.launchAtLogin")}
                    />
                  </SettingsRow>
                </Section>

                <Section title={t("settings.group.updates")} description="">
                  <SettingsRow
                    icon={<Download className="size-4" />}
                    label={t("settings.autoUpdate")}
                    description={t("settings.autoUpdateDesc")}
                  >
                    <Toggle
                      checked={config?.auto_update_check ?? true}
                      disabled={config == null}
                      onChange={(checked) => patchConfig({ auto_update_check: checked })}
                      ariaLabel={t("settings.autoUpdate")}
                    />
                  </SettingsRow>
                  <SettingsRow
                    icon={<Download className="size-4" />}
                    label={t("settings.updateMode")}
                    description={t("settings.updateModeDesc")}
                  >
                    <Segmented
                      options={[
                        { label: t("settings.updateModeOption.notify"), value: "notify" as const },
                        {
                          label: t("settings.updateModeOption.notifyOpen"),
                          value: "notifyOpen" as const,
                        },
                      ]}
                      value={config?.update_auto_open ? "notifyOpen" : "notify"}
                      onChange={(mode) =>
                        patchConfig({ update_auto_open: mode === "notifyOpen" })
                      }
                      ariaLabel={t("settings.updateMode")}
                    />
                  </SettingsRow>
                  <SettingsRow
                    icon={<Download className="size-4" />}
                    label={t("settings.checkUpdates")}
                    description={t("settings.checkUpdatesDesc")}
                  >
                    <ButtonLike onClick={() => onCheckForUpdates?.()}>
                      <RefreshCw className="size-4" />
                      {t("settings.checkUpdatesNow")}
                    </ButtonLike>
                  </SettingsRow>
                </Section>
              </>
            ) : (
              <Section
                title={t("settings.providersTitle")}
                description={t("settings.providersDesc", { count: enabledCount })}
              >
                <div className="space-y-1">
                  {PROVIDER_ORDER.map((provider, index) => {
                    const expanded = expandedProvider === provider;
                    const thresholds = config?.providers[index]?.notify_thresholds ?? [];
                    return (
                      <div key={provider}>
                        <SettingsRow
                          icon={<ProviderMark provider={provider} className="size-4" />}
                          label={PROVIDER_LABEL[provider]}
                          // Per-account display names moved to the account detail
                          // dialog (BUG-2); this row toggles the provider on/off
                          // and (via the chevron) its notification thresholds.
                          description={t("settings.defaultName")}
                        >
                          <div className="flex items-center gap-2">
                            <button
                              type="button"
                              aria-expanded={expanded}
                              aria-label={t(
                                expanded
                                  ? "settings.collapseThresholds"
                                  : "settings.expandThresholds",
                              )}
                              disabled={config == null}
                              onClick={() =>
                                setExpandedProvider(expanded ? null : provider)
                              }
                              className="grid size-7 place-items-center rounded-md border border-border bg-surface text-text-faint transition-colors hover:border-border-strong hover:text-text disabled:opacity-50"
                            >
                              <ChevronDown
                                className={cn(
                                  "size-4 transition-transform",
                                  expanded && "rotate-180",
                                )}
                              />
                            </button>
                            <Toggle
                              checked={config?.providers[index].enabled ?? false}
                              disabled={config == null}
                              onChange={(enabled) => toggleProvider(provider, enabled)}
                              ariaLabel={PROVIDER_LABEL[provider]}
                            />
                          </div>
                        </SettingsRow>
                        {expanded && (
                          <div className="mb-1 ml-9 mr-3 rounded-lg border border-border bg-canvas/60 px-3 py-3">
                            <div className="text-xs font-medium text-text-dim">
                              {t("settings.thresholds")}
                            </div>
                            <div className="mt-1 text-[11px] text-text-faint">
                              {t("settings.thresholdsDesc")}
                            </div>
                            <div
                              role="group"
                              aria-label={t("settings.thresholds")}
                              className="mt-2.5 flex flex-wrap gap-1.5"
                            >
                              {THRESHOLD_CHIPS.map((value) => {
                                const on = thresholds.includes(value);
                                return (
                                  <button
                                    key={value}
                                    type="button"
                                    role="checkbox"
                                    aria-checked={on}
                                    disabled={config == null}
                                    onClick={() => toggleProviderThreshold(provider, value)}
                                    className={cn(
                                      "num rounded-md border px-2.5 py-1 text-xs font-medium transition-colors disabled:opacity-50",
                                      on
                                        ? "border-[#4b9bea] bg-[#2c84d8] text-text"
                                        : "border-border bg-surface text-text-faint hover:border-border-strong hover:text-text",
                                    )}
                                  >
                                    {value}%
                                  </button>
                                );
                              })}
                            </div>
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
                <div className="mt-5 flex flex-wrap gap-2">
                  <ButtonLike onClick={() => onOpenAddAccount?.()}>
                    <Cloud className="size-4" />
                    {t("settings.addProviderAccount")}
                  </ButtonLike>
                  <ButtonLike onClick={() => onReuseLocalSession?.()}>
                    <RefreshCw className="size-4" />
                    {t("settings.scanLocal")}
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
          {description && <p className="mt-1 text-xs text-text-faint">{description}</p>}
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
  ariaLabel,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  ariaLabel: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
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
  ariaLabel,
}: {
  value: number;
  options: { label: string; value: number }[];
  onChange: (value: number) => void;
  disabled?: boolean;
  ariaLabel: string;
}) {
  return (
    <label className="relative block">
      <select
        value={value}
        disabled={disabled}
        aria-label={ariaLabel}
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
  ariaLabel,
}: {
  options: { label: string; value: T }[];
  value: T;
  onChange: (value: T) => void;
  ariaLabel: string;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      className="inline-flex rounded-md border border-border bg-canvas p-0.5"
    >
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={value === option.value}
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

function ButtonLike({
  children,
  onClick,
}: {
  children: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="inline-flex h-9 items-center gap-2 rounded-md border border-border bg-surface px-3 text-sm font-medium text-text-dim transition-colors hover:border-border-strong hover:text-text"
    >
      {children}
    </button>
  );
}
