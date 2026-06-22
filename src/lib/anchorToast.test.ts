import { describe, expect, it } from "vitest";

import { anchorSubject, buildAnchorToast } from "@/lib/anchorToast";

describe("anchorSubject", () => {
  it("names provider and account together", () => {
    expect(anchorSubject("claude", "person@example.invalid")).toBe(
      "Claude (person@example.invalid)",
    );
  });

  it("falls back to provider only when no label", () => {
    // Uses the FRONTEND PROVIDER_LABEL (more descriptive than the Rust label —
    // the in-app toast and the OS notification are separate surfaces).
    expect(anchorSubject("zai", null)).toBe("z.ai Coding Plan");
    expect(anchorSubject("zai", "   ")).toBe("z.ai Coding Plan");
  });

  it("uses the account alone when the provider is unknown", () => {
    expect(anchorSubject(null, "some workspace")).toBe("some workspace");
  });

  it("never invents text when both are missing", () => {
    expect(anchorSubject(null, null)).toBe("");
  });
});

describe("buildAnchorToast", () => {
  it("manual success names the account", () => {
    const t = buildAnchorToast("claude", "person@example.invalid", true, false);
    expect(t.key).toBe("toast.anchorSentFor");
    expect(t.params.name).toBe("Claude (person@example.invalid)");
  });

  it("auto success reads as automatic (distinct key)", () => {
    const t = buildAnchorToast("zai", "z.ai workspace", true, true);
    expect(t.key).toBe("toast.anchorAutoSentFor");
    expect(t.params.name).toBe("z.ai Coding Plan (z.ai workspace)");
  });

  it("manual failure and auto failure use different keys", () => {
    const manual = buildAnchorToast("codex", "acct", false, false, "boom");
    const auto = buildAnchorToast("codex", "acct", false, true, "boom");
    expect(manual.key).toBe("toast.anchorFailedFor");
    expect(auto.key).toBe("toast.anchorAutoFailedFor");
    expect(manual.key).not.toBe(auto.key);
    expect(manual.params.error).toBe("boom");
    expect(auto.params.error).toBe("boom");
  });

  it("failure with no detail passes an empty error string", () => {
    const t = buildAnchorToast("claude", null, false, false);
    expect(t.key).toBe("toast.anchorFailedFor");
    expect(t.params.error).toBe("");
  });
});
