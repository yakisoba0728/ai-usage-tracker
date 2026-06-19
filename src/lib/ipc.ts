import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  AppConfig,
  CliLoginUrl,
  LoginInfo,
  LoginResult,
  Provider,
  StoredCredential,
  UsageSnapshot,
} from "@/lib/types";

const USAGE_UPDATED_EVENT = "usage-updated";
const LOGIN_COMPLETE_EVENT = "login-complete";
const CLI_LOGIN_URL_EVENT = "cli-login-url";

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

/** Start a device-code OAuth login (Codex/Gemini/Copilot). */
export function startLogin(provider: Provider): Promise<LoginInfo> {
  return invoke<LoginInfo>("start_login", { provider });
}

/** Start a CLI-driven OAuth login (codexbar style; Claude). Emits
 * `cli-login-url` then `login-complete`. */
export function loginViaCli(provider: Provider): Promise<void> {
  return invoke<void>("login_via_cli", { provider });
}

/** Providers that can be logged in from the app (CLI-PTY or device-code). */
export function loginOptions(): Promise<Provider[]> {
  return invoke<Provider[]>("login_options");
}

export function listAccounts(): Promise<StoredCredential[]> {
  return invoke<StoredCredential[]>("list_accounts");
}

export function removeAccount(id: string): Promise<boolean> {
  return invoke<boolean>("remove_account", { id });
}

/** Subscribe to backend `usage-updated` pushes. */
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

/** Fired when a CLI login captures the OAuth URL to open in the browser. */
export function onCliLoginUrl(cb: (u: CliLoginUrl) => void): Promise<UnlistenFn> {
  return listen<CliLoginUrl>(CLI_LOGIN_URL_EVENT, (event) => {
    cb(event.payload);
  });
}
