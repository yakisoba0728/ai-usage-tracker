import type { UsageSnapshot } from "@/lib/types";

export interface DashboardModalState {
  addOpen: boolean;
  settingsOpen: boolean;
  detailOpen: boolean;
  moreMenuOpen: boolean;
}

export function transitionToAddAccount(
  state: DashboardModalState,
): DashboardModalState {
  void state;
  return {
    addOpen: true,
    settingsOpen: false,
    detailOpen: false,
    moreMenuOpen: false,
  };
}

export function transitionToSettings(
  state: DashboardModalState,
): DashboardModalState {
  void state;
  return {
    addOpen: false,
    settingsOpen: true,
    detailOpen: false,
    moreMenuOpen: false,
  };
}

export function shouldShowNoResultsOfflineCta(query: string): boolean {
  return query.trim().length === 0;
}

export function shouldProcessThresholdSnapshot(
  previous: UsageSnapshot | null,
  next: UsageSnapshot,
): boolean {
  return previous !== next;
}
