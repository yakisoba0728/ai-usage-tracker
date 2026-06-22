import { Command, RefreshCw, Settings } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";

export function MobileHeader({
  refreshing,
  onRefresh,
  onOpenSettings,
}: {
  refreshing: boolean;
  onRefresh: () => void;
  onOpenSettings: () => void;
}) {
  // The language picker moved into Settings → General → Display; the header now
  // only carries Refresh + Settings. i18n stays wired app-wide via `t`.
  const { t } = useTranslation();
  return (
    <header className="flex h-12 items-center justify-between border-b border-border px-4">
      <div className="flex items-center gap-2 font-semibold">
        <Command className="size-5 text-[#73b8f4]" />
        AI Usage Tracker
      </div>
      <div className="flex items-center gap-1">
        <Button
          variant="ghost"
          size="icon"
          onClick={onRefresh}
          disabled={refreshing}
          aria-label={t("common.refresh")}
        >
          <RefreshCw className={refreshing ? "size-4 animate-spin" : "size-4"} />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          onClick={onOpenSettings}
          aria-label={t("common.settings")}
        >
          <Settings className="size-4" />
        </Button>
      </div>
    </header>
  );
}
