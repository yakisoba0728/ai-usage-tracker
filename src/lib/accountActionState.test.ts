import { describe, expect, it } from "vitest";

import {
  clearAccountAction,
  finishAccountAction,
  getAccountAction,
  isAccountActionPending,
  startAccountAction,
  type AccountActionState,
} from "@/lib/accountActionState";

const empty: AccountActionState = {};

describe("account action state", () => {
  it("starts a pending action for a service id and kind", () => {
    const result = startAccountAction(empty, "stored:claude-work", "refresh");

    expect(result.started).toBe(true);
    expect(getAccountAction(result.state, "stored:claude-work", "refresh")).toBe("pending");
  });

  it("rejects a duplicate pending start for the same service id and kind", () => {
    const first = startAccountAction(empty, "stored:claude-work", "refresh");
    const second = startAccountAction(first.state, "stored:claude-work", "refresh");

    expect(second.started).toBe(false);
    expect(second.state).toBe(first.state);
  });

  it("reports pending only for the matching service id and kind", () => {
    const result = startAccountAction(empty, "stored:claude-work", "anchor");

    expect(isAccountActionPending(result.state, "stored:claude-work", "anchor")).toBe(true);
    expect(isAccountActionPending(result.state, "stored:claude-work", "refresh")).toBe(false);
    expect(isAccountActionPending(result.state, "stored:cursor-work", "anchor")).toBe(false);
  });

  it("finishes a pending action with completion feedback", () => {
    const pending = startAccountAction(empty, "stored:claude-work", "refresh").state;
    const finished = finishAccountAction(pending, "stored:claude-work", "refresh", "success");

    expect(getAccountAction(finished, "stored:claude-work", "refresh")).toBe("success");
    expect(isAccountActionPending(finished, "stored:claude-work", "refresh")).toBe(false);
    expect(getAccountAction(pending, "stored:claude-work", "refresh")).toBe("pending");
  });

  it("clears one action without removing another kind for the same service", () => {
    const refreshPending = startAccountAction(empty, "stored:claude-work", "refresh").state;
    const bothPending = startAccountAction(refreshPending, "stored:claude-work", "anchor").state;
    const refreshFinished = finishAccountAction(bothPending, "stored:claude-work", "refresh", "error");
    const cleared = clearAccountAction(refreshFinished, "stored:claude-work", "refresh");

    expect(getAccountAction(cleared, "stored:claude-work", "refresh")).toBeNull();
    expect(getAccountAction(cleared, "stored:claude-work", "anchor")).toBe("pending");
    expect(getAccountAction(refreshFinished, "stored:claude-work", "refresh")).toBe("error");
  });
});
