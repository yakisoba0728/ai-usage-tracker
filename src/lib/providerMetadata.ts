import type { Provider } from "@/lib/types";

/**
 * Canonical provider order. This must mirror Rust `provider_index` in
 * `config.rs` and `AppConfig.providers: [ProviderConfig; 6]`.
 */
export const PROVIDER_ORDER: Provider[] = [
  "claude",
  "codex",
  "gemini",
  "copilot",
  "cursor",
  "zai",
];

/** Human label per provider, used in headers, modal titles, and rows. */
export const PROVIDER_LABEL: Record<Provider, string> = {
  claude: "Claude",
  codex: "Codex",
  gemini: "Gemini",
  copilot: "GitHub Copilot",
  cursor: "Cursor",
  zai: "z.ai Coding Plan",
};
