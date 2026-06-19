import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  AppConfig,
  LoginInfo,
  LoginResult,
  Provider,
  StoredCredential,
  UsageSnapshot,
} from "@/lib/types";

const USAGE_UPDATED_EVENT = "usage-updated";
const LOGIN_COMPLETE_EVENT = "login-complete";

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

/** Start a device-code OAuth login for a manually-added account. */
export function startLogin(provider: Provider): Promise<LoginInfo> {
  return invoke<LoginInfo>("start_login", { provider });
}

export function listAccounts(): Promise<StoredCredential[]> {
  return invoke<StoredCredential[]>("list_accounts");
}

export function removeAccount(id: string): Promise<boolean> {
  return invoke<boolean>("remove_account", { id });
}

/**
 * Subscribe to backend `usage-updated` pushes. Returns a promise that resolves
 * to an unlisten function once the listener is attached.
 */
export function onUsageUpdated(
  cb: (snapshot: UsageSnapshot) => void,
): Promise<UnlistenFn> {
  return listen<UsageSnapshot>(USAGE_UPDATED_EVENT, (event) => {
    cb(event.payload);
  });
}

/** Fired when a device-code login completes (success or failure). */
export function onLoginComplete(cb: (r: LoginResult) => void): Promise<UnlistenFn> {
  return listen<LoginResult>(LOGIN_COMPLETE_EVENT, (event) => {
    cb(event.payload);
  });
}
