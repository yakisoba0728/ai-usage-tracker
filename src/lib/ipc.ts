import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  AccountInfo,
  AppConfig,
  LoginInfo,
  LoginResult,
  Provider,
  ProviderLoadingPayload,
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

/**
 * Rename ONE account by its service id (per-account; BUG-2 fix). `name = null`
 * clears the override back to the canonical provider label. In the browser dev
 * server this mutates the in-memory config so the UI reflects the change.
 */
export function renameAccount(
  serviceId: string,
  name: string | null,
): Promise<void> {
  if (!hasTauriRuntime()) {
    const trimmed = name?.trim();
    const prev = browserConfig.accounts[serviceId] ?? {};
    const nextAccount = { ...prev, custom_name: trimmed && trimmed.length > 0 ? trimmed : null };
    browserConfig = {
      ...browserConfig,
      accounts: { ...browserConfig.accounts, [serviceId]: nextAccount },
    };
    return Promise.resolve();
  }
  return invoke<void>("rename_account", { serviceId, name });
}

/**
 * Enable/disable launch-at-login (FEAT-4). Calls the `set_launch_at_login`
 * command, which toggles the OS login item AND persists the flag. In the
 * browser dev server this mutates the in-memory config so the toggle reflects.
 */
export function setLaunchAtLogin(enable: boolean): Promise<void> {
  if (!hasTauriRuntime()) {
    browserConfig = { ...browserConfig, launch_at_login: enable };
    return Promise.resolve();
  }
  return invoke<void>("set_launch_at_login", { enable });
}

/** A newer release reported by `check_update_now`, or null when up to date. */
export interface AvailableUpdate {
  version: string;
  html_url: string;
}

/**
 * Manually check GitHub for a newer release NOW (FEAT-5). Runs regardless of the
 * `auto_update_check` toggle and fires an OS notification for a newer release.
 * Resolves to the update info, or null when already up to date. In the browser
 * dev server there is no backend, so it resolves to null.
 */
export function checkUpdateNow(): Promise<AvailableUpdate | null> {
  if (!hasTauriRuntime()) return Promise.resolve(null);
  return invoke<AvailableUpdate | null>("check_update_now");
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

/**
 * The `anchor-result` event payload. Keeps `{id, ok, detail}` and — per the
 * Chunk-2 allowed delta (spec §3) — ADDS `provider` + `label` so the toast can
 * name which account the anchor targeted. `provider`/`label` are nullable: the
 * id may not resolve to a provider, and an account may have no display label.
 */
export interface AnchorResultPayload {
  id: string;
  ok: boolean;
  detail: string | null;
  provider: Provider | null;
  label: string | null;
}

export function onAnchorResult(
  handler: (payload: AnchorResultPayload) => void,
): Promise<UnlistenFn> {
  if (!hasTauriRuntime()) {
    void handler;
    return Promise.resolve(() => undefined);
  }
  return listen<AnchorResultPayload>("anchor-result", (event) => {
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
  cb: (payload: ProviderLoadingPayload) => void,
): Promise<UnlistenFn> {
  if (!hasTauriRuntime()) {
    void cb;
    return Promise.resolve(() => undefined);
  }
  return listen<Provider | ProviderLoadingPayload>(PROVIDER_LOADING_EVENT, (event) => {
    cb(normalizeProviderLoadingPayload(event.payload));
  });
}

function normalizeProviderLoadingPayload(
  payload: Provider | ProviderLoadingPayload,
): ProviderLoadingPayload {
  if (typeof payload === "string") {
    return { id: `auto:${payload}`, provider: payload };
  }
  return payload;
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
    schema_version: 1,
    poll_seconds: 300,
    providers: [
      providerConfig(0),
      providerConfig(1),
      providerConfig(2),
      providerConfig(3),
      providerConfig(4),
      providerConfig(5),
    ],
    accounts: {},
    auto_anchor: {},
    launch_at_login: false,
    auto_update_check: true,
    update_auto_open: false,
  };
}

function providerConfig(sortIndex: number): AppConfig["providers"][number] {
  return {
    enabled: true,
    notify_thresholds: [50, 75, 90, 95, 100],
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
        account: "team-demo@example.invalid",
        error: null,
        windows: [
          windowUsage("5-hour", 92, now + 42 * 60, 184, 200),
          windowUsage("7-day", 68, now + 2 * 24 * 60 * 60, 476, 700),
        ],
        detail_windows: [windowUsage("Extra usage", 12, null, 12.5, 100)],
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
      },
      {
        id: "auto:gemini",
        source: "auto",
        provider: "gemini",
        connected: true,
        plan: "Code Assist",
        account: "person-demo@example.invalid",
        error: null,
        windows: [windowUsage("Daily", 23, now + 11 * 60 * 60, 23, 100)],
        detail_windows: [windowUsage("Model calls", 38, now + 11 * 60 * 60, 38, 100)],
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
