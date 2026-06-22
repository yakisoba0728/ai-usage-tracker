import { useTranslation } from "react-i18next";

import { useNow } from "@/hooks/useNow";
import { formatUpdatedAgo } from "@/lib/format";

/**
 * Isolated per-second clock for the "Updated Xs ago" footer, so only this tiny
 * node re-renders each second instead of the whole dashboard tree.
 */
export function LiveUpdatedAgo({ fetchedAt }: { fetchedAt: number | null }) {
  const { t } = useTranslation();
  const now = useNow(1000);
  return <>{formatUpdatedAgo(fetchedAt, now, t)}</>;
}
