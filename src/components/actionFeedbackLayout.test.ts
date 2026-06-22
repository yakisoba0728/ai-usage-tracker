import { describe, expect, it } from "vitest";

import dashboard from "./Dashboard.tsx?raw";
import accountCard from "./dashboard/AccountCard.tsx?raw";
import accountDetail from "./dashboard/AccountDetailDialog.tsx?raw";
import inspectorSettings from "./dashboard/detail/InspectorSettings.tsx?raw";
import sessionsTab from "./dashboard/detail/SessionsTab.tsx?raw";

describe("action feedback layout wiring", () => {
  it("scopes whole-dashboard refresh feedback to the account list region", () => {
    expect(dashboard).toContain("ActionFeedbackOverlay");
    expect(dashboard).toContain('t("status.refreshingUsage")');
    expect(dashboard).toContain("aria-busy={refreshing}");
  });

  it("passes account action state into account cards and the detail settings pane", () => {
    expect(dashboard).toContain("accountActions");
    expect(dashboard).toContain("onRefreshAccount={handleRefreshAccount}");
    expect(dashboard).toContain("onSendAnchor={handleSendAnchor}");
    expect(accountDetail).toContain("anchorAction=");
    expect(accountDetail).toContain("onSendAnchor={onSendAnchor}");
  });

  it("renders per-card action feedback without calling IPC directly from the card", () => {
    expect(accountCard).toContain("action-feedback-overlay");
    expect(accountCard).toContain("aria-busy={actionBusy}");
    expect(accountCard).toContain("disabled={refreshDisabled}");
    expect(accountCard).not.toContain("sendAnchorNow(row.id)");
  });

  it("uses refresh-result events rather than invoke resolution for refresh success", () => {
    expect(dashboard).toContain("refresh-result is the source of truth");
    expect(dashboard).not.toContain('finishVisibleAccountAction(serviceId, "refresh", "success")');
  });

  it("routes manual anchor confirmation through the dashboard handler", () => {
    expect(inspectorSettings).toContain("onSendAnchor(service.id)");
    expect(inspectorSettings).not.toContain("sendAnchorNow(service.id)");
  });

  it("disables detail refresh controls while the selected account is refreshing", () => {
    expect(accountDetail).toContain("disabled={refreshing}");
    expect(accountDetail).toContain("refreshAction=");
    expect(sessionsTab).toContain("disabled={refreshing}");
  });
});
