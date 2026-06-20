import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";

import {
  formatPercent,
  formatResetShort,
  formatUpdatedAgo,
  formatUsedLimit,
  percentSeverity,
} from "@/lib/format";
import type { LimitWindow } from "@/lib/types";

// Mock t: returns the key, or "key:{json}" when given interpolation values, so
// tests can assert which key + values the formatters chose without i18n setup.
const t = ((key: string, opts?: Record<string, unknown>) =>
  opts ? `${key}:${JSON.stringify(opts)}` : key) as unknown as TFunction;

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
    expect(percentSeverity(85)).toBe("warn");
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
  });
});

describe("formatResetShort", () => {
  const now = 1_000_000_000;
  it("returns null without a reset epoch", () => {
    expect(formatResetShort(null, now, t)).toBeNull();
  });
  it("picks the right key + values per tier", () => {
    expect(formatResetShort(now / 1000 - 1, now, t)).toBe("time.reset.soon");
    expect(formatResetShort(now / 1000 + 30 * 60, now, t)).toBe(
      'time.reset.minutes:{"m":30}',
    );
    expect(formatResetShort(now / 1000 + (3 * 60 + 19) * 60, now, t)).toBe(
      'time.reset.hoursMinutes:{"h":3,"m":19}',
    );
    expect(formatResetShort(now / 1000 + 50 * 60 * 60, now, t)).toBe(
      'time.reset.daysHours:{"d":2,"h":2}',
    );
  });
});

describe("formatUpdatedAgo", () => {
  const fetchedAt = 1_000_000;
  const nowMs = fetchedAt * 1000;
  it("uses the 'Updated' prefix keys by default", () => {
    expect(formatUpdatedAgo(fetchedAt, nowMs, t)).toBe("time.justNow");
    expect(formatUpdatedAgo(fetchedAt, nowMs + 30_000, t)).toBe(
      'time.updatedSeconds:{"count":30}',
    );
    expect(formatUpdatedAgo(null, 0, t)).toBe("time.awaiting");
  });
  it("uses the no-prefix keys when prefix is false", () => {
    expect(formatUpdatedAgo(fetchedAt, nowMs + 30_000, t, { prefix: false })).toBe(
      'time.agoSeconds:{"count":30}',
    );
  });
});
