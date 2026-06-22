import { describe, expect, it } from "vitest";

import { toggleThreshold } from "@/lib/thresholds";

describe("toggleThreshold", () => {
  it("adds a value to an empty list", () => {
    expect(toggleThreshold([], 75)).toEqual([75]);
  });

  it("adds a value and keeps the list sorted ascending (numeric, not lexical)", () => {
    // Lexical sort would order [100, 50, 75] as [100, 50, 75]; numeric must
    // produce [50, 75, 100]. The 100 + two-digit values are the lexical trap.
    expect(toggleThreshold([100, 50], 75)).toEqual([50, 75, 100]);
  });

  it("removes a value that is already present", () => {
    expect(toggleThreshold([50, 75, 100], 75)).toEqual([50, 100]);
  });

  it("dedupes so a value never appears twice", () => {
    // A list that already (defensively) holds a duplicate is collapsed when the
    // toggled value is added back.
    expect(toggleThreshold([50, 50, 90], 100)).toEqual([50, 90, 100]);
  });

  it("does not mutate the input array", () => {
    const input = [50, 90];
    const out = toggleThreshold(input, 75);
    expect(input).toEqual([50, 90]);
    expect(out).toEqual([50, 75, 90]);
  });
});
