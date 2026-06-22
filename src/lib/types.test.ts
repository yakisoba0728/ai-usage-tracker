import { describe, expect, it } from "vitest";

import type {
  AppConfig,
  LimitWindow,
  ProviderLoadingPayload,
  ServiceError,
  ServiceUsage,
} from "@/lib/types";

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
      account: "person@example.invalid",
      error: null,
      windows: [window],
      detail_windows: [],
    };

    expect(Object.keys(service).sort()).toEqual([
      "account",
      "connected",
      "detail_windows",
      "error",
      "id",
      "plan",
      "provider",
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

  it("ServiceError is { code, detail? } — detail omitted, not null, when absent", () => {
    const withDetail: ServiceError = {
      code: "server_error",
      detail: "unexpected response (429): rate limited",
    };
    expect(Object.keys(withDetail).sort()).toEqual(["code", "detail"]);

    // Rust omits `detail` (skip_serializing_if=None), so the wire object may
    // carry only `code` — `detail?` must stay optional to mirror that.
    const codeOnly: ServiceError = { code: "network" };
    expect(Object.keys(codeOnly)).toEqual(["code"]);
  });

  it("AppConfig is a versioned poll interval, a fixed 6-provider tuple, and per-account maps", () => {
    // AccountConfig.custom_name / primary_window are skip_serializing_if=None on
    // the Rust side (in the `accounts` map), so we don't pin those inner key sets
    // here — only the top-level AppConfig shape and the providers tuple length
    // (part of the contract: Rust is `[ProviderConfig; 6]`, indexed by canonical
    // provider order, plus `schema_version` + `accounts` per service id).
    const config: AppConfig = {
      schema_version: 1,
      poll_seconds: 300,
      providers: [
        providerConfig(),
        providerConfig(),
        providerConfig(),
        providerConfig(),
        providerConfig(),
        providerConfig(),
      ],
      accounts: {},
      auto_anchor: {},
    };
    expect(Object.keys(config).sort()).toEqual([
      "accounts",
      "auto_anchor",
      "poll_seconds",
      "providers",
      "schema_version",
    ]);
    expect(config.providers).toHaveLength(6);
  });

  it("ProviderLoadingPayload targets one service id, not every provider card", () => {
    const loading: ProviderLoadingPayload = {
      id: "stored:claude-work",
      provider: "claude",
    };

    expect(Object.keys(loading).sort()).toEqual(["id", "provider"]);
  });
});

function providerConfig(): AppConfig["providers"][number] {
  return {
    enabled: true,
    notify_thresholds: [50, 75, 90, 95, 100],
    sort_index: 0,
  };
}
