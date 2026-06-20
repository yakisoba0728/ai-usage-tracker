import { describe, expect, it } from "vitest";

import { formatUpdatedAgo } from "@/lib/format";

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
