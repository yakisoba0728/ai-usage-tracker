import { describe, expect, it } from "vitest";

import { authOptionsForProvider } from "@/lib/addAccountOptions";

describe("add account options", () => {
  it("shows only supported add methods for each provider", () => {
    expect(authOptionsForProvider("claude").map((option) => option.id)).toEqual([
      "session-key",
      "local-session",
    ]);
    expect(authOptionsForProvider("codex").map((option) => option.id)).toEqual([
      "browser-oauth",
      "local-session",
    ]);
    expect(authOptionsForProvider("gemini").map((option) => option.id)).toEqual([
      "browser-oauth",
      "local-session",
    ]);
    expect(authOptionsForProvider("copilot").map((option) => option.id)).toEqual([
      "session-key",
      "device-code",
      "local-session",
    ]);
    expect(authOptionsForProvider("zai").map((option) => option.id)).toEqual([
      "session-key",
      "local-session",
    ]);
    expect(authOptionsForProvider("cursor").map((option) => option.id)).toEqual([
      "local-session",
    ]);
  });
});
