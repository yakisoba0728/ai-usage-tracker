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

/** Device-code OAuth login (Gemini/Copilot). Returns the device code info. */
export function startLogin(provider: Provider): Promise<LoginInfo> {
  return invoke<LoginInfo>("start_login", { provider });
}

/** CLI-driven OAuth login for Claude (codexbar style: drives `claude /login` in
 * a PTY). Emits `cli-login-url` then `login-complete`. */
export function loginViaCli(provider: Provider): Promise<void> {
  return invoke<void>("login_via_cli", { provider });
}

/** Browser + localhost-callback OAuth (Codex). Returns the authorize URL
 * to open in the browser; emits `login-complete` when done. */
export function loginOAuth(provider: Provider): Promise<string> {
  return invoke<string>("login_oauth", { provider });
}

/** Exchange a pasted code (Claude manual-code flow). */
export function exchangeCode(provider: Provider, code: string): Promise<void> {
  return invoke<void>("exchange_code", { provider, code });
}

/** Cancel the in-progress OAuth login (closes the local callback server). */
export function cancelLogin(): Promise<void> {
  return invoke<void>("cancel_login");
}

/** Add an account by pasting a raw credential (Claude session key). */
export function addSessionKey(provider: Provider, key: string, label?: string): Promise<string> {
  return invoke<string>("add_session_key", { provider, key, label });
}

/** Providers that can be logged in from the app. */
export function loginOptions(): Promise<Provider[]> {
  return invoke<Provider[]>("login_options");
}

export function listAccounts(): Promise<StoredCredential[]> {
  return invoke<StoredCredential[]>("list_accounts");
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

export function onLoginComplete(cb: (r: LoginResult) => void): Promise<UnlistenFn> {
  return listen<LoginResult>(LOGIN_COMPLETE_EVENT, (event) => {
    cb(event.payload);
  });
}

export function onCliLoginUrl(cb: (u: CliLoginUrl) => void): Promise<UnlistenFn> {
  return listen<CliLoginUrl>(CLI_LOGIN_URL_EVENT, (event) => {
    cb(event.payload);
  });
}
