import { Cloud, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";

import { InfoLine } from "@/components/dashboard/detail/primitives";
import { Button } from "@/components/ui/button";
import { formatUpdatedAgo } from "@/lib/format";
import type { ServiceUsage } from "@/lib/types";

export function SessionsTab({
  service,
  fetchedAt,
  nowMs,
  onRefresh,
  onOpenAdd,
}: {
  service: ServiceUsage;
  fetchedAt: number | null;
  nowMs: number;
  onRefresh: () => void;
  onOpenAdd: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-4">
      <div className="rounded-lg border border-border bg-surface/50 p-4">
        <div className="grid gap-4 sm:grid-cols-2">
          <InfoLine
            label={t("detail.sessions.source")}
            value={
              service.source === "stored"
                ? t("detail.sessions.storedCredential")
                : t("detail.sessions.autoDetected")
            }
          />
          <InfoLine
            label={t("detail.sessions.provider")}
            value={service.provider}
            mono
          />
          <InfoLine
            label={t("detail.sessions.account")}
            value={service.account ?? t("detail.sessions.notReported")}
          />
          <InfoLine
            label={t("detail.sessions.lastParsed")}
            value={formatUpdatedAgo(fetchedAt, nowMs, t, { prefix: false })}
            mono
          />
        </div>
      </div>

      <div className="flex flex-wrap gap-2">
        <Button variant="outline" onClick={onRefresh}>
          <RefreshCw className="size-4" />
          {t("detail.sessions.reuseLocal")}
        </Button>
        <Button variant="secondary" onClick={onOpenAdd}>
          <Cloud className="size-4" />
          {t("detail.sessions.reauth")}
        </Button>
      </div>
    </div>
  );
}
