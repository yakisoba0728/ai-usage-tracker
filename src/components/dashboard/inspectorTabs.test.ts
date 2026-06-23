import { describe, expect, it } from "vitest";

import { inspectorTabsForService } from "@/components/dashboard/inspectorTabs";

describe("inspectorTabsForService", () => {
  it("hides the raw tab when no raw response is available", () => {
    expect(inspectorTabsForService({})).toEqual([
      "limits",
      "sessions",
      "settings",
    ]);
    expect(inspectorTabsForService({ raw_response: "" })).toEqual([
      "limits",
      "sessions",
      "settings",
    ]);
  });

  it("shows the raw tab when a raw response is available", () => {
    expect(inspectorTabsForService({ raw_response: "{}" })).toEqual([
      "limits",
      "sessions",
      "raw",
      "settings",
    ]);
  });
});
