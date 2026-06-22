import { Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";

export function LoadingState() {
  const { t } = useTranslation();
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 py-24 text-text-dim">
      <Loader2 className="size-5 animate-spin" />
      <span className="text-sm">{t("detail.loading")}</span>
    </div>
  );
}
