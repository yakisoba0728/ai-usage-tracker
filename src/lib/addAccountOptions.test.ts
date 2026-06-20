import { describe, expect, it } from "vitest";

import { authOptionsForProvider } from "@/lib/addAccountOptions";

describe("add account options", () => {
  it("shows only supported add methods for each provider", () => {
    expect(authOptionsForProvider("claude")).toEqual([
      "session-key",
      "local-session",
    ]);
    expect(authOptionsForProvider("codex")).toEqual([
      "browser-oauth",
      "local-session",
    ]);
    expect(authOptionsForProvider("gemini")).toEqual([
      "browser-oauth",
      "local-session",
    ]);
    expect(authOptionsForProvider("copilot")).toEqual([
      "session-key",
      "device-code",
      "local-session",
    ]);
    expect(authOptionsForProvider("zai")).toEqual([
      "session-key",
      "local-session",
    ]);
    expect(authOptionsForProvider("cursor")).toEqual(["local-session"]);
  });
});
