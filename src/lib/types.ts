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
