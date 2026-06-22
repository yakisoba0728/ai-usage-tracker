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
    case "zai":
      return ["session-key", "local-session"];
    case "codex":
    case "gemini":
      return ["browser-oauth", "local-session"];
    case "copilot":
      return ["session-key", "device-code", "local-session"];
    case "cursor":
      return ["local-session"];
  }
}
