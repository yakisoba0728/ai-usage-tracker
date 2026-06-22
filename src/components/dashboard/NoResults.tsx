import { Search } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { shouldShowNoResultsOfflineCta } from "@/lib/dashboardState";

export function NoResults({
  query,
  onShowOffline,
}: {
  query: string;
  onShowOffline: () => void;
}) {
  const { t } = useTranslation();
  const showOfflineCta = shouldShowNoResultsOfflineCta(query);
  return (
    <div className="rounded-lg border border-border bg-surface/50 px-5 py-12 text-center">
      <Search className="mx-auto mb-3 size-6 text-text-faint" />
      <h2 className="text-sm font-semibold">{t("noResults.title")}</h2>
      <p className="mt-1 text-sm text-text-faint">
        {query ? t("noResults.hintQuery") : t("noResults.hintOffline")}
      </p>
      {showOfflineCta && (
        <Button variant="secondary" size="sm" className="mt-4" onClick={onShowOffline}>
          {t("noResults.showOffline")}
        </Button>
      )}
    </div>
  );
}
