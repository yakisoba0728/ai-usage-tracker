import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";

import {
  formatPercent,
  formatResetShort,
  formatServiceError,
  formatUpdatedAgo,
  formatUsedLimit,
  percentSeverity,
  remainingPercent,
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

describe("remainingPercent", () => {
  it("inverts used → remaining, clamped to [0,100]; null passes through", () => {
    expect(remainingPercent(null)).toBeNull();
    expect(remainingPercent(0)).toBe(100);
    expect(remainingPercent(8)).toBe(92);
    expect(remainingPercent(100)).toBe(0);
    expect(remainingPercent(120)).toBe(0); // over-quota clamps to 0
    expect(remainingPercent(-5)).toBe(100); // clamp top
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
  it("shows 'soon' for a sub-30s reset (rounds to 0m) instead of '0m' (F-4)", () => {
    expect(formatResetShort(now / 1000 + 15, now, t)).toBe("time.reset.soon");
    expect(formatResetShort(now / 1000 + 29, now, t)).toBe("time.reset.soon");
    // ≥30s rounds up to 1m and is shown as "1m" (not "soon") — the boundary.
    expect(formatResetShort(now / 1000 + 45, now, t)).toBe(
      'time.reset.minutes:{"m":1}',
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
  it("treats a 0 / negative fetched_at sentinel as awaiting, not '489000h ago' (F-1)", () => {
    expect(formatUpdatedAgo(0, nowMs, t)).toBe("time.awaiting");
    expect(formatUpdatedAgo(-5, nowMs, t)).toBe("time.awaiting");
  });
  it("uses the no-prefix keys when prefix is false", () => {
    expect(formatUpdatedAgo(fetchedAt, nowMs + 30_000, t, { prefix: false })).toBe(
      'time.agoSeconds:{"count":30}',
    );
  });
  it("has a day tier past 24h instead of unbounded hours (F-8)", () => {
    const ms = nowMs + 50 * 60 * 60 * 1000; // 50h later → 2d 2h
    expect(formatUpdatedAgo(fetchedAt, ms, t)).toBe(
      'time.updatedDays:{"d":2,"h":2}',
    );
    expect(formatUpdatedAgo(fetchedAt, ms, t, { prefix: false })).toBe(
      'time.agoDays:{"d":2,"h":2}',
    );
    // Just under a day still uses the hours tier.
    expect(formatUpdatedAgo(fetchedAt, nowMs + 23 * 60 * 60 * 1000, t)).toBe(
      'time.updatedHours:{"h":23,"m":0}',
    );
  });
});

describe("formatServiceError", () => {
  it("maps the code to error.<code> with the detail as fallback", () => {
    expect(formatServiceError({ code: "network", detail: "timeout" }, t)).toBe(
      'error.network:{"defaultValue":"timeout"}',
    );
  });
  it("falls back to error.unknown when no detail is present", () => {
    expect(formatServiceError({ code: "server_error" }, t)).toBe(
      'error.server_error:{"defaultValue":"error.unknown"}',
    );
  });
});
