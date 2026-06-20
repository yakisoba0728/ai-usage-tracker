import { describe, expect, it } from "vitest";

import { serviceStatus, severityToStatus, summarizeServices } from "@/lib/status";
import type { LimitWindow, Provider, ServiceUsage } from "@/lib/types";

function win(label: string, used_percent: number | null): LimitWindow {
  return { label, used_percent, resets_at: null, used: null, limit: null };
}

function service(partial: Partial<ServiceUsage> & { id?: string }): ServiceUsage {
  return {
    id: partial.id ?? "auto:claude",
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

describe("severityToStatus", () => {
  it("maps each severity (and null) to a status", () => {
    expect(severityToStatus("crit")).toBe("critical");
    expect(severityToStatus("warn")).toBe("warning");
    expect(severityToStatus("ok")).toBe("ok");
    expect(severityToStatus(null)).toBe("unknown");
  });
});

describe("serviceStatus", () => {
  it("is offline when the service is disconnected", () => {
    expect(serviceStatus(service({ connected: false }), null)).toBe("offline");
  });

  it("derives from the headline window when connected", () => {
    expect(
      serviceStatus(service({ windows: [win("5-hour", 92)] }), null),
    ).toBe("critical");
    expect(
      serviceStatus(service({ windows: [win("5-hour", 70)] }), null),
    ).toBe("warning");
    expect(
      serviceStatus(service({ windows: [win("5-hour", 10)] }), null),
    ).toBe("ok");
  });

  it("is unknown when connected but has no usable percent", () => {
    expect(serviceStatus(service({ windows: [] }), null)).toBe("unknown");
  });
});

describe("summarizeServices", () => {
  it("counts connection + severity buckets and tracks the burn leader", () => {
    const provider = (p: Provider, pct: number | null, connected = true) =>
      service({
        id: `auto:${p}`,
        provider: p,
        connected,
        windows: pct == null ? [] : [win("5-hour", pct)],
      });
    const services = [
      provider("claude", 92), // critical
      provider("codex", 70), // warning
      provider("gemini", null, false), // offline
    ];

    const s = summarizeServices(services, null);
    expect(s.total).toBe(3);
    expect(s.connected).toBe(2);
    expect(s.offline).toBe(1);
    expect(s.critical).toBe(1);
    expect(s.warning).toBe(1);
    expect(s.maxPercent).toBe(92);
    expect(s.maxProvider).toBe("claude");
    expect(s.averagePercent).toBe(81); // (92 + 70) / 2
  });
});
