import { describe, expect, it } from "vitest";

import type { AppConfig, LimitWindow, ServiceUsage } from "@/lib/types";

/**
 * Contract guard — the frontend half. Asserts the exact serialized field set the
 * Rust backend produces, mirrored by `service_usage_json_shape_matches_ts_contract`
 * in src-tauri/src/model.rs, so the hand-synced IPC contract can't drift silently:
 * - add/rename a field in Rust   → the model.rs key-set assertion fails
 * - add a required field in types.ts → the typed literal below won't compile (tsc)
 */
describe("IPC contract shape", () => {
  it("ServiceUsage / LimitWindow have exactly the Rust-serialized keys", () => {
    const window: LimitWindow = {
      label: "5-hour",
      used_percent: 92,
      resets_at: 123,
      used: 184,
      limit: 200,
    };
    const service: ServiceUsage = {
      id: "auto:claude",
      source: "auto",
      provider: "claude",
      connected: true,
      plan: "Max",
      account: "a@b.c",
      error: null,
      windows: [window],
      detail_windows: [],
      raw_response: "{}",
    };

    expect(Object.keys(service).sort()).toEqual([
      "account",
      "connected",
      "detail_windows",
      "error",
      "id",
      "plan",
      "provider",
      "raw_response",
      "source",
      "windows",
    ]);
    expect(Object.keys(window).sort()).toEqual([
      "label",
      "limit",
      "resets_at",
      "used",
      "used_percent",
    ]);
  });

  it("AppConfig is a poll interval plus a fixed 6-provider tuple", () => {
    // ProviderConfig.custom_name / primary_window are skip_serializing_if=None on
    // the Rust side, so we don't pin the inner key set here — only the top-level
    // AppConfig shape and the providers tuple length (part of the contract:
    // Rust is `[ProviderConfig; 6]`, indexed by canonical provider order).
    const config: AppConfig = {
      poll_seconds: 300,
      providers: [
        providerConfig(),
        providerConfig(),
        providerConfig(),
        providerConfig(),
        providerConfig(),
        providerConfig(),
      ],
    };
    expect(Object.keys(config).sort()).toEqual(["poll_seconds", "providers"]);
    expect(config.providers).toHaveLength(6);
  });
});

function providerConfig(): AppConfig["providers"][number] {
  return {
    enabled: true,
    custom_name: null,
    notify_thresholds: [50, 75, 90, 95, 100],
    primary_window: null,
    sort_index: 0,
  };
}
