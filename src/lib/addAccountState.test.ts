import { describe, expect, it } from "vitest";

import {
  beginLoginRequest,
  cancelPendingLoginState,
  completeLogin,
  isCurrentLoginRequest,
  receiveLoginInfo,
  selectProviderState,
  type AddAccountState,
} from "@/lib/addAccountState";

const idle: AddAccountState = {
  selectedProvider: null,
  info: null,
  busy: null,
  pendingLoginProvider: null,
  sessionFor: null,
  sessionInput: "",
  error: null,
};

describe("add account login state", () => {
  it("keeps a provider busy while a device-code login waits for completion", () => {
    const pending = beginLoginRequest(idle, "copilot").state;
    const waiting = receiveLoginInfo(pending, {
      provider: "copilot",
      verification_url: "https://github.com/login/device",
      user_code: "ABCD-1234",
      expires_in: 900,
    });

    expect(waiting.busy).toBe("copilot");
    expect(waiting.pendingLoginProvider).toBe("copilot");
    expect(waiting.info?.user_code).toBe("ABCD-1234");
  });

  it("ignores login-complete events from a provider that is no longer pending", () => {
    const pending = beginLoginRequest(idle, "copilot").state;
    const result = completeLogin(pending, {
      provider: "gemini",
      ok: true,
      label: "Personal",
      error: null,
    });

    expect(result.accepted).toBe(false);
    expect(result.state.busy).toBe("copilot");
    expect(result.state.pendingLoginProvider).toBe("copilot");
  });

  it("clears pending login state only for the matching provider", () => {
    const pending = beginLoginRequest(idle, "copilot").state;
    const result = completeLogin(pending, {
      provider: "copilot",
      ok: true,
      label: "Personal",
      error: null,
    });

    expect(result.accepted).toBe(true);
    expect(result.closeDialog).toBe(true);
    expect(result.state.busy).toBeNull();
    expect(result.state.pendingLoginProvider).toBeNull();
  });

  it("cancels a pending device-code login when switching providers", () => {
    const pending = beginLoginRequest(idle, "copilot").state;
    const result = selectProviderState(pending, "cursor");

    expect(result.cancelPendingLogin).toBe(true);
    expect(result.state.selectedProvider).toBe("cursor");
    expect(result.state.busy).toBeNull();
    expect(result.state.pendingLoginProvider).toBeNull();
  });

  it("treats a login response as current only when its request id is still the live one", () => {
    // A response is honored while its captured request id matches the live
    // counter, and ignored once a newer request (provider switch / cancel /
    // dialog close) has bumped the counter past it.
    expect(isCurrentLoginRequest(3, 3)).toBe(true);
    expect(isCurrentLoginRequest(2, 3)).toBe(false);
    expect(isCurrentLoginRequest(4, 3)).toBe(false);
  });

  it("clears pending device-code state when cancelled", () => {
    const pending = receiveLoginInfo(beginLoginRequest(idle, "copilot").state, {
      provider: "copilot",
      verification_url: "https://github.com/login/device",
      user_code: "ABCD-1234",
      expires_in: 900,
    });

    expect(cancelPendingLoginState(pending)).toMatchObject({
      busy: null,
      pendingLoginProvider: null,
      info: null,
    });
  });
});
