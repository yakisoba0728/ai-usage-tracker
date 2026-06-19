/**
 * Data model — mirrors the Rust IPC contract EXACTLY. Do not diverge; the
 * backend serializes into these shapes. A new provider is added to the
 * `Provider` union and gets the full UI for free (no per-provider code).
 */
export type Provider =
  | "claude"
  | "codex"
  | "gemini"
  | "copilot"
  | "cursor"
  | "zai";

export interface LimitWindow {
  label: string;
  used_percent: number | null;
  resets_at: number | null;
  used: number | null;
  limit: number | null;
}

export interface ServiceUsage {
  provider: Provider;
  connected: boolean;
  plan: string | null;
  account: string | null;
  error: string | null;
  /** PRIMARY windows — shown on the card (headline ring + secondary bars). */
  windows: LimitWindow[];
  /** MODAL-ONLY windows — hidden on the card, shown in the detail modal. */
  detail_windows: LimitWindow[];
  /** Pretty-printed raw API response JSON for the "Raw Response" tab. */
  raw_response?: string;
}

export interface UsageSnapshot {
  /** epoch seconds */
  fetched_at: number;
  services: ServiceUsage[];
}

/**
 * 6 enabled slots, one per provider in PROVIDER_ORDER. Adding a 7th provider
 * means appending to PROVIDER_ORDER here AND the Rust AppConfig (the
 * orchestrator owns the Rust side).
 */
export interface AppConfig {
  poll_seconds: number;
  enabled: [boolean, boolean, boolean, boolean, boolean, boolean];
}

/**
 * Display-only projection of a stored credential. The Rust `list_accounts`
 * command masks to this shape — access/refresh tokens never cross IPC (P0 #1).
 */
export interface AccountInfo {
  id: string;
  provider: Provider;
  label: string;
}

export interface LoginInfo {
  provider: Provider;
  verification_url: string;
  user_code: string;
  expires_in: number;
}

export interface LoginResult {
  provider: Provider;
  ok: boolean;
  label: string | null;
  error: string | null;
}
