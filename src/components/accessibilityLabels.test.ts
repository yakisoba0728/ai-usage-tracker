import { describe, expect, it } from "vitest";

import sessionKeyPanel from "./addaccount/SessionKeyPanel.tsx?raw";
import accountToolbar from "./dashboard/AccountToolbar.tsx?raw";
import inspectorSettings from "./dashboard/detail/InspectorSettings.tsx?raw";

describe("programmatic input labels", () => {
  it("labels the account search input for assistive tech", () => {
    expect(accountToolbar).toContain('aria-label={t("toolbar.search")}');
  });

  it("labels the pasted session key input for assistive tech", () => {
    expect(sessionKeyPanel).toContain('aria-label={t("addAccount.sessionKeyInput"');
  });

  it("labels the custom threshold input for assistive tech", () => {
    expect(inspectorSettings).toContain(
      'aria-label={t("detail.settings.thresholdInput")}',
    );
  });
});
