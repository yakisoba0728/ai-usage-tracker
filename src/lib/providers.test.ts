import { describe, expect, it } from "vitest";

import {
  highestBurnWindow,
  providerDisplayName,
  providerIndex,
  resolveHeadlineWindow,
  setAutoAnchor,
} from "@/lib/providers";
import type {
  AccountConfig,
  AppConfig,
  LimitWindow,
  ProviderConfig,
  ServiceUsage,
} from "@/lib/types";

function win(label: string, used_percent: number | null): LimitWindow {
  return { label, used_percent, resets_at: null, used: null, limit: null };
}

function pc(sort_index: number): ProviderConfig {
  return {
    enabled: true,
    notify_thresholds: [50, 75, 90, 95, 100],
    sort_index,
  };
}

function cfg(
  slots: ProviderConfig[],
  accounts: Record<string, AccountConfig> = {},
): AppConfig {
  return {
    schema_version: 1,
    poll_seconds: 300,
    providers: slots as AppConfig["providers"],
    accounts,
    auto_anchor: {},
  };
}

function service(partial: Partial<ServiceUsage>): ServiceUsage {
  return {
    id: "auto:claude",
    source: "auto",
    provider: "claude",
    connected: true,
    plan: null,
    account: null,
    error: null,
    windows: [],
    detail_windows: [],
    ...partial,
  };
}

describe("providerIndex", () => {
  it("maps a provider to its canonical slot", () => {
    expect(providerIndex("claude")).toBe(0);
    expect(providerIndex("zai")).toBe(5);
  });
});

describe("highestBurnWindow", () => {
  it("picks the window with the greatest used_percent", () => {
    expect(
      highestBurnWindow([win("a", 20), win("b", 80), win("c", 50)])?.label,
    ).toBe("b");
  });
  it("skips null percents and keeps the first on a tie", () => {
    expect(
      highestBurnWindow([win("a", null), win("b", 70), win("c", 70)])?.label,
    ).toBe("b");
    expect(highestBurnWindow([win("a", null)])).toBeNull();
    expect(highestBurnWindow([])).toBeNull();
  });
});

describe("setAutoAnchor", () => {
  it("sets and overrides the flag immutably", () => {
    const base = { poll_seconds: 300, providers: [] as never, auto_anchor: {} } as never;
    const a = setAutoAnchor(base, "stored:zai-1", true);
    expect(a.auto_anchor["stored:zai-1"]).toBe(true);
    const b = setAutoAnchor(a, "stored:zai-1", false);
    expect(b.auto_anchor["stored:zai-1"]).toBe(false);
    expect(a.auto_anchor["stored:zai-1"]).toBe(true); // original untouched
  });
});

describe("resolveHeadlineWindow", () => {
  it("returns the first primary window by default", () => {
    const s = service({ windows: [win("5-hour", 92), win("7-day", 40)] });
    expect(resolveHeadlineWindow(s, null)?.label).toBe("5-hour");
  });

  it("honors a per-account pinned primary_window, even a detail-only window", () => {
    const s = service({
      windows: [win("5-hour", 92)],
      detail_windows: [win("Extra usage", 12)],
    });
    // Pinned window is now keyed by SERVICE ID, not the provider slot.
    const config = cfg(
      [pc(0), pc(1), pc(2), pc(3), pc(4), pc(5)],
      { "auto:claude": { primary_window: "Extra usage" } },
    );
    expect(resolveHeadlineWindow(s, config)?.label).toBe("Extra usage");
  });

  it("does not apply ANOTHER account's pinned window to this service", () => {
    const s = service({
      id: "stored:claude-1",
      windows: [win("5-hour", 92)],
      detail_windows: [win("Extra usage", 12)],
    });
    // auto:claude has a pin; the stored Claude account (different service id)
    // must ignore it and fall back to its first primary window.
    const config = cfg(
      [pc(0), pc(1), pc(2), pc(3), pc(4), pc(5)],
      { "auto:claude": { primary_window: "Extra usage" } },
    );
    expect(resolveHeadlineWindow(s, config)?.label).toBe("5-hour");
  });

  it("falls back to the highest-burn detail window when there is no primary", () => {
    const s = service({
      windows: [],
      detail_windows: [win("a", 10), win("b", 65)],
    });
    expect(resolveHeadlineWindow(s, null)?.label).toBe("b");
  });
});

describe("providerDisplayName (per-account; BUG-2 isolation)", () => {
  it("resolves an account's own custom_name keyed by service id", () => {
    const config = cfg(
      [pc(0), pc(1), pc(2), pc(3), pc(4), pc(5)],
      { "auto:claude": { custom_name: "Personal Claude" } },
    );
    expect(providerDisplayName(config, "auto:claude", "claude")).toBe("Personal Claude");
  });

  it("renaming account A does NOT change account B of the same provider", () => {
    // Two Claude accounts; only auto:claude is renamed. stored:claude-1 must
    // still resolve to the canonical provider label — the core of BUG-2.
    const config = cfg(
      [pc(0), pc(1), pc(2), pc(3), pc(4), pc(5)],
      { "auto:claude": { custom_name: "Personal Claude" } },
    );
    expect(providerDisplayName(config, "auto:claude", "claude")).toBe("Personal Claude");
    expect(providerDisplayName(config, "stored:claude-1", "claude")).toBe("Claude");
  });

  it("falls back to the canonical label when no override or no config", () => {
    expect(providerDisplayName(null, "auto:claude", "claude")).toBe("Claude");
    const blank = cfg([pc(0), pc(1), pc(2), pc(3), pc(4), pc(5)]);
    expect(providerDisplayName(blank, "auto:codex", "codex")).toBe("Codex");
    // A whitespace-only custom_name is ignored.
    const ws = cfg(
      [pc(0), pc(1), pc(2), pc(3), pc(4), pc(5)],
      { "auto:zai": { custom_name: "   " } },
    );
    expect(providerDisplayName(ws, "auto:zai", "zai")).toBe("z.ai Coding Plan");
  });
});
