import { describe, expect, it } from "vitest";

import useSnapshotSource from "./useSnapshot.ts?raw";

describe("useSnapshot subscription cleanup", () => {
  it("ignores usage-updated events after the hook has cleaned up", () => {
    expect(useSnapshotSource).toMatch(
      /onUsageUpdated\(\(s\) => \{\s+if \(cancelled\) return;/s,
    );
  });
});
