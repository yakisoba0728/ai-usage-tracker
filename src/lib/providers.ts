import { PROVIDER_LABEL, PROVIDER_ORDER } from "@/lib/providerMetadata";
import type {
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

/**
 * Display name for a provider — honors a user `custom_name`, else the canonical
 * label. Mirrors `AppConfig::display_name` in config.rs.
 */
export function providerDisplayName(
  config: AppConfig | null,
  provider: Provider,
): string {
  const pc = providerConfigFor(config, provider);
  const custom = pc?.custom_name?.trim();
  return custom && custom.length > 0 ? custom : PROVIDER_LABEL[provider];
}

/**
 * Resolve the headline window for a card / toast — the user's pinned
 * `primary_window` if set and present, else the first primary window, else the
 * highest-burn window across primary + detail. Returns null when there is no
 * usable window at all.
 */
export function resolveHeadlineWindow(
  service: ServiceUsage,
  config: AppConfig | null,
): LimitWindow | null {
  const pc = providerConfigFor(config, service.provider);
  const primary = service.windows ?? [];
  if (pc?.primary_window) {
    const all = [...primary, ...(service.detail_windows ?? [])];
    const pinned = all.find((w) => w.label === pc.primary_window);
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
