import { AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { Provider } from "@/lib/types";

/** Full-width centered error — shown when the whole snapshot is unreachable. */
export function ErrorState({
  error,
  provider,
}: {
  error: string | null;
  provider?: Provider;
}) {
  const { t } = useTranslation();
  // Provider-specific remediation hint; falls back to a generic line so a new
  // provider renders correctly with zero changes here.
  const hint = provider
    ? t(`error.hint.${provider}`, { defaultValue: t("error.hint.generic") })
    : t("error.checkConnection");
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 py-24 text-center">
      <AlertTriangle className="size-6 text-text-faint" />
      <div className="space-y-1">
        <p className="text-text" style={{ fontSize: 14, fontWeight: 500 }}>
          {error?.trim() || t("error.loadFailed")}
        </p>
        <p className="mx-auto max-w-xs leading-relaxed text-text-faint" style={{ fontSize: 12 }}>
          {hint}
        </p>
      </div>
    </div>
  );
}
