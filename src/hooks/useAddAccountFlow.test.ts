import { describe, expect, it } from "vitest";

import useAddAccountFlowSource from "./useAddAccountFlow.ts?raw";

describe("useAddAccountFlow stored-account removal", () => {
  it("surfaces persisted account list failures instead of hiding them as empty state", () => {
    expect(useAddAccountFlowSource).toContain("} catch (e) {");
    expect(useAddAccountFlowSource).toContain("setError(scrubErrorText(String(e)))");
    expect(useAddAccountFlowSource).not.toContain(
      "/* ignore - no backend in dev */",
    );
  });

  it("treats a false removeAccount result as a failed removal", () => {
    expect(useAddAccountFlowSource).toContain(
      "const removed = await removeAccount(id);",
    );
    expect(useAddAccountFlowSource).toContain("if (!removed)");
    expect(useAddAccountFlowSource).toContain('t("addAccount.removeFailed"');
    expect(useAddAccountFlowSource).not.toMatch(
      /try\s*\{\s*await removeAccount\(id\);\s*\} catch/s,
    );
  });
});
