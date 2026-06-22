import { describe, expect, it } from "vitest";

import {
  transitionToAddAccount,
  transitionToSettings,
  shouldShowNoResultsOfflineCta,
  shouldProcessThresholdSnapshot,
  type DashboardModalState,
} from "@/lib/dashboardState";

const detailOpen: DashboardModalState = {
  addOpen: false,
  settingsOpen: false,
  detailOpen: true,
  moreMenuOpen: true,
};

describe("dashboard modal transitions", () => {
  it("closes detail and its menu before opening add account", () => {
    expect(transitionToAddAccount(detailOpen)).toEqual({
      addOpen: true,
      settingsOpen: false,
      detailOpen: false,
      moreMenuOpen: false,
    });
  });

  it("closes detail and its menu before opening settings", () => {
    expect(transitionToSettings(detailOpen)).toEqual({
      addOpen: false,
      settingsOpen: true,
      detailOpen: false,
      moreMenuOpen: false,
    });
  });
});

describe("no results state", () => {
  it("does not show the offline CTA for a query-filtered empty result", () => {
    expect(shouldShowNoResultsOfflineCta("codex plus")).toBe(false);
    expect(shouldShowNoResultsOfflineCta("   ")).toBe(true);
  });
});

describe("threshold snapshot processing", () => {
  it("processes distinct same-second snapshot objects", () => {
    const first = { fetched_at: 100, services: [] };
    const second = { fetched_at: 100, services: [] };

    expect(shouldProcessThresholdSnapshot(null, first)).toBe(true);
    expect(shouldProcessThresholdSnapshot(first, first)).toBe(false);
    expect(shouldProcessThresholdSnapshot(first, second)).toBe(true);
  });
});
