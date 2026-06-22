import { describe, expect, it } from "vitest";

import {
  finishRefreshActivity,
  shouldAcceptSnapshot,
  startRefreshActivity,
} from "@/lib/snapshotState";
import type { UsageSnapshot } from "@/lib/types";

function snapshot(fetchedAt: number, id = "auto:claude"): UsageSnapshot {
  return {
    fetched_at: fetchedAt,
    services: [
      {
        id,
        source: "auto",
        provider: "claude",
        connected: true,
        plan: null,
        account: null,
        error: null,
        windows: [],
        detail_windows: [],
      },
    ],
  };
}

describe("snapshot freshness", () => {
  it("rejects an older async response after a newer usage event was applied", () => {
    expect(
      shouldAcceptSnapshot({
        current: snapshot(100, "event"),
        incoming: snapshot(100, "stale-get-usage"),
        incomingOrder: 1,
        latestAcceptedOrder: 2,
      }),
    ).toBe(false);
  });

  it("accepts a newer equal-second snapshot so same-second updates are not lost", () => {
    expect(
      shouldAcceptSnapshot({
        current: snapshot(100, "previous"),
        incoming: snapshot(100, "refresh"),
        incomingOrder: 3,
        latestAcceptedOrder: 2,
      }),
    ).toBe(true);
  });

  it("rejects snapshots with an older fetched_at even when the callback is newer", () => {
    expect(
      shouldAcceptSnapshot({
        current: snapshot(100),
        incoming: snapshot(99),
        incomingOrder: 3,
        latestAcceptedOrder: 2,
      }),
    ).toBe(false);
  });
});

describe("refresh activity", () => {
  it("stays refreshing until every concurrent refresh has finished", () => {
    let activity = startRefreshActivity({ pending: 0, refreshing: false });
    activity = startRefreshActivity(activity);

    expect(activity).toEqual({ pending: 2, refreshing: true });

    activity = finishRefreshActivity(activity);
    expect(activity).toEqual({ pending: 1, refreshing: true });

    activity = finishRefreshActivity(activity);
    expect(activity).toEqual({ pending: 0, refreshing: false });
  });
});
