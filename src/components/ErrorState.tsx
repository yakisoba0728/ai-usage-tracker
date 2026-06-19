import { AlertTriangle } from "lucide-react";

import type { Provider } from "@/lib/types";

/**
 * Provider-specific, actionable remediation hints. A provider NOT in this map
 * falls back to the generic hint — so a new provider renders correctly with
 * zero changes here (this is registry data, not component structure).
 */
const HINT: Partial<Record<Provider, string>> = {
  claude: "Run `claude` to refresh, or re-add the account.",
  codex: "Run `codex login`, or re-add the account.",
  gemini: "Re-authenticate with Google, or re-add the account.",
  copilot: "Re-authorize GitHub Copilot, or re-add the account.",
  cursor: "Sign in to Cursor again, or re-add the account.",
  zai: "Re-add the z.ai account with a fresh session key.",
};

/** Compact inline error block — rendered inside a provider card body. */
export function CardError({
  error,
  connected,
  provider,
}: {
  error: string | null;
  connected: boolean;
  provider: Provider;
}) {
  const message = error?.trim() || (connected ? "Couldn't read usage right now." : "Not connected.");
  return (
    <div className="flex items-start gap-2.5 rounded-md border border-border bg-surface-2 px-3 py-3">
      <AlertTriangle className="mt-0.5 size-4 shrink-0 text-text-faint" />
      <div className="min-w-0">
        <p className="text-text-dim" style={{ fontSize: 13 }}>
          {message}
        </p>
        <p className="mt-1 leading-relaxed text-text-faint" style={{ fontSize: 11 }}>
          {HINT[provider] ?? "Re-add the account, or refresh its credential."}
        </p>
      </div>
    </div>
  );
}

/** Full-width centered error — shown when the whole snapshot is unreachable. */
export function ErrorState({
  error,
  provider,
}: {
  error: string | null;
  provider?: Provider;
}) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 py-24 text-center">
      <AlertTriangle className="size-6 text-text-faint" />
      <div className="space-y-1">
        <p className="text-text" style={{ fontSize: 14, fontWeight: 500 }}>
          {error?.trim() || "Couldn't load usage."}
        </p>
        <p className="mx-auto max-w-xs leading-relaxed text-text-faint" style={{ fontSize: 12 }}>
          {provider
            ? HINT[provider] ?? "Re-add the account, or refresh its credential."
            : "Check the connection and try Refresh."}
        </p>
      </div>
    </div>
  );
}
