import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  AccountInfo,
  AppConfig,
  LoginInfo,
  LoginResult,
  Provider,
  UsageSnapshot,
} from "@/lib/types";

const USAGE_UPDATED_EVENT = "usage-updated";
const LOGIN_COMPLETE_EVENT = "login-complete";
const PROVIDER_LOADING_EVENT = "provider-loading";
const TRIGGER_REFRESH_EVENT = "trigger-refresh";

let browserConfig: AppConfig = createDefaultConfig();

export function getUsage(): Promise<UsageSnapshot> {
  if (!hasTauriRuntime()) return Promise.resolve(createBrowserSnapshot());
  return invoke<UsageSnapshot>("get_usage");
}

export function refreshNow(): Promise<UsageSnapshot> {
  if (!hasTauriRuntime()) return Promise.resolve(createBrowserSnapshot());
  return invoke<UsageSnapshot>("refresh_now");
}

export function getConfig(): Promise<AppConfig> {
  if (!hasTauriRuntime()) return Promise.resolve(browserConfig);
  return invoke<AppConfig>("get_config");
}

export function setConfig(next: AppConfig): Promise<void> {
  if (!hasTauriRuntime()) {
    browserConfig = next;
    return Promise.resolve();
  }
  return invoke<void>("set_config", { new: next });
}

/** Device-code OAuth login (Gemini/Copilot). Returns the device code info. */
export function startLogin(provider: Provider): Promise<LoginInfo> {
  if (!hasTauriRuntime()) return Promise.reject(browserOnlyError(provider));
  return invoke<LoginInfo>("start_login", { provider });
}

/** Browser + localhost-callback OAuth (Codex). Returns the authorize URL
 * to open in the browser; emits `login-complete` when done. */
export function loginOAuth(provider: Provider): Promise<string> {
  if (!hasTauriRuntime()) return Promise.reject(browserOnlyError(provider));
  return invoke<string>("login_oauth", { provider });
}

/** Cancel the in-progress OAuth login (closes the local callback server). */
export function cancelLogin(): Promise<void> {
  if (!hasTauriRuntime()) return Promise.resolve();
  return invoke<void>("cancel_login");
}

/** Add an account by pasting a raw credential (Claude/Copilot/z.ai). */
export function addSessionKey(
  provider: Provider,
  key: string,
  label?: string,
): Promise<string> {
  if (!hasTauriRuntime()) {
    void key;
    void label;
    return Promise.reject(browserOnlyError(provider));
  }
  return invoke<string>("add_session_key", { provider, key, label });
}

export function listAccounts(): Promise<AccountInfo[]> {
  if (!hasTauriRuntime()) return Promise.resolve([]);
  return invoke<AccountInfo[]>("list_accounts");
}

export function removeAccount(id: string): Promise<boolean> {
  if (!hasTauriRuntime()) {
    void id;
    return Promise.resolve(false);
  }
  return invoke<boolean>("remove_account", { id });
}

export function sendAnchorNow(serviceId: string): Promise<void> {
  if (!hasTauriRuntime()) {
    void serviceId;
    return Promise.resolve();
  }
  return invoke<void>("send_anchor_now", { serviceId });
}

export function refreshAccount(serviceId: string): Promise<void> {
  if (!hasTauriRuntime()) {
    void serviceId;
    return Promise.resolve();
  }
  return invoke<void>("refresh_account", { serviceId });
}

export function onAnchorResult(
  handler: (payload: { id: string; ok: boolean; detail: string | null }) => void,
): Promise<UnlistenFn> {
  if (!hasTauriRuntime()) {
    void handler;
    return Promise.resolve(() => undefined);
  }
  return listen<{ id: string; ok: boolean; detail: string | null }>("anchor-result", (event) => {
    handler(event.payload);
  });
}

export function onRefreshResult(
  handler: (payload: { id: string; ok: boolean; detail: string | null }) => void,
): Promise<UnlistenFn> {
  if (!hasTauriRuntime()) {
    void handler;
    return Promise.resolve(() => undefined);
  }
  return listen<{ id: string; ok: boolean; detail: string | null }>(
    "refresh-result",
    (event) => {
      handler(event.payload);
    },
  );
}

export function onUsageUpdated(
  cb: (snapshot: UsageSnapshot) => void,
): Promise<UnlistenFn> {
  if (!hasTauriRuntime()) {
    void cb;
    return Promise.resolve(() => undefined);
  }
  return listen<UsageSnapshot>(USAGE_UPDATED_EVENT, (event) => {
    cb(event.payload);
  });
}

export function onLoginComplete(
  cb: (r: LoginResult) => void,
): Promise<UnlistenFn> {
  if (!hasTauriRuntime()) {
    void cb;
    return Promise.resolve(() => undefined);
  }
  return listen<LoginResult>(LOGIN_COMPLETE_EVENT, (event) => {
    cb(event.payload);
  });
}

/** Fired before each provider's fetch starts (payload = the Provider). */
export function onProviderLoading(
  cb: (provider: Provider) => void,
): Promise<UnlistenFn> {
  if (!hasTauriRuntime()) {
    void cb;
    return Promise.resolve(() => undefined);
  }
  return listen<Provider>(PROVIDER_LOADING_EVENT, (event) => {
    cb(event.payload);
  });
}

/** Fired from the tray "Refresh now" menu item. */
export function onTriggerRefresh(cb: () => void): Promise<UnlistenFn> {
  if (!hasTauriRuntime()) {
    void cb;
    return Promise.resolve(() => undefined);
  }
  return listen<null>(TRIGGER_REFRESH_EVENT, () => {
    cb();
  });
}

function hasTauriRuntime(): boolean {
  if (typeof window === "undefined") return false;
  const tauriWindow = window as Window & { __TAURI_INTERNALS__?: unknown };
  return (
    "__TAURI_INTERNALS__" in tauriWindow &&
    tauriWindow.__TAURI_INTERNALS__ != null
  );
}

function browserOnlyError(provider: Provider): Error {
  return new Error(
    `${provider} login requires the Tauri desktop backend. The browser dev server uses read-only demo data.`,
  );
}

function createDefaultConfig(): AppConfig {
  return {
    poll_seconds: 300,
    providers: [
      providerConfig(0),
      providerConfig(1),
      providerConfig(2),
      providerConfig(3),
      providerConfig(4),
      providerConfig(5),
    ],
    auto_anchor: {},
  };
}

function providerConfig(sortIndex: number): AppConfig["providers"][number] {
  return {
    enabled: true,
    custom_name: null,
    notify_thresholds: [50, 75, 90, 95, 100],
    primary_window: null,
    sort_index: sortIndex,
  };
}

function createBrowserSnapshot(): UsageSnapshot {
  const now = Math.floor(Date.now() / 1000);
  return {
    fetched_at: now,
    services: [
      {
        id: "auto:claude",
        source: "auto",
        provider: "claude",
        connected: true,
        plan: "Max 20x",
        account: "team@example.com",
        error: null,
        windows: [
          windowUsage("5-hour", 92, now + 42 * 60, 184, 200),
          windowUsage("7-day", 68, now + 2 * 24 * 60 * 60, 476, 700),
        ],
        detail_windows: [windowUsage("Extra usage", 12, null, 12.5, 100)],
        raw_response: JSON.stringify(
          {
            five_hour: { utilization: 0.92, resets_at: now + 42 * 60 },
            seven_day: { utilization: 0.68, resets_at: now + 2 * 24 * 60 * 60 },
            extra_usage: { used_usd: 12.5, limit_usd: 100 },
          },
          null,
          2,
        ),
      },
      {
        id: "auto:codex",
        source: "auto",
        provider: "codex",
        connected: true,
        plan: "Plus",
        account: "renews 2026-07-18",
        error: null,
        windows: [
          windowUsage("5-hour", 64, now + 91 * 60, null, null),
          windowUsage("Weekly", 41, now + 4 * 24 * 60 * 60, null, null),
        ],
        detail_windows: [windowUsage("Spark · 5-hour", 73, now + 91 * 60, null, null)],
        raw_response: JSON.stringify(
          {
            rate_limits: {
              "5h": { used_percent: 64, resets_at: now + 91 * 60 },
              weekly: { used_percent: 41, resets_at: now + 4 * 24 * 60 * 60 },
            },
            spark: { used_percent: 73 },
          },
          null,
          2,
        ),
      },
      {
        id: "auto:gemini",
        source: "auto",
        provider: "gemini",
        connected: true,
        plan: "Code Assist",
        account: "personal@gmail.com",
        error: null,
        windows: [windowUsage("Daily", 23, now + 11 * 60 * 60, 23, 100)],
        detail_windows: [windowUsage("Model calls", 38, now + 11 * 60 * 60, 38, 100)],
        raw_response: JSON.stringify(
          {
            daily: { used: 23, limit: 100, resets_at: now + 11 * 60 * 60 },
            model_calls: { used: 38, limit: 100 },
          },
          null,
          2,
        ),
      },
      {
        id: "auto:copilot",
        source: "auto",
        provider: "copilot",
        connected: true,
        plan: "Business",
        account: "github-user",
        error: null,
        windows: [windowUsage("Monthly premium requests", 48, now + 9 * 24 * 60 * 60, 144, 300)],
        detail_windows: [],
        raw_response: JSON.stringify(
          {
            quota_snapshots: {
              premium_interactions: {
                entitlement: 300,
                remaining: 156,
                percent_used: 48,
              },
            },
          },
          null,
          2,
        ),
      },
      {
        id: "auto:cursor",
        source: "auto",
        provider: "cursor",
        connected: false,
        plan: null,
        account: null,
        error: { code: "not_logged_in", detail: "Cursor session not found." },
        windows: [],
        detail_windows: [],
      },
      {
        id: "stored:zai-demo",
        source: "stored",
        provider: "zai",
        connected: true,
        plan: "Pro",
        account: "z.ai workspace",
        error: null,
        windows: [
          windowUsage("5-hour", 0, now + 5 * 60 * 60, 0, 100),
          windowUsage("Weekly", 77, now + 3 * 24 * 60 * 60, 77, 100),
        ],
        detail_windows: [windowUsage("GLM-4.5", 58, now + 3 * 24 * 60 * 60, 58, 100)],
        raw_response: JSON.stringify(
          {
            plan: "Pro",
            weekly: { used: 77, limit: 100, resets_at: now + 3 * 24 * 60 * 60 },
            models: { "GLM-4.5": { used: 58, limit: 100 } },
          },
          null,
          2,
        ),
      },
    ],
  };
}

function windowUsage(
  label: string,
  used_percent: number | null,
  resets_at: number | null,
  used: number | null,
  limit: number | null,
) {
  return { label, used_percent, resets_at, used, limit };
}
