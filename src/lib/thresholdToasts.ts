import { providerConfigFor, resolveHeadlineWindow } from "@/lib/providers";
import type { AppConfig, Provider, UsageSnapshot } from "@/lib/types";

export interface ThresholdCrossing {
  serviceId: string;
  provider: Provider;
  threshold: number;
}

export function collectThresholdCrossings(
  snapshot: UsageSnapshot,
  config: AppConfig,
  previousPercents: Map<string, number>,
): ThresholdCrossing[] {
  const crossings: ThresholdCrossing[] = [];

  for (const service of snapshot.services) {
    const headline = resolveHeadlineWindow(service, config);
    const pct = headline?.used_percent;
    if (pct == null) {
      previousPercents.delete(service.id);
      continue;
    }

    const previous = previousPercents.get(service.id);
    previousPercents.set(service.id, pct);
    if (previous == null) continue;

    const crossed = (providerConfigFor(config, service.provider)?.notify_thresholds ?? [])
      .filter((threshold) => previous < threshold && pct >= threshold)
      .sort((a, b) => b - a)[0];

    if (crossed != null) {
      crossings.push({
        serviceId: service.id,
        provider: service.provider,
        threshold: crossed,
      });
    }
  }

  // Drop previous-percent entries for services no longer in the snapshot (a
  // removed account) so the map doesn't grow forever across churn (UG-3).
  const live = new Set(snapshot.services.map((s) => s.id));
  for (const id of previousPercents.keys()) {
    if (!live.has(id)) previousPercents.delete(id);
  }

  return crossings;
}
