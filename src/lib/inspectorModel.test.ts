import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";

import {
  buildAccountSections,
  buildInspectorSummary,
  selectVisibleServiceId,
} from "@/lib/inspectorModel";
import type { AppConfig, Provider, ServiceUsage } from "@/lib/types";

const t = ((key: string, opts?: Record<string, unknown>) =>
  opts ? `${key}:${JSON.stringify(opts)}` : key) as unknown as TFunction;

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
    notify_thresholds: [70, 90],
    primary_window: null,
    sort_index: index,
  })) as AppConfig["providers"],
};

describe("inspector model", () => {
  it("groups visible accounts by online and offline state", () => {
    const sections = buildAccountSections(sampleServices(), config, {
      query: "",
      showOffline: true,
    });

    expect(sections.map((section) => section.key)).toEqual(["online", "offline"]);
    expect(sections.map((section) => section.count)).toEqual([4, 1]);
    expect(sections.flatMap((section) => section.rows.map((row) => row.id))).toEqual([
      "stored:claude-work",
      "auto:codex",
      "auto:zai",
      "auto:gemini",
      "auto:cursor",
    ]);
  });

  it("filters accounts by provider, label, plan, and account id", () => {
    const sections = buildAccountSections(sampleServices(), config, {
      query: "plus",
      showOffline: true,
    });

    expect(sections).toHaveLength(1);
    expect(sections[0]?.key).toBe("online");
    expect(sections[0]?.rows[0]?.id).toBe("auto:codex");
  });

  it("sorts account rows by the selected display preference", () => {
    const sections = buildAccountSections(sampleServices(), config, {
      query: "",
      showOffline: true,
      sortBy: "name",
    });

    const online = sections.find((section) => section.key === "online");
    expect(online?.rows.map((row) => row.id)).toEqual([
      "stored:claude-work",
      "auto:codex",
      "auto:gemini",
      "auto:zai",
    ]);
  });

  it("keeps modal selection only while the selected account remains visible", () => {
    const sections = buildAccountSections(sampleServices(), config, {
      query: "",
      showOffline: false,
    });

    expect(selectVisibleServiceId("auto:codex", sections)).toBe("auto:codex");
    expect(selectVisibleServiceId("auto:cursor", sections)).toBeNull();
    expect(selectVisibleServiceId(null, [])).toBeNull();
  });

  it("projects the selected service into inspector metrics", () => {
    const summary = buildInspectorSummary(sampleServices()[0]!, config, 1_000_000, t);

    expect(summary.overallPercent).toBe(98);
    expect(summary.resetLabel).toBe('time.reset.minutes:{"m":15}');
    expect(summary.primaryUsedLimit).toBe("4,900 / 5,000");
    expect(summary.metricCards.map((metric) => metric.label)).toEqual([
      "Messages",
      "Tokens (Input)",
      "Tokens (Output)",
    ]);
  });
});

function sampleServices(): ServiceUsage[] {
  return [
    service("stored:claude-work", "claude", true, 98, "Claude - Work", "Max", [
      ["Messages", 98, 4_900, 5_000],
      ["Tokens (Input)", 64, 320_000, 500_000],
      ["Tokens (Output)", 84, 210_000, 250_000],
    ]),
    service("auto:codex", "codex", true, 82, "codex_9c2f7b", "Plus"),
    service("auto:gemini", "gemini", true, 36, "g_8bd91f22", "Code Assist"),
    service("auto:cursor", "cursor", false, null, null, null),
    service("auto:zai", "zai", true, 42, "z.ai workspace", "Pro"),
  ];
}

function service(
  id: string,
  provider: Provider,
  connected: boolean,
  pct: number | null,
  account: string | null,
  plan: string | null,
  detail: [string, number, number, number][] = [],
): ServiceUsage {
  return {
    id,
    source: id.startsWith("stored:") ? "stored" : "auto",
    provider,
    connected,
    plan,
    account,
    error: connected ? null : "Session not found.",
    windows:
      pct == null
        ? []
        : [
            {
              label: "Overall Usage",
              used_percent: pct,
              resets_at: 1_000_000 / 1000 + 15 * 60,
              used: 4_900,
              limit: 5_000,
            },
          ],
    detail_windows: detail.map(([label, usedPercent, used, limit]) => ({
      label,
      used_percent: usedPercent,
      resets_at: 1_000_000 / 1000 + 15 * 60,
      used,
      limit,
    })),
  };
}
