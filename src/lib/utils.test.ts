import { describe, expect, it } from "vitest";

import { clamp, cn } from "@/lib/utils";

describe("clamp", () => {
  it("returns the value inside the range", () => {
    expect(clamp(50, 0, 100)).toBe(50);
  });
  it("clamps below the minimum and above the maximum", () => {
    expect(clamp(-5, 0, 100)).toBe(0);
    expect(clamp(150, 0, 100)).toBe(100);
  });
  it("keeps the bounds themselves", () => {
    expect(clamp(0, 0, 100)).toBe(0);
    expect(clamp(100, 0, 100)).toBe(100);
  });
});

describe("cn", () => {
  it("merges class names and lets later Tailwind classes win", () => {
    expect(cn("px-2", "px-4")).toBe("px-4");
    // A falsy conditional class is dropped (the `cond && "class"` idiom).
    const hidden: string | false = false;
    expect(cn("text-sm", hidden, "font-bold")).toBe("text-sm font-bold");
  });
});
