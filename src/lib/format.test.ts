import { describe, expect, it } from "vitest";

import {
  formatPercent,
  formatResetCountdown,
  formatUpdatedAgo,
  formatUsedLimit,
  percentSeverity,
} from "@/lib/format";
import type { LimitWindow } from "@/lib/types";

function win(partial: Partial<LimitWindow>): LimitWindow {
  return {
    label: "5-hour",
    used_percent: null,
    resets_at: null,
    used: null,
    limit: null,
    ...partial,
  };
}

describe("percentSeverity", () => {
  it("bands on >85 crit, >=60 warn, else ok; null passes through", () => {
    expect(percentSeverity(null)).toBeNull();
    expect(percentSeverity(0)).toBe("ok");
    expect(percentSeverity(59.9)).toBe("ok");
    expect(percentSeverity(60)).toBe("warn");
    expect(percentSeverity(85)).toBe("warn"); // 85 is not > 85
    expect(percentSeverity(85.1)).toBe("crit");
    expect(percentSeverity(100)).toBe("crit");
  });
});

describe("formatPercent", () => {
  it("rounds to a whole percent, dashes on null", () => {
    expect(formatPercent(null)).toBe("—");
    expect(formatPercent(0)).toBe("0%");
    expect(formatPercent(72.4)).toBe("72%");
    expect(formatPercent(72.5)).toBe("73%");
  });
});

describe("formatUsedLimit", () => {
  it("returns null when both used and limit are absent", () => {
    expect(formatUsedLimit(win({ used: null, limit: null }))).toBeNull();
  });

  it("renders a plain count for non-monetary windows", () => {
    expect(formatUsedLimit(win({ label: "5-hour", used: 184, limit: 200 }))).toBe(
      "184 / 200",
    );
  });

  it("renders dollars when the label looks monetary", () => {
    expect(
      formatUsedLimit(win({ label: "Extra usage", used: 12.5, limit: 100 })),
    ).toBe("$12.50 / $100.00");
    expect(
      formatUsedLimit(win({ label: "Plan usage", used: 3, limit: null })),
    ).toBe("$3.00 / —");
  });
});

describe("formatResetCountdown", () => {
  const now = 1_000_000_000; // ms
  it("returns null without a reset epoch", () => {
    expect(formatResetCountdown(null, now)).toBeNull();
  });
  it("says 'resets soon' once the window has elapsed", () => {
    expect(formatResetCountdown(now / 1000 - 1, now)).toBe("resets soon");
  });
  it("shows minutes under an hour", () => {
    expect(formatResetCountdown(now / 1000 + 30 * 60, now)).toBe("resets in 30m");
  });
  it("shows hours and minutes under two days", () => {
    expect(formatResetCountdown(now / 1000 + (3 * 60 + 19) * 60, now)).toBe(
      "resets in 3h 19m",
    );
  });
  it("shows days and hours beyond two days", () => {
    expect(formatResetCountdown(now / 1000 + (50 * 60 + 0) * 60, now)).toBe(
      "resets in 2d 2h",
    );
  });
});

describe("formatUpdatedAgo", () => {
  it("says 'just now' under 5s and counts seconds after", () => {
    const fetchedAt = 1_000_000; // epoch seconds
    const nowMs = fetchedAt * 1000;
    expect(formatUpdatedAgo(fetchedAt, nowMs)).toBe("Updated just now");
    expect(formatUpdatedAgo(fetchedAt, nowMs + 30_000)).toBe("Updated 30s ago");
  });

  it("returns the awaiting message when no timestamp", () => {
    expect(formatUpdatedAgo(null, 0)).toBe("Awaiting first update…");
  });
});
