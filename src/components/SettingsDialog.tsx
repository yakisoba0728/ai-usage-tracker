import { useMemo, type ReactNode } from "react";
import {
  ChevronDown,
  Cloud,
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
import { ProviderMark, PROVIDER_LABEL } from "@/components/ProviderMark";
import { patchProviderConfig, PROVIDER_ORDER } from "@/lib/providers";
import { cn } from "@/lib/utils";
import type { AppConfig, Provider } from "@/lib/types";

export type SortBy = "custom" | "usage" | "name";

type SettingsSection = "general" | "providers";

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
  const { t } = useTranslation();
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

  // The settings surface is small enough that both sections render at once;
  // the nav scrolls to them. (We drop the old 5-section layout — Notifications,
  // Sessions, Advanced were non-functional scaffolding.)
  function patchConfig(patch: Partial<AppConfig>) {
    if (!config) return;
    onConfigChange({ ...config, ...patch });
  }

  function toggleProvider(provider: Provider, enabled: boolean) {
    if (!config) return;
    onConfigChange(patchProviderConfig(config, provider, { enabled }));
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
          <nav className="grid gap-1 p-3 max-md:grid-cols-2">
            {sectionNav.map((item) => (
              <div
                key={item.id}
                className="flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-xs text-text-dim"
              >
                <span className="text-text-faint">{item.icon}</span>
                {item.label}
              </div>
            ))}
          </nav>
        </div>

        <div className="scroll-area min-h-0 min-w-0 overflow-y-auto">
          <div className="sticky top-0 z-10 flex h-14 items-center justify-between border-b border-border bg-[#202225]/95 px-5 backdrop-blur">
            <DialogDescription className="text-xs text-text-faint">
              {t("settings.applied")}
            </DialogDescription>
          </div>

          <div className="space-y-6 p-5">
            <Section
              title={t("settings.generalTitle")}
              description={t("settings.generalDesc")}
            >
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
                />
              </SettingsRow>
              <SettingsRow
                icon={<Shield className="size-4" />}
                label={t("settings.showOffline")}
                description={t("settings.showOfflineDesc")}
              >
                <Toggle checked={showOffline} onChange={onShowOfflineChange} />
              </SettingsRow>
            </Section>

            <Section
              title={t("settings.providersTitle")}
              description={t("settings.providersDesc", { count: enabledCount })}
            >
              <div className="space-y-1">
                {PROVIDER_ORDER.map((provider, index) => (
                  <SettingsRow
                    key={provider}
                    icon={<ProviderMark provider={provider} className="size-4" />}
                    label={PROVIDER_LABEL[provider]}
                    description={
                      config?.providers[index].custom_name
                        ? t("settings.shownAs", {
                            name: config.providers[index].custom_name,
                          })
                        : t("settings.defaultName")
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
                  {t("settings.addProviderAccount")}
                </ButtonLike>
                <ButtonLike onClick={() => onReuseLocalSession?.()}>
                  <RefreshCw className="size-4" />
                  {t("settings.scanLocal")}
                </ButtonLike>
              </div>
            </Section>
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
