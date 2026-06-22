import { describe, expect, it } from "vitest";

import { scrubErrorText } from "@/lib/errorScrub";

describe("scrubErrorText", () => {
  it("redacts tokens, session cookies, and email addresses while preserving context", () => {
    const input =
      "unexpected response (401): token sk-ant-secret for person@example.invalid; sessionKey=sk-ant-session; access_token=gho_secret";

    const scrubbed = scrubErrorText(input);

    expect(scrubbed).toContain("unexpected response (401)");
    expect(scrubbed).toContain("[redacted]");
    expect(scrubbed).not.toContain("person@example.invalid");
    expect(scrubbed).not.toContain("sk-ant-secret");
    expect(scrubbed).not.toContain("sk-ant-session");
    expect(scrubbed).not.toContain("gho_secret");
  });
});
