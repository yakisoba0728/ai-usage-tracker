import { describe, expect, it } from "vitest";

import {
  ADD_ACCOUNT_PROVIDERS,
  authOptionsForProvider,
} from "@/lib/addAccountOptions";
import { PROVIDER_ORDER } from "@/lib/providers";

describe("add account options", () => {
  it("lists every provider with at least one add-account option", () => {
    expect(ADD_ACCOUNT_PROVIDERS).toEqual(PROVIDER_ORDER);
    for (const provider of ADD_ACCOUNT_PROVIDERS) {
      expect(authOptionsForProvider(provider).length).toBeGreaterThan(0);
    }
  });

  it("shows only supported add methods for each provider", () => {
    // Claude is session-key only (FEAT-2/BUG-1): no CLI/OAuth "local-session".
    expect(authOptionsForProvider("claude")).toEqual(["session-key"]);
    expect(authOptionsForProvider("codex")).toEqual([
      "browser-oauth",
      "local-session",
    ]);
    expect(authOptionsForProvider("gemini")).toEqual(["browser-oauth"]);
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
