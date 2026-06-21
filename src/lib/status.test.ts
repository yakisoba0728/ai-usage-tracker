import { describe, expect, it } from "vitest";

import { serviceStatus, severityToStatus } from "@/lib/status";
import type { LimitWindow, ServiceUsage } from "@/lib/types";

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
