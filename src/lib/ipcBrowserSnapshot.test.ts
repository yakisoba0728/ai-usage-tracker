import { describe, expect, it } from "vitest";

import { getUsage } from "@/lib/ipc";

describe("browser fallback usage snapshot", () => {
  it("does not expose raw provider responses in demo data", async () => {
    const snapshot = await getUsage();

    for (const service of snapshot.services) {
      expect(service.raw_response).toBeUndefined();
    }
  });
});
