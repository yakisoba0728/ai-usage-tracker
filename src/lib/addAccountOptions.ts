import type { Provider } from "@/lib/types";

export type AddAccountOptionId =
  | "session-key"
  | "browser-oauth"
  | "device-code"
  | "local-session";

export interface AddAccountOption {
  id: AddAccountOptionId;
  title: string;
  description: string;
}

const OPTIONS: Record<AddAccountOptionId, AddAccountOption> = {
  "session-key": {
    id: "session-key",
    title: "Paste session key",
    description: "Use a provider token or session key already copied from your browser or CLI.",
  },
  "browser-oauth": {
    id: "browser-oauth",
    title: "Browser sign-in",
    description: "Open the provider authorization page and capture the local callback.",
  },
  "device-code": {
    id: "device-code",
    title: "Device code sign-in",
    description: "Use the provider's device-code flow and enter the code in your browser.",
  },
  "local-session": {
    id: "local-session",
    title: "Reuse local session",
    description: "Scan this device for existing CLI or browser sessions.",
  },
};

export function authOptionsForProvider(
  provider: Provider,
): AddAccountOption[] {
  switch (provider) {
    case "claude":
    case "zai":
      return [OPTIONS["session-key"], OPTIONS["local-session"]];
    case "codex":
    case "gemini":
      return [OPTIONS["browser-oauth"], OPTIONS["local-session"]];
    case "copilot":
      return [
        OPTIONS["session-key"],
        OPTIONS["device-code"],
        OPTIONS["local-session"],
      ];
    case "cursor":
      return [OPTIONS["local-session"]];
  }
}
