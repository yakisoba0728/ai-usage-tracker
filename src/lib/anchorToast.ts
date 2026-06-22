import { PROVIDER_LABEL } from "@/lib/providerMetadata";
import type { Provider } from "@/lib/types";

/**
 * The in-app anchor toast decision — pure, so it is unit-tested before the
 * Dashboard JSX wires it (spec §8 "logic-in-lib"). Mirrors the Rust OS-notif
 * text builder's intent: name the provider + account, and read an AUTOMATIC
 * send differently from a manual one (finding notif-missing-account-identity).
 *
 * Returns the i18next key to render plus the interpolation values. The caller
 * does `t(result.key, result.params)`.
 */
export interface AnchorToast {
  /** i18next message key. */
  key:
    | "toast.anchorSentFor"
    | "toast.anchorFailedFor"
    | "toast.anchorAutoSentFor"
    | "toast.anchorAutoFailedFor";
  params: { name: string; error?: string };
}

/**
 * "Claude (person@example.com)" when an account label is present, else just the
 * provider display name. Never renders a raw `stored:<uuid>` id — the label is
 * already the account display string (or null), and `provider` falls back to a
 * generic when the id couldn't be resolved server-side.
 */
export function anchorSubject(
  provider: Provider | null,
  label: string | null,
): string {
  const name = provider ? PROVIDER_LABEL[provider] : "";
  const account = label?.trim();
  if (name && account) return `${name} (${account})`;
  if (name) return name;
  if (account) return account;
  return "";
}

/**
 * Build the anchor toast for an `anchor-result` event.
 * - `isAuto` — the result arrived with no pending manual action, i.e. the
 *   background auto-anchor fired it; reads differently from a manual send.
 * - `error` — scrubbed detail string, only attached on failure.
 */
export function buildAnchorToast(
  provider: Provider | null,
  label: string | null,
  ok: boolean,
  isAuto: boolean,
  error?: string,
): AnchorToast {
  const name = anchorSubject(provider, label);
  if (ok) {
    return {
      key: isAuto ? "toast.anchorAutoSentFor" : "toast.anchorSentFor",
      params: { name },
    };
  }
  return {
    key: isAuto ? "toast.anchorAutoFailedFor" : "toast.anchorFailedFor",
    params: { name, error: error ?? "" },
  };
}
