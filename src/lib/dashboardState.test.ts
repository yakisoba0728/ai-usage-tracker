import { describe, expect, it } from "vitest";

import {
  transitionToAddAccount,
  transitionToSettings,
  shouldShowNoResultsOfflineCta,
  shouldProcessThresholdSnapshot,
  type DashboardModalState,
} from "@/lib/dashboardState";
import * as dashboardState from "@/lib/dashboardState";
import type { AppConfig } from "@/lib/types";

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

function configWithPollSeconds(pollSeconds: number): AppConfig {
  return {
    schema_version: 1,
    poll_seconds: pollSeconds,
    providers: [
      { enabled: true, notify_thresholds: [50], sort_index: 0 },
      { enabled: true, notify_thresholds: [50], sort_index: 1 },
      { enabled: true, notify_thresholds: [50], sort_index: 2 },
      { enabled: true, notify_thresholds: [50], sort_index: 3 },
      { enabled: true, notify_thresholds: [50], sort_index: 4 },
      { enabled: true, notify_thresholds: [50], sort_index: 5 },
    ],
    accounts: {},
    auto_anchor: {},
    launch_at_login: false,
    auto_update_check: true,
  };
}

describe("config persistence failure rollback", () => {
  const rollbackConfigAfterFailedSave = (
    dashboardState as {
      rollbackConfigAfterFailedSave?: (
        current: AppConfig | null,
        attempted: AppConfig,
        lastPersisted: AppConfig | null,
      ) => AppConfig | null;
    }
  ).rollbackConfigAfterFailedSave;

  it("reverts the failed optimistic config when it is still shown", () => {
    const previous = configWithPollSeconds(60);
    const attempted = configWithPollSeconds(300);

    expect(rollbackConfigAfterFailedSave?.(attempted, attempted, previous)).toBe(
      previous,
    );
  });

  it("does not clobber a newer config after an older save fails", () => {
    const previous = configWithPollSeconds(60);
    const attempted = configWithPollSeconds(300);
    const newer = configWithPollSeconds(900);

    expect(rollbackConfigAfterFailedSave?.(newer, attempted, previous)).toBe(
      newer,
    );
  });

  it("rolls back overlapping failed saves to the last successfully persisted config", () => {
    const persisted = configWithPollSeconds(60);
    const firstAttempt = configWithPollSeconds(300);
    const secondAttempt = configWithPollSeconds(900);

    let current = rollbackConfigAfterFailedSave?.(
      secondAttempt,
      firstAttempt,
      persisted,
    );
    expect(current).toBe(secondAttempt);

    current = rollbackConfigAfterFailedSave?.(
      current ?? null,
      secondAttempt,
      persisted,
    );
    expect(current).toBe(persisted);
  });
});
