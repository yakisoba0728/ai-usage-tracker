export type Provider = "claude" | "codex" | "gemini" | "copilot" | "cursor";

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
  /** PRIMARY windows — shown on the card as the headline usage. */
  windows: LimitWindow[];
  /** MODAL-ONLY windows — hidden on the card, shown in the detail modal. */
  detail_windows: LimitWindow[];
}

export interface UsageSnapshot {
  // epoch seconds
  fetched_at: number;
  services: ServiceUsage[];
}

export interface AppConfig {
  poll_seconds: number;
  enabled: [boolean, boolean, boolean, boolean, boolean];
}

export interface StoredCredential {
  id: string;
  provider: Provider;
  label: string;
  access_token: string;
  refresh_token: string | null;
  expires_at: number;
  id_token: string | null;
  account_id: string | null;
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

export interface CliLoginUrl {
  provider: Provider;
  url: string;
}
