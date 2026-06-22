import type { UsageSnapshot } from "@/lib/types";

export interface SnapshotFreshnessInput {
  current: UsageSnapshot | null;
  incoming: UsageSnapshot;
  incomingOrder: number;
  latestAcceptedOrder: number;
}

export function shouldAcceptSnapshot({
  current,
  incoming,
  incomingOrder,
  latestAcceptedOrder,
}: SnapshotFreshnessInput): boolean {
  if (incomingOrder < latestAcceptedOrder) return false;
  if (current != null && incoming.fetched_at < current.fetched_at) return false;
  return true;
}

export interface RefreshActivity {
  pending: number;
  refreshing: boolean;
}

export function startRefreshActivity(activity: RefreshActivity): RefreshActivity {
  return { pending: activity.pending + 1, refreshing: true };
}

export function finishRefreshActivity(activity: RefreshActivity): RefreshActivity {
  const pending = Math.max(0, activity.pending - 1);
  return { pending, refreshing: pending > 0 };
}
