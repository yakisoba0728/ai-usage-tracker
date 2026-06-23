import type { Provider } from "@/lib/types";
import { PROVIDER_ORDER } from "@/lib/providerMetadata";

export type AddAccountOptionId =
  | "session-key"
  | "browser-oauth"
  | "device-code"
  | "local-session";

export const ADD_ACCOUNT_PROVIDERS: Provider[] = PROVIDER_ORDER;

/**
 * The sign-in methods each provider supports, in display order. Titles and
 * descriptions are localized at the render site by id (`addAccount.option.<id>`).
 */
export function authOptionsForProvider(provider: Provider): AddAccountOptionId[] {
  switch (provider) {
    case "claude":
      // Claude is session-key only (FEAT-2/BUG-1): no CLI/OAuth auto-detect, so
      // no "local-session" option — paste a claude.ai sessionKey.
      return ["session-key"];
    case "zai":
      return ["session-key", "local-session"];
    case "codex":
      return ["browser-oauth", "local-session"];
    case "gemini":
      return ["browser-oauth"];
    case "copilot":
      return ["session-key", "device-code", "local-session"];
    case "cursor":
      return ["local-session"];
  }
}
