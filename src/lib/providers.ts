import { PROVIDER_LABEL, PROVIDER_ORDER } from "@/lib/providerMetadata";
import type {
  AccountConfig,
  AppConfig,
  LimitWindow,
  Provider,
  ProviderConfig,
  ServiceUsage,
} from "@/lib/types";

/** Providers where anchoring works (mirrors Rust anchor::supported() = Claude, Codex, z.ai). */
const ANCHOR_SUPPORTED: ReadonlySet<Provider> = new Set<Provider>(["claude", "codex", "zai"]);
export function anchorSupported(provider: Provider): boolean {
  return ANCHOR_SUPPORTED.has(provider);
}

export { PROVIDER_LABEL, PROVIDER_ORDER } from "@/lib/providerMetadata";

/** Index of a provider within `AppConfig.providers` (and PROVIDER_ORDER). */
export function providerIndex(provider: Provider): number {
  return PROVIDER_ORDER.indexOf(provider);
}

/** The ProviderConfig slot for a provider, or null before config has loaded. */
export function providerConfigFor(
  config: AppConfig | null,
  provider: Provider,
) {
  return config?.providers[providerIndex(provider)] ?? null;
}

/**
 * Immutably patch one provider's config slot, centralizing the fixed-tuple
 * map + cast so call sites don't each re-cast `[ProviderConfig; 6]`.
 */
export function patchProviderConfig(
  config: AppConfig,
  provider: Provider,
  patch: Partial<ProviderConfig>,
): AppConfig {
  const providers = config.providers.map((p, i) =>
    i === providerIndex(provider) ? { ...p, ...patch } : p,
  ) as AppConfig["providers"];
  return { ...config, providers };
}

/** Immutably set the per-service auto-anchor opt-in flag. */
export function setAutoAnchor(
  config: AppConfig,
  serviceId: string,
  enabled: boolean,
): AppConfig {
  return { ...config, auto_anchor: { ...config.auto_anchor, [serviceId]: enabled } };
}

/** The per-account config for a service id (`auto:<provider>` / `stored:<id>`),
 * or null before config has loaded / when the account has no overrides yet. */
export function accountConfigFor(
  config: AppConfig | null,
  serviceId: string,
): AccountConfig | null {
  return config?.accounts?.[serviceId] ?? null;
}

/**
 * Immutably patch one account's per-service config (display name / pinned
 * window), keyed by service id. This is the PER-ACCOUNT write path (BUG-2 fix):
 * it only touches `accounts[serviceId]`, never a sibling account.
 */
export function patchAccountConfig(
  config: AppConfig,
  serviceId: string,
  patch: Partial<AccountConfig>,
): AppConfig {
  const prev = config.accounts?.[serviceId] ?? {};
  return {
    ...config,
    accounts: { ...config.accounts, [serviceId]: { ...prev, ...patch } },
  };
}

/**
 * Display name for ONE account — honors that account's `custom_name` (keyed by
 * service id), else the canonical provider label. Per-account (BUG-2 fix): two
 * accounts of one provider resolve independently.
 */
export function providerDisplayName(
  config: AppConfig | null,
  serviceId: string,
  provider: Provider,
): string {
  const custom = accountConfigFor(config, serviceId)?.custom_name?.trim();
  return custom && custom.length > 0 ? custom : PROVIDER_LABEL[provider];
}

/**
 * Resolve the headline window for a card / toast — the account's pinned
 * `primary_window` (keyed by service id) if set and present, else the first
 * primary window, else the highest-burn window across primary + detail. Returns
 * null when there is no usable window at all.
 */
export function resolveHeadlineWindow(
  service: ServiceUsage,
  config: AppConfig | null,
): LimitWindow | null {
  const ac = accountConfigFor(config, service.id);
  const primary = service.windows ?? [];
  if (ac?.primary_window) {
    const all = [...primary, ...(service.detail_windows ?? [])];
    const pinned = all.find((w) => w.label === ac.primary_window);
    if (pinned) return pinned;
  }
  if (primary.length > 0) return primary[0];
  const detail = service.detail_windows ?? [];
  return highestBurnWindow([...primary, ...detail]);
}

/**
 * The window with the highest `used_percent` (ties keep first). Used by the
 * compact popover row and as a last-resort headline fallback. Null percents
 * rank last.
 */
export function highestBurnWindow(
  windows: LimitWindow[],
): LimitWindow | null {
  let best: LimitWindow | null = null;
  let bestPct = -Infinity;
  for (const w of windows) {
    const p = w.used_percent;
    if (p == null) continue;
    if (p > bestPct) {
      bestPct = p;
      best = w;
    }
  }
  return best;
}
