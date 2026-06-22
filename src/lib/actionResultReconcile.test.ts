import { describe, expect, it } from "vitest";

import { reconcileActionResult } from "@/lib/actionResultReconcile";

describe("reconcileActionResult", () => {
  it("finishes a pending action as success and still proceeds to the toast", () => {
    // The promise-vs-event race: if `anchor-result`/`refresh-result` beats the
    // invoke's own `.then`/`.catch`, the event path is what finishes the action
    // AND surfaces the toast. A naive `if (pending) { finish; return; }` would
    // silently drop the event-driven toast.
    expect(reconcileActionResult("pending", true)).toEqual({
      finishStatus: "success",
      proceedToToast: true,
    });
  });

  it("finishes a pending action as error and still proceeds to the toast", () => {
    expect(reconcileActionResult("pending", false)).toEqual({
      finishStatus: "error",
      proceedToToast: true,
    });
  });

  it("does not finish or toast when the action already resolved to success", () => {
    // The invoke's own `.then` already finished + toasted; the later event is a
    // duplicate and must be a no-op (the early `return` in the original).
    expect(reconcileActionResult("success", true)).toEqual({
      finishStatus: null,
      proceedToToast: false,
    });
    expect(reconcileActionResult("success", false)).toEqual({
      finishStatus: null,
      proceedToToast: false,
    });
  });

  it("does not finish or toast when the action already resolved to error", () => {
    expect(reconcileActionResult("error", true)).toEqual({
      finishStatus: null,
      proceedToToast: false,
    });
    expect(reconcileActionResult("error", false)).toEqual({
      finishStatus: null,
      proceedToToast: false,
    });
  });

  it("proceeds to the toast without finishing when no manual action is tracked (auto path)", () => {
    // current == null → the background auto-anchor/refresh fired it; there is no
    // pending manual action to finish, but the toast must still surface (and the
    // caller reads `isAuto = current == null`).
    expect(reconcileActionResult(null, true)).toEqual({
      finishStatus: null,
      proceedToToast: true,
    });
    expect(reconcileActionResult(null, false)).toEqual({
      finishStatus: null,
      proceedToToast: true,
    });
  });
});
