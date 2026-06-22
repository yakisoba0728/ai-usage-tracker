import { describe, expect, it } from "vitest";

import { collectThresholdCrossings } from "@/lib/thresholdToasts";
import type { AppConfig, Provider, ServiceUsage, UsageSnapshot } from "@/lib/types";

const providers: Provider[] = [
  "claude",
  "codex",
  "gemini",
  "copilot",
  "cursor",
  "zai",
];

const config: AppConfig = {
  poll_seconds: 300,
  providers: providers.map((_, index) => ({
    enabled: true,
    custom_name: null,
    notify_thresholds: [50, 75, 90],
    primary_window: null,
    sort_index: index,
  })) as AppConfig["providers"],
  auto_anchor: {},
};

function usage(fetchedAt: number, pct: number): UsageSnapshot {
  return {
    fetched_at: fetchedAt,
    services: [service(pct)],
  };
}

function service(pct: number): ServiceUsage {
  return {
    id: "auto:claude",
    source: "auto",
    provider: "claude",
    connected: true,
    plan: null,
    account: null,
    error: null,
    windows: [
      {
        label: "5-hour",
        used_percent: pct,
        resets_at: null,
        used: null,
        limit: null,
      },
    ],
    detail_windows: [],
  };
}

describe("collectThresholdCrossings", () => {
  it("reports a same-second crossing when the snapshot object changes", () => {
    const previous = new Map<string, number>();

    expect(collectThresholdCrossings(usage(100, 74), config, previous)).toEqual([]);
    expect(collectThresholdCrossings(usage(100, 76), config, previous)).toEqual([
      { serviceId: "auto:claude", provider: "claude", threshold: 75 },
    ]);
  });

  it("reports only the highest crossed threshold for one snapshot", () => {
    const previous = new Map<string, number>([["auto:claude", 40]]);

    expect(collectThresholdCrossings(usage(101, 92), config, previous)).toEqual([
      { serviceId: "auto:claude", provider: "claude", threshold: 90 },
    ]);
  });
});
