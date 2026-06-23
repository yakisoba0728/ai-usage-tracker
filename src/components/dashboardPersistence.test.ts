import { describe, expect, it } from "vitest";

import dashboard from "./Dashboard.tsx?raw";

describe("dashboard persistence failure handling", () => {
  it("surfaces config save failures and rolls back the failed optimistic config", () => {
    expect(dashboard).toContain("rollbackConfigAfterFailedSave");
    expect(dashboard).toContain("lastPersistedConfigRef");
    expect(dashboard).toContain('t("toast.configSaveFailed"');
    expect(dashboard).toMatch(/setConfig\(next\)[\s\S]*\.catch/);
  });

  it("treats a false removeAccount result as a failed removal", () => {
    expect(dashboard).toContain("const removed = await removeAccount(accountId);");
    expect(dashboard).toContain("if (!removed)");
    expect(dashboard).toContain('t("toast.removeFailed"');
    expect(dashboard).not.toMatch(
      /try\s*\{\s*await removeAccount\(accountId\);\s*setMoreMenuOpen/s,
    );
  });
});
