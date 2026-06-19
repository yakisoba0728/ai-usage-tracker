import { PROVIDER_LABEL } from "@/components/ProviderMark";
import type { AppConfig, LimitWindow, Provider, ServiceUsage } from "@/lib/types";

/**
 * Canonical provider order — MUST mirror `provider_index` in the Rust
 * `config.rs` and `AppConfig.providers: [ProviderConfig; 6]`. The index of a
 * provider in this array is also its index into `AppConfig.providers`.
 */
export const PROVIDER_ORDER: Provider[] = [
  "claude",
  "codex",
  "gemini",
  "copilot",
  "cursor",
  "zai",
];

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
 * Ordered provider list for the grid, derived from `sort_index`. When every
 * sort_index is 0 (the default / fresh state), falls back to canonical order so
 * the very first render is deterministic before the user reorders anything.
 */
export function providerOrder(config: AppConfig | null): Provider[] {
  if (!config) return [...PROVIDER_ORDER];
  const allDefault = config.providers.every((p) => p.sort_index === 0);
  if (allDefault) return [...PROVIDER_ORDER];
  return [...PROVIDER_ORDER].sort((a, b) => {
    const sa = config.providers[providerIndex(a)].sort_index;
    const sb = config.providers[providerIndex(b)].sort_index;
    if (sa !== sb) return sa - sb;
    // Stable tie-break on canonical position.
    return providerIndex(a) - providerIndex(b);
  });
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

/**
 * Windows to render on a card: the resolved headline first, then the remaining
 * primary windows (the headline is not duplicated). Detail windows never spill
 * onto the card except when the user has explicitly pinned one as primary.
 */
export function cardWindows(
  service: ServiceUsage,
  config: AppConfig | null,
): LimitWindow[] {
  const primary = service.windows ?? [];
  const headline = resolveHeadlineWindow(service, config);
  if (!headline) return primary;
  const rest = primary.filter((w) => w !== headline);
  return [headline, ...rest];
}
