import { AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";

/** Full-width centered error — shown when the whole snapshot is unreachable.
 * This view is provider-agnostic (the snapshot, not one provider, failed), so
 * there is no per-provider hint (F-12). */
export function ErrorState({ error }: { error: string | null }) {
  const { t } = useTranslation();
  const hint = t("error.checkConnection");
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
