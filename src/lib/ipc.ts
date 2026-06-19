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

export function getUsage(): Promise<UsageSnapshot> {
  return invoke<UsageSnapshot>("get_usage");
}

export function refreshNow(): Promise<UsageSnapshot> {
  return invoke<UsageSnapshot>("refresh_now");
}

export function getConfig(): Promise<AppConfig> {
  return invoke<AppConfig>("get_config");
}

export function setConfig(next: AppConfig): Promise<void> {
  return invoke<void>("set_config", { new: next });
}

/** Device-code OAuth login (Gemini/Copilot). Returns the device code info. */
export function startLogin(provider: Provider): Promise<LoginInfo> {
  return invoke<LoginInfo>("start_login", { provider });
}

/** Browser + localhost-callback OAuth (Codex). Returns the authorize URL
 * to open in the browser; emits `login-complete` when done. */
export function loginOAuth(provider: Provider): Promise<string> {
  return invoke<string>("login_oauth", { provider });
}

/** Cancel the in-progress OAuth login (closes the local callback server). */
export function cancelLogin(): Promise<void> {
  return invoke<void>("cancel_login");
}

/** Add an account by pasting a raw credential (Claude/Copilot/z.ai). */
export function addSessionKey(
  provider: Provider,
  key: string,
  label?: string,
): Promise<string> {
  return invoke<string>("add_session_key", { provider, key, label });
}

export function listAccounts(): Promise<AccountInfo[]> {
  return invoke<AccountInfo[]>("list_accounts");
}

export function removeAccount(id: string): Promise<boolean> {
  return invoke<boolean>("remove_account", { id });
}

export function onUsageUpdated(
  cb: (snapshot: UsageSnapshot) => void,
): Promise<UnlistenFn> {
  return listen<UsageSnapshot>(USAGE_UPDATED_EVENT, (event) => {
    cb(event.payload);
  });
}

export function onLoginComplete(
  cb: (r: LoginResult) => void,
): Promise<UnlistenFn> {
  return listen<LoginResult>(LOGIN_COMPLETE_EVENT, (event) => {
    cb(event.payload);
  });
}

/** Fired before each provider's fetch starts (payload = the Provider). */
export function onProviderLoading(
  cb: (provider: Provider) => void,
): Promise<UnlistenFn> {
  return listen<Provider>(PROVIDER_LOADING_EVENT, (event) => {
    cb(event.payload);
  });
}

/** Fired from the tray "Refresh now" menu item. */
export function onTriggerRefresh(cb: () => void): Promise<UnlistenFn> {
  return listen<null>(TRIGGER_REFRESH_EVENT, () => {
    cb();
  });
}
