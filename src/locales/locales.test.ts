import { describe, expect, it } from "vitest";

import { SERVICE_ERROR_CODES } from "@/lib/types";
import en from "@/locales/en.json";
import ko from "@/locales/ko.json";

/** Recursively collect dotted leaf keys from a nested catalog object. */
function leafKeys(obj: Record<string, unknown>, prefix = ""): string[] {
  return Object.entries(obj).flatMap(([k, v]) => {
    const key = prefix ? `${prefix}.${k}` : k;
    return v && typeof v === "object" && !Array.isArray(v)
      ? leafKeys(v as Record<string, unknown>, key)
      : [key];
  });
}

describe("locale catalogs", () => {
  it("en and ko expose an identical key set (no missing translations)", () => {
    const enKeys = leafKeys(en as Record<string, unknown>).sort();
    const koKeys = leafKeys(ko as Record<string, unknown>).sort();
    expect(koKeys).toEqual(enKeys);
  });

  it("every backend error code has a localized message in both catalogs", () => {
    for (const code of SERVICE_ERROR_CODES) {
      expect(en.error, `en missing error.${code}`).toHaveProperty(code);
      expect(ko.error, `ko missing error.${code}`).toHaveProperty(code);
    }
  });
});
