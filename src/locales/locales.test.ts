import { describe, expect, it } from "vitest";

import { PROVIDER_ORDER } from "@/lib/providers";
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

  it("every provider has Add Account copy in both catalogs", () => {
    for (const provider of PROVIDER_ORDER) {
      expect(en.addAccount.copy, `en missing addAccount.copy.${provider}`).toHaveProperty(
        provider,
      );
      expect(ko.addAccount.copy, `ko missing addAccount.copy.${provider}`).toHaveProperty(
        provider,
      );
    }
  });

  it("localizes programmatic input labels and persistence failure messages", () => {
    expect(en.toolbar).toHaveProperty("search");
    expect(ko.toolbar).toHaveProperty("search");
    expect(en.addAccount).toHaveProperty("sessionKeyInput");
    expect(ko.addAccount).toHaveProperty("sessionKeyInput");
    expect(en.addAccount).toHaveProperty("removeFailed");
    expect(ko.addAccount).toHaveProperty("removeFailed");
    expect(en.detail.settings).toHaveProperty("thresholdInput");
    expect(ko.detail.settings).toHaveProperty("thresholdInput");
    expect(en.toast).toHaveProperty("configSaveFailed");
    expect(ko.toast).toHaveProperty("configSaveFailed");
  });
});
