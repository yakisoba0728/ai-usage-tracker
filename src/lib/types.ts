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

export type ServiceSource = "auto" | "stored";

export interface LimitWindow {
  label: string;
  used_percent: number | null;
  resets_at: number | null;
  used: number | null;
  limit: number | null;
}

/**
 * The closed set of stable error codes the backend emits (one per
 * `ProviderError` variant in `src-tauri/src/providers/mod.rs`). Each MUST have a
 * matching `error.<code>` key in every locale catalog — guarded by a test. The
 * runtime array is the single source of truth for the `ServiceErrorCode` union.
 */
export const SERVICE_ERROR_CODES = [
  "not_logged_in",
  "token_expired",
  "network",
  "server_error",
  "parse_error",
] as const;

export type ServiceErrorCode = (typeof SERVICE_ERROR_CODES)[number];

/**
 * Structured, localizable service error. `code` is a stable machine key the UI
 * maps to a localized message (`error.<code>`); `detail` is the English
 * technical string, shown as a fallback when no key matches that code.
 */
export interface ServiceError {
  code: ServiceErrorCode;
  detail?: string;
}

export interface ServiceUsage {
  id: string;
  source: ServiceSource;
  provider: Provider;
  connected: boolean;
  plan: string | null;
  account: string | null;
  error: ServiceError | null;
  /** PRIMARY windows — shown on the card (headline ring + secondary bars). */
  windows: LimitWindow[];
  /** MODAL-ONLY windows — hidden on the card, shown in the detail modal. */
  detail_windows: LimitWindow[];
  /** Redacted debug-only raw API response JSON; omitted from IPC by default. */
  raw_response?: string;
}

export interface UsageSnapshot {
  /** epoch seconds */
  fetched_at: number;
  services: ServiceUsage[];
}


/**
 * Per-provider user settings. Mirrors the Rust `ProviderConfig` 1:1 (config.rs).
 * Indexed in `AppConfig.providers` by canonical `PROVIDER_ORDER`. These are
 * genuinely provider-level — every account of a provider shares them. Per-ACCOUNT
 * name/window live in `AppConfig.accounts` (keyed by service_id), fixing BUG-2.
 */
export interface ProviderConfig {
  enabled: boolean;
  /** Notification thresholds in percent (0–100). Default [50,75,90,95,100]. */
  notify_thresholds: number[];
  /** Sort index for drag-and-drop reordering. Lower = earlier. */
  sort_index: number;
}

/**
 * Per-ACCOUNT user settings, keyed by service_id (`auto:<provider>` /
 * `stored:<id>`) in `AppConfig.accounts`. Mirrors the Rust `AccountConfig`.
 * Two accounts of the same provider have independent entries (BUG-2 fix).
 */
export interface AccountConfig {
  /**
   * Override the display name shown on the card / modal title. Optional AND
   * nullable: Rust serializes this with `skip_serializing_if = Option::is_none`,
   * so when unset the key is ABSENT (→ `undefined`) over IPC, while the TS write
   * side may set it back to `null` (F-3).
   */
  custom_name?: string | null;
  /** Which window label to surface as the card headline. Absent/null = auto. */
  primary_window?: string | null;
}

/**
 * Persisted user configuration. `providers` is a fixed 6-tuple in canonical
 * order [claude, codex, gemini, copilot, cursor, zai] — exactly mirrors the
 * Rust `AppConfig.providers: [ProviderConfig; 6]`.
 */
export interface AppConfig {
  /** On-disk schema version (1 = current). Drives one-time Rust-side migration. */
  schema_version: number;
  poll_seconds: number;
  providers: [
    ProviderConfig,
    ProviderConfig,
    ProviderConfig,
    ProviderConfig,
    ProviderConfig,
    ProviderConfig,
  ];
  /** Per-service_id account settings (display name + pinned window). */
  accounts: Record<string, AccountConfig>;
  /** Per-service_id opt-in for auto window-anchoring. */
  auto_anchor: Record<string, boolean>;
  /** Launch the app at login (FEAT-4). Mirrors the OS login item; the
   * `set_launch_at_login` command toggles both the OS item and this flag. */
  launch_at_login: boolean;
  /** Whether the background GitHub-release update notifier runs (FEAT-5).
   * Defaults true. */
  auto_update_check: boolean;
  /** On a new release, also open the release page (not just notify). Defaults
   * false (notify-only). The manual "Check for updates" always opens. */
  update_auto_open?: boolean;
  /**
   * Last release version we notified about, so the daily check doesn't repeat
   * (FEAT-5). Rust serializes with `skip_serializing_if = Option::is_none`, so
   * the key is ABSENT (→ `undefined`) until the first notification. Managed by
   * the backend; the UI never writes it. */
  last_notified_version?: string | null;
  /**
   * Stable per-install device id (the `anthropic-device-id` for the claude.ai
   * web anchor, FEAT-2). Rust serializes with `skip_serializing_if =
   * Option::is_none`, so the key is ABSENT until generated at startup. Managed
   * entirely by the backend; the UI never reads or writes it. */
  device_id?: string | null;
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

export interface ProviderLoadingPayload {
  id: string;
  provider: Provider;
}
