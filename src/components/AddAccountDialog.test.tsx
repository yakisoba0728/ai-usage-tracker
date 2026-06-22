import { describe, expect, it } from "vitest";

import { SESSION_INPUT_SECURITY_PROPS } from "@/lib/addAccountSecurity";

describe("AddAccountDialog session credential input", () => {
  it("uses password-style browser controls for pasted secrets", () => {
    expect(SESSION_INPUT_SECURITY_PROPS).toMatchObject({
      type: "password",
      autoComplete: "off",
      spellCheck: false,
    });
  });
});
